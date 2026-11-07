use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use perkolator_core::{
    ActionAvailability, ActionEffect, ActionId, ClipboardPayload, ResourceDetails, ResourceKind,
    ResourceRecord,
};
use thiserror::Error;

use crate::compose::{ComposeFormat, compose_payload, invocation_line, reference_text};
use crate::context::ActionContext;
use crate::mcp_config::mcp_config_snippet;
use crate::registry::{availability, is_expandable_prompt, offered};
use crate::template::{TemplateError, expand_template, strip_frontmatter};

/// Upper bound for "Copy contents" / template expansion reads.
pub const MAX_COPY_BYTES: u64 = 1024 * 1024;

/// User-supplied inputs for actions that need them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionInputs {
    /// Query typed in the composer; turns Compose/Insert into the composed payload.
    pub compose_text: Option<String>,
    pub template_values: BTreeMap<String, String>,
    /// Copy MCP secrets verbatim. Only an explicit, confirmed UI path may set this.
    pub include_secrets: bool,
}

#[derive(Debug, Error)]
pub enum ActionError {
    #[error("“{}” is not offered for this {}", id.label(), kind.label())]
    NotOffered { id: ActionId, kind: ResourceKind },
    #[error("{reason}")]
    Unavailable { id: ActionId, reason: String },
    #[error("{} is {size} bytes; copying is limited to {limit} bytes", path.display())]
    TooLarge {
        path: PathBuf,
        size: u64,
        limit: u64,
    },
    #[error("{} is not UTF-8 text", path.display())]
    NotText { path: PathBuf },
    #[error("could not read {}: {message}", path.display())]
    Io { path: PathBuf, message: String },
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error("MCP server `{entry}` uses an unrecognised transport; no configuration can be copied")]
    UnknownTransport { entry: String },
    #[error("this resource has no local path")]
    NoLocalPath,
}

fn clipboard(label: impl Into<String>, text: impl Into<String>) -> ActionEffect {
    ActionEffect::WriteClipboard(ClipboardPayload {
        label: label.into(),
        text: text.into(),
    })
}

/// Resolve an offered, available action into its effect. Never mutates
/// anything: mutating actions only open the planner.
pub fn resolve(
    id: ActionId,
    record: &ResourceRecord,
    ctx: &ActionContext,
    inputs: &ActionInputs,
) -> Result<ActionEffect, ActionError> {
    if !offered(record).any(|offered_id| offered_id == id) {
        return Err(ActionError::NotOffered {
            id,
            kind: record.kind(),
        });
    }
    if let ActionAvailability::Disabled { reason } = availability(id, record, ctx) {
        return Err(ActionError::Unavailable { id, reason });
    }
    if id.is_mutating() {
        return Ok(ActionEffect::OpenDeployPlanner {
            id: record.id.clone(),
        });
    }
    let local_path = || record.local_path().ok_or(ActionError::NoLocalPath);
    match id {
        ActionId::ShowDetails => Ok(ActionEffect::OpenDetails {
            id: record.id.clone(),
        }),
        ActionId::CopyPath => {
            let label = if record.kind() == ResourceKind::McpServer {
                "config file path"
            } else {
                "path"
            };
            Ok(clipboard(label, path_text(local_path()?)))
        }
        ActionId::CopyDeployedPath => {
            let deployed = record
                .deployments()
                .next()
                .ok_or(ActionError::NoLocalPath)?;
            Ok(clipboard("deployed path", path_text(&deployed.path)))
        }
        ActionId::CopySourceUrl => {
            let url = record.provenance.origin_url.clone().unwrap_or_default();
            Ok(clipboard("source URL", url))
        }
        ActionId::CopyContent => {
            let (label, text) = primary_text(record)?;
            Ok(clipboard(label, text))
        }
        ActionId::CopyConfig => copy_config(record, inputs),
        ActionId::CopyReference => Ok(clipboard("reference", reference_text(record))),
        ActionId::CopyInvocation => Ok(clipboard("invocation", invocation_line(record))),
        ActionId::ExpandTemplate => Ok(clipboard("expanded prompt", expand(record, inputs)?)),
        ActionId::Compose => Ok(match &inputs.compose_text {
            Some(query) => clipboard(
                "composed query",
                compose_payload(&[record], query, ComposeFormat::Generic),
            ),
            None => ActionEffect::EnterCompose {
                id: record.id.clone(),
            },
        }),
        ActionId::InsertIntoSession => {
            let (label, text) = insert_payload(record, inputs)?;
            Ok(clipboard(label, text))
        }
        ActionId::RevealInFileManager => Ok(ActionEffect::Reveal {
            path: local_path()?.clone(),
        }),
        ActionId::PlanDeploy
        | ActionId::RemoveDeployment
        | ActionId::RepairLink
        | ActionId::Enable
        | ActionId::Disable => Ok(ActionEffect::OpenDeployPlanner {
            id: record.id.clone(),
        }),
    }
}

/// Build a clipboard effect for a multi-resource composed query (the
/// composer's Enter). Pair with [`crate::deliver`] for insert/fallback.
pub fn compose_effect(
    resources: &[&ResourceRecord],
    query: &str,
    format: ComposeFormat,
) -> ActionEffect {
    clipboard("composed query", compose_payload(resources, query, format))
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn copy_config(
    record: &ResourceRecord,
    inputs: &ActionInputs,
) -> Result<ActionEffect, ActionError> {
    let ResourceDetails::McpServer(mcp) = &record.details else {
        return Err(ActionError::NotOffered {
            id: ActionId::CopyConfig,
            kind: record.kind(),
        });
    };
    let snippet = mcp_config_snippet(mcp, inputs.include_secrets).ok_or_else(|| {
        ActionError::UnknownTransport {
            entry: mcp.entry.clone(),
        }
    })?;
    let label = match (inputs.include_secrets, snippet.redacted) {
        (true, _) => "MCP configuration (includes secrets)",
        (false, true) => "MCP configuration (secrets redacted)",
        (false, false) => "MCP configuration",
    };
    Ok(clipboard(label, snippet.text))
}

fn primary_text(record: &ResourceRecord) -> Result<(String, String), ActionError> {
    let path = record
        .details
        .primary_file()
        .ok_or(ActionError::NoLocalPath)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| record.name.clone());
    Ok((
        format!("{name} contents"),
        read_text_bounded(path, MAX_COPY_BYTES)?,
    ))
}

fn expand(record: &ResourceRecord, inputs: &ActionInputs) -> Result<String, ActionError> {
    let ResourceDetails::Prompt(prompt) = &record.details else {
        return Err(ActionError::NotOffered {
            id: ActionId::ExpandTemplate,
            kind: record.kind(),
        });
    };
    let text = read_text_bounded(&prompt.file, MAX_COPY_BYTES)?;
    Ok(expand_template(
        &text,
        prompt.syntax,
        &inputs.template_values,
        &prompt.variables,
    )?)
}

/// What insertion would send: the composed query when one was typed,
/// otherwise the expanded prompt / prompt text, or the invocation line.
fn insert_payload(
    record: &ResourceRecord,
    inputs: &ActionInputs,
) -> Result<(String, String), ActionError> {
    if let Some(query) = &inputs.compose_text {
        return Ok((
            "composed query".to_owned(),
            compose_payload(&[record], query, ComposeFormat::Generic),
        ));
    }
    match &record.details {
        ResourceDetails::Prompt(_) if is_expandable_prompt(record) => {
            Ok(("expanded prompt".to_owned(), expand(record, inputs)?))
        }
        ResourceDetails::Prompt(prompt) => {
            let text = read_text_bounded(&prompt.file, MAX_COPY_BYTES)?;
            Ok(("prompt".to_owned(), strip_frontmatter(&text).to_owned()))
        }
        _ => Ok(("invocation".to_owned(), invocation_line(record))),
    }
}

/// Read a regular file as UTF-8, refusing anything larger than `limit`.
/// Opening is the only operation; nothing is executed.
pub fn read_text_bounded(path: &Path, limit: u64) -> Result<String, ActionError> {
    let io = |error: std::io::Error| ActionError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    };
    let metadata = fs::metadata(path).map_err(io)?;
    if !metadata.is_file() {
        return Err(ActionError::Io {
            path: path.to_path_buf(),
            message: "not a regular file".to_owned(),
        });
    }
    let too_large = |size| ActionError::TooLarge {
        path: path.to_path_buf(),
        size,
        limit,
    };
    if metadata.len() > limit {
        return Err(too_large(metadata.len()));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > limit {
        return Err(too_large(bytes.len() as u64));
    }
    String::from_utf8(bytes).map_err(|_| ActionError::NotText {
        path: path.to_path_buf(),
    })
}
