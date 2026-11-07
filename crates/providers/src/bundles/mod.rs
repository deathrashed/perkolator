//! Plugin bundles and agent indexes (Worker C): claude-plugins,
//! cursor-plugins, wshobson-agents, voltagent-subagents, awesome-claude.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

pub mod awesome_claude;
pub mod claude_plugins;
mod common;
pub mod cursor_plugins;
mod plugin_marketplace;
pub mod voltagent_subagents;
pub mod wshobson_agents;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    vec![
        claude_plugins::provider(),
        cursor_plugins::provider(),
        wshobson_agents::provider(),
        voltagent_subagents::provider(),
        awesome_claude::provider(),
    ]
}
