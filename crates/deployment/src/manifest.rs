use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use perkolator_core::Scope;
use serde::{Deserialize, Serialize};

use crate::DeploymentMode;
use crate::fsutil::sync_dir;

pub const MANIFEST_VERSION: u32 = 1;

/// Target id used for the `managed_root/<name>` entry of managed modes.
pub const MANAGED_TARGET_ID: &str = "perkolator-managed";

/// One path Perkolator created and therefore owns. Ownership is never trusted
/// on its own: every destructive step re-verifies identity against
/// `link_target` (links) or `fingerprint` (copies).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub destination: PathBuf,
    /// Canonical skill source this deployment came from.
    pub source: PathBuf,
    pub mode: DeploymentMode,
    pub target_id: String,
    pub scope: Scope,
    /// Exact symlink contents written, for link entries.
    #[serde(default)]
    pub link_target: Option<PathBuf>,
    /// Tree fingerprint written, for copy entries.
    #[serde(default)]
    pub fingerprint: Option<String>,
    pub applied_at_unix: u64,
}

impl ManifestEntry {
    pub fn is_link(&self) -> bool {
        self.link_target.is_some()
    }

    pub fn is_copy(&self) -> bool {
        self.link_target.is_none() && self.fingerprint.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    /// Run id of the last committed apply; lets recovery tell whether a crash
    /// happened after the manifest was durably written.
    #[serde(default)]
    pub last_run_id: Option<String>,
    #[serde(default)]
    pub entries: Vec<ManifestEntry>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            last_run_id: None,
            entries: Vec::new(),
        }
    }
}

impl Manifest {
    /// A missing file is an empty manifest; anything unreadable is an error
    /// (never silently treated as empty, which would orphan deployments).
    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if manifest.version != MANIFEST_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported manifest version {}", manifest.version),
            ));
        }
        Ok(manifest)
    }

    /// Atomic replace: write a sibling temp file, fsync, rename, fsync dir.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "manifest needs a parent")
            })?;
        fs::create_dir_all(parent)?;
        let file_name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "manifest needs a name"))?;
        let mut tmp_name = file_name.to_owned();
        tmp_name.push(format!(".tmp-{}", std::process::id()));
        let tmp = parent.join(tmp_name);
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let result = (|| {
            let mut file: File = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            file.write_all(&json)?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            sync_dir(parent)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    pub fn entry(&self, destination: &Path) -> Option<&ManifestEntry> {
        self.entries
            .iter()
            .find(|entry| entry.destination == destination)
    }

    /// Entries whose symlink points at `path` (e.g. links to a managed copy).
    pub fn dependents_of(&self, path: &Path) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.destination != path && entry.link_target.as_deref() == Some(path))
            .count()
    }

    pub fn upsert(&mut self, entry: ManifestEntry) {
        match self
            .entries
            .iter_mut()
            .find(|existing| existing.destination == entry.destination)
        {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
    }

    pub fn remove(&mut self, destination: &Path) {
        self.entries
            .retain(|entry| entry.destination != destination);
    }
}
