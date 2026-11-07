//! Read-only planning. Nothing in this module writes to disk.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use perkolator_core::Scope;
use walkdir::WalkDir;

use crate::fsutil::{
    PathState, fingerprint_tree, is_clean_absolute, is_loop_error, normalize, probe, real_location,
    real_path, resolve_link_target, tree_stats, unsafe_copy_symlink,
};
use crate::manifest::MANAGED_TARGET_ID;
use crate::{
    Conflict, DeployRequest, DeploymentMode, Manifest, ManifestEntry, Plan, PlanKind, PlannedOp,
};

const MAX_NAME_LEN: usize = 255;

/// Flat directory name: `[a-z0-9._-]+`, not starting with `.`, at most 255 bytes.
pub fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with('.')
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

struct SourceInfo {
    real: PathBuf,
    /// Present for copy modes.
    fingerprint: Option<String>,
    files: usize,
    bytes: u64,
}

/// What a planned path should end up as.
enum Desired<'a> {
    Link(&'a Path),
    Copy,
}

struct Builder<'a> {
    plan: Plan,
    manifest: &'a Manifest,
    created_dirs: BTreeSet<PathBuf>,
}

impl<'a> Builder<'a> {
    fn conflict(&mut self, conflict: Conflict) {
        if !self.plan.conflicts.contains(&conflict) {
            self.plan.conflicts.push(conflict);
        }
    }

    fn op(&mut self, op: PlannedOp) {
        self.plan.operations.push(op);
    }

    /// Plan creation of missing ancestors, top-down. Existing ancestors above
    /// the skills dir may be symlinks (e.g. macOS `/var`); they must resolve
    /// to directories.
    fn ensure_dir(&mut self, dir: &Path) -> bool {
        let mut missing = Vec::new();
        let mut current = dir;
        loop {
            if self.created_dirs.contains(current) {
                break;
            }
            match probe(current) {
                Ok(PathState::Missing) => {
                    missing.push(current.to_path_buf());
                    match current.parent() {
                        Some(parent) => current = parent,
                        None => break,
                    }
                }
                Ok(PathState::Dir) => break,
                Ok(PathState::Symlink { target }) => {
                    if fs::metadata(current).map(|m| m.is_dir()).unwrap_or(false) {
                        break;
                    }
                    self.conflict(Conflict::BrokenForeignSymlink {
                        path: current.to_path_buf(),
                        target,
                    });
                    return false;
                }
                Ok(state) => {
                    self.conflict(Conflict::UnmanagedExists {
                        path: current.to_path_buf(),
                        kind: state.kind_label().to_owned(),
                    });
                    return false;
                }
                Err(error) => {
                    self.conflict(Conflict::SourceUnreadable {
                        path: current.to_path_buf(),
                        error: error.to_string(),
                    });
                    return false;
                }
            }
        }
        for path in missing.into_iter().rev() {
            self.created_dirs.insert(path.clone());
            self.op(PlannedOp::CreateDir { path });
        }
        true
    }

    /// Plan one path (destination or managed path) towards `desired`.
    fn plan_path(
        &mut self,
        path: &Path,
        desired: Desired<'_>,
        source: &SourceInfo,
        entry: ManifestEntry,
    ) {
        let owned = self.manifest.entry(path).cloned();
        let source_path = entry.source.clone();
        let create = |builder: &mut Self| match desired {
            Desired::Link(target) => builder.op(PlannedOp::CreateSymlink {
                link: path.to_path_buf(),
                target: target.to_path_buf(),
            }),
            Desired::Copy => builder.op(PlannedOp::CopyTree {
                from: source_path.clone(),
                to: path.to_path_buf(),
                files: source.files,
                bytes: source.bytes,
                fingerprint: source.fingerprint.clone().unwrap_or_default(),
            }),
        };
        let state = match probe(path) {
            Ok(state) => state,
            Err(error) => {
                self.conflict(Conflict::SourceUnreadable {
                    path: path.to_path_buf(),
                    error: error.to_string(),
                });
                return;
            }
        };
        match state {
            PathState::Missing => create(self),
            PathState::Symlink { target: raw } => {
                let owned_link = owned
                    .as_ref()
                    .is_some_and(|e| e.link_target.as_deref() == Some(raw.as_path()));
                let matches_desired = match desired {
                    Desired::Link(target) => resolve_link_target(path, &raw) == normalize(target),
                    Desired::Copy => false,
                };
                if matches_desired {
                    self.op(PlannedOp::AlreadySatisfied {
                        path: path.to_path_buf(),
                        owned: owned_link,
                    });
                    if !owned_link {
                        return;
                    }
                } else if owned_link {
                    match desired {
                        Desired::Link(target) => self.op(PlannedOp::ReplaceOwnedSymlink {
                            link: path.to_path_buf(),
                            old_target: raw,
                            new_target: target.to_path_buf(),
                        }),
                        Desired::Copy => {
                            self.op(PlannedOp::RemoveOwnedSymlink {
                                link: path.to_path_buf(),
                                target: raw,
                            });
                            create(self);
                        }
                    }
                } else if fs::metadata(path).is_err() {
                    self.conflict(Conflict::BrokenForeignSymlink {
                        path: path.to_path_buf(),
                        target: raw,
                    });
                    return;
                } else {
                    self.conflict(Conflict::ForeignSymlink {
                        path: path.to_path_buf(),
                        target: raw,
                    });
                    return;
                }
            }
            PathState::Dir => {
                let Some(recorded) = owned
                    .as_ref()
                    .filter(|e| e.is_copy())
                    .and_then(|e| e.fingerprint.clone())
                else {
                    self.conflict(Conflict::UnmanagedExists {
                        path: path.to_path_buf(),
                        kind: "directory".to_owned(),
                    });
                    return;
                };
                match fingerprint_tree(path) {
                    Ok(current) if current != recorded => {
                        self.conflict(Conflict::DivergedCopy {
                            path: path.to_path_buf(),
                        });
                        return;
                    }
                    Ok(current) => {
                        if matches!(desired, Desired::Copy)
                            && source.fingerprint.as_deref() == Some(current.as_str())
                        {
                            self.op(PlannedOp::AlreadySatisfied {
                                path: path.to_path_buf(),
                                owned: true,
                            });
                        } else {
                            self.op(PlannedOp::RemoveOwnedCopy {
                                path: path.to_path_buf(),
                                fingerprint: current,
                            });
                            create(self);
                        }
                    }
                    Err(error) => {
                        self.conflict(Conflict::SourceUnreadable {
                            path: path.to_path_buf(),
                            error: error.to_string(),
                        });
                        return;
                    }
                }
            }
            other => {
                self.conflict(Conflict::UnmanagedExists {
                    path: path.to_path_buf(),
                    kind: other.kind_label().to_owned(),
                });
                return;
            }
        }
        self.plan.manifest_upserts.push(entry);
    }
}

/// Approved-root and symlinked-parent policy for a path we would write.
///
/// - With approved roots: the path must lie lexically inside a root, and its
///   physical location (parents resolved) must also lie inside a root;
///   a symlinked parent that leads elsewhere is `SymlinkedParent`.
/// - Without approved roots: the immediate parent (the skills dir) must not
///   be a symlink. Approve its real location to allow it explicitly.
fn check_location(
    path: &Path,
    approved: &[PathBuf],
    canonical: &[PathBuf],
    builder: &mut Builder<'_>,
) -> bool {
    let parent = path.parent().unwrap_or(Path::new("/"));
    if approved.is_empty() {
        if let Ok(PathState::Symlink { .. }) = probe(parent) {
            builder.conflict(Conflict::SymlinkedParent {
                path: parent.to_path_buf(),
            });
            return false;
        }
        return true;
    }
    let inside_lexically = approved
        .iter()
        .zip(canonical)
        .any(|(root, real_root)| path.starts_with(root) || path.starts_with(real_root));
    if !inside_lexically {
        builder.conflict(Conflict::OutsideApprovedRoots {
            path: path.to_path_buf(),
        });
        return false;
    }
    let real = real_location(path);
    if !canonical.iter().any(|root| real.starts_with(root)) {
        let symlinked = path
            .ancestors()
            .skip(1)
            .find(|ancestor| matches!(probe(ancestor), Ok(PathState::Symlink { .. })))
            .unwrap_or(parent);
        builder.conflict(Conflict::SymlinkedParent {
            path: symlinked.to_path_buf(),
        });
        return false;
    }
    true
}

fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn check_source(request: &DeployRequest, builder: &mut Builder<'_>) -> Option<SourceInfo> {
    let source = &request.source;
    if !is_clean_absolute(source) {
        builder.conflict(Conflict::InvalidPath {
            path: source.clone(),
        });
        return None;
    }
    let Ok(meta) = fs::metadata(source) else {
        builder.conflict(Conflict::SourceMissing {
            path: source.clone(),
        });
        return None;
    };
    let skill_md = fs::symlink_metadata(source.join("SKILL.md"));
    if !meta.is_dir() || !skill_md.map(|m| m.is_file()).unwrap_or(false) {
        builder.conflict(Conflict::SourceNotSkill {
            path: source.clone(),
        });
        return None;
    }
    let real = match fs::canonicalize(source) {
        Ok(real) => real,
        Err(error) => {
            builder.conflict(Conflict::SourceUnreadable {
                path: source.clone(),
                error: error.to_string(),
            });
            return None;
        }
    };
    let copies = request.mode.copies();
    let before = builder.plan.conflicts.len();
    for entry in WalkDir::new(&real).follow_links(false).min_depth(1) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                builder.conflict(Conflict::SourceUnreadable {
                    path: error
                        .path()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| real.clone()),
                    error: error.to_string(),
                });
                continue;
            }
        };
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            if let Err(error) = fs::canonicalize(entry.path())
                && is_loop_error(&error)
            {
                builder.conflict(Conflict::SourceSymlinkLoop {
                    path: entry.path().to_path_buf(),
                });
                continue;
            }
            if copies {
                let raw = fs::read_link(entry.path()).unwrap_or_default();
                if unsafe_copy_symlink(&real, entry.path(), &raw) {
                    builder.conflict(Conflict::UnsafeSourceSymlink {
                        path: entry.path().to_path_buf(),
                        target: raw,
                    });
                }
            }
        } else if copies && !file_type.is_dir() && !file_type.is_file() {
            builder.conflict(Conflict::UnsupportedFileType {
                path: entry.path().to_path_buf(),
            });
        }
    }
    if builder.plan.conflicts.len() != before {
        return None;
    }
    let (fingerprint, files, bytes) = if copies {
        match (fingerprint_tree(&real), tree_stats(&real)) {
            (Ok(fingerprint), Ok((files, bytes))) => (Some(fingerprint), files, bytes),
            (Err(error), _) | (_, Err(error)) => {
                builder.conflict(Conflict::SourceUnreadable {
                    path: source.clone(),
                    error: error.to_string(),
                });
                return None;
            }
        }
    } else {
        (None, 0, 0)
    };
    Some(SourceInfo {
        real,
        fingerprint,
        files,
        bytes,
    })
}

/// Dry-run a deployment. Read-only: lstat, readlink, fingerprint.
pub fn plan_deploy(request: &DeployRequest, manifest: &Manifest) -> Plan {
    let mut builder = Builder {
        plan: Plan::new(PlanKind::Deploy, Some(request.clone())),
        manifest,
        created_dirs: BTreeSet::new(),
    };
    if !is_valid_skill_name(&request.name) {
        builder.conflict(Conflict::InvalidName {
            name: request.name.clone(),
        });
    }
    let source = check_source(request, &mut builder);
    let canonical_roots: Vec<PathBuf> = request
        .approved_roots
        .iter()
        .map(|root| real_path(root))
        .collect();
    for root in &request.approved_roots {
        if !is_clean_absolute(root) {
            builder.conflict(Conflict::InvalidPath { path: root.clone() });
        }
    }
    if request.destinations.is_empty() && !request.mode.needs_managed_root() {
        return builder.plan;
    }
    let name_ok = is_valid_skill_name(&request.name);
    let mut seen = BTreeSet::new();

    let managed_path = if request.mode.needs_managed_root() {
        match &request.managed_root {
            None => {
                builder.conflict(Conflict::ManagedRootRequired);
                None
            }
            Some(root) if !is_clean_absolute(root) => {
                builder.conflict(Conflict::InvalidPath { path: root.clone() });
                None
            }
            Some(root) => Some(root.join(&request.name)),
        }
    } else {
        None
    };

    let (Some(source), true) = (source, name_ok) else {
        return builder.plan;
    };

    let entry_for = |destination: &Path,
                     target_id: &str,
                     scope: Scope,
                     link_target: Option<PathBuf>| ManifestEntry {
        destination: destination.to_path_buf(),
        source: request.source.clone(),
        mode: request.mode,
        target_id: target_id.to_owned(),
        scope,
        fingerprint: if link_target.is_none() {
            source.fingerprint.clone()
        } else {
            None
        },
        link_target,
        applied_at_unix: 0,
    };

    if let Some(managed) = &managed_path {
        let root = managed.parent().unwrap_or(Path::new("/")).to_path_buf();
        let real = real_location(managed);
        if overlaps(&real, &source.real) {
            builder.conflict(Conflict::SelfReference {
                path: managed.clone(),
            });
        } else if check_location(
            managed,
            &request.approved_roots,
            &canonical_roots,
            &mut builder,
        ) && builder.ensure_dir(&root)
        {
            seen.insert(real);
            let scope = Scope::Custom { path: root };
            match request.mode {
                DeploymentMode::LinkSource => {
                    let entry = entry_for(
                        managed,
                        MANAGED_TARGET_ID,
                        scope,
                        Some(request.source.clone()),
                    );
                    builder.plan_path(managed, Desired::Link(&request.source), &source, entry);
                }
                _ => {
                    let entry = entry_for(managed, MANAGED_TARGET_ID, scope, None);
                    builder.plan_path(managed, Desired::Copy, &source, entry);
                }
            }
        }
    }
    if request.mode.needs_managed_root() && managed_path.is_none() {
        return builder.plan;
    }
    let link_target: Option<&Path> = match request.mode {
        DeploymentMode::Symlink => Some(&request.source),
        DeploymentMode::ManagedCopy | DeploymentMode::LinkSource => managed_path.as_deref(),
        DeploymentMode::IndependentCopy => None,
    };

    for destination in &request.destinations {
        if request.mode.uses_symlinks() && !destination.supports_symlinks {
            builder.conflict(Conflict::ModeUnsupportedByTarget {
                target_id: destination.target_id.clone(),
            });
            continue;
        }
        if !is_clean_absolute(&destination.skills_dir) {
            builder.conflict(Conflict::InvalidPath {
                path: destination.skills_dir.clone(),
            });
            continue;
        }
        let path = destination.skills_dir.join(&request.name);
        if !check_location(
            &path,
            &request.approved_roots,
            &canonical_roots,
            &mut builder,
        ) {
            continue;
        }
        let real = real_location(&path);
        if overlaps(&real, &source.real) {
            builder.conflict(Conflict::SelfReference { path });
            continue;
        }
        if !seen.insert(real) {
            builder.conflict(Conflict::DuplicateDestination { path });
            continue;
        }
        if !builder.ensure_dir(&destination.skills_dir) {
            continue;
        }
        let entry = entry_for(
            &path,
            &destination.target_id,
            destination.scope.clone(),
            link_target.map(Path::to_path_buf),
        );
        let desired = match link_target {
            Some(target) => Desired::Link(target),
            None => Desired::Copy,
        };
        builder.plan_path(&path, desired, &source, entry);
    }
    builder.plan
}

/// Plan removal of an owned deployment. Only the deployment itself is
/// removed (link or verified copy), never the canonical source.
pub fn plan_remove(destination_path: &Path, manifest: &Manifest) -> Plan {
    let mut builder = Builder {
        plan: Plan::new(PlanKind::Remove, None),
        manifest,
        created_dirs: BTreeSet::new(),
    };
    let path = destination_path;
    let Some(entry) = manifest.entry(path).cloned() else {
        builder.conflict(Conflict::NotManaged {
            path: path.to_path_buf(),
        });
        return builder.plan;
    };
    let dependents = manifest.dependents_of(path);
    if dependents > 0 {
        builder.conflict(Conflict::ManagedPathInUse {
            path: path.to_path_buf(),
            dependents,
        });
        return builder.plan;
    }
    if manifest
        .entries
        .iter()
        .any(|e| overlaps(&normalize(&e.source), &normalize(path)))
    {
        builder.conflict(Conflict::SelfReference {
            path: path.to_path_buf(),
        });
        return builder.plan;
    }
    match probe(path) {
        Ok(PathState::Missing) => builder.op(PlannedOp::ForgetEntry {
            path: path.to_path_buf(),
        }),
        Ok(PathState::Symlink { target }) => {
            if entry.link_target.as_deref() == Some(target.as_path()) {
                builder.op(PlannedOp::RemoveOwnedSymlink {
                    link: path.to_path_buf(),
                    target,
                });
            } else if fs::metadata(path).is_err() {
                builder.conflict(Conflict::BrokenForeignSymlink {
                    path: path.to_path_buf(),
                    target,
                });
            } else {
                builder.conflict(Conflict::ForeignSymlink {
                    path: path.to_path_buf(),
                    target,
                });
            }
        }
        Ok(PathState::Dir) if entry.is_copy() => {
            let recorded = entry.fingerprint.clone().unwrap_or_default();
            match fingerprint_tree(path) {
                Ok(current) if current == recorded => builder.op(PlannedOp::RemoveOwnedCopy {
                    path: path.to_path_buf(),
                    fingerprint: current,
                }),
                Ok(_) => builder.conflict(Conflict::DivergedCopy {
                    path: path.to_path_buf(),
                }),
                Err(error) => builder.conflict(Conflict::SourceUnreadable {
                    path: path.to_path_buf(),
                    error: error.to_string(),
                }),
            }
        }
        Ok(state) => builder.conflict(Conflict::UnmanagedExists {
            path: path.to_path_buf(),
            kind: state.kind_label().to_owned(),
        }),
        Err(error) => builder.conflict(Conflict::SourceUnreadable {
            path: path.to_path_buf(),
            error: error.to_string(),
        }),
    }
    builder.plan.manifest_removals.push(path.to_path_buf());
    builder.plan
}

/// Plan repair of an owned link: recreate it when missing, or re-point it to
/// `new_target` (e.g. after the library moved). Target data is never touched.
/// Foreign or unowned links are refused.
pub fn plan_repair(
    destination_path: &Path,
    manifest: &Manifest,
    new_target: Option<&Path>,
) -> Plan {
    let mut builder = Builder {
        plan: Plan::new(PlanKind::Repair, None),
        manifest,
        created_dirs: BTreeSet::new(),
    };
    let path = destination_path;
    let Some(entry) = manifest.entry(path).cloned() else {
        match probe(path) {
            Ok(PathState::Symlink { target }) if fs::metadata(path).is_err() => {
                builder.conflict(Conflict::BrokenForeignSymlink {
                    path: path.to_path_buf(),
                    target,
                });
            }
            Ok(PathState::Symlink { target }) => builder.conflict(Conflict::ForeignSymlink {
                path: path.to_path_buf(),
                target,
            }),
            _ => builder.conflict(Conflict::NotManaged {
                path: path.to_path_buf(),
            }),
        }
        return builder.plan;
    };
    let Some(recorded) = entry.link_target.clone() else {
        builder.conflict(Conflict::RepairUnsupported {
            path: path.to_path_buf(),
        });
        return builder.plan;
    };
    let desired = new_target
        .map(Path::to_path_buf)
        .unwrap_or_else(|| recorded.clone());
    if !is_clean_absolute(&desired) {
        builder.conflict(Conflict::InvalidPath { path: desired });
        return builder.plan;
    }
    let is_skill = fs::metadata(&desired).map(|m| m.is_dir()).unwrap_or(false)
        && fs::metadata(desired.join("SKILL.md"))
            .map(|m| m.is_file())
            .unwrap_or(false);
    if !is_skill {
        builder.conflict(Conflict::RepairTargetMissing { path: desired });
        return builder.plan;
    }
    if let Ok(real_desired) = fs::canonicalize(&desired)
        && overlaps(&real_location(path), &real_desired)
    {
        builder.conflict(Conflict::SelfReference {
            path: path.to_path_buf(),
        });
        return builder.plan;
    }
    match probe(path) {
        Ok(PathState::Missing) => {
            if !builder.ensure_dir(path.parent().unwrap_or(Path::new("/"))) {
                return builder.plan;
            }
            builder.op(PlannedOp::CreateSymlink {
                link: path.to_path_buf(),
                target: desired.clone(),
            });
        }
        Ok(PathState::Symlink { target: raw }) if raw == recorded => {
            if raw == desired {
                builder.op(PlannedOp::AlreadySatisfied {
                    path: path.to_path_buf(),
                    owned: true,
                });
            } else {
                builder.op(PlannedOp::ReplaceOwnedSymlink {
                    link: path.to_path_buf(),
                    old_target: raw,
                    new_target: desired.clone(),
                });
            }
        }
        Ok(PathState::Symlink { target }) => {
            let conflict = if fs::metadata(path).is_err() {
                Conflict::BrokenForeignSymlink {
                    path: path.to_path_buf(),
                    target,
                }
            } else {
                Conflict::ForeignSymlink {
                    path: path.to_path_buf(),
                    target,
                }
            };
            builder.conflict(conflict);
            return builder.plan;
        }
        Ok(state) => {
            builder.conflict(Conflict::UnmanagedExists {
                path: path.to_path_buf(),
                kind: state.kind_label().to_owned(),
            });
            return builder.plan;
        }
        Err(error) => {
            builder.conflict(Conflict::SourceUnreadable {
                path: path.to_path_buf(),
                error: error.to_string(),
            });
            return builder.plan;
        }
    }
    let mut repaired = entry;
    if repaired.mode == DeploymentMode::Symlink {
        repaired.source = desired.clone();
    }
    repaired.link_target = Some(desired);
    builder.plan.manifest_upserts.push(repaired);
    builder.plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_names() {
        for good in ["a", "my-skill", "x_1.2"] {
            assert!(is_valid_skill_name(good), "{good}");
        }
        for bad in [
            "",
            ".hidden",
            "..",
            "a/b",
            "A",
            "a b",
            "../x",
            &"a".repeat(256),
        ] {
            assert!(!is_valid_skill_name(bad), "{bad}");
        }
    }
}
