//! Dry-run-first, journalled, recoverable filesystem deployment of skills.
//!
//! Flow: [`plan_deploy`] / [`plan_remove`] / [`plan_repair`] produce a
//! read-only [`Plan`] (operations + conflicts). Only an applicable plan can
//! be passed to [`Executor::apply`], which re-checks every precondition,
//! journals each step with fsync before and after, verifies the result and
//! rolls back on failure. [`Executor::recover`] finishes runs interrupted by
//! a crash.
//!
//! Safety invariants:
//! - lstat semantics everywhere; real directories, owned links, foreign
//!   links, broken links and symlinked parents are distinguished.
//! - Nothing is deleted unless it is in the manifest *and* its identity is
//!   re-verified (same link contents / same fingerprint), or it is a
//!   generated staging/trash name recorded in the journal before creation.
//!   Canonical sources are never removed; unmanaged dirs and foreign links
//!   are never replaced.
//! - A failed symlink is a failure: there is no silent copy fallback.
//! - Multi-destination apply is recoverable, not ACID (see [`Executor`]).

mod executor;
mod fsutil;
mod manifest;
mod model;
mod plan;
mod status;

pub use executor::{
    ApplyReport, ApplyStatus, DeployError, Executor, FaultInjection, RecoveredRun, RecoveryReport,
    RunOutcome, Step,
};
pub use fsutil::fingerprint_tree;
pub use manifest::{MANAGED_TARGET_ID, MANIFEST_VERSION, Manifest, ManifestEntry};
pub use model::{Conflict, DeployRequest, DeploymentMode, Destination, Plan, PlanKind, PlannedOp};
pub use plan::{is_valid_skill_name, plan_deploy, plan_remove, plan_repair};
pub use status::{DeploymentStatus, MatrixCell, deployment_matrix, status_of};
