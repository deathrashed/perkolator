use serde::{Deserialize, Serialize};

/// OS integrations that differ between macOS and KDE Wayland.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlatformFeature {
    GlobalShortcut,
    QuickPanelShow,
    FocusRestore,
    ForegroundContext,
    ClipboardWrite,
    PasteToTarget,
    AgentSessionInsert,
    FileReveal,
    Tray,
    Autostart,
}

impl PlatformFeature {
    pub const ALL: [PlatformFeature; 10] = [
        PlatformFeature::GlobalShortcut,
        PlatformFeature::QuickPanelShow,
        PlatformFeature::FocusRestore,
        PlatformFeature::ForegroundContext,
        PlatformFeature::ClipboardWrite,
        PlatformFeature::PasteToTarget,
        PlatformFeature::AgentSessionInsert,
        PlatformFeature::FileReveal,
        PlatformFeature::Tray,
        PlatformFeature::Autostart,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlatformFeature::GlobalShortcut => "Global shortcut",
            PlatformFeature::QuickPanelShow => "Quick panel show/hide",
            PlatformFeature::FocusRestore => "Restore previous focus",
            PlatformFeature::ForegroundContext => "Foreground app context",
            PlatformFeature::ClipboardWrite => "Clipboard copy",
            PlatformFeature::PasteToTarget => "Paste into target app",
            PlatformFeature::AgentSessionInsert => "Insert into agent session",
            PlatformFeature::FileReveal => "Reveal in file manager",
            PlatformFeature::Tray => "Tray icon",
            PlatformFeature::Autostart => "Start at login",
        }
    }
}

/// Honest support state with an actionable explanation for the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "support", rename_all = "kebab-case")]
pub enum FeatureSupport {
    Supported,
    PermissionRequired {
        explanation: String,
    },
    Fallback {
        explanation: String,
    },
    Unavailable {
        explanation: String,
    },
    /// Not yet probed on this machine.
    Unknown,
}

impl FeatureSupport {
    pub fn is_usable(&self) -> bool {
        matches!(
            self,
            FeatureSupport::Supported | FeatureSupport::Fallback { .. }
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            FeatureSupport::Supported => "Supported",
            FeatureSupport::PermissionRequired { .. } => "Permission required",
            FeatureSupport::Fallback { .. } => "Fallback",
            FeatureSupport::Unavailable { .. } => "Unavailable",
            FeatureSupport::Unknown => "Not checked",
        }
    }

    pub fn explanation(&self) -> Option<&str> {
        match self {
            FeatureSupport::PermissionRequired { explanation }
            | FeatureSupport::Fallback { explanation }
            | FeatureSupport::Unavailable { explanation } => Some(explanation),
            FeatureSupport::Supported | FeatureSupport::Unknown => None,
        }
    }
}
