use std::fmt::Write as _;
use std::path::PathBuf;

use perkolator_core::Scope;
use perkolator_targets::{AgentTarget, HomeEnv, TargetError, resolve_skill_dir};
use serde::{Deserialize, Serialize};

use crate::ManifestEntry;

/// How a skill reaches a destination.
///
/// Semantics follow vercel-labs/skills `src/installer.ts` (commit d3701c1,
/// 2026-11-06) where they overlap, minus its unsafe parts: Perkolator never
/// deletes an existing destination and never falls back from symlink to copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentMode {
    /// Destination is a symlink to the canonical source directory.
    Symlink,
    /// Source is copied into `managed_root/<name>`; destinations link to it.
    ManagedCopy,
    /// `managed_root/<name>` is a symlink to the source; destinations link to it.
    LinkSource,
    /// Source is copied into every destination; tracked by fingerprint.
    IndependentCopy,
}

impl DeploymentMode {
    pub fn uses_symlinks(self) -> bool {
        !matches!(self, DeploymentMode::IndependentCopy)
    }

    pub fn copies(self) -> bool {
        matches!(
            self,
            DeploymentMode::ManagedCopy | DeploymentMode::IndependentCopy
        )
    }

    pub fn needs_managed_root(self) -> bool {
        matches!(
            self,
            DeploymentMode::ManagedCopy | DeploymentMode::LinkSource
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            DeploymentMode::Symlink => "symlink",
            DeploymentMode::ManagedCopy => "managed copy",
            DeploymentMode::LinkSource => "link source",
            DeploymentMode::IndependentCopy => "independent copy",
        }
    }
}

/// A resolved skills directory of one agent target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub target_id: String,
    pub scope: Scope,
    pub skills_dir: PathBuf,
    pub supports_symlinks: bool,
}

impl Destination {
    pub fn for_target(
        target: &AgentTarget,
        scope: Scope,
        home: &HomeEnv,
    ) -> Result<Self, TargetError> {
        let skills_dir = resolve_skill_dir(target, &scope, home)?;
        Ok(Self {
            target_id: target.id.clone(),
            scope,
            skills_dir,
            supports_symlinks: target.supports_symlinks,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployRequest {
    /// Canonical skill directory (absolute; must contain `SKILL.md`).
    pub source: PathBuf,
    /// Flat directory name created in each skills dir: `[a-z0-9._-]`,
    /// not starting with `.`.
    pub name: String,
    pub mode: DeploymentMode,
    pub destinations: Vec<Destination>,
    /// Required for `ManagedCopy` / `LinkSource`.
    pub managed_root: Option<PathBuf>,
    /// When non-empty, every written path must resolve inside one of these.
    pub approved_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum PlannedOp {
    CreateDir {
        path: PathBuf,
    },
    CreateSymlink {
        link: PathBuf,
        target: PathBuf,
    },
    /// Copied into a sibling staging dir, verified, then renamed into place.
    CopyTree {
        from: PathBuf,
        to: PathBuf,
        files: usize,
        bytes: u64,
        fingerprint: String,
    },
    AlreadySatisfied {
        path: PathBuf,
        /// `false` when an unmanaged entry already matches; it is left
        /// unmanaged (never adopted silently).
        owned: bool,
    },
    ReplaceOwnedSymlink {
        link: PathBuf,
        old_target: PathBuf,
        new_target: PathBuf,
    },
    RemoveOwnedSymlink {
        link: PathBuf,
        target: PathBuf,
    },
    /// Moved to a sibling trash dir; deleted only after the manifest commits.
    RemoveOwnedCopy {
        path: PathBuf,
        fingerprint: String,
    },
    /// Manifest-only: the owned entry no longer exists on disk.
    ForgetEntry {
        path: PathBuf,
    },
}

impl PlannedOp {
    pub fn describe(&self) -> String {
        match self {
            PlannedOp::CreateDir { path } => format!("create directory {}", path.display()),
            PlannedOp::CreateSymlink { link, target } => {
                format!("link {} -> {}", link.display(), target.display())
            }
            PlannedOp::CopyTree {
                from,
                to,
                files,
                bytes,
                ..
            } => format!(
                "copy {} -> {} ({files} files, {bytes} bytes)",
                from.display(),
                to.display()
            ),
            PlannedOp::AlreadySatisfied { path, owned } => format!(
                "already in place: {}{}",
                path.display(),
                if *owned {
                    ""
                } else {
                    " (unmanaged, left as is)"
                }
            ),
            PlannedOp::ReplaceOwnedSymlink {
                link,
                old_target,
                new_target,
            } => format!(
                "re-point {}: {} -> {}",
                link.display(),
                old_target.display(),
                new_target.display()
            ),
            PlannedOp::RemoveOwnedSymlink { link, .. } => {
                format!("remove managed link {}", link.display())
            }
            PlannedOp::RemoveOwnedCopy { path, .. } => {
                format!("remove managed copy {}", path.display())
            }
            PlannedOp::ForgetEntry { path } => {
                format!("forget missing deployment {}", path.display())
            }
        }
    }

    pub fn is_noop(&self) -> bool {
        matches!(
            self,
            PlannedOp::AlreadySatisfied { .. } | PlannedOp::ForgetEntry { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "conflict", rename_all = "kebab-case")]
pub enum Conflict {
    SourceMissing {
        path: PathBuf,
    },
    /// Not a directory or no `SKILL.md` regular file at its root.
    SourceNotSkill {
        path: PathBuf,
    },
    SourceSymlinkLoop {
        path: PathBuf,
    },
    /// Copy modes only: symlink is absolute or escapes the source root.
    UnsafeSourceSymlink {
        path: PathBuf,
        target: PathBuf,
    },
    /// Copy modes only: fifo, socket, device.
    UnsupportedFileType {
        path: PathBuf,
    },
    SourceUnreadable {
        path: PathBuf,
        error: String,
    },
    InvalidName {
        name: String,
    },
    /// Must be absolute without `.`/`..` components.
    InvalidPath {
        path: PathBuf,
    },
    ManagedRootRequired,
    UnmanagedExists {
        path: PathBuf,
        kind: String,
    },
    ForeignSymlink {
        path: PathBuf,
        target: PathBuf,
    },
    BrokenForeignSymlink {
        path: PathBuf,
        target: PathBuf,
    },
    DivergedCopy {
        path: PathBuf,
    },
    SelfReference {
        path: PathBuf,
    },
    OutsideApprovedRoots {
        path: PathBuf,
    },
    SymlinkedParent {
        path: PathBuf,
    },
    DuplicateDestination {
        path: PathBuf,
    },
    ModeUnsupportedByTarget {
        target_id: String,
    },
    NotManaged {
        path: PathBuf,
    },
    ManagedPathInUse {
        path: PathBuf,
        dependents: usize,
    },
    RepairTargetMissing {
        path: PathBuf,
    },
    RepairUnsupported {
        path: PathBuf,
    },
}

impl Conflict {
    pub fn describe(&self) -> String {
        match self {
            Conflict::SourceMissing { path } => format!("source {} does not exist", path.display()),
            Conflict::SourceNotSkill { path } => {
                format!("{} is not a skill directory (no SKILL.md)", path.display())
            }
            Conflict::SourceSymlinkLoop { path } => {
                format!("symlink loop in source at {}", path.display())
            }
            Conflict::UnsafeSourceSymlink { path, target } => format!(
                "symlink {} -> {} leaves the source; refusing to copy",
                path.display(),
                target.display()
            ),
            Conflict::UnsupportedFileType { path } => {
                format!("unsupported special file {}", path.display())
            }
            Conflict::SourceUnreadable { path, error } => {
                format!("cannot read {}: {error}", path.display())
            }
            Conflict::InvalidName { name } => format!("invalid skill directory name `{name}`"),
            Conflict::InvalidPath { path } => {
                format!("{} must be an absolute, normalised path", path.display())
            }
            Conflict::ManagedRootRequired => "this mode needs a managed library root".to_owned(),
            Conflict::UnmanagedExists { path, kind } => {
                format!("unmanaged {kind} already exists at {}", path.display())
            }
            Conflict::ForeignSymlink { path, target } => format!(
                "{} is a symlink to {} not created by Perkolator",
                path.display(),
                target.display()
            ),
            Conflict::BrokenForeignSymlink { path, target } => format!(
                "{} is a broken symlink to {} not created by Perkolator",
                path.display(),
                target.display()
            ),
            Conflict::DivergedCopy { path } => {
                format!(
                    "managed copy {} was modified since deployment",
                    path.display()
                )
            }
            Conflict::SelfReference { path } => {
                format!("{} overlaps the source", path.display())
            }
            Conflict::OutsideApprovedRoots { path } => {
                format!("{} is outside the approved roots", path.display())
            }
            Conflict::SymlinkedParent { path } => format!(
                "{} is a symlink; the write would land elsewhere",
                path.display()
            ),
            Conflict::DuplicateDestination { path } => {
                format!("{} is targeted more than once", path.display())
            }
            Conflict::ModeUnsupportedByTarget { target_id } => {
                format!("target `{target_id}` does not support symlinks")
            }
            Conflict::NotManaged { path } => {
                format!("{} is not a Perkolator deployment", path.display())
            }
            Conflict::ManagedPathInUse { path, dependents } => format!(
                "{} is still linked by {dependents} deployment(s)",
                path.display()
            ),
            Conflict::RepairTargetMissing { path } => {
                format!("repair target {} is missing or not a skill", path.display())
            }
            Conflict::RepairUnsupported { path } => {
                format!("{} is a copy; repair only re-points links", path.display())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanKind {
    Deploy,
    Remove,
    Repair,
}

/// A read-only dry run. Nothing is written until an [`crate::Executor`]
/// applies it, and it re-verifies every precondition first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub kind: PlanKind,
    pub request: Option<DeployRequest>,
    pub operations: Vec<PlannedOp>,
    pub conflicts: Vec<Conflict>,
    /// Entries written to the manifest after every operation succeeds
    /// (`applied_at_unix` is stamped at commit).
    pub manifest_upserts: Vec<ManifestEntry>,
    pub manifest_removals: Vec<PathBuf>,
}

impl Plan {
    pub(crate) fn new(kind: PlanKind, request: Option<DeployRequest>) -> Self {
        Self {
            kind,
            request,
            operations: Vec::new(),
            conflicts: Vec::new(),
            manifest_upserts: Vec::new(),
            manifest_removals: Vec::new(),
        }
    }

    pub fn is_applicable(&self) -> bool {
        self.conflicts.is_empty()
    }

    /// `true` when applying would not touch the filesystem.
    pub fn is_noop(&self) -> bool {
        self.operations.iter().all(PlannedOp::is_noop)
    }

    pub fn summary(&self) -> String {
        let mut out = String::new();
        let heading = match (&self.kind, &self.request) {
            (PlanKind::Deploy, Some(request)) => format!(
                "Deploy `{}` ({}) to {} destination(s)",
                request.name,
                request.mode.label(),
                request.destinations.len()
            ),
            (PlanKind::Deploy, None) => "Deploy".to_owned(),
            (PlanKind::Remove, _) => "Remove deployment".to_owned(),
            (PlanKind::Repair, _) => "Repair deployment".to_owned(),
        };
        let changes = self.operations.iter().filter(|op| !op.is_noop()).count();
        let _ = writeln!(
            out,
            "{heading}: {changes} change(s), {} conflict(s)",
            self.conflicts.len()
        );
        for op in &self.operations {
            let _ = writeln!(out, "  + {}", op.describe());
        }
        for conflict in &self.conflicts {
            let _ = writeln!(out, "  ! {}", conflict.describe());
        }
        out
    }
}
