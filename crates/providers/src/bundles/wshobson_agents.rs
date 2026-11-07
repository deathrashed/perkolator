//! `wshobson-agents`: https://github.com/wshobson/agents, a Claude Code
//! format marketplace (`claude-code-workflows`) of agent/skill bundles.

use std::sync::Arc;

use perkolator_core::ResourceKind;
use perkolator_marketplace::{
    MarketplaceProvider, Pagination, ProviderCapabilities, ProviderDescriptor, Transport,
};

use super::common::{github_auth, provider_id};
use super::plugin_marketplace::{MarketplaceSpec, PluginMarketplace};

pub const ID: &str = "wshobson-agents";

pub fn provider() -> Arc<dyn MarketplaceProvider> {
    Arc::new(PluginMarketplace::new(
        ProviderDescriptor {
            id: provider_id(ID),
            display_name: "wshobson/agents".into(),
            homepage: "https://github.com/wshobson/agents".into(),
            docs_url: None,
            kinds: vec![
                ResourceKind::Agent,
                ResourceKind::Skill,
                ResourceKind::Plugin,
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
                categories: true,
                rate_limit_policy: Some(
                    "Index via raw.githubusercontent.com each search; agents/skills use one \
                     GitHub API tree request (60/h anonymous), cached 10 minutes"
                        .into(),
                ),
                requires_attribution: None,
            },
            notes: vec![
                "Index: .claude-plugin/marketplace.json. Agents and skills come from \
                 plugins/<name>/agents and plugins/<name>/skills in the repository tree."
                    .into(),
                "Externally sourced entries (git-subdir) are listed without components and are \
                 not assumed installable. Hooks are flagged, never run."
                    .into(),
            ],
        },
        MarketplaceSpec {
            owner: "wshobson",
            repo: "agents",
            index_path: ".claude-plugin/marketplace.json",
            manifest_path: ".claude-plugin/plugin.json",
            install_refs: true,
            client: None,
        },
    ))
}
