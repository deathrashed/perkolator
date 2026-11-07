//! KDE Plasma Wayland platform adapter.
//!
//! This crate reports what Perkolator can honestly do on the current Linux
//! session and provides the pieces the desktop shell needs (single-instance
//! socket location, desktop entry). It never injects input, never enumerates
//! other applications' windows and never relies on X11-only tooling.
//!
//! Probes are cheap environment checks; anything that needs the session bus
//! (the GlobalShortcuts portal) is passed in as a [`PortalStatus`] so tests
//! stay hermetic.

mod desktop_entry;
mod session;

use std::collections::BTreeMap;

use perkolator_core::{FeatureSupport, PlatformFeature};

pub use desktop_entry::{APP_ID, desktop_entry, toggle_command};
pub use session::{DisplayServer, SessionInfo, instance_socket_path};

/// Result of probing `org.freedesktop.portal.GlobalShortcuts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortalStatus {
    /// Interface exported by the running `xdg-desktop-portal`.
    Available { version: u32 },
    /// The portal frontend runs but does not export GlobalShortcuts (for
    /// example because the KDE backend is not the active implementation).
    InterfaceMissing,
    /// No session bus / portal reachable.
    Unreachable { reason: String },
    /// Not probed yet.
    NotProbed,
}

/// Capability report for the current Linux session.
pub fn probe_features(
    session: &SessionInfo,
    portal: &PortalStatus,
) -> BTreeMap<PlatformFeature, FeatureSupport> {
    let mut features = BTreeMap::new();
    let wayland = session.display_server == DisplayServer::Wayland;
    let toggle = toggle_command();

    let global_shortcut = match portal {
        PortalStatus::Available { version } => FeatureSupport::Fallback {
            explanation: format!(
                "GlobalShortcuts portal v{version} is available but registration is not \
                 implemented yet; bind a KDE custom shortcut to `{toggle}`"
            ),
        },
        PortalStatus::InterfaceMissing => FeatureSupport::Fallback {
            explanation: format!(
                "The running xdg-desktop-portal does not export GlobalShortcuts (KDE backend \
                 inactive); bind a KDE custom shortcut to `{toggle}`"
            ),
        },
        PortalStatus::Unreachable { reason } => FeatureSupport::Fallback {
            explanation: format!(
                "Portal unreachable ({reason}); bind a desktop shortcut to `{toggle}`"
            ),
        },
        PortalStatus::NotProbed => FeatureSupport::Fallback {
            explanation: format!("Bind a desktop shortcut to `{toggle}`"),
        },
    };
    features.insert(PlatformFeature::GlobalShortcut, global_shortcut);

    features.insert(
        PlatformFeature::QuickPanelShow,
        if wayland {
            FeatureSupport::Supported
        } else {
            FeatureSupport::Fallback {
                explanation: "Not a Wayland session; the launcher opens as a normal popup window"
                    .into(),
            }
        },
    );
    features.insert(
        PlatformFeature::FocusRestore,
        FeatureSupport::Fallback {
            explanation: "The compositor returns focus to the previous window when the \
                          launcher closes; Perkolator cannot target a specific window"
                .into(),
        },
    );
    features.insert(
        PlatformFeature::ForegroundContext,
        FeatureSupport::Unavailable {
            explanation: "Wayland does not expose the focused application to clients; \
                          use an explicit agent adapter"
                .into(),
        },
    );
    features.insert(
        PlatformFeature::ClipboardWrite,
        if session.display_server == DisplayServer::Unknown {
            FeatureSupport::Unavailable {
                explanation: "No display server detected".into(),
            }
        } else {
            FeatureSupport::Supported
        },
    );
    features.insert(
        PlatformFeature::PasteToTarget,
        FeatureSupport::Unavailable {
            explanation: "Synthetic key input into other apps is not used on Wayland; \
                          Perkolator copies and you paste"
                .into(),
        },
    );
    features.insert(
        PlatformFeature::AgentSessionInsert,
        FeatureSupport::Unavailable {
            explanation: "No verified agent session adapter yet; use Copy invocation".into(),
        },
    );
    features.insert(
        PlatformFeature::FileReveal,
        FeatureSupport::Fallback {
            explanation: "Opens the containing folder via the desktop's file manager".into(),
        },
    );
    features.insert(
        PlatformFeature::Tray,
        FeatureSupport::Unavailable {
            explanation: "StatusNotifierItem tray is not implemented yet".into(),
        },
    );
    features.insert(
        PlatformFeature::Autostart,
        FeatureSupport::Unavailable {
            explanation: "Autostart entry is not installed automatically; see \
                          docs/platform/KDE_WAYLAND.md"
                .into(),
        },
    );
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kde_wayland() -> SessionInfo {
        SessionInfo {
            display_server: DisplayServer::Wayland,
            desktop: Some("KDE".into()),
            wayland_display: Some("wayland-1".into()),
            runtime_dir: Some("/run/user/1000".into()),
        }
    }

    #[test]
    fn every_feature_is_reported() {
        let features = probe_features(&kde_wayland(), &PortalStatus::NotProbed);
        for feature in PlatformFeature::ALL {
            assert!(features.contains_key(&feature), "missing {feature:?}");
        }
    }

    #[test]
    fn wayland_never_claims_paste_or_foreground_detection() {
        let features = probe_features(&kde_wayland(), &PortalStatus::Available { version: 1 });
        assert!(!features[&PlatformFeature::PasteToTarget].is_usable());
        assert!(!features[&PlatformFeature::ForegroundContext].is_usable());
        assert!(!features[&PlatformFeature::AgentSessionInsert].is_usable());
        assert_eq!(
            features[&PlatformFeature::ClipboardWrite],
            FeatureSupport::Supported
        );
    }

    #[test]
    fn missing_portal_explains_shortcut_fallback() {
        let features = probe_features(&kde_wayland(), &PortalStatus::InterfaceMissing);
        let explanation = features[&PlatformFeature::GlobalShortcut]
            .explanation()
            .unwrap()
            .to_owned();
        assert!(explanation.contains("KDE backend inactive"));
        assert!(explanation.contains("--toggle"));
    }
}
