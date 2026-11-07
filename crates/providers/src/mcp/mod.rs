//! MCP registries (Worker B): mcp-official, smithery, glama, pulsemcp.
//! Registry metadata only: no adapter connects to or starts an MCP server.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

mod common;
pub mod glama;
pub mod official;
pub mod pulsemcp;
mod registry;
pub mod smithery;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    vec![
        Arc::new(official::McpOfficial::new()),
        Arc::new(smithery::Smithery::new()),
        Arc::new(glama::Glama::new()),
        Arc::new(pulsemcp::PulseMcp::new()),
    ]
}
