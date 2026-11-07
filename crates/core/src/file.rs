use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::RelPath;

/// Inert classification of a file inside a resource. Classification never
/// opens or executes the file; it is based on name and metadata only.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FileKind {
    Directory,
    Markdown,
    Text,
    Code { language: String },
    Json,
    Toml,
    Yaml,
    Image,
    Binary,
    Symlink { target: PathBuf, broken: bool },
}

impl FileKind {
    /// Classify a regular file by its name.
    pub fn classify_file_name(name: &str) -> FileKind {
        let lower = name.to_ascii_lowercase();
        let extension = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
        let code = |language: &str| FileKind::Code {
            language: language.to_owned(),
        };
        match extension {
            "md" | "mdc" | "markdown" | "mdx" => FileKind::Markdown,
            "json" | "jsonc" | "json5" => FileKind::Json,
            "toml" => FileKind::Toml,
            "yaml" | "yml" => FileKind::Yaml,
            "txt" | "text" | "log" | "csv" | "tsv" | "ini" | "cfg" | "conf" | "env" => {
                FileKind::Text
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" => FileKind::Image,
            "rs" => code("rust"),
            "py" => code("python"),
            "ts" | "tsx" | "mts" | "cts" => code("typescript"),
            "js" | "jsx" | "mjs" | "cjs" => code("javascript"),
            "sh" | "bash" | "zsh" | "fish" => code("shell"),
            "xml" | "xsd" | "plist" => code("xml"),
            "html" | "htm" => code("html"),
            "css" | "scss" => code("css"),
            "go" => code("go"),
            "rb" => code("ruby"),
            "lua" => code("lua"),
            "java" => code("java"),
            "kt" | "kts" => code("kotlin"),
            "swift" => code("swift"),
            "c" | "h" => code("c"),
            "cc" | "cpp" | "hpp" | "cxx" => code("cpp"),
            "sql" => code("sql"),
            "zip" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "7z" | "pdf" | "wasm" | "so"
            | "dylib" | "dll" | "exe" | "bin" | "ttf" | "otf" | "woff" | "woff2" | "mp3"
            | "mp4" | "mov" | "wav" | "docx" | "xlsx" | "pptx" => FileKind::Binary,
            _ => match lower.as_str() {
                "dockerfile" | "makefile" | "justfile" => code("shell"),
                "license" | "licence" | "readme" | "notice" | "changelog" => FileKind::Text,
                _ => FileKind::Text,
            },
        }
    }

    pub fn is_textual(&self) -> bool {
        matches!(
            self,
            FileKind::Markdown
                | FileKind::Text
                | FileKind::Code { .. }
                | FileKind::Json
                | FileKind::Toml
                | FileKind::Yaml
        )
    }
}

/// One entry in a resource's file inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceFile {
    pub path: RelPath,
    pub kind: FileKind,
    pub size: u64,
    /// Executable bit (Unix) or script-like extension; informational only.
    pub executable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_files() {
        assert_eq!(FileKind::classify_file_name("SKILL.md"), FileKind::Markdown);
        assert_eq!(FileKind::classify_file_name("rule.mdc"), FileKind::Markdown);
        assert_eq!(FileKind::classify_file_name("plugin.json"), FileKind::Json);
        assert_eq!(
            FileKind::classify_file_name("run.sh"),
            FileKind::Code {
                language: "shell".into()
            }
        );
        assert_eq!(FileKind::classify_file_name("logo.PNG"), FileKind::Image);
        assert_eq!(FileKind::classify_file_name("bundle.zip"), FileKind::Binary);
        assert!(FileKind::classify_file_name("config.yaml").is_textual());
        assert!(!FileKind::Image.is_textual());
    }
}
