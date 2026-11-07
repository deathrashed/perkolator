//! Typed, capability-gated action registry and composer.
//!
//! Headless: the registry decides which actions a resource offers in a
//! context ([`actions_for`]), resolves them to [`ActionEffect`]s
//! ([`resolve`]), and decides truthfully whether text is inserted into an
//! agent session or copied ([`deliver`]). UI code only presents entries and
//! performs effects. Mutating actions only ever open the deployment planner.
//!
//! [`ActionEffect`]: perkolator_core::ActionEffect

mod compose;
mod context;
mod delivery;
mod mcp_config;
mod registry;
mod resolve;
mod template;

#[cfg(test)]
mod tests;

pub use compose::{ComposeFormat, compose_payload, invocation_line, reference_text};
pub use context::{ActionContext, DEFAULT_SESSION_MAX_AGE, SessionTarget, Surface};
pub use delivery::{Delivery, StaleTarget, deliver, validate_session};
pub use mcp_config::{ConfigSnippet, mcp_config_snippet};
pub use registry::{ActionEntry, actions_for, default_action};
pub use resolve::{
    ActionError, ActionInputs, MAX_COPY_BYTES, compose_effect, read_text_bounded, resolve,
};
pub use template::{TemplateError, expand_template, strip_frontmatter};
