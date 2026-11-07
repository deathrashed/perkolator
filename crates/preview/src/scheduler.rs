use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use perkolator_core::CancelToken;

/// Hands out monotonically increasing preview generations.
///
/// Each [`begin`](Self::begin) cancels the previous request's token, so
/// in-flight work stops cooperatively, and [`is_current`](Self::is_current)
/// lets the UI drop any result that arrives after a newer request started.
/// Shareable across threads (`Arc<PreviewScheduler>`).
#[derive(Debug, Default)]
pub struct PreviewScheduler {
    generation: AtomicU64,
    current: Mutex<CancelToken>,
}

impl PreviewScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new preview request, cancelling the previous one.
    pub fn begin(&self) -> (u64, CancelToken) {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        current.cancel();
        let token = CancelToken::new();
        *current = token.clone();
        // Incremented under the lock so generation order matches token order.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        (generation, token)
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Cancel in-flight work and invalidate its generation (e.g. on close).
    pub fn cancel(&self) {
        let current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        current.cancel();
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}
