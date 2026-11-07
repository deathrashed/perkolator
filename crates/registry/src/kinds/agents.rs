//! Agent profiles / subagent definitions: `*.md` files with frontmatter.

use perkolator_core::{
    AgentProfileDetails, ResourceDetails, ResourceId, ResourceKind, ValidationIssue,
};
use serde_json::Value;

use crate::ScanRoot;
use crate::files::{self, canonical, file_stem};
use crate::frontmatter::{self, Fields};
use crate::scan::{
    RecordDraft, RecordMeta, Scanner, build_record, file_locations, key_for, scope_for,
};
use crate::text;

pub(crate) fn scan(scanner: &mut Scanner, root: &ScanRoot, host: &str) {
    let scope = scope_for(&root.source);
    for found in scanner.document_files(&root.path, &["md"], Some(host)) {
        if scanner.cancel.is_cancelled() {
            return;
        }
        let contents = match files::read_text(&found, scanner.limits) {
            Ok(contents) => contents,
            Err(message) => {
                scanner.problem(&found, message);
                continue;
            }
        };
        let mut issues = Vec::new();
        let document = frontmatter::parse(&contents);
        let fields = match document.fields {
            Some(Ok(fields)) => fields,
            Some(Err(message)) => {
                scanner.problem(&found, message.clone());
                issues.push(ValidationIssue::error(message));
                Fields::new()
            }
            None => {
                issues.push(ValidationIssue::warning(
                    "no frontmatter; name taken from the file name",
                ));
                Fields::new()
            }
        };
        let description = frontmatter::string(&fields, "description");
        if description.is_none() {
            issues.push(ValidationIssue::warning("missing `description`"));
        }
        let canonical_file = canonical(&found);
        let details = AgentProfileDetails {
            host: Some(host.to_owned()),
            file: found.clone(),
            model: frontmatter::string(&fields, "model"),
            tools: frontmatter::string_list(fields.get("tools")),
            skills: frontmatter::string_list(fields.get("skills")),
            mcp_servers: mcp_server_names(fields.get("mcpServers")),
        };
        let record = build_record(
            RecordDraft {
                id: ResourceId::new(
                    ResourceKind::Agent,
                    root.provider.clone(),
                    key_for(&canonical_file),
                ),
                name: frontmatter::string(&fields, "name").unwrap_or_else(|| file_stem(&found)),
                summary: text::summary(description.as_deref(), document.body),
                details: ResourceDetails::Agent(details),
                source: root.source.clone(),
                locations: file_locations(&found, &canonical_file, Some(host), &scope),
            },
            RecordMeta {
                tags: frontmatter::string_list(fields.get("tags")),
                fingerprint: Some(text::fingerprint(contents.as_bytes())),
                issues,
                ..RecordMeta::default()
            },
        );
        scanner.upsert(Some(&canonical_file), record);
    }
}

/// `mcpServers` as a list, comma string, or object keyed by server name.
fn mcp_server_names(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        other => frontmatter::string_list(other),
    }
}
