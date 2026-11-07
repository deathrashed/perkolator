use std::cmp::Ordering;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use perkolator_core::{CancelToken, FileKind, RelPath};

use crate::markdown::parse_markdown;
use crate::{Document, FileTree, PreviewError, PreviewLimits, TreeNode};

/// Directories listed in trees but never descended into: they are large,
/// generated, and not part of a resource's authored content.
const OPAQUE_DIRS: &[&str] = &[".git", "node_modules", "target", ".venv", "__pycache__"];

/// Names whose content is likely a credential. Preview refuses them so
/// secrets cannot appear in screenshots; the user can open them explicitly.
fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let env_template = [".example", ".sample", ".template", ".dist"]
        .iter()
        .any(|suffix| lower.ends_with(suffix));
    (lower == ".env" || (lower.starts_with(".env.") && !env_template))
        || [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        || ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"].contains(&lower.as_str())
        || [
            ".npmrc",
            ".netrc",
            ".pypirc",
            "credentials",
            "credentials.json",
        ]
        .contains(&lower.as_str())
}

/// Read `rel` inside `root` without leaving it.
///
/// Both paths are canonicalized and containment is required, so `..` is
/// impossible (by [`RelPath`]) and symlinks resolving outside the root are
/// refused. Only regular files are read (no FIFOs/devices). Content is
/// classified by name via [`FileKind`], then confirmed: NUL bytes or invalid
/// UTF-8 make a "text" file [`Document::Binary`].
pub fn read_confined(root: &Path, rel: &RelPath, limits: &PreviewLimits) -> Document {
    let canonical_root = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) => {
            return Document::unavailable(format!("resource root not accessible: {error}"));
        }
    };
    let target = match fs::canonicalize(rel.to_path(&canonical_root)) {
        Ok(path) => path,
        Err(error) => return Document::unavailable(format!("{rel}: {error}")),
    };
    if !target.starts_with(&canonical_root) {
        return Document::unavailable(format!(
            "{rel}: refused, resolves outside the resource root"
        ));
    }
    let metadata = match fs::metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) => return Document::unavailable(format!("{rel}: {error}")),
    };
    if metadata.is_dir() {
        return Document::unavailable(format!("{rel} is a directory"));
    }
    if !metadata.is_file() {
        return Document::unavailable(format!("{rel} is not a regular file"));
    }
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.file_name().to_owned());
    if is_sensitive_name(&name) || is_sensitive_name(rel.file_name()) {
        return Document::unavailable(format!(
            "{rel} may contain credentials; preview is disabled for this file"
        ));
    }
    let size = metadata.len();
    let kind = FileKind::classify_file_name(&name);
    if kind == FileKind::Binary {
        return Document::Binary { size };
    }
    if size > limits.max_file_bytes {
        return Document::TooLarge {
            size,
            limit: limits.max_file_bytes,
        };
    }
    if kind == FileKind::Image {
        return Document::Image { path: target };
    }
    let bytes = match read_bounded(&target, limits.max_file_bytes) {
        Ok(bytes) => bytes,
        Err(error) => return Document::unavailable(format!("{rel}: {error}")),
    };
    if bytes.len() as u64 > limits.max_file_bytes {
        // The file grew after the metadata check.
        return Document::TooLarge {
            size: bytes.len() as u64,
            limit: limits.max_file_bytes,
        };
    }
    if bytes.contains(&0) {
        return Document::Binary { size };
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Document::Binary { size };
    };
    text_document(kind, text, rel)
}

/// Preview a file selected in a [`FileTree`]. Same confinement as
/// [`read_confined`].
pub fn preview_file(root: &Path, rel: &RelPath, limits: &PreviewLimits) -> Document {
    read_confined(root, rel, limits)
}

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn text_document(kind: FileKind, text: String, rel: &RelPath) -> Document {
    let code = |language: &str, text: String| Document::Code {
        language: language.to_owned(),
        text,
    };
    match kind {
        FileKind::Markdown => parse_markdown(&text, Some(rel)),
        FileKind::Json => code("json", text),
        FileKind::Toml => code("toml", text),
        FileKind::Yaml => code("yaml", text),
        FileKind::Code { language } => code(&language, text),
        _ => Document::PlainText(text),
    }
}

/// Flattened display-order tree of `root`: `SKILL.md` first, then
/// directories before files, case-insensitive alphabetical. Symlinks are
/// listed (with their target) but never followed.
pub fn file_tree(root: &Path, limits: &PreviewLimits) -> FileTree {
    walk_tree(root, limits, None).unwrap_or_default()
}

pub(crate) fn walk_tree(
    root: &Path,
    limits: &PreviewLimits,
    cancel: Option<&CancelToken>,
) -> Result<FileTree, PreviewError> {
    let mut tree = FileTree::default();
    // The root itself may be a symlink (e.g. a deployed skill); resolve it
    // once, then never follow links inside it.
    if let Ok(canonical_root) = fs::canonicalize(root) {
        visit(&canonical_root, None, 0, limits, cancel, &mut tree)?;
    }
    Ok(tree)
}

struct DirItem {
    name: String,
    path: PathBuf,
    file_type: fs::FileType,
}

fn display_order(a: &DirItem, b: &DirItem) -> Ordering {
    let rank = |item: &DirItem| (item.name != "SKILL.md", !item.file_type.is_dir());
    rank(a)
        .cmp(&rank(b))
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

fn visit(
    dir: &Path,
    dir_rel: Option<&RelPath>,
    depth: usize,
    limits: &PreviewLimits,
    cancel: Option<&CancelToken>,
    tree: &mut FileTree,
) -> Result<(), PreviewError> {
    if cancel.is_some_and(CancelToken::is_cancelled) {
        return Err(PreviewError::Cancelled);
    }
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Ok(());
    };
    let mut items: Vec<DirItem> = read_dir
        .filter_map(Result::ok)
        .filter_map(|entry| {
            // Names that are not UTF-8 or contain `\` cannot be a RelPath
            // faithfully; skip rather than misrepresent them.
            let name = entry.file_name().into_string().ok()?;
            if name.contains('\\') {
                return None;
            }
            Some(DirItem {
                path: entry.path(),
                file_type: entry.file_type().ok()?,
                name,
            })
        })
        .collect();
    items.sort_by(display_order);

    for item in items {
        if tree.nodes.len() >= limits.max_tree_entries {
            tree.truncated = true;
            return Ok(());
        }
        let Ok(name_rel) = RelPath::new(&item.name) else {
            continue;
        };
        let rel = match dir_rel {
            Some(parent) => parent.join(&name_rel),
            None => name_rel,
        };
        let is_dir = item.file_type.is_dir();
        let kind = if item.file_type.is_symlink() {
            FileKind::Symlink {
                target: fs::read_link(&item.path).unwrap_or_default(),
                broken: fs::metadata(&item.path).is_err(),
            }
        } else if is_dir {
            FileKind::Directory
        } else {
            FileKind::classify_file_name(&item.name)
        };
        tree.nodes.push(TreeNode {
            path: rel.clone(),
            name: item.name.clone(),
            depth,
            is_dir,
            kind,
        });
        if is_dir && depth + 1 < limits.max_tree_depth && !OPAQUE_DIRS.contains(&item.name.as_str())
        {
            visit(&item.path, Some(&rel), depth + 1, limits, cancel, tree)?;
            if tree.truncated {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_names() {
        for name in [
            ".env",
            ".env.local",
            "server.pem",
            "id_ed25519",
            ".npmrc",
            "tls.KEY",
        ] {
            assert!(is_sensitive_name(name), "{name}");
        }
        for name in [
            ".env.example",
            "SKILL.md",
            "id_ed25519.pub",
            "keys.md",
            "env.ts",
        ] {
            assert!(!is_sensitive_name(name), "{name}");
        }
    }
}
