use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// A validated, normalised relative path that can never escape its root:
/// no absolute paths, no `..`, no prefixes, no empty components. Uses `/`
/// separators internally regardless of platform.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl RelPath {
    pub fn new(value: impl AsRef<str>) -> Result<Self, CoreError> {
        let raw = value.as_ref();
        let unsafe_path = |reason| CoreError::UnsafeRelativePath {
            path: raw.to_owned(),
            reason,
        };
        if raw.is_empty() {
            return Err(unsafe_path("empty path"));
        }
        if raw.contains('\0') {
            return Err(unsafe_path("contains NUL byte"));
        }
        if raw.starts_with('/') || raw.starts_with('\\') {
            return Err(unsafe_path("absolute path"));
        }
        let mut parts = Vec::new();
        for part in raw.split(['/', '\\']) {
            match part {
                "" | "." => continue,
                ".." => return Err(unsafe_path("parent traversal")),
                part if part.len() >= 2 && part.as_bytes()[1] == b':' => {
                    return Err(unsafe_path("drive prefix"));
                }
                part => parts.push(part),
            }
        }
        if parts.is_empty() {
            return Err(unsafe_path("empty path"));
        }
        Ok(Self(parts.join("/")))
    }

    /// Build from a path known to be inside `root` (e.g. produced by a walk).
    pub fn from_child(root: &Path, child: &Path) -> Result<Self, CoreError> {
        let relative = child
            .strip_prefix(root)
            .map_err(|_| CoreError::UnsafeRelativePath {
                path: child.display().to_string(),
                reason: "not inside root",
            })?;
        let mut parts = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
                Component::CurDir => {}
                _ => {
                    return Err(CoreError::UnsafeRelativePath {
                        path: child.display().to_string(),
                        reason: "non-normal component",
                    });
                }
            }
        }
        Self::new(parts.join("/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    pub fn depth(&self) -> usize {
        self.0.split('/').count()
    }

    pub fn parent(&self) -> Option<RelPath> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _)| RelPath(parent.to_owned()))
    }

    pub fn join(&self, child: &RelPath) -> RelPath {
        RelPath(format!("{}/{}", self.0, child.0))
    }

    /// Lexically join onto a root. The caller still must check symlinks on disk.
    pub fn to_path(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for part in self.0.split('/') {
            path.push(part);
        }
        path
    }
}

impl TryFrom<String> for RelPath {
    type Error = CoreError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<RelPath> for String {
    fn from(value: RelPath) -> Self {
        value.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_separators_and_dots() {
        assert_eq!(RelPath::new("./a//b/./c.md").unwrap().as_str(), "a/b/c.md");
        assert_eq!(RelPath::new("a\\b").unwrap().as_str(), "a/b");
    }

    #[test]
    fn rejects_escapes() {
        for bad in ["", "/etc/passwd", "../x", "a/../../b", "C:/x", "\\\\server\\s", ".", "a\0b"] {
            assert!(RelPath::new(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn from_child_requires_containment() {
        let root = Path::new("/lib/skill");
        assert_eq!(
            RelPath::from_child(root, Path::new("/lib/skill/references/a.md"))
                .unwrap()
                .as_str(),
            "references/a.md"
        );
        assert!(RelPath::from_child(root, Path::new("/lib/other/a.md")).is_err());
    }

    #[test]
    fn parent_and_file_name() {
        let path = RelPath::new("references/deep/x.md").unwrap();
        assert_eq!(path.file_name(), "x.md");
        assert_eq!(path.parent().unwrap().as_str(), "references/deep");
        assert_eq!(path.depth(), 3);
        assert!(RelPath::new("x.md").unwrap().parent().is_none());
    }
}
