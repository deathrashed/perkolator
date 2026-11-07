//! MCP registries (Worker B): mcp-official, smithery, glama, pulsemcp.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    Vec::new()
}
