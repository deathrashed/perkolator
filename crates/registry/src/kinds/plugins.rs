//! Plugins: directories holding a plugin manifest. Contents are inventoried
//! and hashed, never executed.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use perkolator_core::{
    FileKind, InstallState, LocationRole, PluginComponent, PluginDetails, PluginHost, RelPath,
    ResourceDetails, ResourceFile, ResourceId, ResourceKind, ResourceLocation, ValidationIssue,
};
use serde_json::Value;

use crate::ScanRoot;
use crate::files::{self, canonical, could_execute, file_name, is_skipped_dir, sorted_entries};
use crate::frontmatter;
use crate::scan::{RecordDraft, RecordMeta, Scanner, build_record, key_for};
use crate::{jsonc, text};

/// Manifest locations, in lookup order.
const MANIFESTS: [&str; 3] = [
    ".cursor-plugin/plugin.json",
    ".claude-plugin/plugin.json",
    "perkolator-plugin.json",
];
const PERKOLATOR_MANIFEST: &str = "perkolator-plugin.json";

/// How deep below a plugins root to look for manifests (covers
/// `cache/<marketplace>/<plugin>/<version>/`).
const PLUGIN_SEARCH_DEPTH: usize = 4;

/// Component keys and the conventional path used when undeclared.
const COMPONENTS: [(&str, &str); 6] = [
    ("skills", "skills"),
    ("agents", "agents"),
    ("commands", "commands"),
    ("rules", "rules"),
    ("hooks", "hooks/hooks.json"),
    ("mcpServers", ".mcp.json"),
];
const INLINE_COMPONENT: &str = "(inline in manifest)";
const HASH_DOMAIN: &[u8] = b"perkolator-plugin-hash-v1\0";

fn manifest_in(dir: &Path) -> Option<PathBuf> {
    MANIFESTS
        .iter()
        .map(|relative| dir.join(relative))
        .find(|path| path.is_file())
}

pub(crate) fn scan(scanner: &mut Scanner, root: &ScanRoot, host: &PluginHost) {
    if let Some(manifest) = manifest_in(&root.path) {
        add_plugin(scanner, root, host, &root.path, &manifest);
        return;
    }
    search(scanner, root, host, &root.path, 0);
}

fn search(scanner: &mut Scanner, root: &ScanRoot, host: &PluginHost, dir: &Path, depth: usize) {
    let host_name = match host {
        PluginHost::Perkolator => None,
        PluginHost::External { name } => Some(name.as_str()),
    };
    for entry in sorted_entries(dir, &mut scanner.problems) {
        if scanner.cancel.is_cancelled() {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || is_skipped_dir(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            // Linked plugin checkouts are common; follow one level only.
            match fs::metadata(&path) {
                Err(_) => scanner.broken_link(&path, host_name),
                Ok(metadata) if metadata.is_dir() => {
                    if let Some(manifest) = manifest_in(&path) {
                        add_plugin(scanner, root, host, &path, &manifest);
                    }
                }
                Ok(_) => {}
            }
        } else if file_type.is_dir() {
            if let Some(manifest) = manifest_in(&path) {
                add_plugin(scanner, root, host, &path, &manifest);
            } else if depth + 1 < PLUGIN_SEARCH_DEPTH.min(scanner.limits.max_depth) {
                search(scanner, root, host, &path, depth + 1);
            }
        }
    }
}

fn add_plugin(
    scanner: &mut Scanner,
    root: &ScanRoot,
    layout_host: &PluginHost,
    found: &Path,
    manifest: &Path,
) {
    let mut issues = Vec::new();
    let manifest_bytes = match files::read_bytes(manifest, scanner.limits.max_file_bytes) {
        Ok(bytes) => bytes,
        Err(message) => return scanner.problem(manifest, message),
    };
    let value = match std::str::from_utf8(&manifest_bytes)
        .map_err(|_| "manifest is not valid UTF-8".to_owned())
        .and_then(jsonc::parse)
    {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(_) => {
            issues.push(ValidationIssue::error("manifest is not a JSON object"));
            Value::Null
        }
        Err(message) => {
            let message = format!("invalid plugin manifest: {message}");
            scanner.problem(manifest, message.clone());
            issues.push(ValidationIssue::error(message));
            Value::Null
        }
    };
    let string = |key: &str| value.get(key).and_then(frontmatter::value_string);
    let name = string("name").unwrap_or_else(|| {
        issues.push(ValidationIssue::warning(
            "manifest has no `name`; using the directory name",
        ));
        file_name(found)
    });
    let description = string("description");

    let inventory = files::inventory(found, scanner.limits, scanner.cancel, &mut scanner.problems);
    if inventory.truncated {
        issues.push(ValidationIssue::warning(format!(
            "file inventory truncated at {} entries; hash may not cover every executable",
            scanner.limits.max_files_per_resource
        )));
    }
    let mut executable: Vec<&ResourceFile> = inventory
        .files
        .iter()
        .filter(|file| could_execute(file))
        .collect();
    executable.sort_by(|a, b| a.path.cmp(&b.path));
    let content_hash = hash_contents(
        found,
        &manifest_bytes,
        &executable,
        scanner.limits.max_hash_bytes_per_file,
        &mut issues,
    );
    let executable_files: Vec<RelPath> = executable.iter().map(|file| file.path.clone()).collect();

    let host = if manifest
        .file_name()
        .is_some_and(|name| name == PERKOLATOR_MANIFEST)
    {
        PluginHost::Perkolator
    } else {
        layout_host.clone()
    };
    let canonical_root = canonical(found);
    let details = PluginDetails {
        manifest: manifest.to_path_buf(),
        root: found.to_path_buf(),
        host,
        components: components(&value, found),
        permissions: permissions(value.get("permissions")),
        executable_files,
        content_hash: Some(content_hash.clone()),
        files: inventory.files,
    };
    let record = build_record(
        RecordDraft {
            id: ResourceId::new(
                ResourceKind::Plugin,
                root.provider.clone(),
                key_for(&canonical_root),
            ),
            name,
            summary: text::summary(description.as_deref(), ""),
            details: ResourceDetails::Plugin(details),
            source: root.source.clone(),
            locations: vec![ResourceLocation {
                path: canonical_root.clone(),
                role: LocationRole::Canonical,
                state: InstallState::Installed,
            }],
        },
        RecordMeta {
            publisher: frontmatter::author(value.get("author")),
            tags: frontmatter::string_list(value.get("keywords")),
            version: string("version"),
            origin_url: frontmatter::http_url(value.get("repository"))
                .or_else(|| frontmatter::http_url(value.get("homepage"))),
            fingerprint: Some(content_hash),
            issues,
        },
    );
    scanner.upsert(Some(&canonical_root), record);
}

fn components(manifest: &Value, root: &Path) -> Vec<PluginComponent> {
    let mut components = Vec::new();
    for (kind, conventional) in COMPONENTS {
        let component = |path: &str| PluginComponent {
            kind: kind.to_owned(),
            path: path.to_owned(),
        };
        match manifest.get(kind) {
            Some(Value::String(path)) => components.push(component(path)),
            Some(Value::Array(items)) => {
                components.extend(items.iter().filter_map(Value::as_str).map(component))
            }
            Some(Value::Object(_)) => components.push(component(INLINE_COMPONENT)),
            Some(_) => {}
            None if root.join(conventional).exists() => components.push(component(conventional)),
            None => {}
        }
    }
    components
}

fn permissions(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        other => frontmatter::string_list(other),
    }
}

/// blake3 over the manifest plus every potentially executable file, in path
/// order. Symlinks contribute their target path, not the target's bytes.
fn hash_contents(
    root: &Path,
    manifest: &[u8],
    executable: &[&ResourceFile],
    max_bytes_per_file: u64,
    issues: &mut Vec<ValidationIssue>,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(HASH_DOMAIN);
    hasher.update(&(manifest.len() as u64).to_le_bytes());
    hasher.update(manifest);
    for file in executable {
        hasher.update(file.path.as_str().as_bytes());
        hasher.update(b"\0");
        if let FileKind::Symlink { target, .. } = &file.kind {
            hasher.update(b"symlink:");
            hasher.update(target.to_string_lossy().as_bytes());
            continue;
        }
        let mut bytes = Vec::new();
        let read = File::open(file.path.to_path(root))
            .and_then(|handle| handle.take(max_bytes_per_file).read_to_end(&mut bytes));
        match read {
            Ok(_) => {
                if file.size > max_bytes_per_file {
                    issues.push(ValidationIssue::warning(format!(
                        "hash covers only the first {max_bytes_per_file} bytes of {}",
                        file.path
                    )));
                }
                hasher.update(&(bytes.len() as u64).to_le_bytes());
                hasher.update(&bytes);
            }
            Err(error) => {
                issues.push(ValidationIssue::warning(format!(
                    "could not hash {}: {error}",
                    file.path
                )));
                hasher.update(b"unreadable");
            }
        }
    }
    hasher.finalize().to_hex().to_string()
}
