//! Shared read-only GitHub access for GitHub-index sources: repository
//! trees, raw files and code search. Anonymous by default; the optional
//! `github` credential slot raises rate limits.

use serde_json::Value;

use crate::{HttpRequest, ProviderEnv, ProviderError};

pub const API: &str = "https://api.github.com";
pub const RAW: &str = "https://raw.githubusercontent.com";
pub const CREDENTIAL_SLOT: &str = "github";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub repo: String,
    /// Branch, tag or commit; `HEAD` resolves the default branch.
    pub reference: String,
}

impl RepoRef {
    pub fn new(owner: &str, repo: &str) -> Self {
        Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            reference: "HEAD".to_owned(),
        }
    }

    /// Parse `owner/repo` or a `https://github.com/owner/repo[...]` URL.
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value
            .trim()
            .trim_start_matches("https://github.com/")
            .trim_end_matches(".git");
        let mut parts = trimmed.split('/').filter(|part| !part.is_empty());
        let owner = parts.next()?;
        let repo = parts.next()?;
        let valid = |part: &str| {
            part.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
                && part != ".."
        };
        (valid(owner) && valid(repo)).then(|| Self::new(owner, repo))
    }

    pub fn html_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repo)
    }

    pub fn blob_url(&self, path: &str) -> String {
        format!("{}/blob/{}/{}", self.html_url(), self.reference, path)
    }

    pub fn tree_url(&self, path: &str) -> String {
        format!("{}/tree/{}/{}", self.html_url(), self.reference, path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

fn request(env: &ProviderEnv, url: String) -> HttpRequest {
    let mut request = HttpRequest::get(url)
        .header("User-Agent", env.user_agent.clone())
        .header("Accept", "application/vnd.github+json");
    if let Some(token) = env.credentials.get(CREDENTIAL_SLOT) {
        request = request.header("Authorization", format!("Bearer {}", token.expose()));
    }
    request
}

/// Recursive tree listing (one request). `truncated` repos return a partial tree.
pub fn tree(env: &ProviderEnv, repo: &RepoRef) -> Result<(Vec<TreeEntry>, bool), ProviderError> {
    let url = format!(
        "{API}/repos/{}/{}/git/trees/{}?recursive=1",
        repo.owner, repo.repo, repo.reference
    );
    let json = env.http.get(&request(env, url))?.json()?;
    let truncated = json["truncated"].as_bool().unwrap_or(false);
    let entries = json["tree"]
        .as_array()
        .ok_or_else(|| ProviderError::malformed("tree response without `tree`"))?
        .iter()
        .filter_map(|entry| {
            Some(TreeEntry {
                path: entry["path"].as_str()?.to_owned(),
                is_dir: entry["type"].as_str()? == "tree",
                size: entry["size"].as_u64(),
            })
        })
        .collect();
    Ok((entries, truncated))
}

/// Raw file text from raw.githubusercontent.com (not rate limited like the API).
pub fn raw_file(env: &ProviderEnv, repo: &RepoRef, path: &str) -> Result<String, ProviderError> {
    if path.split('/').any(|part| part == "..") {
        return Err(ProviderError::unsupported("path traversal"));
    }
    let url = format!(
        "{RAW}/{}/{}/{}/{}",
        repo.owner, repo.repo, repo.reference, path
    );
    env.http.get(&request(env, url))?.text()
}

/// GitHub code search (requires a token for the code endpoint).
pub fn search_code(
    env: &ProviderEnv,
    query: &str,
    per_page: usize,
) -> Result<Value, ProviderError> {
    let url = format!(
        "{API}/search/code?q={}&per_page={}",
        encode_query(query),
        per_page.clamp(1, 100)
    );
    env.http.get(&request(env, url))?.json()
}

/// Repository search (works anonymously, low quota).
pub fn search_repositories(
    env: &ProviderEnv,
    query: &str,
    per_page: usize,
) -> Result<Value, ProviderError> {
    let url = format!(
        "{API}/search/repositories?q={}&per_page={}",
        encode_query(query),
        per_page.clamp(1, 100)
    );
    env.http.get(&request(env, url))?.json()
}

/// Minimal application/x-www-form-urlencoded encoding for query strings.
pub fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_refs() {
        assert_eq!(RepoRef::parse("cursor/plugins").unwrap().repo, "plugins");
        assert_eq!(
            RepoRef::parse("https://github.com/wshobson/agents.git")
                .unwrap()
                .owner,
            "wshobson"
        );
        assert!(RepoRef::parse("only-owner").is_none());
        assert!(RepoRef::parse("a/b c").is_none());
    }

    #[test]
    fn encodes_queries() {
        assert_eq!(encode_query("pdf skill&x=1"), "pdf+skill%26x%3D1");
    }
}
