//! `cursor-plugins`: Cursor's official plugin marketplace,
//! https://github.com/cursor/plugins (`.cursor-plugin/` layout).

use std::sync::Arc;

use perkolator_core::ResourceKind;
use perkolator_marketplace::{
    MarketplaceProvider, Pagination, ProviderCapabilities, ProviderDescriptor, Transport,
};

use super::common::{github_auth, provider_id};
use super::plugin_marketplace::{MarketplaceSpec, PluginMarketplace};

pub const ID: &str = "cursor-plugins";

pub fn provider() -> Arc<dyn MarketplaceProvider> {
    Arc::new(PluginMarketplace::new(
        ProviderDescriptor {
            id: provider_id(ID),
            display_name: "Cursor plugins".into(),
            homepage: "https://github.com/cursor/plugins".into(),
            docs_url: Some("https://cursor.com/docs/plugins".into()),
            kinds: vec![
                ResourceKind::Plugin,
                ResourceKind::Rule,
                ResourceKind::Skill,
                ResourceKind::Agent,
                ResourceKind::McpServer,
            ],
            transport: Transport::GithubIndex,
            auth: github_auth(),
            capabilities: ProviderCapabilities {
                search: true,
                detail: true,
                fetch_files: true,
                download: true,
                install: false,
                pagination: Pagination::Page,
                // marketplace.json carries no categories (they live in each plugin.json).
                categories: false,
                rate_limit_policy: Some(
                    "Index via raw.githubusercontent.com each search; component listing uses one \
                     GitHub API tree request (60/h anonymous), cached 10 minutes"
                        .into(),
                ),
                requires_attribution: None,
            },
            notes: vec![
                "Index: .cursor-plugin/marketplace.json (name, source, description, \
                 minClientVersions). Rules, skills, agents and mcp.json are found by the \
                 conventional plugin folder layout; plugin.json is read in detail."
                    .into(),
                "Hooks are flagged and never enabled automatically. Entries marked \
                 `minClientVersions.cursor = never` are shown as unavailable for Cursor."
                    .into(),
                "Category filters are ignored: the marketplace index has no categories.".into(),
            ],
        },
        MarketplaceSpec {
            owner: "cursor",
            repo: "plugins",
            index_path: ".cursor-plugin/marketplace.json",
            manifest_path: ".cursor-plugin/plugin.json",
            install_refs: false,
            client: Some("cursor"),
        },
    ))
}
