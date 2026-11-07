//! PulseMCP sub-registry (api.pulsemcp.com/v0.1): the generic MCP Registry
//! API behind partner credentials (`X-API-Key` + `X-Tenant-ID`). There is no
//! anonymous or self-serve access; without both credentials no request is
//! made and the source reports `Needs access`.

use perkolator_core::{CancelToken, McpToolDecl, ResourceKind, SecretString};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteDetail, RemoteItem, SearchPage,
    SearchRequest, Transport,
};
use serde_json::Value;

use super::common::{get_json, pretty, provider_id, tools_from, wants_mcp};
use super::registry::RegistryApi;

pub const ID: &str = "pulsemcp";
pub const API_BASE: &str = "https://api.pulsemcp.com/v0.1";
pub const DOCS_URL: &str = "https://www.pulsemcp.com/api";
/// Credential slot for the partner API key (`X-API-Key`).
pub const KEY_SLOT: &str = "pulsemcp";
/// Credential slot for the partner tenant id (`X-Tenant-ID`).
pub const TENANT_SLOT: &str = "pulsemcp-tenant";
const VERSION_META: &str = "com.pulsemcp/server-version";

pub struct PulseMcp {
    descriptor: ProviderDescriptor,
    api: RegistryApi,
}

impl PulseMcp {
    pub fn new() -> Self {
        let id = provider_id(ID);
        Self {
            descriptor: ProviderDescriptor {
                id: id.clone(),
                display_name: "PulseMCP".into(),
                homepage: "https://www.pulsemcp.com".into(),
                docs_url: Some(DOCS_URL.into()),
                kinds: vec![ResourceKind::McpServer],
                transport: Transport::Rest,
                auth: AuthStrategy::PartnerAccess {
                    docs_url: DOCS_URL.into(),
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
                        "200 requests/minute, 5,000/hour, 10,000/day per API key".into(),
                    ),
                    requires_attribution: None,
                },
                notes: vec![
                    "Partner API: needs a PulseMCP-issued API key and tenant ID (arranged via hello@pulsemcp.com); there is no self-serve signup.".into(),
                    "Tool lists appear only when the tenant has PulseMCP's premium tools enrichment.".into(),
                    "Visitor and popularity estimates are not shown.".into(),
                ],
            },
            api: RegistryApi {
                source: id,
                base: API_BASE,
                min_search_len: 0,
                public_canonical: false,
            },
        }
    }

    fn credentials(env: &ProviderEnv) -> Option<(SecretString, SecretString)> {
        let key = env
            .credentials
            .get(KEY_SLOT)
            .filter(|key| !key.is_empty())?;
        let tenant = env
            .credentials
            .get(TENANT_SLOT)
            .filter(|tenant| !tenant.is_empty())?;
        Some((key, tenant))
    }

    fn get(env: &ProviderEnv, url: String, cancel: &CancelToken) -> Result<Value, ProviderError> {
        let (key, tenant) = Self::credentials(env).ok_or(ProviderError::NeedsAuth)?;
        get_json(
            env,
            url,
            vec![
                ("X-API-Key", key.expose().to_owned()),
                ("X-Tenant-ID", tenant.expose().to_owned()),
            ],
            cancel,
        )
    }
}

impl Default for PulseMcp {
    fn default() -> Self {
        Self::new()
    }
}

/// Tools from the premium `remotes[N]` / `packages[N]` enrichments.
fn enrichment_tools(entry: &Value) -> Vec<McpToolDecl> {
    let mut tools: Vec<McpToolDecl> = Vec::new();
    let Some(meta) = entry["_meta"][VERSION_META].as_object() else {
        return tools;
    };
    for (key, enrichment) in meta {
        if !(key.starts_with("remotes[") || key.starts_with("packages[")) {
            continue;
        }
        for tool in tools_from(&enrichment["tools"]) {
            if !tools.iter().any(|known| known.name == tool.name) {
                tools.push(tool);
            }
        }
    }
    tools
}

impl MarketplaceProvider for PulseMcp {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus {
        match Self::credentials(env) {
            Some(_) => AuthStatus::Ready { identity: None },
            None => AuthStatus::NeedsAccess,
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
        let Some(url) = self.api.list_url(request) else {
            return Ok(SearchPage::empty());
        };
        let json = Self::get(env, url, cancel)?;
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
        let json = Self::get(env, url, cancel)?;
        let (fresh, manifest) = self.api.parse_detail(&json)?;
        Ok(RemoteDetail {
            item: fresh,
            readme_markdown: None,
            manifest_json: pretty(&manifest),
            files: Vec::new(),
            tools: enrichment_tools(&json),
        })
    }
}
