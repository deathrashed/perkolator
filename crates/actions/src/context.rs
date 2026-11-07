use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use perkolator_core::{FeatureSupport, PlatformFeature};
use serde::{Deserialize, Serialize};

/// How long a captured session target stays trustworthy for insertion.
pub const DEFAULT_SESSION_MAX_AGE: Duration = Duration::from_secs(10 * 60);

/// Which presentation is asking. Registry semantics are identical; only a
/// few actions (compose, reveal) depend on an interactive surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Surface {
    Launcher,
    Manager,
    Cli,
}

impl Surface {
    pub fn is_interactive(self) -> bool {
        matches!(self, Surface::Launcher | Surface::Manager)
    }
}

/// A foreground agent session as observed by a platform/agent adapter.
///
/// Captured once when the launcher opens, then re-observed right before
/// delivery so a stale or changed target is never silently used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTarget {
    /// Platform app identifier (bundle id / Wayland app id).
    pub app_id: String,
    /// Human label shown in outcomes, e.g. "Claude Code — perkolator".
    pub label: String,
    /// Agent adapter that recognised the session, if any.
    pub agent: Option<String>,
    pub project_root: Option<PathBuf>,
    pub captured_at: SystemTime,
}

/// Everything the registry needs to decide availability, without UI types.
#[derive(Clone, Debug)]
pub struct ActionContext {
    pub surface: Surface,
    /// Probed platform support. A missing feature means [`FeatureSupport::Unknown`].
    pub platform: HashMap<PlatformFeature, FeatureSupport>,
    /// Session captured when the launcher was activated.
    pub session: Option<SessionTarget>,
    /// Session re-observed immediately before delivery (revalidation input).
    pub current_target: Option<SessionTarget>,
    pub session_max_age: Duration,
}

impl ActionContext {
    pub fn new(surface: Surface) -> Self {
        Self {
            surface,
            platform: HashMap::new(),
            session: None,
            current_target: None,
            session_max_age: DEFAULT_SESSION_MAX_AGE,
        }
    }

    pub fn with_feature(mut self, feature: PlatformFeature, support: FeatureSupport) -> Self {
        self.platform.insert(feature, support);
        self
    }

    pub fn with_session(mut self, session: SessionTarget) -> Self {
        self.session = Some(session);
        self
    }

    pub fn with_current_target(mut self, target: SessionTarget) -> Self {
        self.current_target = Some(target);
        self
    }

    pub fn support(&self, feature: PlatformFeature) -> FeatureSupport {
        self.platform
            .get(&feature)
            .cloned()
            .unwrap_or(FeatureSupport::Unknown)
    }

    pub fn is_supported(&self, feature: PlatformFeature) -> bool {
        matches!(self.support(feature), FeatureSupport::Supported)
    }
}
