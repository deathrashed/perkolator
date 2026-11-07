//! `officialskills`: the officialskills.sh curated directory. Its only public
//! structured data is the robots-advertised `sitemap.xml` (the `/api/` paths
//! are disallowed and undocumented). Skill pages are
//! `/{owner}/skills/{slug}`; titles and descriptions come from the VoltAgent
//! awesome-agent-skills README the site is maintained from, when it links
//! the same page. Listings are metadata-only.

use std::collections::HashMap;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, AuthStrategy, HttpRequest, MarketplaceProvider, Pagination,
    ProviderCapabilities, ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions,
    RemoteItem, SearchPage, SearchRequest, Transport, now_unix,
};

use super::common::{IndexCache, ensure_active, matches_query, paginate, provider_id};
use super::curated::{self, IndexEntry};

pub const ID: &str = "officialskills";
pub const HOMEPAGE: &str = "https://officialskills.sh";
pub const SITEMAP_URL: &str = "https://officialskills.sh/sitemap.xml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryEntry {
    pub owner: String,
    pub slug: String,
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub section: Option<String>,
}

pub struct OfficialSkills {
    descriptor: ProviderDescriptor,
    index: IndexCache<Vec<DirectoryEntry>>,
}

impl OfficialSkills {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: provider_id(ID),
                display_name: "OfficialSkills".into(),
                homepage: HOMEPAGE.into(),
                docs_url: Some(format!("{HOMEPAGE}/about")),
                kinds: vec![ResourceKind::Skill],
                transport: Transport::CuratedDirectory,
                auth: AuthStrategy::None,
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: false,
                    fetch_files: false,
                    download: false,
                    install: false,
                    pagination: Pagination::Page,
                    categories: true,
                    rate_limit_policy: Some(
                        "Two cached fetches (sitemap.xml + VoltAgent README) per 10 minutes".into(),
                    ),
                    requires_attribution: None,
                },
                notes: vec![
                    "No public API: listings come from the site's sitemap.xml; /api/ is disallowed by robots.txt and not used.".into(),
                    "Descriptions and sections come from VoltAgent/awesome-agent-skills, which maintains the site; unmatched pages show only owner and slug.".into(),
                    "Metadata-only: open the directory page for the source repository.".into(),
                ],
            },
            index: IndexCache::default(),
        }
    }
}

impl Default for OfficialSkills {
    fn default() -> Self {
        Self::new()
    }
}

/// Skill page URLs (`/{owner}/skills/{slug}`) listed in the sitemap.
pub(crate) fn parse_sitemap(xml: &str) -> Vec<(String, String, String)> {
    let mut pages = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<loc>") {
        rest = &rest[start + "<loc>".len()..];
        let Some(end) = rest.find("</loc>") else {
            break;
        };
        let url = rest[..end].trim().replace("&amp;", "&");
        rest = &rest[end..];
        let Some(path) = url.strip_prefix("https://officialskills.sh/") else {
            continue;
        };
        let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        let valid = |part: &str| {
            !part.is_empty()
                && part != ".."
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        };
        if let [owner, "skills", slug] = parts.as_slice()
            && valid(owner)
            && valid(slug)
        {
            pages.push(((*owner).to_owned(), (*slug).to_owned(), url.clone()));
        }
    }
    pages
}

pub(crate) fn build_directory(
    pages: Vec<(String, String, String)>,
    descriptions: &[IndexEntry],
) -> Vec<DirectoryEntry> {
    let by_url: HashMap<&str, &IndexEntry> = descriptions
        .iter()
        .map(|entry| (entry.url.trim_end_matches('/'), entry))
        .collect();
    pages
        .into_iter()
        .map(|(owner, slug, url)| {
            let described = by_url.get(url.trim_end_matches('/')).copied();
            DirectoryEntry {
                title: described.map(|entry| entry.name.clone()),
                description: described
                    .map(|entry| entry.description.clone())
                    .filter(|text| !text.is_empty()),
                section: described.and_then(|entry| entry.section.clone()),
                owner,
                slug,
                url,
            }
        })
        .collect()
}

fn load(env: &ProviderEnv, cancel: &CancelToken) -> Result<Vec<DirectoryEntry>, ProviderError> {
    ensure_active(cancel)?;
    let xml = env
        .http
        .get(&HttpRequest::get(SITEMAP_URL).header("User-Agent", env.user_agent.clone()))?
        .text()?;
    let pages = parse_sitemap(&xml);
    if pages.is_empty() {
        return Err(ProviderError::malformed(
            "officialskills.sh sitemap lists no skill pages",
        ));
    }
    let descriptions = match curated::fetch_index(env, cancel) {
        Ok(entries) => entries,
        Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
        // Descriptions are an enrichment; the sitemap alone is still truthful.
        Err(_) => Vec::new(),
    };
    Ok(build_directory(pages, &descriptions))
}

fn to_item(entry: &DirectoryEntry, fetched_at_unix: u64) -> RemoteItem {
    RemoteItem {
        source: provider_id(ID),
        upstream_id: format!("{}/{}", entry.owner, entry.slug),
        kind: ResourceKind::Skill,
        title: entry
            .title
            .clone()
            .unwrap_or_else(|| format!("{}/{}", entry.owner, entry.slug)),
        summary: entry.description.clone().unwrap_or_default(),
        canonical_url: Some(entry.url.clone()),
        repo_url: None,
        resource_path: None,
        package_ref: None,
        version: None,
        author: Some(entry.owner.clone()),
        categories: entry.section.iter().cloned().collect(),
        actions: RemoteActions::default(),
        fetched_at_unix,
    }
}

impl MarketplaceProvider for OfficialSkills {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, _env: &ProviderEnv) -> AuthStatus {
        AuthStatus::NotRequired
    }

    fn connect_intent(&self) -> AuthIntent {
        AuthIntent::NoAuthRequired
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
        let directory = self.index.get_or_load(|| load(env, cancel))?;
        let category = request.category.as_deref().map(str::to_lowercase);
        let fetched_at_unix = now_unix();
        let items = directory
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
                        &entry.owner,
                        &entry.slug,
                        entry.title.as_deref().unwrap_or_default(),
                        entry.description.as_deref().unwrap_or_default(),
                        entry.section.as_deref().unwrap_or_default(),
                    ],
                )
            })
            .map(|entry| to_item(entry, fetched_at_unix))
            .collect();
        paginate(items, request)
    }
}
