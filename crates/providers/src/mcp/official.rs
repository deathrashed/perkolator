//! Official MCP Registry (registry.modelcontextprotocol.io): public,
//! anonymous, cursor-paginated `v0.1` API. Registry metadata only — it
//! publishes packages and remotes but no tool schemas.

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteDetail, RemoteItem, SearchPage,
    SearchRequest, Transport,
};

use super::common::{get_json, pretty, provider_id, wants_mcp};
use super::registry::RegistryApi;

pub const ID: &str = "mcp-official";
pub const API_BASE: &str = "https://registry.modelcontextprotocol.io/v0.1";
/// The registry rejects a non-empty `search` shorter than this with HTTP 422.
pub const MIN_SEARCH_LEN: usize = 3;

pub struct McpOfficial {
    descriptor: ProviderDescriptor,
    api: RegistryApi,
}

impl McpOfficial {
    pub fn new() -> Self {
        let id = provider_id(ID);
        Self {
            descriptor: ProviderDescriptor {
                id: id.clone(),
                display_name: "Official MCP Registry".into(),
                homepage: "https://registry.modelcontextprotocol.io".into(),
                docs_url: Some("https://registry.modelcontextprotocol.io/docs".into()),
                kinds: vec![ResourceKind::McpServer],
                transport: Transport::Rest,
                auth: AuthStrategy::None,
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: true,
                    fetch_files: false,
                    download: false,
                    install: false,
                    pagination: Pagination::Cursor,
                    categories: false,
                    rate_limit_policy: None,
                    requires_attribution: None,
                },
                notes: vec![
                    "Search is a substring match on server names and needs at least 3 characters; shorter queries return no registry results.".into(),
                    "Only the latest version of each server is listed; deleted entries are hidden.".into(),
                    "The registry publishes packages and remotes, not tool schemas.".into(),
                    "Secret header and environment values are redacted in previews.".into(),
                ],
            },
            api: RegistryApi {
                source: id,
                base: API_BASE,
                min_search_len: MIN_SEARCH_LEN,
                public_canonical: true,
            },
        }
    }
}

impl Default for McpOfficial {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketplaceProvider for McpOfficial {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, _env: &ProviderEnv) -> AuthStatus {
        AuthStatus::NotRequired
    }

    fn search(
        &self,
        request: &SearchRequest,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<SearchPage, ProviderError> {
        if !wants_mcp(request) {
            return Ok(SearchPage::empty());
        }
        let Some(url) = self.api.list_url(request) else {
            return Ok(SearchPage::empty());
        };
        let json = get_json(env, url, Vec::new(), cancel)?;
        self.api.parse_list(&json)
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        if item.source != self.descriptor.id {
            return Err(ProviderError::unsupported("item belongs to another source"));
        }
        let name = item.package_ref.as_deref().unwrap_or(&item.upstream_id);
        let url = self.api.version_url(name, item.version.as_deref());
        let json = get_json(env, url, Vec::new(), cancel)?;
        let (fresh, manifest) = self.api.parse_detail(&json)?;
        Ok(RemoteDetail {
            item: fresh,
            readme_markdown: None,
            manifest_json: pretty(&manifest),
            files: Vec::new(),
            tools: Vec::new(),
        })
    }
}
