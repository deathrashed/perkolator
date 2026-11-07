//! Prompts and generic GitHub (Worker D): prompts-chat, github-direct.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    Vec::new()
}
