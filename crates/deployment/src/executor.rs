//! Journalled apply and crash recovery.
//!
//! Every filesystem step is described by a [`Step`] that carries all concrete
//! paths (including generated staging/trash names) and is journalled with
//! fsync *before* it runs. Undo is derived from the step plus the current
//! on-disk state, so it is idempotent and works for completed and
//! half-completed steps alike. The manifest is committed only after every
//! step succeeded; until then recovery rolls the run back.
//!
//! Atomicity: a single step is atomic where POSIX allows (symlink creation,
//! `rename`); a multi-destination run is *recoverable*, not ACID. Between the
//! last pre-rename check and `rename` another process could create the
//! destination; `rename` onto an existing empty directory would then replace
//! it (std exposes no `RENAME_NOREPLACE`). Staging and trash directories are
//! hidden siblings (`.perkolator-*`) of the destination so renames never cross
//! filesystems.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use perkolator_core::CancelToken;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::fsutil::{
    PathState, copy_tree_contents, create_symlink, dir_is_empty, fingerprint_tree, probe, sync_dir,
};
use crate::{Conflict, Manifest, Plan, PlannedOp};

const JOURNAL_EXT: &str = "jsonl";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "kebab-case")]
pub enum Step {
    CreateDir {
        path: PathBuf,
    },
    CreateSymlink {
        link: PathBuf,
        target: PathBuf,
    },
    CopyTree {
        from: PathBuf,
        staging: PathBuf,
        to: PathBuf,
        fingerprint: String,
    },
    ReplaceSymlink {
        link: PathBuf,
        temp: PathBuf,
        old_target: PathBuf,
        new_target: PathBuf,
    },
    RemoveSymlink {
        link: PathBuf,
        target: PathBuf,
    },
    TrashCopy {
        path: PathBuf,
        trash: PathBuf,
        fingerprint: String,
    },
    Noop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Record {
    Begin { run_id: String, operations: usize },
    Start { index: usize, step: Step },
    Done { index: usize },
    Failed { index: usize, error: String },
    Undone { index: usize },
    UndoFailed { index: usize, error: String },
    ManifestCommitted,
    End { outcome: RunOutcome },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunOutcome {
    Applied,
    RolledBack,
    /// Recovery found the manifest committed and finished cleanup.
    Finalized,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ApplyStatus {
    Applied,
    /// Every completed step was undone; disk matches the pre-run state.
    RolledBack {
        failed_index: usize,
        error: String,
    },
    /// Some undo could not be verified; the journal stays open for
    /// [`Executor::recover`] and `issues` lists what was left in place.
    RollbackIncomplete {
        failed_index: usize,
        error: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReport {
    pub run_id: String,
    pub status: ApplyStatus,
    pub steps_applied: usize,
    pub steps_undone: usize,
    pub issues: Vec<String>,
    pub journal: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveredRun {
    pub run_id: String,
    pub outcome: Option<RunOutcome>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub runs: Vec<RecoveredRun>,
}

impl RecoveryReport {
    pub fn is_clean(&self) -> bool {
        self.runs
            .iter()
            .all(|run| run.outcome.is_some() && run.issues.is_empty())
    }
}

#[derive(Debug, Error)]
pub enum DeployError {
    #[error("plan has {} conflict(s); resolve them before applying", .0.len())]
    NotApplicable(Vec<Conflict>),
    #[error("operation failed: {}", describe_status(&.0.status))]
    OperationFailed(Box<ApplyReport>),
    #[error("journal error at {path}: {source}")]
    Journal {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("simulated crash after {0} step(s)")]
    SimulatedCrash(usize),
}

fn describe_status(status: &ApplyStatus) -> String {
    match status {
        ApplyStatus::Applied => "applied".to_owned(),
        ApplyStatus::RolledBack {
            failed_index,
            error,
        } => {
            format!("step {failed_index} failed ({error}); rolled back")
        }
        ApplyStatus::RollbackIncomplete {
            failed_index,
            error,
        } => {
            format!("step {failed_index} failed ({error}); rollback incomplete, run recover()")
        }
    }
}

/// Test hooks that emulate process death or an I/O failure at a precise point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultInjection {
    /// Stop (no undo, no journal end) after this many steps completed.
    CrashAfterSteps(usize),
    /// Stop in the middle of the first copy: staging written, not renamed.
    CrashMidCopy,
    /// Make step `n` fail with an I/O error after it was journalled.
    FailStep(usize),
}

struct Journal {
    path: PathBuf,
    file: File,
}

impl Journal {
    fn create(dir: &Path, run_id: &str) -> Result<Self, DeployError> {
        let path = dir.join(format!("{run_id}.{JOURNAL_EXT}"));
        let open = || -> io::Result<File> {
            fs::create_dir_all(dir)?;
            let file = OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(&path)?;
            sync_dir(dir)?;
            Ok(file)
        };
        let file = open().map_err(|source| DeployError::Journal {
            path: path.clone(),
            source,
        })?;
        Ok(Self { path, file })
    }

    fn reopen(path: &Path) -> Result<Self, DeployError> {
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|source| DeployError::Journal {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    fn append(&mut self, record: &Record) -> Result<(), DeployError> {
        let mut line = serde_json::to_vec(record).map_err(|error| DeployError::Journal {
            path: self.path.clone(),
            source: io::Error::new(io::ErrorKind::InvalidData, error),
        })?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.sync_data())
            .map_err(|source| DeployError::Journal {
                path: self.path.clone(),
                source,
            })
    }
}

/// Reads a journal, tolerating a torn final line from a crash mid-write.
fn read_journal(path: &Path) -> io::Result<Vec<Record>> {
    let reader = BufReader::new(File::open(path)?);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        match serde_json::from_str(&line) {
            Ok(record) => records.push(record),
            Err(_) => break,
        }
    }
    Ok(records)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:020}-{}", std::process::id())
}

fn sibling(path: &Path, kind: &str, run_id: &str, index: usize) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new("/"));
    parent.join(format!(".perkolator-{kind}-{run_id}-{index}"))
}

fn step_for(op: &PlannedOp, run_id: &str, index: usize) -> Step {
    match op {
        PlannedOp::CreateDir { path } => Step::CreateDir { path: path.clone() },
        PlannedOp::CreateSymlink { link, target } => Step::CreateSymlink {
            link: link.clone(),
            target: target.clone(),
        },
        PlannedOp::CopyTree {
            from,
            to,
            fingerprint,
            ..
        } => Step::CopyTree {
            from: from.clone(),
            staging: sibling(to, "staging", run_id, index),
            to: to.clone(),
            fingerprint: fingerprint.clone(),
        },
        PlannedOp::ReplaceOwnedSymlink {
            link,
            old_target,
            new_target,
        } => Step::ReplaceSymlink {
            link: link.clone(),
            temp: sibling(link, "link", run_id, index),
            old_target: old_target.clone(),
            new_target: new_target.clone(),
        },
        PlannedOp::RemoveOwnedSymlink { link, target } => Step::RemoveSymlink {
            link: link.clone(),
            target: target.clone(),
        },
        PlannedOp::RemoveOwnedCopy { path, fingerprint } => Step::TrashCopy {
            path: path.clone(),
            trash: sibling(path, "trash", run_id, index),
            fingerprint: fingerprint.clone(),
        },
        PlannedOp::AlreadySatisfied { .. } | PlannedOp::ForgetEntry { .. } => Step::Noop,
    }
}

fn state(path: &Path) -> Result<PathState, String> {
    probe(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn is_link_to(path: &Path, target: &Path) -> Result<bool, String> {
    Ok(matches!(state(path)?, PathState::Symlink { target: raw } if raw == target))
}

fn is_real_dir(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

fn require(condition: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message()) }
}

fn fingerprint_matches(path: &Path, expected: &str) -> bool {
    fingerprint_tree(path).is_ok_and(|actual| actual == expected)
}

/// Re-check that disk still matches what the plan saw.
fn preflight(step: &Step) -> Result<(), String> {
    let missing = |path: &Path| -> Result<(), String> {
        require(state(path)? == PathState::Missing, || {
            format!("{} appeared since planning", path.display())
        })
    };
    match step {
        Step::CreateDir { path } => {
            missing(path)?;
            let parent = path.parent().unwrap_or(Path::new("/"));
            require(is_real_dir(parent), || {
                format!("{} is not a directory", parent.display())
            })
        }
        Step::CreateSymlink { link, target } => {
            missing(link)?;
            require(is_real_dir(target), || {
                format!("link target {} is missing", target.display())
            })
        }
        Step::CopyTree {
            from,
            staging,
            to,
            fingerprint,
        } => {
            missing(to)?;
            missing(staging)?;
            require(fingerprint_matches(from, fingerprint), || {
                format!("source {} changed since planning", from.display())
            })
        }
        Step::ReplaceSymlink {
            link,
            temp,
            old_target,
            new_target,
        } => {
            require(is_link_to(link, old_target)?, || {
                format!("{} changed since planning", link.display())
            })?;
            missing(temp)?;
            require(is_real_dir(new_target), || {
                format!("link target {} is missing", new_target.display())
            })
        }
        Step::RemoveSymlink { link, target } => require(is_link_to(link, target)?, || {
            format!("{} changed since planning", link.display())
        }),
        Step::TrashCopy {
            path,
            trash,
            fingerprint,
        } => {
            require(
                state(path)? == PathState::Dir && fingerprint_matches(path, fingerprint),
                || format!("{} changed since planning", path.display()),
            )?;
            missing(trash)
        }
        Step::Noop => Ok(()),
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> String + '_ {
    move |error| format!("{}: {error}", path.display())
}

fn perform(step: &Step, crash_mid_copy: bool) -> Result<bool, String> {
    match step {
        Step::CreateDir { path } => fs::create_dir(path).map_err(io_err(path))?,
        Step::CreateSymlink { link, target } => {
            create_symlink(target, link).map_err(io_err(link))?
        }
        Step::CopyTree {
            from,
            staging,
            to,
            fingerprint,
        } => {
            fs::create_dir(staging).map_err(io_err(staging))?;
            copy_tree_contents(from, staging).map_err(io_err(from))?;
            if crash_mid_copy {
                return Ok(false);
            }
            require(fingerprint_matches(staging, fingerprint), || {
                format!(
                    "staged copy {} does not match the source",
                    staging.display()
                )
            })?;
            require(state(to)? == PathState::Missing, || {
                format!("{} appeared during copy", to.display())
            })?;
            fs::rename(staging, to).map_err(io_err(to))?;
            if let Some(parent) = to.parent() {
                sync_dir(parent).map_err(io_err(parent))?;
            }
        }
        Step::ReplaceSymlink {
            link,
            temp,
            new_target,
            ..
        } => {
            create_symlink(new_target, temp).map_err(io_err(temp))?;
            fs::rename(temp, link).map_err(io_err(link))?;
        }
        Step::RemoveSymlink { link, .. } => fs::remove_file(link).map_err(io_err(link))?,
        Step::TrashCopy { path, trash, .. } => fs::rename(path, trash).map_err(io_err(path))?,
        Step::Noop => {}
    }
    Ok(true)
}

fn verify(step: &Step) -> Result<(), String> {
    match step {
        Step::CreateDir { path } => require(state(path)? == PathState::Dir, || {
            format!("{} was not created", path.display())
        }),
        Step::CreateSymlink { link, target } => require(is_link_to(link, target)?, || {
            format!("{} does not point at {}", link.display(), target.display())
        }),
        Step::ReplaceSymlink {
            link, new_target, ..
        } => require(is_link_to(link, new_target)?, || {
            format!(
                "{} does not point at {}",
                link.display(),
                new_target.display()
            )
        }),
        Step::CopyTree {
            to, fingerprint, ..
        } => require(
            state(to)? == PathState::Dir && fingerprint_matches(to, fingerprint),
            || format!("copy at {} does not match the source", to.display()),
        ),
        Step::RemoveSymlink { link, .. } => require(state(link)? == PathState::Missing, || {
            format!("{} still exists", link.display())
        }),
        Step::TrashCopy { path, trash, .. } => require(
            state(path)? == PathState::Missing && state(trash)? == PathState::Dir,
            || format!("{} was not moved aside", path.display()),
        ),
        Step::Noop => Ok(()),
    }
}

/// Undo `step` based on current disk state. Idempotent; only removes what is
/// provably ours (recorded generated names, or exact link target / fingerprint
/// match). Anything else is left in place and reported.
fn undo(step: &Step) -> Result<(), String> {
    match step {
        Step::CreateDir { path } => match state(path)? {
            PathState::Missing => Ok(()),
            PathState::Dir if dir_is_empty(path).map_err(io_err(path))? => {
                fs::remove_dir(path).map_err(io_err(path))
            }
            _ => Err(format!("{} is not empty; left in place", path.display())),
        },
        Step::CreateSymlink { link, target } => match state(link)? {
            PathState::Missing => Ok(()),
            PathState::Symlink { target: raw } if &raw == target => {
                fs::remove_file(link).map_err(io_err(link))
            }
            _ => Err(format!("{} changed; left in place", link.display())),
        },
        Step::CopyTree {
            staging,
            to,
            fingerprint,
            ..
        } => {
            match state(staging)? {
                PathState::Missing => {}
                PathState::Dir => fs::remove_dir_all(staging).map_err(io_err(staging))?,
                _ => {
                    return Err(format!(
                        "unexpected entry at {}; left in place",
                        staging.display()
                    ));
                }
            }
            match state(to)? {
                PathState::Missing => Ok(()),
                PathState::Dir if fingerprint_matches(to, fingerprint) => {
                    fs::remove_dir_all(to).map_err(io_err(to))
                }
                _ => Err(format!(
                    "{} was modified after copying; left in place",
                    to.display()
                )),
            }
        }
        Step::ReplaceSymlink {
            link,
            temp,
            old_target,
            new_target,
        } => {
            if let PathState::Symlink { .. } = state(temp)? {
                fs::remove_file(temp).map_err(io_err(temp))?;
            }
            match state(link)? {
                PathState::Symlink { target: raw } if &raw == old_target => Ok(()),
                PathState::Symlink { target: raw } if &raw == new_target => {
                    create_symlink(old_target, temp).map_err(io_err(temp))?;
                    fs::rename(temp, link).map_err(io_err(link))
                }
                _ => Err(format!("{} changed; left in place", link.display())),
            }
        }
        Step::RemoveSymlink { link, target } => match state(link)? {
            PathState::Missing => create_symlink(target, link).map_err(io_err(link)),
            PathState::Symlink { target: raw } if &raw == target => Ok(()),
            _ => Err(format!(
                "{} was replaced; cannot restore link",
                link.display()
            )),
        },
        Step::TrashCopy { path, trash, .. } => match (state(path)?, state(trash)?) {
            (PathState::Missing, PathState::Dir) => fs::rename(trash, path).map_err(io_err(trash)),
            (PathState::Dir, PathState::Missing) => Ok(()),
            _ => Err(format!(
                "cannot restore {} from {}",
                path.display(),
                trash.display()
            )),
        },
        Step::Noop => Ok(()),
    }
}

/// Post-commit cleanup: delete trashed copies whose identity still matches.
fn finalize(step: &Step) -> Result<(), String> {
    match step {
        Step::TrashCopy {
            trash, fingerprint, ..
        } => match state(trash)? {
            PathState::Missing => Ok(()),
            PathState::Dir if fingerprint_matches(trash, fingerprint) => {
                fs::remove_dir_all(trash).map_err(io_err(trash))
            }
            _ => Err(format!("trash {} changed; left in place", trash.display())),
        },
        _ => Ok(()),
    }
}

/// Undo started steps in reverse order, journalling each result.
fn rollback(
    journal: &mut Journal,
    started: &[(usize, Step)],
    issues: &mut Vec<String>,
) -> Result<usize, DeployError> {
    let mut undone = 0;
    for (index, step) in started.iter().rev() {
        match undo(step) {
            Ok(()) => {
                journal.append(&Record::Undone { index: *index })?;
                undone += 1;
            }
            Err(error) => {
                journal.append(&Record::UndoFailed {
                    index: *index,
                    error: error.clone(),
                })?;
                issues.push(error);
            }
        }
    }
    Ok(undone)
}

/// Applies plans and recovers interrupted runs.
#[derive(Clone, Debug)]
pub struct Executor {
    journal_dir: PathBuf,
    manifest_path: PathBuf,
    cancel: Option<CancelToken>,
    fault: Option<FaultInjection>,
}

impl Executor {
    /// `journal_dir` holds one JSON-lines journal per run; `manifest_path` is
    /// where the manifest is atomically committed.
    pub fn new(journal_dir: impl Into<PathBuf>, manifest_path: impl Into<PathBuf>) -> Self {
        Self {
            journal_dir: journal_dir.into(),
            manifest_path: manifest_path.into(),
            cancel: None,
            fault: None,
        }
    }

    /// Cancellation is checked between steps; a cancelled run rolls back.
    pub fn with_cancel(mut self, token: CancelToken) -> Self {
        self.cancel = Some(token);
        self
    }

    #[doc(hidden)]
    pub fn with_fault(mut self, fault: FaultInjection) -> Self {
        self.fault = Some(fault);
        self
    }

    pub fn journal_dir(&self) -> &Path {
        &self.journal_dir
    }

    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// Apply `plan`. `Ok` only when every step succeeded and the manifest was
    /// committed; any failure is rolled back and reported as
    /// [`DeployError::OperationFailed`].
    pub fn apply(&self, plan: &Plan, manifest: &mut Manifest) -> Result<ApplyReport, DeployError> {
        if !plan.is_applicable() {
            return Err(DeployError::NotApplicable(plan.conflicts.clone()));
        }
        let run_id = new_run_id();
        let mut journal = Journal::create(&self.journal_dir, &run_id)?;
        journal.append(&Record::Begin {
            run_id: run_id.clone(),
            operations: plan.operations.len(),
        })?;
        let mut started: Vec<(usize, Step)> = Vec::new();
        let mut completed = 0usize;

        for (index, op) in plan.operations.iter().enumerate() {
            if self.fault == Some(FaultInjection::CrashAfterSteps(completed)) {
                return Err(DeployError::SimulatedCrash(completed));
            }
            let step = step_for(op, &run_id, index);
            let failure = if self.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
                Some("cancelled".to_owned())
            } else {
                preflight(&step).err()
            };
            if let Some(error) = failure {
                journal.append(&Record::Failed {
                    index,
                    error: error.clone(),
                })?;
                return Err(self.fail(&mut journal, &run_id, &started, index, error, completed));
            }
            journal.append(&Record::Start {
                index,
                step: step.clone(),
            })?;
            started.push((index, step.clone()));
            let crash_mid_copy = self.fault == Some(FaultInjection::CrashMidCopy)
                && matches!(step, Step::CopyTree { .. });
            let result = if self.fault == Some(FaultInjection::FailStep(index)) {
                Err("injected failure".to_owned())
            } else {
                perform(&step, crash_mid_copy)
            };
            match result {
                Ok(false) => return Err(DeployError::SimulatedCrash(completed)),
                Ok(true) => {}
                Err(error) => {
                    journal.append(&Record::Failed {
                        index,
                        error: error.clone(),
                    })?;
                    return Err(self.fail(
                        &mut journal,
                        &run_id,
                        &started,
                        index,
                        error,
                        completed,
                    ));
                }
            }
            if let Err(error) = verify(&step) {
                journal.append(&Record::Failed {
                    index,
                    error: error.clone(),
                })?;
                return Err(self.fail(&mut journal, &run_id, &started, index, error, completed));
            }
            journal.append(&Record::Done { index })?;
            completed += 1;
        }
        if self.fault == Some(FaultInjection::CrashAfterSteps(completed)) {
            return Err(DeployError::SimulatedCrash(completed));
        }

        let previous = manifest.clone();
        let now = unix_now();
        for path in &plan.manifest_removals {
            manifest.remove(path);
        }
        for entry in &plan.manifest_upserts {
            let mut entry = entry.clone();
            entry.applied_at_unix = now;
            manifest.upsert(entry);
        }
        manifest.last_run_id = Some(run_id.clone());
        if let Err(error) = manifest.save(&self.manifest_path) {
            *manifest = previous;
            let index = plan.operations.len();
            let error = format!("manifest commit failed: {error}");
            journal.append(&Record::Failed {
                index,
                error: error.clone(),
            })?;
            return Err(self.fail(&mut journal, &run_id, &started, index, error, completed));
        }
        journal.append(&Record::ManifestCommitted)?;

        let mut issues = Vec::new();
        for (_, step) in &started {
            if let Err(error) = finalize(step) {
                issues.push(error);
            }
        }
        journal.append(&Record::End {
            outcome: RunOutcome::Applied,
        })?;
        Ok(ApplyReport {
            run_id,
            status: ApplyStatus::Applied,
            steps_applied: completed,
            steps_undone: 0,
            issues,
            journal: journal.path.clone(),
        })
    }

    fn fail(
        &self,
        journal: &mut Journal,
        run_id: &str,
        started: &[(usize, Step)],
        failed_index: usize,
        error: String,
        completed: usize,
    ) -> DeployError {
        let mut issues = Vec::new();
        let undone = match rollback(journal, started, &mut issues) {
            Ok(undone) => undone,
            Err(journal_error) => return journal_error,
        };
        let status = if issues.is_empty() {
            if let Err(journal_error) = journal.append(&Record::End {
                outcome: RunOutcome::RolledBack,
            }) {
                return journal_error;
            }
            ApplyStatus::RolledBack {
                failed_index,
                error,
            }
        } else {
            ApplyStatus::RollbackIncomplete {
                failed_index,
                error,
            }
        };
        DeployError::OperationFailed(Box::new(ApplyReport {
            run_id: run_id.to_owned(),
            status,
            steps_applied: completed,
            steps_undone: undone,
            issues,
            journal: journal.path.clone(),
        }))
    }

    /// Finish every journal without an `End` record: finalize runs whose
    /// manifest was committed, roll back all others. Safe to call repeatedly
    /// (including after a crash during recovery).
    pub fn recover(&self, manifest: &mut Manifest) -> Result<RecoveryReport, DeployError> {
        let mut report = RecoveryReport::default();
        let entries = match fs::read_dir(&self.journal_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(report),
            Err(source) => {
                return Err(DeployError::Journal {
                    path: self.journal_dir.clone(),
                    source,
                });
            }
        };
        let mut journals: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == JOURNAL_EXT))
            .collect();
        journals.sort();
        for path in journals {
            let records = read_journal(&path).map_err(|source| DeployError::Journal {
                path: path.clone(),
                source,
            })?;
            if records
                .iter()
                .any(|record| matches!(record, Record::End { .. }))
            {
                continue;
            }
            let run_id = records
                .iter()
                .find_map(|record| match record {
                    Record::Begin { run_id, .. } => Some(run_id.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
            let committed = records
                .iter()
                .any(|record| matches!(record, Record::ManifestCommitted))
                || manifest.last_run_id.as_deref() == Some(run_id.as_str());
            let mut started: Vec<(usize, Step)> = Vec::new();
            for record in &records {
                if let Record::Start { index, step } = record {
                    started.push((*index, step.clone()));
                }
            }
            let mut journal = Journal::reopen(&path)?;
            let mut issues = Vec::new();
            let outcome = if committed {
                for (_, step) in &started {
                    if let Err(error) = finalize(step) {
                        issues.push(error);
                    }
                }
                if !records
                    .iter()
                    .any(|record| matches!(record, Record::ManifestCommitted))
                {
                    journal.append(&Record::ManifestCommitted)?;
                }
                RunOutcome::Finalized
            } else {
                rollback(&mut journal, &started, &mut issues)?;
                RunOutcome::RolledBack
            };
            let outcome = if issues.is_empty() || committed {
                journal.append(&Record::End { outcome })?;
                Some(outcome)
            } else {
                None
            };
            report.runs.push(RecoveredRun {
                run_id,
                outcome,
                issues,
            });
        }
        Ok(report)
    }
}
