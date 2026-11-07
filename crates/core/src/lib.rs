//! Shared, headless domain contract for Perkolator.
//!
//! Every crate (registry, deployment, search, preview, actions, UI) speaks in
//! these types. The six resource kinds share identity, provenance, locations
//! and capabilities, but keep their own typed details: there is no universal
//! "install" or "toggle" operation.

mod action;
mod cancel;
mod capability;
mod details;
mod error;
mod file;
mod id;
mod kind;
mod platform;
mod record;
mod rel_path;
mod secret;

pub use action::{ActionAvailability, ActionEffect, ActionId, ActionOutcome, ClipboardPayload};
pub use cancel::CancelToken;
pub use capability::{Capability, CapabilitySet};
pub use details::{
    AgentProfileDetails, ConfigFormat, EnvBinding, McpServerDetails, McpToolDecl, McpTransport,
    PluginComponent, PluginDetails, PluginHost, PromptTemplateDetails, PromptVariable,
    ResourceDetails, RuleDetails, RuleFormat, SkillDetails, TemplateSyntax,
};
pub use error::CoreError;
pub use file::{FileKind, ResourceFile};
pub use id::{ProviderId, ResourceId};
pub use kind::ResourceKind;
pub use platform::{FeatureSupport, PlatformFeature};
pub use record::{
    InstallState, IssueSeverity, LocationRole, Provenance, ResourceLocation, ResourceRecord, Scope,
    SourceKind, ValidationIssue,
};
pub use rel_path::RelPath;
pub use secret::SecretString;
