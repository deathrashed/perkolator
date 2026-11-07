use perkolator_core::{
    ActionAvailability, ActionId, Capability, FeatureSupport, InstallState, PlatformFeature,
    ResourceDetails, ResourceKind, ResourceRecord, TemplateSyntax,
};
use serde::Serialize;

use crate::context::ActionContext;

/// One row in an action menu. The first entry for a record is its default
/// (Enter) action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ActionEntry {
    pub id: ActionId,
    pub label: &'static str,
    pub shortcut_hint: Option<&'static str>,
    pub availability: ActionAvailability,
}

impl ActionEntry {
    pub fn is_available(&self) -> bool {
        self.availability == ActionAvailability::Available
    }
}

use ActionId::*;

/// Preferred order per kind; entries are then filtered by what the record
/// genuinely supports. `ShowDetails` is in every list so a default always exists.
///
/// Skill default is `CopyInvocation` on every surface: Enter must be a single
/// predictable keystroke, and composing is reached with Tab in the launcher.
const SKILL_ORDER: &[ActionId] = &[
    CopyInvocation,
    Compose,
    InsertIntoSession,
    CopyReference,
    CopyContent,
    CopyPath,
    CopyDeployedPath,
    CopySourceUrl,
    RevealInFileManager,
    ShowDetails,
    PlanDeploy,
    RepairLink,
    RemoveDeployment,
    Enable,
    Disable,
];

const MCP_ORDER: &[ActionId] = &[
    CopyConfig,
    ShowDetails,
    CopyPath,
    CopySourceUrl,
    RevealInFileManager,
    PlanDeploy,
    Enable,
    Disable,
    RepairLink,
    RemoveDeployment,
];

const AGENT_ORDER: &[ActionId] = SKILL_ORDER;

const PROMPT_ORDER: &[ActionId] = &[
    ExpandTemplate,
    CopyContent,
    Compose,
    InsertIntoSession,
    CopyReference,
    CopyPath,
    CopyDeployedPath,
    CopySourceUrl,
    RevealInFileManager,
    ShowDetails,
    PlanDeploy,
    RepairLink,
    RemoveDeployment,
];

const RULE_ORDER: &[ActionId] = &[
    CopyContent,
    CopyReference,
    Compose,
    InsertIntoSession,
    CopyPath,
    CopyDeployedPath,
    CopySourceUrl,
    RevealInFileManager,
    ShowDetails,
    Enable,
    Disable,
    PlanDeploy,
    RepairLink,
    RemoveDeployment,
];

const PLUGIN_ORDER: &[ActionId] = &[
    ShowDetails,
    CopyPath,
    CopySourceUrl,
    RevealInFileManager,
    CopyContent,
    CopyDeployedPath,
    PlanDeploy,
    Enable,
    Disable,
    RepairLink,
    RemoveDeployment,
];

/// Copy-style actions offered as the fallback when insertion is impossible.
const INSERT_FALLBACKS: &[ActionId] = &[CopyInvocation, ExpandTemplate, CopyContent, CopyReference];

fn order_for(kind: ResourceKind) -> &'static [ActionId] {
    match kind {
        ResourceKind::Skill => SKILL_ORDER,
        ResourceKind::McpServer => MCP_ORDER,
        ResourceKind::Agent => AGENT_ORDER,
        ResourceKind::Prompt => PROMPT_ORDER,
        ResourceKind::Rule => RULE_ORDER,
        ResourceKind::Plugin => PLUGIN_ORDER,
    }
}

/// Ordered, capability-gated actions for a record in a context.
pub fn actions_for(record: &ResourceRecord, ctx: &ActionContext) -> Vec<ActionEntry> {
    offered(record)
        .enumerate()
        .map(|(index, id)| ActionEntry {
            id,
            label: label_for(record.kind(), id),
            shortcut_hint: match (index, id) {
                (0, _) => Some("Enter"),
                (_, Compose) => Some("Tab"),
                _ => None,
            },
            availability: availability(id, record, ctx),
        })
        .collect()
}

/// The Enter action for a record; independent of context so it is stable
/// across launcher and manager.
pub fn default_action(record: &ResourceRecord) -> ActionId {
    offered(record).next().unwrap_or(ShowDetails)
}

pub(crate) fn offered(record: &ResourceRecord) -> impl Iterator<Item = ActionId> + '_ {
    order_for(record.kind())
        .iter()
        .copied()
        .filter(move |id| is_offered(*id, record))
}

/// A real filesystem path exists. Remote/catalogue results and pathless
/// entries never get path actions (U10).
pub(crate) fn has_local_path(record: &ResourceRecord) -> bool {
    record.capabilities.contains(Capability::LocalPath) && record.local_path().is_some()
}

/// Template that actually has something to expand offline.
pub(crate) fn is_expandable_prompt(record: &ResourceRecord) -> bool {
    match &record.details {
        ResourceDetails::Prompt(prompt) => {
            record.capabilities.contains(Capability::ExpandVariables)
                && match prompt.syntax {
                    TemplateSyntax::Plain => false,
                    TemplateSyntax::Arguments => true,
                    TemplateSyntax::DoubleBrace => !prompt.variables.is_empty(),
                }
        }
        _ => false,
    }
}

/// Current enable state from the most specific source available.
pub(crate) fn current_enabled(record: &ResourceRecord) -> Option<bool> {
    let from_locations = record
        .locations
        .iter()
        .find_map(|location| match location.state {
            InstallState::Enabled => Some(true),
            InstallState::Disabled => Some(false),
            _ => None,
        });
    from_locations.or(match &record.details {
        ResourceDetails::McpServer(mcp) => mcp.enabled,
        ResourceDetails::Rule(rule) => rule.always_apply,
        _ => None,
    })
}

fn is_offered(id: ActionId, record: &ResourceRecord) -> bool {
    let caps = &record.capabilities;
    let kind = record.kind();
    let path = has_local_path(record);
    match id {
        ShowDetails => true,
        CopyPath | RevealInFileManager => path,
        CopyDeployedPath | RemoveDeployment => path && record.deployments().next().is_some(),
        RepairLink => record
            .deployments()
            .any(|location| location.state == InstallState::Broken),
        CopySourceUrl => {
            caps.contains(Capability::SourceUrl) && record.provenance.origin_url.is_some()
        }
        // MCP config files hold other entries' credentials: only the redacted
        // per-entry snippet is ever copied.
        CopyContent => {
            caps.contains(Capability::CopyContent) && path && kind != ResourceKind::McpServer
        }
        CopyConfig => {
            caps.contains(Capability::CopyConfig)
                && matches!(record.details, ResourceDetails::McpServer(_))
        }
        CopyReference => path && (caps.contains(Capability::Compose) || kind == ResourceKind::Rule),
        CopyInvocation => path && matches!(kind, ResourceKind::Skill | ResourceKind::Agent),
        ExpandTemplate => path && is_expandable_prompt(record),
        // An agent can only use what it can read: remote results must be
        // installed before they can be referenced.
        Compose | InsertIntoSession => path && caps.contains(Capability::Compose),
        PlanDeploy => caps.contains(Capability::DeployFiles),
        Enable => {
            caps.contains(Capability::ToggleEnabled) && current_enabled(record) == Some(false)
        }
        Disable => {
            caps.contains(Capability::ToggleEnabled) && current_enabled(record) == Some(true)
        }
    }
}

fn label_for(kind: ResourceKind, id: ActionId) -> &'static str {
    match (kind, id) {
        (ResourceKind::McpServer, CopyPath) => "Copy config file path",
        (ResourceKind::McpServer, RevealInFileManager) => "Reveal config file",
        (ResourceKind::Skill, CopyContent) => "Copy SKILL.md",
        (ResourceKind::Plugin, CopyContent) => "Copy manifest",
        _ => id.label(),
    }
}

fn is_copy_effect(id: ActionId) -> bool {
    matches!(
        id,
        CopyPath
            | CopyDeployedPath
            | CopySourceUrl
            | CopyContent
            | CopyConfig
            | CopyReference
            | CopyInvocation
            | ExpandTemplate
    )
}

pub(crate) fn availability(
    id: ActionId,
    record: &ResourceRecord,
    ctx: &ActionContext,
) -> ActionAvailability {
    let disabled = |reason: String| ActionAvailability::Disabled { reason };
    match id {
        InsertIntoSession => match insert_blocker(record, ctx) {
            Some(reason) => disabled(reason),
            None => ActionAvailability::Available,
        },
        Compose if !ctx.surface.is_interactive() => {
            disabled("Composing needs the launcher or manager window".to_owned())
        }
        RevealInFileManager => match ctx.support(PlatformFeature::FileReveal) {
            FeatureSupport::PermissionRequired { explanation }
            | FeatureSupport::Unavailable { explanation } => {
                disabled(format!("{explanation} — use Copy path"))
            }
            _ => ActionAvailability::Available,
        },
        PlanDeploy if record.has_errors() => {
            disabled("Fix the validation errors shown in details before deploying".to_owned())
        }
        id if is_copy_effect(id) => match ctx.support(PlatformFeature::ClipboardWrite) {
            FeatureSupport::PermissionRequired { explanation }
            | FeatureSupport::Unavailable { explanation } => disabled(explanation),
            _ => ActionAvailability::Available,
        },
        _ => ActionAvailability::Available,
    }
}

/// Why insertion into an agent session is impossible right now, phrased so
/// the user knows what to do instead. `None` when insertion may be attempted
/// (delivery still revalidates the target).
pub(crate) fn insert_blocker(record: &ResourceRecord, ctx: &ActionContext) -> Option<String> {
    let reason = insert_unsupported_reason(ctx)?;
    let fallback = offered(record)
        .find(|id| INSERT_FALLBACKS.contains(id))
        .map(|id| label_for(record.kind(), id))
        .unwrap_or("copy");
    Some(format!("{reason} — use {fallback}"))
}

/// Platform/session reason that rules out insertion, independent of record.
pub(crate) fn insert_unsupported_reason(ctx: &ActionContext) -> Option<String> {
    match ctx.support(PlatformFeature::AgentSessionInsert) {
        FeatureSupport::Supported if ctx.session.is_some() => None,
        FeatureSupport::Supported => {
            Some("No agent session was captured when Perkolator opened".to_owned())
        }
        FeatureSupport::Unknown => {
            Some("Insertion into agent sessions has not been verified on this machine".to_owned())
        }
        FeatureSupport::PermissionRequired { explanation }
        | FeatureSupport::Fallback { explanation }
        | FeatureSupport::Unavailable { explanation } => Some(explanation),
    }
}
