//! Built-in marketplace source adapters. Each group module is owned by one
//! workstream (docs/MARKETPLACE_PARALLEL_WORK.md) and returns its providers;
//! this file only assembles them.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

pub mod bundles;
pub mod mcp;
pub mod prompts;
pub mod skills;

/// Every remote source id in config/marketplace-providers.example.toml.
/// `local-inventory` is the local index, not a remote provider.
pub const REMOTE_SOURCE_IDS: [&str; 16] = [
    "lobehub",
    "skillssh",
    "skillsmp",
    "mcp-official",
    "smithery",
    "glama",
    "claude-plugins",
    "cursor-plugins",
    "wshobson-agents",
    "voltagent-skills",
    "voltagent-subagents",
    "officialskills",
    "prompts-chat",
    "awesome-claude",
    "pulsemcp",
    "github-direct",
];

pub fn builtin() -> Vec<Arc<dyn MarketplaceProvider>> {
    let mut providers = Vec::new();
    providers.extend(skills::providers());
    providers.extend(mcp::providers());
    providers.extend(bundles::providers());
    providers.extend(prompts::providers());
    providers
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn provider_ids_are_unique_and_known() {
        let ids: Vec<String> = builtin()
            .iter()
            .map(|provider| provider.descriptor().id.to_string())
            .collect();
        let unique: BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate provider ids: {ids:?}");
        for id in &ids {
            assert!(REMOTE_SOURCE_IDS.contains(&id.as_str()), "unknown id {id}");
        }
    }

    #[test]
    fn every_provider_declares_kinds_and_a_homepage() {
        for provider in builtin() {
            let descriptor = provider.descriptor();
            assert!(
                !descriptor.kinds.is_empty(),
                "{} has no kinds",
                descriptor.id
            );
            assert!(
                descriptor.homepage.starts_with("https://"),
                "{}",
                descriptor.id
            );
        }
    }
}
