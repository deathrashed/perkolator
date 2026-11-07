use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ResourceId;

/// Typed, context-sensitive actions. The action registry decides which are
/// offered for a given resource + context; the UI only dispatches them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionId {
    ShowDetails,
    CopyPath,
    CopyDeployedPath,
    CopySourceUrl,
    CopyContent,
    CopyConfig,
    CopyReference,
    CopyInvocation,
    ExpandTemplate,
    Compose,
    InsertIntoSession,
    RevealInFileManager,
    PlanDeploy,
    RemoveDeployment,
    RepairLink,
    Enable,
    Disable,
}

impl ActionId {
    pub fn label(self) -> &'static str {
        match self {
            ActionId::ShowDetails => "Open details",
            ActionId::CopyPath => "Copy path",
            ActionId::CopyDeployedPath => "Copy deployed path",
            ActionId::CopySourceUrl => "Copy source URL",
            ActionId::CopyContent => "Copy contents",
            ActionId::CopyConfig => "Copy configuration",
            ActionId::CopyReference => "Copy reference",
            ActionId::CopyInvocation => "Copy invocation",
            ActionId::ExpandTemplate => "Expand template",
            ActionId::Compose => "Compose query",
            ActionId::InsertIntoSession => "Insert into session",
            ActionId::RevealInFileManager => "Reveal in file manager",
            ActionId::PlanDeploy => "Deploy…",
            ActionId::RemoveDeployment => "Remove deployment…",
            ActionId::RepairLink => "Repair link…",
            ActionId::Enable => "Enable…",
            ActionId::Disable => "Disable…",
        }
    }

    /// Mutating actions always go through plan → confirm → apply.
    pub fn is_mutating(self) -> bool {
        matches!(
            self,
            ActionId::PlanDeploy
                | ActionId::RemoveDeployment
                | ActionId::RepairLink
                | ActionId::Enable
                | ActionId::Disable
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "kebab-case")]
pub enum ActionAvailability {
    Available,
    /// Shown but disabled, with a reason the user can act on.
    Disabled { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardPayload {
    /// Short human label for what was copied, e.g. "SKILL.md contents".
    pub label: String,
    pub text: String,
}

/// The side effect an action resolves to. Headless crates produce effects;
/// the UI/platform layer performs them and reports an [`ActionOutcome`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "kebab-case")]
pub enum ActionEffect {
    WriteClipboard(ClipboardPayload),
    Reveal { path: PathBuf },
    OpenUrl { url: String },
    OpenDetails { id: ResourceId },
    EnterCompose { id: ResourceId },
    /// Open the deployment planner; nothing is mutated yet.
    OpenDeployPlanner { id: ResourceId },
}

/// What actually happened. Clipboard fallback is never reported as insertion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum ActionOutcome {
    Copied { label: String },
    CopiedAsFallback { label: String, reason: String },
    Inserted { target: String },
    Revealed { path: PathBuf },
    Opened,
    Failed { message: String },
}

impl ActionOutcome {
    pub fn message(&self) -> String {
        match self {
            ActionOutcome::Copied { label } => format!("Copied {label}"),
            ActionOutcome::CopiedAsFallback { label, reason } => {
                format!("Copied {label} — paste it yourself ({reason})")
            }
            ActionOutcome::Inserted { target } => format!("Inserted into {target}"),
            ActionOutcome::Revealed { path } => format!("Revealed {}", path.display()),
            ActionOutcome::Opened => "Opened".to_owned(),
            ActionOutcome::Failed { message } => message.clone(),
        }
    }
}
