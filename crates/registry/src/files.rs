//! Bounded, inert filesystem helpers. Nothing here opens files for anything
//! but reading, and symlinks are never followed during inventory walks.

use std::fs;
use std::path::{Path, PathBuf};

use perkolator_core::{CancelToken, FileKind, RelPath, ResourceFile};
use walkdir::WalkDir;

use crate::{ScanLimits, ScanProblem};

/// Directory names never descended into.
const SKIPPED_DIRS: [&str; 2] = [".git", "node_modules"];

/// Extensions treated as potentially executable regardless of mode bits.
const SCRIPT_EXTENSIONS: [&str; 20] = [
    "sh", "bash", "zsh", "fish", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "php", "ps1", "bat",
    "cmd", "exe", "bin", "so", "dylib", "wasm",
];

pub(crate) fn is_skipped_dir(name: &str) -> bool {
    SKIPPED_DIRS.contains(&name)
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(crate) fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Read a file as bytes, refusing anything over `max_bytes`.
pub(crate) fn read_bytes(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("cannot read: {error}"))?;
    if metadata.len() > max_bytes {
        return Err(format!(
            "file is {} bytes, larger than the {max_bytes}-byte scan limit; not parsed",
            metadata.len()
        ));
    }
    fs::read(path).map_err(|error| format!("cannot read: {error}"))
}

pub(crate) fn read_text(path: &Path, limits: &ScanLimits) -> Result<String, String> {
    let bytes = read_bytes(path, limits.max_file_bytes)?;
    String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8".to_owned())
}

/// Canonical (symlink-resolved) path, falling back to the path itself.
pub(crate) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Sorted directory entries; unreadable directories become a problem.
pub(crate) fn sorted_entries(dir: &Path, problems: &mut Vec<ScanProblem>) -> Vec<fs::DirEntry> {
    match fs::read_dir(dir) {
        Ok(entries) => {
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            entries.sort_by_key(|entry| entry.file_name());
            entries
        }
        Err(error) => {
            problems.push(ScanProblem::new(
                dir,
                format!("cannot list directory: {error}"),
            ));
            Vec::new()
        }
    }
}

pub(crate) struct Inventory {
    pub files: Vec<ResourceFile>,
    pub truncated: bool,
}

/// Inventory of a resource directory without following symlinks.
pub(crate) fn inventory(
    root: &Path,
    limits: &ScanLimits,
    cancel: &CancelToken,
    problems: &mut Vec<ScanProblem>,
) -> Inventory {
    let mut files = Vec::new();
    let mut truncated = false;
    let walker = WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .max_depth(limits.max_depth)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            !(entry.file_type().is_dir() && is_skipped_dir(&entry.file_name().to_string_lossy()))
        });
    for entry in walker {
        if cancel.is_cancelled() {
            break;
        }
        if files.len() >= limits.max_files_per_resource {
            truncated = true;
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                let path = error.path().unwrap_or(root).to_path_buf();
                problems.push(ScanProblem::new(path, format!("cannot inventory: {error}")));
                continue;
            }
        };
        let Ok(relative) = RelPath::from_child(root, entry.path()) else {
            continue;
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                problems.push(ScanProblem::new(
                    entry.path(),
                    format!("cannot stat: {error}"),
                ));
                continue;
            }
        };
        let file_type = entry.file_type();
        let (kind, executable) = if file_type.is_symlink() {
            let target = fs::read_link(entry.path()).unwrap_or_default();
            let broken = fs::metadata(entry.path()).is_err();
            (FileKind::Symlink { target, broken }, false)
        } else if file_type.is_dir() {
            (FileKind::Directory, false)
        } else {
            let kind = FileKind::classify_file_name(&entry.file_name().to_string_lossy());
            (kind, mode_executable(&metadata))
        };
        files.push(ResourceFile {
            path: relative,
            kind,
            size: if file_type.is_file() {
                metadata.len()
            } else {
                0
            },
            executable,
        });
    }
    Inventory { files, truncated }
}

#[cfg(unix)]
fn mode_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn mode_executable(_metadata: &fs::Metadata) -> bool {
    false
}

/// Whether an inventory entry could be executed by a host: exec bit, a
/// script/binary extension, or anything under `hooks/`.
pub(crate) fn could_execute(file: &ResourceFile) -> bool {
    if file.kind == FileKind::Directory {
        return false;
    }
    let extension = file
        .path
        .file_name()
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    file.executable
        || SCRIPT_EXTENSIONS.contains(&extension.as_str())
        || file.path.as_str().starts_with("hooks/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_skips_git_and_records_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::create_dir_all(root.join("references")).unwrap();
        fs::write(root.join("references/a.md"), "# A").unwrap();
        fs::write(root.join(".git/objects/x"), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("missing", root.join("dangling")).unwrap();
        let inventory = inventory(
            root,
            &ScanLimits::default(),
            &CancelToken::new(),
            &mut Vec::new(),
        );
        let paths: Vec<_> = inventory
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert!(paths.contains(&"references/a.md"));
        assert!(!paths.iter().any(|path| path.starts_with(".git/objects")));
        #[cfg(unix)]
        assert!(
            inventory
                .files
                .iter()
                .any(|file| matches!(&file.kind, FileKind::Symlink { broken: true, .. }))
        );
    }

    #[test]
    fn inventory_respects_file_limit() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..5 {
            fs::write(dir.path().join(format!("{index}.md")), "x").unwrap();
        }
        let limits = ScanLimits {
            max_files_per_resource: 3,
            ..ScanLimits::default()
        };
        let inventory = inventory(dir.path(), &limits, &CancelToken::new(), &mut Vec::new());
        assert_eq!(inventory.files.len(), 3);
        assert!(inventory.truncated);
    }

    #[test]
    fn read_bytes_enforces_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.json");
        fs::write(&path, "0123456789").unwrap();
        assert!(read_bytes(&path, 5).is_err());
        assert_eq!(read_bytes(&path, 10).unwrap().len(), 10);
    }
}
