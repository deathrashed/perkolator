//! Helpers shared by the skill catalogue adapters: GitHub link parsing,
//! local filtering/pagination over cached indexes and SKILL.md previews.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use perkolator_core::{CancelToken, ProviderId};
use perkolator_marketplace::github::{self, RepoRef};
use perkolator_marketplace::{
    ProviderEnv, ProviderError, RemoteDetail, RemoteFile, RemoteItem, SearchPage, SearchRequest,
};

/// How long a fetched curated index is reused before it is fetched again.
pub(crate) const INDEX_TTL: Duration = Duration::from_secs(10 * 60);

pub(crate) fn provider_id(id: &'static str) -> ProviderId {
    ProviderId::new(id).expect("built-in provider ids are valid")
}

pub(crate) fn ensure_active(cancel: &CancelToken) -> Result<(), ProviderError> {
    if cancel.is_cancelled() {
        Err(ProviderError::Cancelled)
    } else {
        Ok(())
    }
}

/// A `https://github.com/owner/repo[/tree|blob/<ref>/<path>]` link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GithubLink {
    pub repo: RepoRef,
    /// Path inside the repository, if the link points below the root.
    pub path: Option<String>,
    /// `blob` links point at a file rather than a directory.
    pub is_file: bool,
}

impl GithubLink {
    pub fn parse(url: &str) -> Option<Self> {
        let rest = url.trim().strip_prefix("https://github.com/")?;
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let mut parts = rest.split('/').filter(|part| !part.is_empty());
        let owner = parts.next()?;
        let repo = parts.next()?;
        let mut repo = RepoRef::parse(&format!("{owner}/{repo}"))?;
        let (path, is_file) = match parts.next() {
            None => (None, false),
            Some(kind @ ("tree" | "blob")) => {
                repo.reference = parts.next()?.to_owned();
                let path: Vec<&str> = parts.collect();
                if path.contains(&"..") {
                    return None;
                }
                let path = (!path.is_empty()).then(|| path.join("/"));
                (path, kind == "blob")
            }
            Some(_) => return None,
        };
        Some(Self {
            repo,
            path,
            is_file,
        })
    }

    /// A link that names a concrete `SKILL.md` file.
    pub fn is_skill_file(&self) -> bool {
        self.is_file
            && self
                .path
                .as_deref()
                .is_some_and(|path| path == "SKILL.md" || path.ends_with("/SKILL.md"))
    }

    /// The skill directory: the parent of a `SKILL.md` link, else the path.
    pub fn resource_path(&self) -> Option<String> {
        let path = self.path.as_deref()?;
        if self.is_skill_file() {
            return path
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_owned())
                .filter(|parent| !parent.is_empty());
        }
        Some(path.to_owned())
    }
}

/// Case-insensitive "every query word appears somewhere" match.
pub(crate) fn matches_query(query: &str, haystack: &[&str]) -> bool {
    let haystack = haystack.join("\n").to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

/// Offset pagination over an already filtered local list. The cursor is the
/// decimal offset of the next item.
pub(crate) fn paginate(
    items: Vec<RemoteItem>,
    request: &SearchRequest,
) -> Result<SearchPage, ProviderError> {
    let offset = match request.cursor.as_deref() {
        None => 0,
        Some(cursor) => cursor
            .parse::<usize>()
            .map_err(|_| ProviderError::malformed(format!("invalid cursor `{cursor}`")))?,
    };
    let page_size = request.page_size.max(1);
    let total = items.len();
    let end = offset.saturating_add(page_size).min(total);
    let page: Vec<RemoteItem> = items
        .into_iter()
        .skip(offset)
        .take(end.saturating_sub(offset))
        .collect();
    Ok(SearchPage {
        items: page,
        next_cursor: (end < total).then(|| end.to_string()),
        total: Some(total as u64),
    })
}

/// Reuses an expensive-to-fetch index for [`INDEX_TTL`].
pub(crate) struct IndexCache<T> {
    slot: Mutex<Option<(Instant, Arc<T>)>>,
}

impl<T> Default for IndexCache<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }
}

impl<T> IndexCache<T> {
    pub fn get_or_load(
        &self,
        load: impl FnOnce() -> Result<T, ProviderError>,
    ) -> Result<Arc<T>, ProviderError> {
        if let Some((loaded_at, value)) = self.slot.lock().expect("index cache lock").as_ref()
            && loaded_at.elapsed() < INDEX_TTL
        {
            return Ok(value.clone());
        }
        let value = Arc::new(load()?);
        *self.slot.lock().expect("index cache lock") = Some((Instant::now(), value.clone()));
        Ok(value)
    }
}

/// Fetch a skill's `SKILL.md` (falling back to `README.md` for directory and
/// repository links) from GitHub for an inert preview.
pub(crate) fn github_skill_detail(
    item: &RemoteItem,
    link: &GithubLink,
    env: &ProviderEnv,
    cancel: &CancelToken,
) -> Result<RemoteDetail, ProviderError> {
    ensure_active(cancel)?;
    let directory = link.resource_path();
    let join = |file: &str| match &directory {
        Some(dir) => format!("{dir}/{file}"),
        None => file.to_owned(),
    };
    let skill_md = if link.is_skill_file() {
        link.path.clone().unwrap_or_else(|| "SKILL.md".to_owned())
    } else {
        join("SKILL.md")
    };
    let (path, markdown, is_skill) = match github::raw_file(env, &link.repo, &skill_md) {
        Ok(text) => (skill_md, text, true),
        Err(ProviderError::Http { status: 404 }) if !link.is_skill_file() => {
            ensure_active(cancel)?;
            let readme = join("README.md");
            let text = github::raw_file(env, &link.repo, &readme)?;
            (readme, text, false)
        }
        Err(error) => return Err(error),
    };
    ensure_active(cancel)?;
    let mut item = item.clone();
    item.actions.view_source = true;
    item.actions.fetch_files = is_skill;
    Ok(RemoteDetail {
        item,
        readme_markdown: Some(markdown.clone()),
        manifest_json: None,
        files: vec![RemoteFile {
            url: Some(link.repo.blob_url(&path)),
            path,
            size: Some(markdown.len() as u64),
        }],
        tools: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_link_forms() {
        let root = GithubLink::parse("https://github.com/qdrant/skills").unwrap();
        assert_eq!(root.repo.reference, "HEAD");
        assert_eq!(root.path, None);

        let tree =
            GithubLink::parse("https://github.com/LambdaTest/agent-skills/tree/main/api-skill")
                .unwrap();
        assert_eq!(tree.repo.reference, "main");
        assert_eq!(tree.resource_path().as_deref(), Some("api-skill"));
        assert!(!tree.is_skill_file());

        let blob = GithubLink::parse(
            "https://github.com/obra/superpowers/blob/main/skills/brainstorming/SKILL.md",
        )
        .unwrap();
        assert!(blob.is_skill_file());
        assert_eq!(
            blob.resource_path().as_deref(),
            Some("skills/brainstorming")
        );

        assert!(GithubLink::parse("https://github.com/a/b/tree/main/../x").is_none());
        assert!(GithubLink::parse("https://github.com/a/b/issues/1").is_none());
        assert!(GithubLink::parse("https://officialskills.sh/a/skills/b").is_none());
    }

    #[test]
    fn query_matches_all_words_case_insensitively() {
        assert!(matches_query("PDF forms", &["pdf", "Handle forms"]));
        assert!(!matches_query("pdf excel", &["pdf"]));
        assert!(matches_query("", &["anything"]));
    }
}
