//! `claude-plugins`: Anthropic's official Claude Code plugin directory,
//! https://github.com/anthropics/claude-plugins-official.

use std::sync::Arc;

use perkolator_core::ResourceKind;
use perkolator_marketplace::{
    MarketplaceProvider, Pagination, ProviderCapabilities, ProviderDescriptor, Transport,
};

use super::common::{github_auth, provider_id};
use super::plugin_marketplace::{MarketplaceSpec, PluginMarketplace};

pub const ID: &str = "claude-plugins";

pub fn provider() -> Arc<dyn MarketplaceProvider> {
    Arc::new(PluginMarketplace::new(
        ProviderDescriptor {
            id: provider_id(ID),
            display_name: "Claude Code plugins (Anthropic)".into(),
            homepage: "https://github.com/anthropics/claude-plugins-official".into(),
            docs_url: Some("https://code.claude.com/docs/en/plugin-marketplaces".into()),
            kinds: vec![
                ResourceKind::Plugin,
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
                categories: true,
                rate_limit_policy: Some(
                    "Index via raw.githubusercontent.com each search; component listing uses one \
                     GitHub API tree request (60/h anonymous), cached 10 minutes"
                        .into(),
                ),
                requires_attribution: None,
            },
            notes: vec![
                "Index: .claude-plugin/marketplace.json. Plugins hosted in this repository list \
                 their skills, agents and MCP configs from the repository tree; externally \
                 hosted plugins list only what their marketplace entry declares."
                    .into(),
                "Hooks and LSP servers are executable: they are flagged in summaries and never \
                 enabled or run during discovery. Command-based sources are never run."
                    .into(),
                "Commands are counted in the plugin summary (no separate command kind). \
                 package_ref is the Claude Code `name@claude-plugins-official` reference."
                    .into(),
            ],
        },
        MarketplaceSpec {
            owner: "anthropics",
            repo: "claude-plugins-official",
            index_path: ".claude-plugin/marketplace.json",
            manifest_path: ".claude-plugin/plugin.json",
            install_refs: true,
            client: None,
        },
    ))
}
