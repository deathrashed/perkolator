//! `skillsmp`: SkillsMP REST search (https://skillsmp.com/docs/api).
//! `GET /api/v1/skills/search` works anonymously (keyword search only,
//! 50/day and 10/min); an optional `Authorization: Bearer <key>` uses the
//! account allowance. Previews read the indexed `SKILL.md` from GitHub.

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::github::{RepoRef, encode_query};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, HttpRequest, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteItem,
    SearchPage, SearchRequest, Transport, now_unix,
};
use serde_json::Value;

use super::common::{GithubLink, ensure_active, github_skill_detail, provider_id};

pub const ID: &str = "skillsmp";
pub const CREDENTIAL_SLOT: &str = "skillsmp";
pub const SEARCH_URL: &str = "https://skillsmp.com/api/v1/skills/search";
pub const DOCS_URL: &str = "https://skillsmp.com/docs/api";
/// Documented maximum `limit`.
const MAX_PAGE_SIZE: usize = 50;

pub struct SkillsMp {
    descriptor: ProviderDescriptor,
}

impl SkillsMp {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: provider_id(ID),
                display_name: "SkillsMP".into(),
                homepage: "https://skillsmp.com".into(),
                docs_url: Some(DOCS_URL.into()),
                kinds: vec![ResourceKind::Skill],
                transport: Transport::Rest,
                auth: AuthStrategy::OptionalToken {
                    slot: CREDENTIAL_SLOT.into(),
                },
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: true,
                    fetch_files: true,
                    download: false,
                    install: false,
                    pagination: Pagination::Page,
                    categories: true,
                    rate_limit_policy: Some(
                        "Anonymous: 50 searches/day and 10/min. API key: account allowance (Free 50/day), 30 attempts/min.".into(),
                    ),
                    requires_attribution: None,
                },
                notes: vec![
                    "Community index of SKILL.md files on public GitHub repositories; not affiliated with Anthropic or OpenAI.".into(),
                    "Anonymous access is keyword search only: the category filter is sent only when an API key is configured.".into(),
                    "Each successful search uses one unit of the daily/monthly allowance.".into(),
                ],
            },
        }
    }
}

impl Default for SkillsMp {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_page(cursor: Option<&str>) -> Result<u64, ProviderError> {
    match cursor {
        None => Ok(1),
        Some(cursor) => cursor
            .parse::<u64>()
            .ok()
            .filter(|page| *page >= 1)
            .ok_or_else(|| ProviderError::malformed(format!("invalid cursor `{cursor}`"))),
    }
}

fn to_item(skill: &Value, fetched_at_unix: u64) -> Option<RemoteItem> {
    let upstream_id = skill["id"].as_str()?.to_owned();
    let title = skill["name"].as_str()?.to_owned();
    let link = skill["githubUrl"].as_str().and_then(GithubLink::parse);
    Some(RemoteItem {
        source: provider_id(ID),
        upstream_id,
        kind: ResourceKind::Skill,
        title,
        summary: skill["description"].as_str().unwrap_or_default().to_owned(),
        canonical_url: skill["skillUrl"].as_str().map(str::to_owned),
        repo_url: link.as_ref().map(|link| link.repo.html_url()),
        resource_path: link.as_ref().and_then(GithubLink::resource_path),
        package_ref: None,
        version: None,
        author: skill["author"].as_str().map(str::to_owned),
        categories: Vec::new(),
        actions: RemoteActions {
            view_source: link.is_some(),
            fetch_files: link.is_some(),
            install: false,
        },
        fetched_at_unix,
    })
}

pub(crate) fn parse_response(json: &Value, page: u64) -> Result<SearchPage, ProviderError> {
    if json["success"].as_bool() != Some(true) {
        let code = json["error"]["code"].as_str().unwrap_or("unknown");
        return Err(ProviderError::malformed(format!(
            "SkillsMP reported an error ({code})"
        )));
    }
    let data = &json["data"];
    let skills = data["skills"]
        .as_array()
        .ok_or_else(|| ProviderError::malformed("SkillsMP response without `data.skills`"))?;
    let fetched_at_unix = now_unix();
    let items = skills
        .iter()
        .filter_map(|skill| to_item(skill, fetched_at_unix))
        .collect();
    let pagination = &data["pagination"];
    Ok(SearchPage {
        items,
        next_cursor: (pagination["hasNext"].as_bool() == Some(true))
            .then(|| (page + 1).to_string()),
        total: (pagination["totalIsExact"].as_bool() == Some(true))
            .then(|| pagination["total"].as_u64())
            .flatten(),
    })
}

impl MarketplaceProvider for SkillsMp {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus {
        if env.credentials.get(CREDENTIAL_SLOT).is_some() {
            AuthStatus::Ready { identity: None }
        } else {
            AuthStatus::NotRequired
        }
    }

    fn search(
        &self,
        request: &SearchRequest,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<SearchPage, ProviderError> {
        if !request.wants(ResourceKind::Skill) {
            return Ok(SearchPage::empty());
        }
        let query = request.query.trim();
        // The API rejects keywords without a letter or digit and wildcards;
        // skip the call rather than spend allowance on a 400.
        if !query.chars().any(char::is_alphanumeric) {
            return Ok(SearchPage::empty());
        }
        let page = parse_page(request.cursor.as_deref())?;
        let limit = request.page_size.clamp(1, MAX_PAGE_SIZE);
        let key = env.credentials.get(CREDENTIAL_SLOT);
        let mut url = format!(
            "{SEARCH_URL}?q={}&page={page}&limit={limit}",
            encode_query(query)
        );
        if key.is_some()
            && let Some(category) = request.category.as_deref()
        {
            url.push_str(&format!("&category={}", encode_query(category)));
        }
        let mut http = HttpRequest::get(url).header("User-Agent", env.user_agent.clone());
        if let Some(key) = key {
            http = http.header("Authorization", format!("Bearer {}", key.expose()));
        }
        ensure_active(cancel)?;
        let response = env.http.get(&http)?;
        ensure_active(cancel)?;
        parse_response(&response.json()?, page)
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let repo = item
            .repo_url
            .as_deref()
            .and_then(RepoRef::parse)
            .ok_or_else(|| ProviderError::unsupported("this skill has no GitHub source"))?;
        let link = GithubLink {
            repo,
            path: item.resource_path.clone(),
            is_file: false,
        };
        github_skill_detail(item, &link, env, cancel)
    }
}
