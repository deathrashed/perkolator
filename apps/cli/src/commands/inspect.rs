//! Per-resource inspection: show, actions, copy.

use std::fmt::Write as _;

use perkolator_actions::{ActionContext, ActionInputs, Surface, actions_for, resolve};
use perkolator_core::{ActionAvailability, ActionEffect, ActionId, CancelToken, ResourceRecord};
use perkolator_preview::{Document, PreviewLimits, PreviewTab, build_preview};
use serde_json::json;

use crate::env::Env;
use crate::lookup;
use crate::output::{first_lines, role_label, slug, source_label, truncate};
use crate::{CliError, CommandOutput};

const PREVIEW_LINES: usize = 3;
const PREVIEW_WIDTH: usize = 100;
const PREVIEW_ITEMS: usize = 6;

/// The CLI never touches a clipboard or a session; platform features stay
/// unprobed so copy actions resolve to text and insertion stays disabled.
fn cli_context() -> ActionContext {
    ActionContext::new(Surface::Cli)
}

pub fn show(env: &Env, query: &str) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let record = lookup::find(&scanned.registry, query)?;
    let mut out = String::new();
    let _ = writeln!(out, "{} — {}", record.name, record.kind().label());
    let _ = writeln!(out, "id:        {}", record.id);
    let _ = writeln!(out, "provider:  {}", record.id.provider);
    let _ = writeln!(out, "source:    {}", source_label(record));
    if !record.summary.is_empty() {
        let _ = writeln!(
            out,
            "summary:   {}",
            truncate(&record.summary, PREVIEW_WIDTH)
        );
    }
    if let Some(version) = &record.version {
        let _ = writeln!(out, "version:   {version}");
    }
    if let Some(publisher) = &record.publisher {
        let _ = writeln!(out, "publisher: {publisher}");
    }
    if !record.tags.is_empty() {
        let _ = writeln!(out, "tags:      {}", record.tags.join(", "));
    }
    if let Some(url) = &record.provenance.origin_url {
        let _ = writeln!(out, "origin:    {url}");
    }
    if let Some(fingerprint) = &record.provenance.fingerprint {
        let _ = writeln!(out, "hash:      {fingerprint}");
    }

    let _ = writeln!(out, "\nLocations:");
    for location in &record.locations {
        let _ = writeln!(
            out,
            "  {:<9} {:<40} {}",
            location.state.label(),
            role_label(&location.role),
            location.path.display()
        );
    }
    let capabilities: Vec<String> = record.capabilities.iter().map(|c| slug(json!(c))).collect();
    let _ = writeln!(out, "\nCapabilities: {}", capabilities.join(", "));
    let _ = writeln!(out, "\nIssues: {}", record.issues.len());
    for issue in &record.issues {
        let _ = writeln!(out, "  {}: {}", slug(json!(issue.severity)), issue.message);
    }

    let _ = writeln!(out, "\nPreview:");
    match build_preview(record, 0, &PreviewLimits::default(), &CancelToken::new()) {
        Ok(preview) => {
            for tab in &preview.tabs {
                write_tab(&mut out, tab);
            }
        }
        Err(error) => {
            let _ = writeln!(out, "  unavailable: {error}");
        }
    }
    Ok(CommandOutput::ok(out))
}

fn write_tab(out: &mut String, tab: &PreviewTab) {
    let (title, lines): (String, Vec<String>) = match tab {
        PreviewTab::Overview(document) => ("Overview".to_owned(), document_lines(document)),
        PreviewTab::Files(tree) => (
            format!(
                "Files ({} entries{})",
                tree.nodes.len(),
                if tree.truncated { ", truncated" } else { "" }
            ),
            tree.nodes
                .iter()
                .take(PREVIEW_ITEMS)
                .map(|node| {
                    format!(
                        "{}{}{}",
                        "  ".repeat(node.depth),
                        node.name,
                        if node.is_dir { "/" } else { "" }
                    )
                })
                .collect(),
        ),
        PreviewTab::Config { format, text } => (
            format!("Config ({format}, redacted)"),
            first_lines(text, PREVIEW_ITEMS, PREVIEW_WIDTH),
        ),
        PreviewTab::Tools(tools) => (
            format!("Tools ({})", tools.len()),
            tools
                .iter()
                .take(PREVIEW_ITEMS)
                .map(|tool| tool.name.clone())
                .collect(),
        ),
        PreviewTab::Template { variables, raw } => {
            let mut lines = vec![format!(
                "variables: {}",
                if variables.is_empty() {
                    "-".to_owned()
                } else {
                    variables
                        .iter()
                        .map(|variable| variable.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            )];
            lines.extend(first_lines(raw, PREVIEW_LINES, PREVIEW_WIDTH));
            ("Template".to_owned(), lines)
        }
        PreviewTab::Precedence(pairs) => ("Precedence".to_owned(), pair_lines(pairs)),
        PreviewTab::Permissions {
            permissions,
            executable_files,
            content_hash,
        } => {
            let mut lines = vec![format!(
                "permissions: {}",
                if permissions.is_empty() {
                    "-".to_owned()
                } else {
                    permissions.join(", ")
                }
            )];
            lines.push(format!(
                "executable files: {}",
                if executable_files.is_empty() {
                    "-".to_owned()
                } else {
                    executable_files
                        .iter()
                        .map(|file| file.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ));
            if let Some(hash) = content_hash {
                lines.push(format!("content hash: {hash}"));
            }
            ("Permissions".to_owned(), lines)
        }
        PreviewTab::Metadata(pairs) => ("Metadata".to_owned(), pair_lines(pairs)),
    };
    let _ = writeln!(out, "  [{title}]");
    for line in lines {
        let _ = writeln!(out, "    {line}");
    }
}

fn document_lines(document: &Document) -> Vec<String> {
    match document {
        Document::Markdown { .. } | Document::Code { .. } | Document::PlainText(_) => first_lines(
            document.text().unwrap_or_default(),
            PREVIEW_LINES,
            PREVIEW_WIDTH,
        ),
        Document::Image { path } => vec![format!("image {}", path.display())],
        Document::Binary { size } => vec![format!("binary file, {size} bytes")],
        Document::TooLarge { size, limit } => {
            vec![format!("too large to preview ({size} > {limit} bytes)")]
        }
        Document::Unavailable { reason } => vec![format!("unavailable: {reason}")],
    }
}

fn pair_lines(pairs: &[(String, String)]) -> Vec<String> {
    pairs
        .iter()
        .take(PREVIEW_ITEMS)
        .map(|(key, value)| truncate(&format!("{key}: {value}"), PREVIEW_WIDTH))
        .collect()
}

pub fn actions(env: &Env, query: &str) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let record = lookup::find(&scanned.registry, query)?;
    let mut out = String::new();
    let _ = writeln!(out, "Actions for {} ({}):", record.name, record.id);
    for entry in actions_for(record, &cli_context()) {
        let hint = entry
            .shortcut_hint
            .map(|hint| format!(" [{hint}]"))
            .unwrap_or_default();
        let state = match &entry.availability {
            ActionAvailability::Available => String::new(),
            ActionAvailability::Disabled { reason } => format!("  (disabled: {reason})"),
        };
        let _ = writeln!(
            out,
            "  {:<22} {}{hint}{state}",
            slug(json!(entry.id)),
            entry.label
        );
    }
    Ok(CommandOutput::ok(out))
}

/// Prints the exact text an action would put on the clipboard. Secrets stay
/// redacted: `include_secrets` is never set from the CLI.
pub fn copy(env: &Env, query: &str, action: &str) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let record = lookup::find(&scanned.registry, query)?;
    let ctx = cli_context();
    let id = parse_action(action, record, &ctx)?;
    let effect = resolve(id, record, &ctx, &ActionInputs::default()).map_err(CliError::failed)?;
    match effect {
        ActionEffect::WriteClipboard(payload) => {
            let mut text = payload.text;
            if !text.ends_with('\n') {
                text.push('\n');
            }
            Ok(CommandOutput::ok(text))
        }
        other => Err(CliError::usage(format!(
            "`{action}` does not produce text ({}); try `perk show` or `perk deploy`",
            effect_label(&other)
        ))),
    }
}

fn parse_action(
    action: &str,
    record: &ResourceRecord,
    ctx: &ActionContext,
) -> Result<ActionId, CliError> {
    serde_json::from_value::<ActionId>(json!(action)).map_err(|_| {
        let known: Vec<String> = actions_for(record, ctx)
            .iter()
            .map(|entry| slug(json!(entry.id)))
            .collect();
        CliError::usage(format!(
            "unknown action `{action}`; actions for {}: {}",
            record.name,
            known.join(", ")
        ))
    })
}

fn effect_label(effect: &ActionEffect) -> &'static str {
    match effect {
        ActionEffect::WriteClipboard(_) => "clipboard text",
        ActionEffect::Reveal { .. } => "reveals a path in the file manager",
        ActionEffect::OpenUrl { .. } => "opens a URL",
        ActionEffect::OpenDetails { .. } => "opens the details view",
        ActionEffect::EnterCompose { .. } => "enters the composer",
        ActionEffect::OpenDeployPlanner { .. } => "opens the deployment planner",
    }
}
