//! Agent Skills: directories containing `SKILL.md`.

use std::fs;
use std::path::Path;

use perkolator_core::{
    InstallState, LocationRole, ProviderId, ResourceDetails, ResourceId, ResourceKind,
    ResourceLocation, Scope, SkillDetails, SourceKind, ValidationIssue,
};

use crate::ScanRoot;
use crate::files::{self, canonical, file_name, is_skipped_dir, sorted_entries};
use crate::frontmatter::{self, Fields};
use crate::scan::{
    PendingSkillLink, RecordDraft, RecordMeta, Scanner, build_record, key_for, scope_for,
};
use crate::text;

const ENTRY_FILE: &str = "SKILL.md";
const MAX_NAME_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 1024;

fn has_entry(dir: &Path) -> bool {
    dir.join(ENTRY_FILE).is_file()
}

fn hidden_or_skipped(name: &str) -> bool {
    name.starts_with('.') || is_skipped_dir(name)
}

pub(crate) fn scan_library(scanner: &mut Scanner, root: &ScanRoot) {
    if has_entry(&root.path) {
        add_library_skill(scanner, root, &root.path, Vec::new());
        return;
    }
    let mut category = Vec::new();
    walk_library(scanner, root, &root.path, &mut category);
}

fn walk_library(scanner: &mut Scanner, root: &ScanRoot, dir: &Path, category: &mut Vec<String>) {
    for entry in sorted_entries(dir, &mut scanner.problems) {
        if scanner.cancel.is_cancelled() {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if hidden_or_skipped(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            match fs::metadata(&path) {
                Err(_) => scanner.broken_link(&path, None),
                // Symlinked category folders are not followed (loop safety).
                Ok(metadata) if metadata.is_dir() && has_entry(&path) => {
                    add_library_skill(scanner, root, &path, category.clone());
                }
                Ok(_) => {}
            }
        } else if file_type.is_dir() {
            if has_entry(&path) {
                add_library_skill(scanner, root, &path, category.clone());
            } else if category.len() + 1 < scanner.limits.max_depth {
                category.push(name);
                walk_library(scanner, root, &path, category);
                category.pop();
            }
        }
    }
}

fn add_library_skill(scanner: &mut Scanner, root: &ScanRoot, found: &Path, category: Vec<String>) {
    let canonical_dir = canonical(found);
    let location = ResourceLocation {
        path: canonical_dir.clone(),
        role: LocationRole::Canonical,
        state: InstallState::Installed,
    };
    let record = skill_record(
        scanner,
        found,
        &canonical_dir,
        category,
        &root.provider,
        root.source.clone(),
        vec![location],
    );
    scanner.upsert(Some(&canonical_dir), record);
}

/// Flat agent skills directory: real directories are unmanaged deployments,
/// symlinks are resolved after all roots are scanned.
pub(crate) fn scan_directory(scanner: &mut Scanner, root: &ScanRoot, host: &str) {
    let scope = scope_for(&root.source);
    for entry in sorted_entries(&root.path, &mut scanner.problems) {
        if scanner.cancel.is_cancelled() {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if hidden_or_skipped(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            match fs::metadata(&path) {
                Err(_) => scanner.broken_link(&path, Some(host)),
                Ok(metadata) if metadata.is_dir() => {
                    let target = canonical(&path);
                    if has_entry(&target) {
                        scanner.pending_links.push(PendingSkillLink {
                            link: path,
                            target,
                            host: host.to_owned(),
                            scope: scope.clone(),
                            provider: root.provider.clone(),
                            source: root.source.clone(),
                        });
                    } else {
                        scanner.problem(&path, "linked directory has no SKILL.md");
                    }
                }
                Ok(_) => {}
            }
        } else if file_type.is_dir() && has_entry(&path) {
            let canonical_dir = canonical(&path);
            let location = ResourceLocation {
                path: path.clone(),
                role: deployed(host, &scope),
                state: InstallState::Unmanaged,
            };
            let record = skill_record(
                scanner,
                &path,
                &canonical_dir,
                Vec::new(),
                &root.provider,
                root.source.clone(),
                vec![location],
            );
            scanner.upsert(Some(&canonical_dir), record);
        }
    }
}

fn deployed(host: &str, scope: &Scope) -> LocationRole {
    LocationRole::Deployed {
        target: host.to_owned(),
        scope: scope.clone(),
    }
}

/// Attach symlinked deployments to the record of their resolved target, or
/// create a record for targets outside every scanned root.
pub(crate) fn resolve_links(scanner: &mut Scanner) {
    for link in std::mem::take(&mut scanner.pending_links) {
        if scanner.cancel.is_cancelled() {
            return;
        }
        let linked = ResourceLocation {
            path: link.link.clone(),
            role: deployed(&link.host, &link.scope),
            state: InstallState::Linked,
        };
        if let Some(id) = scanner.known(ResourceKind::Skill, &link.target) {
            scanner.registry.add_location(&id, linked);
            continue;
        }
        let canonical_location = ResourceLocation {
            path: link.target.clone(),
            role: LocationRole::Canonical,
            state: InstallState::Installed,
        };
        let locations = vec![canonical_location, linked];
        let record = skill_record(
            scanner,
            &link.link,
            &link.target,
            Vec::new(),
            &link.provider,
            link.source.clone(),
            locations,
        );
        scanner.upsert(Some(&link.target), record);
    }
}

fn skill_record(
    scanner: &mut Scanner,
    found: &Path,
    canonical_dir: &Path,
    category_path: Vec<String>,
    provider: &ProviderId,
    source: SourceKind,
    locations: Vec<ResourceLocation>,
) -> perkolator_core::ResourceRecord {
    let entry = found.join(ENTRY_FILE);
    let mut issues = Vec::new();
    let mut fingerprint = None;
    let text = match files::read_text(&entry, scanner.limits) {
        Ok(text) => {
            fingerprint = Some(text::fingerprint(text.as_bytes()));
            text
        }
        Err(message) => {
            issues.push(ValidationIssue::error(format!(
                "SKILL.md could not be read: {message}"
            )));
            scanner.problem(&entry, message);
            String::new()
        }
    };
    let document = frontmatter::parse(&text);
    // Missing-field errors only make sense once a frontmatter block parsed.
    let parsed = matches!(document.fields, Some(Ok(_)));
    let fields = match document.fields {
        Some(Ok(fields)) => fields,
        Some(Err(message)) => {
            scanner.problem(&entry, message.clone());
            issues.push(ValidationIssue::error(message));
            Fields::new()
        }
        None => {
            if fingerprint.is_some() {
                issues.push(ValidationIssue::error(
                    "SKILL.md has no YAML frontmatter; `name` and `description` are required",
                ));
            }
            Fields::new()
        }
    };

    let dir_name = file_name(canonical_dir);
    let declared_name = frontmatter::string(&fields, "name");
    match &declared_name {
        None if parsed => {
            issues.push(ValidationIssue::error("missing required `name`"));
        }
        None => {}
        Some(name) => {
            if let Some(message) = invalid_name(name) {
                issues.push(ValidationIssue::error(message));
            }
            if *name != dir_name {
                issues.push(ValidationIssue::warning(format!(
                    "`name` ({name}) does not match the directory name ({dir_name})"
                )));
            }
        }
    }
    let description = frontmatter::string(&fields, "description");
    match &description {
        None if parsed => {
            issues.push(ValidationIssue::error("missing required `description`"));
        }
        Some(description) if description.chars().count() > MAX_DESCRIPTION_CHARS => {
            issues.push(ValidationIssue::error(format!(
                "`description` exceeds {MAX_DESCRIPTION_CHARS} characters"
            )));
        }
        _ => {}
    }

    let inventory = files::inventory(found, scanner.limits, scanner.cancel, &mut scanner.problems);
    if inventory.truncated {
        issues.push(ValidationIssue::warning(format!(
            "file inventory truncated at {} entries",
            scanner.limits.max_files_per_resource
        )));
    }

    let meta = RecordMeta {
        publisher: frontmatter::author(frontmatter::field_or_metadata(&fields, "author")),
        tags: frontmatter::string_list(frontmatter::field_or_metadata(&fields, "tags")),
        version: frontmatter::field_or_metadata(&fields, "version")
            .and_then(frontmatter::value_string),
        origin_url: ["source", "repository", "homepage"]
            .iter()
            .find_map(|key| frontmatter::http_url(frontmatter::field_or_metadata(&fields, key))),
        fingerprint,
        issues,
    };
    let summary = text::summary(description.as_deref(), document.body);
    let name = declared_name
        .filter(|name| invalid_name(name).is_none())
        .unwrap_or_else(|| dir_name.clone());
    let details = SkillDetails {
        root: found.to_path_buf(),
        entry,
        category_path,
        frontmatter: fields,
        files: inventory.files,
    };
    build_record(
        RecordDraft {
            id: ResourceId::new(
                ResourceKind::Skill,
                provider.clone(),
                key_for(canonical_dir),
            ),
            name,
            summary,
            details: ResourceDetails::Skill(details),
            source,
            locations,
        },
        meta,
    )
}

/// Agent Skills `name` rules: 1-64 chars of lowercase letters, digits and
/// single hyphens, not starting or ending with a hyphen.
fn invalid_name(name: &str) -> Option<String> {
    if name.chars().count() > MAX_NAME_CHARS {
        return Some(format!("`name` exceeds {MAX_NAME_CHARS} characters"));
    }
    let allowed = name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !allowed {
        return Some("`name` may only contain lowercase letters, digits and hyphens".into());
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Some("`name` must not start/end with a hyphen or contain `--`".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_rules() {
        assert!(invalid_name("frontend-design").is_none());
        assert!(invalid_name("Frontend").is_some());
        assert!(invalid_name("a--b").is_some());
        assert!(invalid_name("-a").is_some());
        assert!(invalid_name(&"a".repeat(65)).is_some());
    }
}
