//! Instruction files (`AGENTS.md`, `CLAUDE.md`, ...) and rule directories
//! (`.cursor/rules/*.mdc`).

use std::path::Path;

use perkolator_core::{
    ResourceDetails, ResourceId, ResourceKind, RuleDetails, RuleFormat, Scope, ValidationIssue,
};
use serde_json::Value;

use crate::ScanRoot;
use crate::files::{self, canonical, file_name};
use crate::frontmatter::{self, Fields};
use crate::scan::{RecordDraft, RecordMeta, Scanner, build_record, file_locations, key_for};
use crate::text;

pub(crate) fn scan_file(
    scanner: &mut Scanner,
    root: &ScanRoot,
    host: Option<&str>,
    format: RuleFormat,
    scope: &Scope,
) {
    if !root.path.is_file() {
        return scanner.problem(&root.path, "rule path is not a file");
    }
    let name = rule_file_name(&root.path, format, scope, host);
    add_rule(scanner, root, &root.path, name, format, host, scope);
}

pub(crate) fn scan_directory(
    scanner: &mut Scanner,
    root: &ScanRoot,
    host: Option<&str>,
    scope: &Scope,
) {
    for found in scanner.document_files(&root.path, &["mdc", "md"], host) {
        if scanner.cancel.is_cancelled() {
            return;
        }
        let format = match found.extension().and_then(|extension| extension.to_str()) {
            Some("mdc") => RuleFormat::CursorMdc,
            _ => RuleFormat::Generic,
        };
        let name = found
            .strip_prefix(&root.path)
            .unwrap_or(&found)
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        add_rule(scanner, root, &found, name, format, host, scope);
    }
}

fn add_rule(
    scanner: &mut Scanner,
    root: &ScanRoot,
    found: &Path,
    name: String,
    format: RuleFormat,
    host: Option<&str>,
    scope: &Scope,
) {
    let contents = match files::read_text(found, scanner.limits) {
        Ok(contents) => contents,
        Err(message) => return scanner.problem(found, message),
    };
    let mut issues = Vec::new();
    let (document, lenient) = frontmatter::parse_lenient(&contents);
    if lenient {
        issues.push(ValidationIssue {
            severity: perkolator_core::IssueSeverity::Info,
            message: "frontmatter is not strict YAML; read as `key: value` lines".into(),
        });
    }
    let fields = match document.fields {
        Some(Ok(fields)) => fields,
        Some(Err(message)) => {
            scanner.problem(found, message.clone());
            issues.push(ValidationIssue::error(message));
            Fields::new()
        }
        None => Fields::new(),
    };
    let description = frontmatter::string(&fields, "description");
    let canonical_file = canonical(found);
    let details = RuleDetails {
        file: found.to_path_buf(),
        format,
        host: host.map(str::to_owned),
        scope: scope.clone(),
        description: description.clone(),
        always_apply: always_apply(&fields),
        globs: frontmatter::string_list(fields.get("globs")),
        precedence: None,
    };
    let record = build_record(
        RecordDraft {
            id: ResourceId::new(
                ResourceKind::Rule,
                root.provider.clone(),
                key_for(&canonical_file),
            ),
            name,
            summary: text::summary(description.as_deref(), document.body),
            details: ResourceDetails::Rule(details),
            source: root.source.clone(),
            locations: file_locations(found, &canonical_file, host, scope),
        },
        RecordMeta {
            fingerprint: Some(text::fingerprint(contents.as_bytes())),
            issues,
            ..RecordMeta::default()
        },
    );
    scanner.upsert(Some(&canonical_file), record);
}

fn always_apply(fields: &Fields) -> Option<bool> {
    match fields.get("alwaysApply") {
        Some(Value::Bool(flag)) => Some(*flag),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    }
}

/// Human-meaningful name: `AGENTS.md — demo-app`, `CLAUDE.md (global)`,
/// `AGENTS.md (global, codex)`.
fn rule_file_name(path: &Path, format: RuleFormat, scope: &Scope, host: Option<&str>) -> String {
    let file = file_name(path);
    let host_implied_by_file = matches!(
        format,
        RuleFormat::ClaudeMd
            | RuleFormat::GeminiMd
            | RuleFormat::CopilotInstructions
            | RuleFormat::CursorMdc
    );
    match scope {
        Scope::Global => match host {
            Some(host) if !host_implied_by_file => format!("{file} (global, {host})"),
            _ => format!("{file} (global)"),
        },
        Scope::Project { root } => format!("{file} — {}", file_name(root)),
        Scope::Custom { path } => format!("{file} — {}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn names_are_human_meaningful() {
        let project = Scope::Project {
            root: PathBuf::from("/w/demo-app"),
        };
        assert_eq!(
            rule_file_name(
                Path::new("/w/demo-app/AGENTS.md"),
                RuleFormat::AgentsMd,
                &project,
                None
            ),
            "AGENTS.md — demo-app"
        );
        assert_eq!(
            rule_file_name(
                Path::new("/h/.claude/CLAUDE.md"),
                RuleFormat::ClaudeMd,
                &Scope::Global,
                Some("claude-code")
            ),
            "CLAUDE.md (global)"
        );
        assert_eq!(
            rule_file_name(
                Path::new("/h/.codex/AGENTS.md"),
                RuleFormat::AgentsMd,
                &Scope::Global,
                Some("codex")
            ),
            "AGENTS.md (global, codex)"
        );
    }
}
