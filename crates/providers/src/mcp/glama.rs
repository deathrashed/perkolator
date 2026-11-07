//! Glama MCP directory API (glama.ai/api/mcp/v1). Every directory read
//! needs a user API key (`Authorization: Bearer`), and the API Data License
//! requires visible attribution plus a link to each record's Glama listing.

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteItem,
    SearchPage, SearchRequest, Transport, now_unix,
};
use serde_json::Value;

use super::common::{
    encode_component, get_json, https_url, pretty, provider_id, redact_schema_defaults, text,
    tools_from, wants_mcp,
};

pub const ID: &str = "glama";
pub const API_BASE: &str = "https://glama.ai/api/mcp/v1";
pub const DOCS_URL: &str = "https://glama.ai/mcp/reference";
pub const KEYS_URL: &str = "https://glama.ai/settings/api-keys";
pub const KEY_SLOT: &str = "glama";
pub const ATTRIBUTION: &str = "Glama API Data License: every view showing Glama data needs a visible credit linking to https://glama.ai (no nofollow/sponsored/ugc), and every listed server must link to its Glama listing (the item's canonical URL).";
/// Server-side maximum for `first`.
const MAX_FIRST: usize = 100;
/// Newest-first ordering is documented as unaffected by paid Boost
/// placement, so results never need an advertising label.
const SORT: &str = "added-at:desc";

pub struct Glama {
    descriptor: ProviderDescriptor,
}

impl Glama {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: provider_id(ID),
                display_name: "Glama".into(),
                homepage: "https://glama.ai/mcp/servers".into(),
                docs_url: Some(DOCS_URL.into()),
                kinds: vec![ResourceKind::McpServer],
                transport: Transport::Rest,
                auth: AuthStrategy::ApiKey {
                    slot: KEY_SLOT.into(),
                    docs_url: KEYS_URL.into(),
                },
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: true,
                    fetch_files: false,
                    download: false,
                    install: false,
                    pagination: Pagination::Cursor,
                    categories: false,
                    rate_limit_policy: Some(
                        "100 requests/second per IP across Glama APIs; 429 means wait RateLimit-Reset seconds; continued requests earn a 403 ban that does not clear on its own.".into(),
                    ),
                    requires_attribution: Some(ATTRIBUTION.into()),
                },
                notes: vec![
                    "Needs a Glama API key; directory reads are not anonymous.".into(),
                    "Results are ordered newest first (Boost-free ordering), not by relevance.".into(),
                    "Glama quality scores and Boost flags are not shown.".into(),
                ],
            },
        }
    }

    fn get(env: &ProviderEnv, url: String, cancel: &CancelToken) -> Result<Value, ProviderError> {
        let key = env
            .credentials
            .get(KEY_SLOT)
            .filter(|key| !key.is_empty())
            .ok_or(ProviderError::NeedsAuth)?;
        get_json(
            env,
            url,
            vec![("Authorization", format!("Bearer {}", key.expose()))],
            cancel,
        )
    }

    fn search_url(request: &SearchRequest) -> String {
        let mut url = format!(
            "{API_BASE}/servers?first={}&sort={}",
            request.page_size.clamp(1, MAX_FIRST),
            encode_component(SORT)
        );
        let query = request.query.trim();
        if !query.is_empty() {
            url.push_str("&query=");
            url.push_str(&encode_component(query));
        }
        if let Some(cursor) = request
            .cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
        {
            url.push_str("&after=");
            url.push_str(&encode_component(cursor));
        }
        url
    }

    fn item(&self, server: &Value, fetched_at_unix: u64) -> Option<RemoteItem> {
        let id = text(server, "id")?;
        let repo_url = https_url(&server["repository"], "url");
        Some(RemoteItem {
            source: self.descriptor.id.clone(),
            title: text(server, "name").unwrap_or_else(|| id.clone()),
            upstream_id: id,
            kind: ResourceKind::McpServer,
            summary: text(server, "description").unwrap_or_default(),
            canonical_url: https_url(server, "url"),
            actions: RemoteActions {
                view_source: repo_url.is_some(),
                fetch_files: false,
                install: false,
            },
            repo_url,
            resource_path: None,
            package_ref: None,
            version: None,
            author: text(server, "namespace"),
            categories: server["attributes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|attribute| attribute.as_str().map(str::to_owned))
                .collect(),
            fetched_at_unix,
        })
    }
}

impl Default for Glama {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketplaceProvider for Glama {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus {
        match env.credentials.get(KEY_SLOT).filter(|key| !key.is_empty()) {
            Some(_) => AuthStatus::Ready { identity: None },
            None => AuthStatus::NotConfigured,
        }
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
        let json = Self::get(env, Self::search_url(request), cancel)?;
        let servers = json["servers"]
            .as_array()
            .ok_or_else(|| ProviderError::malformed("Glama list without a `servers` array"))?;
        let fetched_at_unix = now_unix();
        let items = servers
            .iter()
            .filter_map(|server| self.item(server, fetched_at_unix))
            .collect();
        let page_info = &json["pageInfo"];
        let next_cursor = page_info["hasNextPage"]
            .as_bool()
            .unwrap_or(false)
            .then(|| text(page_info, "endCursor"))
            .flatten();
        Ok(SearchPage {
            items,
            next_cursor,
            total: None,
        })
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
        let url = format!("{API_BASE}/servers/{}", encode_component(&item.upstream_id));
        let json = Self::get(env, url, cancel)?;
        let fresh = self
            .item(&json, now_unix())
            .ok_or_else(|| ProviderError::malformed("Glama server without an `id`"))?;
        let tools = tools_from(&json["tools"]);
        let mut manifest = json;
        if let Some(schema) = manifest.get_mut("environmentVariablesJsonSchema") {
            redact_schema_defaults(schema);
        }
        Ok(RemoteDetail {
            item: fresh,
            readme_markdown: None,
            manifest_json: pretty(&manifest),
            files: Vec::new(),
            tools,
        })
    }
}
