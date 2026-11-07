//! `voltagent-skills`: the curated VoltAgent/awesome-agent-skills README.
//! Search filters the parsed index locally; previews fetch the publisher's
//! `SKILL.md` from GitHub when the entry links to a GitHub location.

use std::collections::HashSet;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteItem,
    SearchPage, SearchRequest, Transport, now_unix,
};

use super::common::{
    GithubLink, IndexCache, ensure_active, github_skill_detail, matches_query, paginate,
    provider_id,
};
use super::curated::{self, IndexEntry};

pub const ID: &str = "voltagent-skills";

pub struct VoltAgentSkills {
    descriptor: ProviderDescriptor,
    index: IndexCache<Vec<IndexEntry>>,
}

impl VoltAgentSkills {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: provider_id(ID),
                display_name: "VoltAgent Awesome Agent Skills".into(),
                homepage: "https://github.com/VoltAgent/awesome-agent-skills".into(),
                docs_url: Some("https://github.com/VoltAgent/awesome-agent-skills#readme".into()),
                kinds: vec![ResourceKind::Skill],
                transport: Transport::GithubIndex,
                auth: AuthStrategy::None,
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: true,
                    fetch_files: true,
                    download: false,
                    install: false,
                    pagination: Pagination::Page,
                    categories: true,
                    rate_limit_policy: Some(
                        "One README fetch from raw.githubusercontent.com, cached for 10 minutes"
                            .into(),
                    ),
                    requires_attribution: Some(
                        "Index by VoltAgent, MIT licence (github.com/VoltAgent/awesome-agent-skills)"
                            .into(),
                    ),
                },
                notes: vec![
                    "Curated list, not audited: entries link to publisher repositories or officialskills.sh pages.".into(),
                    "Only entries linking to a concrete SKILL.md advertise View source before preview; repository-level entries may be collections.".into(),
                    "Category filter matches the README section heading.".into(),
                ],
            },
            index: IndexCache::default(),
        }
    }
}

impl Default for VoltAgentSkills {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn to_items(entries: &[IndexEntry], fetched_at_unix: u64) -> Vec<RemoteItem> {
    let mut seen_ids = HashSet::new();
    let mut seen_pairs = HashSet::new();
    let mut items = Vec::new();
    for entry in entries {
        if !seen_pairs.insert((entry.name.as_str(), entry.url.as_str())) {
            continue;
        }
        let upstream_id = if seen_ids.insert(entry.name.clone()) {
            entry.name.clone()
        } else {
            format!("{} {}", entry.name, entry.url)
        };
        let link = GithubLink::parse(&entry.url);
        items.push(RemoteItem {
            source: provider_id(ID),
            upstream_id,
            kind: ResourceKind::Skill,
            title: entry.name.clone(),
            summary: entry.description.clone(),
            canonical_url: Some(entry.url.clone()),
            repo_url: link.as_ref().map(|link| link.repo.html_url()),
            resource_path: link.as_ref().and_then(GithubLink::resource_path),
            package_ref: None,
            version: None,
            author: entry.publisher().map(str::to_owned),
            categories: entry.section.iter().cloned().collect(),
            actions: RemoteActions {
                view_source: link.as_ref().is_some_and(GithubLink::is_skill_file),
                fetch_files: false,
                install: false,
            },
            fetched_at_unix,
        });
    }
    items
}

impl MarketplaceProvider for VoltAgentSkills {
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
        if !request.wants(ResourceKind::Skill) {
            return Ok(SearchPage::empty());
        }
        ensure_active(cancel)?;
        let entries = self
            .index
            .get_or_load(|| curated::fetch_index(env, cancel))?;
        let category = request.category.as_deref().map(str::to_lowercase);
        let matching: Vec<IndexEntry> = entries
            .iter()
            .filter(|entry| {
                category.as_deref().is_none_or(|category| {
                    entry
                        .section
                        .as_deref()
                        .is_some_and(|section| section.to_lowercase() == category)
                })
            })
            .filter(|entry| {
                matches_query(
                    &request.query,
                    &[
                        &entry.name,
                        &entry.description,
                        entry.section.as_deref().unwrap_or_default(),
                    ],
                )
            })
            .cloned()
            .collect();
        paginate(to_items(&matching, now_unix()), request)
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let link = item
            .canonical_url
            .as_deref()
            .and_then(GithubLink::parse)
            .ok_or_else(|| {
                ProviderError::unsupported("this entry does not link to a GitHub source")
            })?;
        github_skill_detail(item, &link, env, cancel)
    }
}
