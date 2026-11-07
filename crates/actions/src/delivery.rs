//! Truthful delivery: insertion is reported only when a verified adapter can
//! insert into a revalidated session. Everything else is an explicit
//! clipboard copy, with the reason when the user asked for insertion.

use std::path::PathBuf;
use std::time::Duration;

use perkolator_core::{ActionEffect, ActionOutcome, ClipboardPayload};
use thiserror::Error;

use crate::context::{ActionContext, SessionTarget};
use crate::registry::insert_unsupported_reason;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    Clipboard {
        payload: ClipboardPayload,
    },
    /// Insertion was requested but is not possible; the text is copied instead.
    ClipboardFallback {
        payload: ClipboardPayload,
        reason: String,
    },
    /// Insert into this revalidated target via the platform's session adapter.
    Insert {
        payload: ClipboardPayload,
        target: SessionTarget,
    },
}

impl Delivery {
    pub fn payload(&self) -> &ClipboardPayload {
        match self {
            Delivery::Clipboard { payload }
            | Delivery::ClipboardFallback { payload, .. }
            | Delivery::Insert { payload, .. } => payload,
        }
    }

    /// Outcome to report once the platform layer has performed the delivery
    /// successfully. Failures are reported as [`ActionOutcome::Failed`] by
    /// the caller; a fallback copy is never reported as insertion.
    pub fn outcome(&self) -> ActionOutcome {
        match self {
            Delivery::Clipboard { payload } => ActionOutcome::Copied {
                label: payload.label.clone(),
            },
            Delivery::ClipboardFallback { payload, reason } => ActionOutcome::CopiedAsFallback {
                label: payload.label.clone(),
                reason: reason.clone(),
            },
            Delivery::Insert { target, .. } => ActionOutcome::Inserted {
                target: target.label.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum StaleTarget {
    #[error("the foreground app changed from {captured} to {current}")]
    AppChanged { captured: String, current: String },
    #[error("the agent session changed")]
    AgentChanged {
        captured: Option<String>,
        current: Option<String>,
    },
    #[error("the session's project changed")]
    ProjectChanged {
        captured: Option<PathBuf>,
        current: Option<PathBuf>,
    },
    #[error("the session was captured {}s ago (limit {}s)", age.as_secs(), max_age.as_secs())]
    TooOld { age: Duration, max_age: Duration },
    #[error("the session's capture time is later than the re-check")]
    ClockSkew,
}

/// Check that the target re-observed now (`current`) is still the session
/// captured at activation, and that the capture is recent enough. Age is
/// measured between the two observations' `captured_at`.
pub fn validate_session(
    captured: &SessionTarget,
    current: &SessionTarget,
    max_age: Duration,
) -> Result<(), StaleTarget> {
    if captured.app_id != current.app_id {
        return Err(StaleTarget::AppChanged {
            captured: captured.app_id.clone(),
            current: current.app_id.clone(),
        });
    }
    if captured.agent.is_some() && captured.agent != current.agent {
        return Err(StaleTarget::AgentChanged {
            captured: captured.agent.clone(),
            current: current.agent.clone(),
        });
    }
    if captured.project_root != current.project_root {
        return Err(StaleTarget::ProjectChanged {
            captured: captured.project_root.clone(),
            current: current.project_root.clone(),
        });
    }
    let age = current
        .captured_at
        .duration_since(captured.captured_at)
        .map_err(|_| StaleTarget::ClockSkew)?;
    if age > max_age {
        return Err(StaleTarget::TooOld { age, max_age });
    }
    Ok(())
}

/// Decide how a text effect reaches the user. `None` for effects that carry
/// no text (details, reveal, planner, compose entry).
pub fn deliver(effect: &ActionEffect, ctx: &ActionContext, wants_insert: bool) -> Option<Delivery> {
    let ActionEffect::WriteClipboard(payload) = effect else {
        return None;
    };
    let payload = payload.clone();
    if !wants_insert {
        return Some(Delivery::Clipboard { payload });
    }
    let fallback = |reason: String| {
        Some(Delivery::ClipboardFallback {
            payload: payload.clone(),
            reason,
        })
    };
    if let Some(reason) = insert_unsupported_reason(ctx) {
        return fallback(reason);
    }
    let Some(captured) = ctx.session.as_ref() else {
        return fallback("No agent session was captured when Perkolator opened".to_owned());
    };
    let Some(current) = ctx.current_target.as_ref() else {
        return fallback("The target session could not be re-checked before inserting".to_owned());
    };
    if let Err(stale) = validate_session(captured, current, ctx.session_max_age) {
        return fallback(format!("Target session is no longer valid: {stale}"));
    }
    Some(Delivery::Insert {
        payload,
        target: current.clone(),
    })
}
