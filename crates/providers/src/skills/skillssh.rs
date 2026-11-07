//! `skillssh`: the skills.sh REST API (https://skills.sh/docs/api). Every
//! endpoint requires a Vercel OIDC token (`Authorization: Bearer`); anonymous
//! requests return 401. The token is project-scoped and short-lived (~12 h),
//! so it is read from the `skillssh` credential slot and never minted here.
//! `npx skills` is not used: it downloads and runs remote code.

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::github::{RepoRef, encode_query};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, HttpRequest, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteFile,
    RemoteItem, SearchPage, SearchRequest, Transport, now_unix,
};
use serde_json::Value;

use super::common::{ensure_active, provider_id};

pub const ID: &str = "skillssh";
pub const CREDENTIAL_SLOT: &str = "skillssh";
pub const API: &str = "https://skills.sh/api/v1/skills";
pub const AUTH_DOCS_URL: &str = "https://skills.sh/docs/api#authentication";
/// The search endpoint requires at least this many characters.
const MIN_QUERY_CHARS: usize = 2;
const MAX_SEARCH_LIMIT: usize = 200;
const MAX_LIST_PAGE_SIZE: usize = 500;

pub struct SkillsSh {
    descriptor: ProviderDescriptor,
}

impl SkillsSh {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: provider_id(ID),
                display_name: "skills.sh".into(),
                homepage: "https://skills.sh".into(),
                docs_url: Some("https://skills.sh/docs/api".into()),
                kinds: vec![ResourceKind::Skill],
                transport: Transport::Rest,
                auth: AuthStrategy::ApiKey {
                    slot: CREDENTIAL_SLOT.into(),
                    docs_url: AUTH_DOCS_URL.into(),
                },
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: true,
                    fetch_files: true,
                    download: false,
                    install: false,
                    pagination: Pagination::Page,
                    categories: false,
                    rate_limit_policy: Some(
                        "600 requests/min per Vercel team and project".into(),
                    ),
                    requires_attribution: None,
                },
                notes: vec![
                    "Requires a Vercel OIDC token (project-scoped, ~12 h lifetime, e.g. from `vercel env pull`); there is no anonymous or personal API key access.".into(),
                    "Search returns names and install sources only (no descriptions). Empty or 1-character queries list the all-time leaderboard.".into(),
                    "Entries flagged as duplicates by skills.sh are hidden.".into(),
                ],
            },
        }
    }
}

impl Default for SkillsSh {
    fn default() -> Self {
        Self::new()
    }
}

fn authorised(env: &ProviderEnv, url: String) -> Result<HttpRequest, ProviderError> {
    let token = env
        .credentials
        .get(CREDENTIAL_SLOT)
        .ok_or(ProviderError::NeedsAuth)?;
    Ok(HttpRequest::get(url)
        .header("User-Agent", env.user_agent.clone())
        .header("Authorization", format!("Bearer {}", token.expose())))
}

/// `owner/repo/slug` or `domain/slug`, each segment URL-safe.
fn valid_skill_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('/').collect();
    (2..=3).contains(&parts.len())
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != ".."
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        })
}

fn to_item(skill: &Value, fetched_at_unix: u64) -> Option<RemoteItem> {
    if skill["isDuplicate"].as_bool() == Some(true) {
        return None;
    }
    let upstream_id = skill["id"].as_str()?.to_owned();
    let title = skill["name"]
        .as_str()
        .or_else(|| skill["slug"].as_str())?
        .to_owned();
    let source = skill["source"].as_str().unwrap_or_default();
    let github = (skill["sourceType"].as_str() == Some("github"))
        .then(|| RepoRef::parse(source))
        .flatten();
    Some(RemoteItem {
        source: provider_id(ID),
        kind: ResourceKind::Skill,
        title,
        summary: String::new(),
        canonical_url: skill["url"].as_str().map(str::to_owned),
        repo_url: github.as_ref().map(RepoRef::html_url),
        resource_path: None,
        package_ref: None,
        version: None,
        author: github.as_ref().map(|repo| repo.owner.clone()),
        categories: Vec::new(),
        actions: RemoteActions {
            view_source: false,
            fetch_files: valid_skill_id(&upstream_id),
            install: false,
        },
        upstream_id,
        fetched_at_unix,
    })
}

fn items(json: &Value) -> Result<Vec<RemoteItem>, ProviderError> {
    let data = json["data"]
        .as_array()
        .ok_or_else(|| ProviderError::malformed("skills.sh response without `data`"))?;
    let fetched_at_unix = now_unix();
    Ok(data
        .iter()
        .filter_map(|skill| to_item(skill, fetched_at_unix))
        .collect())
}

pub(crate) fn parse_search(json: &Value) -> Result<SearchPage, ProviderError> {
    Ok(SearchPage {
        items: items(json)?,
        next_cursor: None,
        total: None,
    })
}

pub(crate) fn parse_leaderboard(json: &Value) -> Result<SearchPage, ProviderError> {
    let pagination = &json["pagination"];
    let next_cursor = (pagination["hasMore"].as_bool() == Some(true))
        .then(|| {
            pagination["page"]
                .as_u64()
                .map(|page| (page + 1).to_string())
        })
        .flatten();
    Ok(SearchPage {
        items: items(json)?,
        next_cursor,
        total: pagination["total"].as_u64(),
    })
}

impl MarketplaceProvider for SkillsSh {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus {
        if env.credentials.get(CREDENTIAL_SLOT).is_some() {
            AuthStatus::Ready { identity: None }
        } else {
            AuthStatus::NotConfigured
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
        let leaderboard = query.chars().count() < MIN_QUERY_CHARS;
        let url = if leaderboard {
            let page = match request.cursor.as_deref() {
                None => 0,
                Some(cursor) => cursor
                    .parse::<u64>()
                    .map_err(|_| ProviderError::malformed(format!("invalid cursor `{cursor}`")))?,
            };
            format!(
                "{API}?page={page}&per_page={}",
                request.page_size.clamp(1, MAX_LIST_PAGE_SIZE)
            )
        } else {
            if request.cursor.is_some() {
                // Search results are a single ranked list without pages.
                return Ok(SearchPage::empty());
            }
            format!(
                "{API}/search?q={}&limit={}",
                encode_query(query),
                request.page_size.clamp(1, MAX_SEARCH_LIMIT)
            )
        };
        let http = authorised(env, url)?;
        ensure_active(cancel)?;
        let response = env.http.get(&http)?;
        ensure_active(cancel)?;
        let json = response.json()?;
        if leaderboard {
            parse_leaderboard(&json)
        } else {
            parse_search(&json)
        }
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        if !valid_skill_id(&item.upstream_id) {
            return Err(ProviderError::unsupported("unrecognised skills.sh id"));
        }
        let http = authorised(env, format!("{API}/{}", item.upstream_id))?;
        ensure_active(cancel)?;
        let json = env.http.get(&http)?.json()?;
        ensure_active(cancel)?;
        let files = json["files"].as_array();
        let readme_markdown = files
            .and_then(|files| {
                files
                    .iter()
                    .find(|file| file["path"].as_str() == Some("SKILL.md"))
            })
            .and_then(|file| file["contents"].as_str())
            .map(str::to_owned);
        let files = files
            .into_iter()
            .flatten()
            .filter_map(|file| {
                Some(RemoteFile {
                    path: file["path"].as_str()?.to_owned(),
                    size: file["contents"].as_str().map(|text| text.len() as u64),
                    url: None,
                })
            })
            .collect();
        Ok(RemoteDetail {
            item: item.clone(),
            readme_markdown,
            manifest_json: None,
            files,
            tools: Vec::new(),
        })
    }
}
