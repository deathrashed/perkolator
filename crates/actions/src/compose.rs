//! Adapter-neutral reference text and the composed query payload.
//!
//! The text produced here is exactly what is copied or sent; the UI previews
//! it verbatim. It never assumes host-specific syntax such as slash commands
//! or `@` mentions, which unknown hosts may not support.

use std::path::Path;

use perkolator_core::{ResourceDetails, ResourceId, ResourceRecord};
use serde::{Deserialize, Serialize};

use crate::registry::has_local_path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComposeFormat {
    /// One instruction sentence per resource, then the query.
    #[default]
    Generic,
    /// One bare path per resource, then the query.
    PathReferences,
}

/// The file an agent should read for this resource, if it has a real path.
fn reference_path(record: &ResourceRecord) -> Option<&Path> {
    if !has_local_path(record) {
        return None;
    }
    match &record.details {
        // Config entries are referenced by name, not by the shared config file.
        ResourceDetails::McpServer(_) => None,
        details => details.primary_file().map(|path| path.as_path()),
    }
}

/// Imperative, adapter-neutral instruction for one resource, e.g.
/// ``Use the `pdf` skill: /lib/pdf/SKILL.md``.
pub fn invocation_line(record: &ResourceRecord) -> String {
    let name = &record.name;
    let subject = match &record.details {
        ResourceDetails::Skill(_) => format!("Use the `{name}` skill"),
        ResourceDetails::Agent(_) => format!("Use the `{name}` agent profile"),
        ResourceDetails::Prompt(_) => format!("Follow the `{name}` prompt"),
        ResourceDetails::Rule(_) => format!("Apply the `{name}` rule"),
        ResourceDetails::Plugin(_) => format!("Use the `{name}` plugin"),
        ResourceDetails::McpServer(mcp) => {
            return format!(
                "Use the `{}` MCP server (configured in {}).",
                mcp.entry, mcp.client
            );
        }
    };
    match reference_path(record) {
        Some(path) => format!("{subject}: {}", path.display()),
        None => format!("{subject}."),
    }
}

/// Short label + path, e.g. ``Skill `pdf`: /lib/pdf/SKILL.md``.
pub fn reference_text(record: &ResourceRecord) -> String {
    let head = format!("{} `{}`", record.kind().label(), record.name);
    match reference_path(record) {
        Some(path) => format!("{head}: {}", path.display()),
        None => head,
    }
}

/// Build the exact text for a composed query referencing `resources` in the
/// given order (duplicates removed, first occurrence wins). An empty query
/// yields only the resource lines.
pub fn compose_payload(
    resources: &[&ResourceRecord],
    query: &str,
    format: ComposeFormat,
) -> String {
    let mut seen: Vec<&ResourceId> = Vec::with_capacity(resources.len());
    let mut lines: Vec<String> = Vec::with_capacity(resources.len());
    for record in resources {
        if seen.contains(&&record.id) {
            continue;
        }
        seen.push(&record.id);
        lines.push(match format {
            ComposeFormat::Generic => invocation_line(record),
            ComposeFormat::PathReferences => reference_path(record)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| reference_text(record)),
        });
    }
    let query = query.trim();
    let mut payload = lines.join("\n");
    if !query.is_empty() {
        if !payload.is_empty() {
            payload.push_str("\n\n");
        }
        payload.push_str(query);
    }
    payload
}
