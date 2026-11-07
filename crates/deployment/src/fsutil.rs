//! lstat-based filesystem primitives. Nothing here follows a symlink unless
//! the function name says so.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

/// What is at a path, without following a final symlink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathState {
    Missing,
    Dir,
    File,
    Symlink { target: PathBuf },
    Other,
}

impl PathState {
    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            PathState::Missing => "nothing",
            PathState::Dir => "directory",
            PathState::File => "file",
            PathState::Symlink { .. } => "symlink",
            PathState::Other => "special file",
        }
    }
}

pub(crate) fn probe(path: &Path) -> io::Result<PathState> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(PathState::Missing),
        Err(error) => return Err(error),
    };
    let file_type = meta.file_type();
    Ok(if file_type.is_symlink() {
        PathState::Symlink {
            target: fs::read_link(path)?,
        }
    } else if file_type.is_dir() {
        PathState::Dir
    } else if file_type.is_file() {
        PathState::File
    } else {
        PathState::Other
    })
}

/// Lexically remove `.` and resolve `..` (never above the root).
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Absolute, normalised form of a symlink's raw contents.
pub(crate) fn resolve_link_target(link: &Path, raw: &Path) -> PathBuf {
    if raw.is_absolute() {
        normalize(raw)
    } else {
        normalize(&link.parent().unwrap_or(Path::new("/")).join(raw))
    }
}

pub(crate) fn is_clean_absolute(path: &Path) -> bool {
    perkolator_targets::is_clean_absolute(path)
}

/// Canonicalise the deepest existing ancestor (following symlinks) and append
/// the missing remainder lexically. Shows where a not-yet-created path would
/// physically land.
pub(crate) fn real_path(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut tail: Vec<OsString> = Vec::new();
    loop {
        if let Ok(real) = fs::canonicalize(existing) {
            let mut out = real;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                existing = parent;
            }
            _ => return normalize(path),
        }
    }
}

/// Physical location of `path` itself without following its final component.
pub(crate) fn real_location(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => real_path(parent).join(name),
        _ => normalize(path),
    }
}

pub(crate) fn is_loop_error(error: &io::Error) -> bool {
    #[cfg(target_os = "linux")]
    const ELOOP: i32 = 40;
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    const ELOOP: i32 = 62;
    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd"
    )))]
    const ELOOP: i32 = -1;
    error.raw_os_error() == Some(ELOOP)
}

pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

pub(crate) fn create_symlink(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symlinks are only supported on macOS and Linux",
        ))
    }
}

fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn rel_bytes(root: &Path, path: &Path) -> Vec<u8> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut out = Vec::new();
    for (index, component) in relative.components().enumerate() {
        if index > 0 {
            out.push(b'/');
        }
        out.extend_from_slice(component.as_os_str().as_encoded_bytes());
    }
    out
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    false
}

/// blake3 over every entry in sorted order: relative path, type, executable
/// bit, file bytes, symlink contents. Never follows symlinks; the root must
/// be a real directory.
pub fn fingerprint_tree(dir: &Path) -> io::Result<String> {
    if !fs::symlink_metadata(dir)?.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a real directory", dir.display()),
        ));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"perkolator-tree-v1\0");
    let mut buffer = vec![0u8; 64 * 1024];
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .follow_root_links(false)
        .min_depth(1)
        .sort_by_file_name()
    {
        let entry = entry.map_err(io::Error::from)?;
        let file_type = entry.file_type();
        hash_field(&mut hasher, &rel_bytes(dir, entry.path()));
        if file_type.is_symlink() {
            hasher.update(b"l");
            let target = fs::read_link(entry.path())?;
            hash_field(&mut hasher, target.as_os_str().as_encoded_bytes());
        } else if file_type.is_dir() {
            hasher.update(b"d");
        } else if file_type.is_file() {
            let meta = entry.metadata().map_err(io::Error::from)?;
            hasher.update(if is_executable(&meta) { b"x" } else { b"f" });
            hasher.update(&meta.len().to_le_bytes());
            let mut file = File::open(entry.path())?;
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
        } else {
            hasher.update(b"o");
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// `(files, bytes)` of regular files, not following symlinks.
pub(crate) fn tree_stats(dir: &Path) -> io::Result<(usize, u64)> {
    let mut files = 0;
    let mut bytes = 0;
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .follow_root_links(false)
    {
        let entry = entry.map_err(io::Error::from)?;
        if entry.file_type().is_file() {
            files += 1;
            bytes += entry.metadata().map_err(io::Error::from)?.len();
        }
    }
    Ok((files, bytes))
}

/// Why a symlink inside a source tree cannot be copied verbatim.
pub(crate) fn unsafe_copy_symlink(root: &Path, link: &Path, raw: &Path) -> bool {
    raw.is_absolute() || !resolve_link_target(link, raw).starts_with(normalize(root))
}

/// Copy `from`'s contents into the existing empty directory `into`.
/// Symlinks are recreated verbatim only when relative and inside `from`;
/// anything else aborts (the planner reports these as conflicts first).
pub(crate) fn copy_tree_contents(from: &Path, into: &Path) -> io::Result<()> {
    for entry in WalkDir::new(from)
        .follow_links(false)
        .follow_root_links(false)
        .min_depth(1)
        .sort_by_file_name()
    {
        let entry = entry.map_err(io::Error::from)?;
        let relative = entry
            .path()
            .strip_prefix(from)
            .map_err(|_| io::Error::other("walk escaped the source root"))?;
        let dest = into.join(relative);
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            let raw = fs::read_link(entry.path())?;
            if unsafe_copy_symlink(from, entry.path(), &raw) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("symlink {} escapes the source", entry.path().display()),
                ));
            }
            create_symlink(&raw, &dest)?;
        } else if file_type.is_dir() {
            fs::create_dir(&dest)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &dest)?;
            File::open(&dest)?.sync_all()?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported file type at {}", entry.path().display()),
            ));
        }
    }
    Ok(())
}

pub(crate) fn dir_is_empty(dir: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(dir)?.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_resolves_lexically() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), Path::new("/a/c"));
        assert_eq!(normalize(Path::new("/../a")), Path::new("/a"));
    }

    #[test]
    fn fingerprint_changes_with_content_and_exec_bit() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SKILL.md"), "a").unwrap();
        let first = fingerprint_tree(dir.path()).unwrap();
        assert_eq!(first, fingerprint_tree(dir.path()).unwrap());
        fs::write(dir.path().join("SKILL.md"), "b").unwrap();
        let second = fingerprint_tree(dir.path()).unwrap();
        assert_ne!(first, second);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                dir.path().join("SKILL.md"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            assert_ne!(second, fingerprint_tree(dir.path()).unwrap());
        }
    }

    #[cfg(unix)]
    #[test]
    fn fingerprint_does_not_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("x"), "1").unwrap();
        create_symlink(outside.path(), &dir.path().join("link")).unwrap();
        let first = fingerprint_tree(dir.path()).unwrap();
        fs::write(outside.path().join("x"), "2").unwrap();
        assert_eq!(first, fingerprint_tree(dir.path()).unwrap());
        assert!(fingerprint_tree(&dir.path().join("link")).is_err());
    }

    #[test]
    fn real_path_appends_missing_tail() {
        let dir = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(dir.path()).unwrap();
        assert_eq!(real_path(&dir.path().join("a/b")), real.join("a/b"));
    }
}
