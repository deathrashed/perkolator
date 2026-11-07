//! prompts.chat through its documented public REST endpoint
//! (`GET https://prompts.chat/api/prompts?q=&type=&page=&perPage=`, see
//! https://prompts.chat/docs/api). Anonymous; an API key only unlocks the
//! caller's private prompts and saving, which Perkolator does not use.
//!
//! Bots (`type=BOT`) are agent definitions (instructions, skills, routines,
//! connectors and per-platform install entries) and map to agents. Prompt
//! types map to prompts. `SKILL` and `CLASSIFIER` entries are outside this
//! source's declared kinds and are skipped rather than mislabelled.

use perkolator_core::{CancelToken, ProviderId, ResourceKind};
use perkolator_marketplace::github::encode_query;
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteFile,
    RemoteItem, SearchPage, SearchRequest, Transport, now_unix,
};
use serde::Deserialize;
use serde_json::{Map, Value};

pub const ID: &str = "prompts-chat";
const BASE: &str = "https://prompts.chat";
/// Server-side cap (`MAX_API_PER_PAGE` in the upstream route).
const MAX_PER_PAGE: usize = 100;
const BOT_TYPE: &str = "BOT";

pub struct PromptsChat {
    descriptor: ProviderDescriptor,
}

impl PromptsChat {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: ProviderId::new(ID).expect("valid provider id"),
                display_name: "prompts.chat".into(),
                homepage: BASE.into(),
                docs_url: Some(format!("{BASE}/docs/api")),
                kinds: vec![ResourceKind::Prompt, ResourceKind::Agent],
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
                    rate_limit_policy: Some(
                        "Public API without a published numeric limit; the docs ask for \
                         respectful request frequency. One request per wanted kind per page."
                            .into(),
                    ),
                    requires_attribution: None,
                },
                notes: vec![
                    "Uses the documented REST search endpoint; no login needed for public prompts."
                        .into(),
                    "Bots are listed as agents; prompts.chat skills and classifiers are skipped."
                        .into(),
                    "Prompt content is CC0 1.0 per the upstream LICENSE; bodies are untrusted text."
                        .into(),
                    "The MCP endpoint (POST /api/mcp) is not used: the HTTP seam is GET-only."
                        .into(),
                    "Category filters are ignored: the API filters by internal category id only."
                        .into(),
                ],
            },
        }
    }
}

impl Default for PromptsChat {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    prompts: Vec<Value>,
    total: u64,
    total_pages: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    id: String,
    title: String,
    slug: Option<String>,
    description: Option<String>,
    content: Option<String>,
    #[serde(rename = "type")]
    prompt_type: String,
    author: Option<Author>,
    category: Option<Category>,
    #[serde(default)]
    tags: Vec<TagLink>,
    bot_platforms: Option<Vec<String>>,
    bot_config: Option<Value>,
    computer_requirements: Option<Vec<String>>,
    install_url: Option<String>,
}

#[derive(Deserialize)]
struct Author {
    username: Option<String>,
}

#[derive(Deserialize)]
struct Category {
    name: String,
}

#[derive(Deserialize)]
struct TagLink {
    tag: Tag,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
}

fn kind_for_type(prompt_type: &str) -> Option<ResourceKind> {
    match prompt_type {
        BOT_TYPE => Some(ResourceKind::Agent),
        "TEXT" | "STRUCTURED" | "IMAGE" | "VIDEO" | "AUDIO" | "TASTE" => Some(ResourceKind::Prompt),
        _ => None,
    }
}

/// Upstream ids are cuids; anything else is refused before it reaches a URL.
fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn list_url(query: &str, prompt_type: Option<&str>, page: u64, per_page: usize) -> String {
    let mut url = format!("{BASE}/api/prompts?page={page}&perPage={per_page}");
    let query = query.trim();
    if !query.is_empty() {
        url.push_str("&q=");
        url.push_str(&encode_query(query));
    }
    if let Some(prompt_type) = prompt_type {
        url.push_str("&type=");
        url.push_str(prompt_type);
    }
    url
}

fn parse_page_cursor(cursor: Option<&str>) -> Result<u64, ProviderError> {
    match cursor {
        None => Ok(1),
        Some(value) => value
            .parse::<u64>()
            .ok()
            .filter(|page| *page >= 1)
            .ok_or_else(|| ProviderError::malformed("invalid page cursor")),
    }
}

fn parse_entry(value: Value) -> Result<Entry, ProviderError> {
    serde_json::from_value(value)
        .map_err(|error| ProviderError::malformed(format!("prompt entry: {error}")))
}

fn to_item(entry: &Entry, source: &ProviderId, fetched_at_unix: u64) -> Option<RemoteItem> {
    let kind = kind_for_type(&entry.prompt_type)?;
    if !is_safe_segment(&entry.id) {
        return None;
    }
    let section = if kind == ResourceKind::Agent {
        "bots"
    } else {
        "prompts"
    };
    let canonical_url = match entry.slug.as_deref().filter(|slug| is_safe_segment(slug)) {
        Some(slug) => format!("{BASE}/{section}/{}_{slug}", entry.id),
        None => format!("{BASE}/{section}/{}", entry.id),
    };
    let mut categories = Vec::new();
    if let Some(category) = &entry.category {
        categories.push(category.name.clone());
    }
    for link in &entry.tags {
        if !categories.contains(&link.tag.name) {
            categories.push(link.tag.name.clone());
        }
    }
    Some(RemoteItem {
        source: source.clone(),
        upstream_id: entry.id.clone(),
        kind,
        title: entry.title.clone(),
        summary: entry.description.clone().unwrap_or_default(),
        canonical_url: Some(canonical_url),
        repo_url: None,
        resource_path: None,
        package_ref: None,
        version: None,
        author: entry
            .author
            .as_ref()
            .and_then(|author| author.username.clone()),
        categories,
        actions: RemoteActions {
            view_source: true,
            fetch_files: false,
            install: false,
        },
        fetched_at_unix,
    })
}

/// Bot definition fields exactly as the provider reports them.
fn bot_manifest(entry: &Entry) -> Option<String> {
    let mut manifest = Map::new();
    if let Some(platforms) = &entry.bot_platforms {
        manifest.insert("botPlatforms".into(), Value::from(platforms.clone()));
    }
    if let Some(config) = entry.bot_config.as_ref().filter(|value| !value.is_null()) {
        manifest.insert("botConfig".into(), config.clone());
    }
    if let Some(requirements) = &entry.computer_requirements {
        manifest.insert(
            "computerRequirements".into(),
            Value::from(requirements.clone()),
        );
    }
    if let Some(url) = &entry.install_url {
        manifest.insert("installUrl".into(), Value::from(url.clone()));
    }
    (!manifest.is_empty())
        .then(|| serde_json::to_string_pretty(&Value::Object(manifest)).ok())
        .flatten()
}

impl MarketplaceProvider for PromptsChat {
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
        let want_prompts = request.wants(ResourceKind::Prompt);
        let want_agents = request.wants(ResourceKind::Agent);
        if !want_prompts && !want_agents {
            return Ok(SearchPage::empty());
        }
        let page = parse_page_cursor(request.cursor.as_deref())?;
        let per_page = request.page_size.clamp(1, MAX_PER_PAGE);
        let fetched_at_unix = now_unix();
        let mut items = Vec::new();
        let mut has_more = false;
        let mut total = None;
        // The untyped listing excludes bots upstream, so the two never overlap.
        let listings = [(want_prompts, None), (want_agents, Some(BOT_TYPE))];
        for (_, prompt_type) in listings.into_iter().filter(|(wanted, _)| *wanted) {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            let json = env.get_json(&list_url(&request.query, prompt_type, page, per_page))?;
            let listing: Listing = serde_json::from_value(json)
                .map_err(|error| ProviderError::malformed(format!("prompt listing: {error}")))?;
            has_more |= page < listing.total_pages;
            for value in listing.prompts {
                let entry = parse_entry(value)?;
                if let Some(item) = to_item(&entry, &self.descriptor.id, fetched_at_unix)
                    && request.wants(item.kind)
                {
                    items.push(item);
                }
            }
            // The untyped total also counts skipped skills, so only the bot
            // listing's total is an exact count for this source's kinds.
            if prompt_type.is_some() && !want_prompts {
                total = Some(listing.total);
            }
        }
        Ok(SearchPage {
            items,
            next_cursor: has_more.then(|| (page + 1).to_string()),
            total,
        })
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        if !is_safe_segment(&item.upstream_id) {
            return Err(ProviderError::malformed("invalid prompts.chat id"));
        }
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let url = format!("{BASE}/api/prompts/{}", item.upstream_id);
        let entry = parse_entry(env.get_json(&url)?)?;
        if entry.id != item.upstream_id {
            return Err(ProviderError::malformed(
                "detail id does not match the item",
            ));
        }
        let detail_item = to_item(&entry, &self.descriptor.id, now_unix()).ok_or_else(|| {
            ProviderError::unsupported(format!("prompt type `{}`", entry.prompt_type))
        })?;
        let manifest_json = (detail_item.kind == ResourceKind::Agent)
            .then(|| bot_manifest(&entry))
            .flatten();
        Ok(RemoteDetail {
            readme_markdown: entry.content.clone(),
            manifest_json,
            files: Vec::<RemoteFile>::new(),
            tools: Vec::new(),
            item: detail_item,
        })
    }
}
