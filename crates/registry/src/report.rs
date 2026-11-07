use std::path::PathBuf;

use perkolator_core::ResourceRecord;
use serde::Serialize;

/// Something that could not be read or parsed. Messages never contain file
/// contents, so they are safe to log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScanProblem {
    pub path: PathBuf,
    pub message: String,
}

impl ScanProblem {
    pub(crate) fn new(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// A symlink whose target does not exist. Never turned into a record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BrokenLink {
    pub path: PathBuf,
    pub target: PathBuf,
    pub host: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScanReport {
    pub records: Vec<ResourceRecord>,
    pub problems: Vec<ScanProblem>,
    pub broken_links: Vec<BrokenLink>,
    /// True when the scan stopped early because its token was cancelled.
    pub cancelled: bool,
}
