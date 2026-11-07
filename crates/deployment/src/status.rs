use std::fs;
use std::path::{Path, PathBuf};

use perkolator_core::InstallState;
use serde::{Deserialize, Serialize};

use crate::fsutil::{PathState, fingerprint_tree, probe};
use crate::{Destination, Manifest, ManifestEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentStatus {
    /// Owned link pointing at its recorded target, which resolves.
    Linked,
    /// Owned copy matching its recorded fingerprint.
    Copy,
    /// Nothing at the path.
    Missing,
    /// Owned copy edited since deployment.
    Modified,
    /// Owned link whose target no longer resolves.
    Broken,
    /// Something other than what Perkolator recorded is at the path.
    Conflict,
    /// Present but not in the manifest.
    Unmanaged,
}

impl DeploymentStatus {
    pub fn install_state(self) -> InstallState {
        match self {
            DeploymentStatus::Linked => InstallState::Linked,
            DeploymentStatus::Copy => InstallState::Copy,
            DeploymentStatus::Missing => InstallState::Available,
            DeploymentStatus::Modified => InstallState::Outdated,
            DeploymentStatus::Broken | DeploymentStatus::Conflict => InstallState::Broken,
            DeploymentStatus::Unmanaged => InstallState::Unmanaged,
        }
    }
}

/// Current on-disk status of an owned deployment (read-only).
pub fn status_of(entry: &ManifestEntry) -> DeploymentStatus {
    let path = &entry.destination;
    match probe(path) {
        Ok(PathState::Missing) => DeploymentStatus::Missing,
        Ok(PathState::Symlink { target }) => match &entry.link_target {
            Some(recorded) if *recorded == target => {
                if fs::metadata(path).is_ok() {
                    DeploymentStatus::Linked
                } else {
                    DeploymentStatus::Broken
                }
            }
            _ => DeploymentStatus::Conflict,
        },
        Ok(PathState::Dir) => match (&entry.link_target, &entry.fingerprint) {
            (None, Some(recorded)) => match fingerprint_tree(path) {
                Ok(current) if current == *recorded => DeploymentStatus::Copy,
                Ok(_) => DeploymentStatus::Modified,
                Err(_) => DeploymentStatus::Conflict,
            },
            _ => DeploymentStatus::Conflict,
        },
        _ => DeploymentStatus::Conflict,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixCell {
    pub target_id: String,
    pub source: PathBuf,
    pub path: PathBuf,
    pub status: DeploymentStatus,
}

/// Agent × skill matrix: one row per destination, one column per source.
/// A cell uses the manifest entry for that source inside the destination's
/// skills dir; otherwise it probes `skills_dir/<source dir name>`.
pub fn deployment_matrix(
    manifest: &Manifest,
    sources: &[PathBuf],
    destinations: &[Destination],
) -> Vec<Vec<MatrixCell>> {
    destinations
        .iter()
        .map(|destination| {
            sources
                .iter()
                .map(|source| matrix_cell(manifest, source, destination))
                .collect()
        })
        .collect()
}

fn matrix_cell(manifest: &Manifest, source: &Path, destination: &Destination) -> MatrixCell {
    let owned = manifest.entries.iter().find(|entry| {
        entry.source == source
            && entry.destination.parent() == Some(destination.skills_dir.as_path())
    });
    let (path, status) = match owned {
        Some(entry) => (entry.destination.clone(), status_of(entry)),
        None => {
            let path = destination
                .skills_dir
                .join(source.file_name().unwrap_or_default());
            let status = match manifest.entry(&path) {
                Some(_) => DeploymentStatus::Conflict,
                None => match probe(&path) {
                    Ok(PathState::Missing) => DeploymentStatus::Missing,
                    _ => DeploymentStatus::Unmanaged,
                },
            };
            (path, status)
        }
    };
    MatrixCell {
        target_id: destination.target_id.clone(),
        source: source.to_path_buf(),
        path,
        status,
    }
}
