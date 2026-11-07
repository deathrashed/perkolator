//! Smithery registry (registry.smithery.ai). Smithery shut down its API
//! keys, CLI, Connect and `run.tools`/`server.smithery.ai` hosting on
//! 2026-10-31 and kept smithery.ai as a public directory. The anonymous,
//! page-numbered `/servers` listing and `/servers/{name}` detail still
//! answer; text search (`q`) fails server-side and that error is surfaced
//! unchanged rather than filtered locally.

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteItem,
    SearchPage, SearchRequest, Transport, now_unix,
};
use serde_json::Value;

use super::common::{
    encode_component, get_json, pretty, provider_id, redact_schema_defaults, text, tools_from,
    wants_mcp,
};

pub const ID: &str = "smithery";
pub const API_BASE: &str = "https://registry.smithery.ai";
pub const WEB_BASE: &str = "https://smithery.ai";
pub const SHUTDOWN_NOTICE_URL: &str = "https://smithery.ai/future";
/// Server-side maximum for `pageSize` (HTTP 400 above it).
const MAX_PAGE_SIZE: usize = 100;

pub struct Smithery {
    descriptor: ProviderDescriptor,
}

impl Smithery {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: provider_id(ID),
                display_name: "Smithery".into(),
                homepage: WEB_BASE.into(),
                docs_url: Some(SHUTDOWN_NOTICE_URL.into()),
                kinds: vec![ResourceKind::McpServer],
                transport: Transport::Rest,
                auth: AuthStrategy::None,
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: true,
                    fetch_files: false,
                    download: false,
                    install: false,
                    pagination: Pagination::Page,
                    categories: false,
                    rate_limit_policy: None,
                    requires_attribution: None,
                },
                notes: vec![
                    "Smithery discontinued its API keys, CLI and hosted server URLs on 2026-10-31; smithery.ai remains a public directory.".into(),
                    "Browsing works anonymously, but registry text search returned HTTP 500 for every query on 2026-11-07.".into(),
                    "Deployment URLs on run.tools / server.smithery.ai shown in previews no longer work; use the vendor's own MCP URL.".into(),
                    "Use counts and verification badges are not shown.".into(),
                ],
            },
        }
    }

    fn search_url(request: &SearchRequest) -> Result<String, ProviderError> {
        let page = match request
            .cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
        {
            Some(cursor) => cursor
                .parse::<u64>()
                .ok()
                .filter(|page| *page >= 1)
                .ok_or_else(|| {
                    ProviderError::malformed("Smithery page cursor is not a page number")
                })?,
            None => 1,
        };
        let mut url = format!(
            "{API_BASE}/servers?page={page}&pageSize={}",
            request.page_size.clamp(1, MAX_PAGE_SIZE)
        );
        let query = request.query.trim();
        if !query.is_empty() {
            url.push_str("&q=");
            url.push_str(&encode_component(query));
        }
        Ok(url)
    }

    /// `/servers/{namespace}/{slug}` with each segment encoded.
    fn server_path(qualified_name: &str) -> Result<String, ProviderError> {
        let segments: Vec<&str> = qualified_name.split('/').collect();
        if segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
        {
            return Err(ProviderError::unsupported("invalid Smithery server name"));
        }
        Ok(segments
            .iter()
            .map(|segment| encode_component(segment))
            .collect::<Vec<_>>()
            .join("/"))
    }

    fn item(&self, server: &Value, fetched_at_unix: u64) -> Option<RemoteItem> {
        let name = text(server, "qualifiedName")?;
        let canonical_url = Self::server_path(&name)
            .ok()
            .map(|path| format!("{WEB_BASE}/servers/{path}"));
        Some(RemoteItem {
            source: self.descriptor.id.clone(),
            title: text(server, "displayName").unwrap_or_else(|| name.clone()),
            upstream_id: name.clone(),
            kind: ResourceKind::McpServer,
            summary: text(server, "description").unwrap_or_default(),
            canonical_url,
            repo_url: None,
            resource_path: None,
            package_ref: Some(name),
            version: None,
            author: text(server, "namespace"),
            categories: Vec::new(),
            actions: RemoteActions::default(),
            fetched_at_unix,
        })
    }
}

impl Default for Smithery {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketplaceProvider for Smithery {
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
        let json = get_json(env, Self::search_url(request)?, Vec::new(), cancel)?;
        let servers = json["servers"]
            .as_array()
            .ok_or_else(|| ProviderError::malformed("Smithery list without a `servers` array"))?;
        let fetched_at_unix = now_unix();
        let items = servers
            .iter()
            .filter(|server| {
                !server["unlisted"].as_bool().unwrap_or(false)
                    && !server["inactive"].as_bool().unwrap_or(false)
            })
            .filter_map(|server| self.item(server, fetched_at_unix))
            .collect();
        let pagination = &json["pagination"];
        let next_cursor = match (
            pagination["currentPage"].as_u64(),
            pagination["totalPages"].as_u64(),
        ) {
            (Some(current), Some(pages)) if current < pages => Some((current + 1).to_string()),
            _ => None,
        };
        Ok(SearchPage {
            items,
            next_cursor,
            total: pagination["totalCount"].as_u64(),
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
        let name = item.package_ref.as_deref().unwrap_or(&item.upstream_id);
        let url = format!("{API_BASE}/servers/{}", Self::server_path(name)?);
        let json = get_json(env, url, Vec::new(), cancel)?;
        if text(&json, "qualifiedName").is_none() {
            return Err(ProviderError::malformed(
                "Smithery detail without `qualifiedName`",
            ));
        }
        let mut fresh = item.clone();
        if let Some(title) = text(&json, "displayName") {
            fresh.title = title;
        }
        if let Some(summary) = text(&json, "description") {
            fresh.summary = summary;
        }
        fresh.fetched_at_unix = now_unix();
        let tools = tools_from(&json["tools"]);
        let mut manifest = json;
        for connection in manifest
            .get_mut("connections")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(schema) = connection.get_mut("configSchema") {
                redact_schema_defaults(schema);
            }
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
