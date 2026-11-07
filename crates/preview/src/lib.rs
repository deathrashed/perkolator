//! Inert, cancellable preview model shared by the quick launcher and the
//! full manager.
//!
//! Previews never execute anything, never follow symlinks out of a resource
//! root, cap file sizes, and show configuration only through redaction.
//! Building a preview does blocking I/O: run [`build_preview`] off the UI
//! thread, paired with a [`PreviewScheduler`] generation so stale results
//! can be dropped.

mod build;
mod fs;
mod markdown;
mod redact;
mod scheduler;

use std::path::PathBuf;

use perkolator_core::{FileKind, PromptVariable, RelPath, ResourceId};
use serde::Serialize;

pub use build::build_preview;
pub use fs::{file_tree, preview_file, read_confined};
pub use markdown::parse_markdown;
pub use redact::{mcp_config_snippet, redact_args, redact_url};
pub use scheduler::PreviewScheduler;

const DEFAULT_MAX_FILE_BYTES: u64 = 512 * 1024;
const DEFAULT_MAX_TREE_ENTRIES: usize = 2_000;
const DEFAULT_MAX_TREE_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PreviewLimits {
    /// Larger files yield [`Document::TooLarge`] without being read.
    pub max_file_bytes: u64,
    /// File trees stop (and report truncation) after this many entries.
    pub max_tree_entries: usize,
    /// Directories deeper than this are listed but not descended into.
    pub max_tree_depth: usize,
}

impl Default for PreviewLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_tree_entries: DEFAULT_MAX_TREE_ENTRIES,
            max_tree_depth: DEFAULT_MAX_TREE_DEPTH,
        }
    }
}

/// A file's previewable content. Text is kept verbatim; the UI renders it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
pub enum Document {
    Markdown {
        /// Body without the frontmatter block.
        text: String,
        frontmatter: Vec<(String, String)>,
        headings: Vec<Heading>,
        links: Vec<LinkRef>,
    },
    Code {
        language: String,
        text: String,
    },
    PlainText(String),
    /// Image inside the root; the UI decodes it with its own limits.
    Image {
        path: PathBuf,
    },
    Binary {
        size: u64,
    },
    TooLarge {
        size: u64,
        limit: u64,
    },
    Unavailable {
        reason: String,
    },
}

impl Document {
    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Document::Unavailable {
            reason: reason.into(),
        }
    }

    /// The text content, for textual documents.
    pub fn text(&self) -> Option<&str> {
        match self {
            Document::Markdown { text, .. }
            | Document::Code { text, .. }
            | Document::PlainText(text) => Some(text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// GitHub-style anchor, unique within the document.
    pub anchor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LinkRef {
    /// Destination exactly as written.
    pub target: String,
    /// Lexically resolved path inside the browse root, for relative links
    /// that stay inside it. Opening it still goes through [`read_confined`].
    pub resolved: Option<RelPath>,
    /// Has a URL scheme (`https:`, `mailto:` ...).
    pub external: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TreeNode {
    pub path: RelPath,
    pub name: String,
    /// 0 for entries directly inside the root.
    pub depth: usize,
    pub is_dir: bool,
    pub kind: FileKind,
}

/// Flattened display-order tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FileTree {
    pub nodes: Vec<TreeNode>,
    /// True when `max_tree_entries` stopped the walk early.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToolParam {
    pub name: String,
    pub type_name: Option<String>,
    pub required: bool,
    pub description: Option<String>,
}

/// A statically declared MCP tool. Live tool discovery requires an explicit,
/// permissioned connection test and is not part of preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToolView {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Vec<ToolParam>,
    /// Pretty-printed input schema.
    pub schema: Option<String>,
}

/// Only tabs that apply to the resource are produced; there are no
/// placeholder tabs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "tab", content = "value", rename_all = "kebab-case")]
pub enum PreviewTab {
    Overview(Document),
    Files(FileTree),
    /// Redacted configuration snippet of just this entry.
    Config {
        format: String,
        text: String,
    },
    Tools(Vec<ToolView>),
    Template {
        variables: Vec<PromptVariable>,
        raw: String,
    },
    Precedence(Vec<(String, String)>),
    Permissions {
        permissions: Vec<String>,
        executable_files: Vec<RelPath>,
        content_hash: Option<String>,
    },
    Metadata(Vec<(String, String)>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Preview {
    /// Scheduler generation this preview was built for.
    pub generation: u64,
    pub id: ResourceId,
    pub title: String,
    pub tabs: Vec<PreviewTab>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreviewError {
    #[error("preview cancelled")]
    Cancelled,
}
