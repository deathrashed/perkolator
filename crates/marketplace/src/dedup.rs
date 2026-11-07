//! Provenance-preserving merge of remote results. Items are merged only by
//! verified location (repository + resource path, or canonical URL) and
//! kind — never by title — and every original source record is kept.

use std::collections::HashMap;

use perkolator_core::ResourceKind;

use crate::RemoteItem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergedItem {
    /// First item seen for this resource (stable display choice).
    pub primary: RemoteItem,
    /// Other sources/records describing the same resource.
    pub alternates: Vec<RemoteItem>,
}

impl MergedItem {
    pub fn sources(&self) -> impl Iterator<Item = &RemoteItem> {
        std::iter::once(&self.primary).chain(self.alternates.iter())
    }
}

/// Identity used for merging; `None` means "never merge".
pub fn merge_key(item: &RemoteItem) -> Option<(ResourceKind, String)> {
    if let Some(repo) = item.repo_url.as_deref().and_then(normalize_repo) {
        let path = item
            .resource_path
            .as_deref()
            .map(|path| path.trim_matches('/').to_ascii_lowercase())
            .unwrap_or_default();
        return Some((item.kind, format!("{repo}#{path}")));
    }
    item.canonical_url
        .as_deref()
        .map(|url| (item.kind, normalize_url(url)))
}

fn normalize_repo(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .strip_prefix("https://github.com/")
        .or_else(|| url.trim().strip_prefix("http://github.com/"))?;
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let owner = parts.next()?.to_ascii_lowercase();
    let repo = parts.next()?.trim_end_matches(".git").to_ascii_lowercase();
    Some(format!("github.com/{owner}/{repo}"))
}

fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let without_fragment = without_scheme.split('#').next().unwrap_or(without_scheme);
    without_fragment.trim_end_matches('/').to_ascii_lowercase()
}

/// Merge in arrival order. Items without a verifiable location stay separate.
pub fn merge(items: impl IntoIterator<Item = RemoteItem>) -> Vec<MergedItem> {
    let mut merged: Vec<MergedItem> = Vec::new();
    let mut index: HashMap<(ResourceKind, String), usize> = HashMap::new();
    for item in items {
        match merge_key(&item) {
            Some(key) => match index.get(&key) {
                Some(&ix) => {
                    let entry = &mut merged[ix];
                    let duplicate = entry
                        .sources()
                        .any(|seen| seen.source == item.source && seen.upstream_id == item.upstream_id);
                    if !duplicate {
                        entry.alternates.push(item);
                    }
                }
                None => {
                    index.insert(key, merged.len());
                    merged.push(MergedItem {
                        primary: item,
                        alternates: Vec::new(),
                    });
                }
            },
            None => merged.push(MergedItem {
                primary: item,
                alternates: Vec::new(),
            }),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use perkolator_core::ProviderId;

    use super::*;
    use crate::RemoteActions;

    fn item(source: &str, id: &str, title: &str, repo: Option<&str>, path: Option<&str>) -> RemoteItem {
        RemoteItem {
            source: ProviderId::new(source).unwrap(),
            upstream_id: id.into(),
            kind: ResourceKind::Skill,
            title: title.into(),
            summary: String::new(),
            canonical_url: None,
            repo_url: repo.map(Into::into),
            resource_path: path.map(Into::into),
            package_ref: None,
            version: None,
            author: None,
            categories: vec![],
            actions: RemoteActions::default(),
            fetched_at_unix: 0,
        }
    }

    #[test]
    fn same_repo_path_merges_and_keeps_both_sources() {
        let merged = merge([
            item("voltagent-skills", "a", "PDF", Some("https://github.com/Anthropics/skills"), Some("pdf/")),
            item("github-direct", "b", "pdf tools", Some("https://github.com/anthropics/skills.git"), Some("/pdf")),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].alternates.len(), 1);
        assert_eq!(merged[0].sources().count(), 2);
    }

    #[test]
    fn same_title_different_location_stays_separate() {
        let merged = merge([
            item("skillsmp", "a", "pdf", Some("https://github.com/x/skills"), Some("pdf")),
            item("skillsmp", "b", "pdf", Some("https://github.com/y/skills"), Some("pdf")),
            item("officialskills", "c", "pdf", None, None),
        ]);
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn exact_duplicate_records_are_not_repeated() {
        let one = item("skillsmp", "a", "pdf", Some("https://github.com/x/s"), Some("pdf"));
        let merged = merge([one.clone(), one]);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].alternates.is_empty());
    }
}
