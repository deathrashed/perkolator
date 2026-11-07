//! Plugin bundles and agent indexes (Worker C): claude-plugins,
//! cursor-plugins, wshobson-agents, voltagent-subagents, awesome-claude.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    Vec::new()
}
