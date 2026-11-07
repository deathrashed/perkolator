//! Generic GitHub resolver for all six kinds.
//!
//! Two modes, chosen from the query text:
//! - A repository location (`owner/repo[/path]` or a `github.com` URL,
//!   optionally `/tree/<ref>/<path>`) is resolved with one tree request plus
//!   raw reads of each recognised manifest on the current page.
//! - Anything else is a keyword search over repositories carrying the GitHub
//!   topic of each wanted kind (topics cannot be OR-ed in one query). Each
//!   page spends at most two search requests anonymously (GitHub allows ten
//!   per minute) or six with a token; the cursor walks the remaining kinds.
//!   Such hits are repositories whose kind comes from their topic only.
//!
//! Recognised formats: `SKILL.md` directories (skill),
//! `.claude-plugin/plugin.json` / `.cursor-plugin/plugin.json` (plugin),
//! `server.json` MCP registry manifests (mcp), `AGENTS.md` and `.mdc` files
//! (rule), markdown with frontmatter under `agents/` (agent) and markdown
//! under `prompts/` or `commands/` (prompt).

use perkolator_core::{CancelToken, ProviderId, ResourceKind};
use perkolator_marketplace::github::{self, RepoRef, TreeEntry};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteFile,
    RemoteItem, SearchPage, SearchRequest, Transport, now_unix,
};
use serde::Deserialize;
use serde_json::Value;

pub const ID: &str = "github-direct";

/// Kind → repository topic used by keyword search, in search order.
const KIND_TOPICS: [(ResourceKind, &str); 6] = [
    (ResourceKind::Skill, "agent-skills"),
    (ResourceKind::McpServer, "mcp-server"),
    (ResourceKind::Plugin, "claude-code-plugin"),
    (ResourceKind::Agent, "claude-code-subagents"),
    (ResourceKind::Rule, "cursor-rules"),
    (ResourceKind::Prompt, "prompts"),
];
const ANONYMOUS_SEARCHES_PER_PAGE: usize = 2;
const TOKEN_SEARCHES_PER_PAGE: usize = 6;
/// Each resolved item costs one raw read, so resolution pages stay small.
const MAX_RESOLVE_PAGE: usize = 20;
const MAX_SEARCH_PER_PAGE: usize = 100;
const MAX_DETAIL_FILES: usize = 500;
const SKIPPED_DIRS: [&str; 2] = ["node_modules", ".git"];
const RESOLVE_CURSOR: char = 'r';
const TOPIC_CURSOR: char = 't';

pub struct GithubDirect {
    descriptor: ProviderDescriptor,
}

impl GithubDirect {
    pub fn new() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: ProviderId::new(ID).expect("valid provider id"),
                display_name: "GitHub".into(),
                homepage: "https://github.com".into(),
                docs_url: Some("https://docs.github.com/en/rest/search/search".into()),
                kinds: KIND_TOPICS.iter().map(|(kind, _)| *kind).collect(),
                transport: Transport::Github,
                auth: AuthStrategy::OptionalToken {
                    slot: github::CREDENTIAL_SLOT.into(),
                },
                capabilities: ProviderCapabilities {
                    search: true,
                    detail: true,
                    fetch_files: true,
                    download: false,
                    install: false,
                    pagination: Pagination::Cursor,
                    categories: false,
                    rate_limit_policy: Some(
                        "GitHub REST quotas per IP: anonymous search 10/min and core 60/h; a \
                         token raises them. Keyword search uses at most 2 search requests per \
                         page anonymously (6 with a token); resolving a repository uses 1 tree \
                         request plus raw file reads."
                            .into(),
                    ),
                    requires_attribution: None,
                },
                notes: vec![
                    "Type owner/repo[/path] or a github.com URL to resolve concrete resources."
                        .into(),
                    "Keyword hits are repositories tagged with a kind's topic; their contents are \
                     verified only when the repository is resolved."
                        .into(),
                    "Licences vary per repository; check the repository before reuse.".into(),
                    "Large repositories may return a truncated tree; resolution then lists what \
                     GitHub returned."
                        .into(),
                ],
            },
        }
    }
}

impl Default for GithubDirect {
    fn default() -> Self {
        Self::new()
    }
}

/// A repository location typed into the search box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoTarget {
    pub repo: RepoRef,
    /// Path inside the repository; empty for the whole repository.
    pub path: String,
    /// Typed as a `github.com` URL rather than a bare `owner/repo`.
    pub explicit_url: bool,
}

impl RepoTarget {
    /// Parse `owner/repo[/path]`, `github.com/owner/repo[...]` or a full URL,
    /// including `/tree/<ref>/<path>` and `/blob/<ref>/<path>` forms. A ref
    /// containing `/` cannot be told apart from a path, so only its first
    /// segment is taken.
    pub fn parse(query: &str) -> Option<Self> {
        let query = query.trim();
        if query.is_empty() || query.chars().any(char::is_whitespace) {
            return None;
        }
        let (rest, explicit_url) = match query
            .strip_prefix("https://")
            .or_else(|| query.strip_prefix("http://"))
        {
            Some(rest) => (
                rest.strip_prefix("www.")
                    .unwrap_or(rest)
                    .strip_prefix("github.com/")?,
                true,
            ),
            None => match query.strip_prefix("github.com/") {
                Some(rest) => (rest, true),
                None => (query, false),
            },
        };
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let segments: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
        let [owner, repo, tail @ ..] = segments.as_slice() else {
            return None;
        };
        let mut repo = RepoRef::parse(&format!("{owner}/{repo}"))?;
        let mut tail = tail;
        if let [marker, reference, path @ ..] = tail
            && matches!(*marker, "tree" | "blob")
        {
            if !is_safe_ref(reference) {
                return None;
            }
            repo.reference = (*reference).to_owned();
            tail = path;
        }
        if tail.iter().any(|part| matches!(*part, "." | "..")) {
            return None;
        }
        Some(Self {
            repo,
            path: tail.join("/"),
            explicit_url,
        })
    }
}

fn is_safe_ref(value: &str) -> bool {
    !value.is_empty()
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn is_safe_path(path: &str) -> bool {
    path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

fn kind_rank(kind: ResourceKind) -> usize {
    KIND_TOPICS
        .iter()
        .position(|(candidate, _)| *candidate == kind)
        .unwrap_or(KIND_TOPICS.len())
}

fn split_parent(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn file_stem(path: &str) -> &str {
    let name = split_parent(path).1;
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn last_segment(path: &str) -> &str {
    split_parent(path).1
}

fn raw_url(repo: &RepoRef, path: &str) -> String {
    format!(
        "{}/{}/{}/{}/{}",
        github::RAW,
        repo.owner,
        repo.repo,
        repo.reference,
        path
    )
}

/// Stable within this source: `<kind>:<owner>/<repo>:<ref>:<file>`. The file
/// is empty for repository-level keyword hits. Refs and repo names never
/// contain `:`, and the file comes last, so the id parses back unambiguously.
fn item_id(kind: ResourceKind, repo: &RepoRef, file: &str) -> String {
    format!(
        "{}:{}/{}:{}:{}",
        kind.slug(),
        repo.owner,
        repo.repo,
        repo.reference,
        file
    )
}

fn parse_item_id(id: &str) -> Option<(ResourceKind, RepoRef, String)> {
    let mut parts = id.splitn(4, ':');
    let kind = parts.next()?.parse::<ResourceKind>().ok()?;
    let mut repo = RepoRef::parse(parts.next()?)?;
    let reference = parts.next()?;
    let file = parts.next()?;
    if !is_safe_ref(reference) || !(file.is_empty() || is_safe_path(file)) {
        return None;
    }
    repo.reference = reference.to_owned();
    Some((kind, repo, file.to_owned()))
}

fn parse_cursor(cursor: Option<&str>, prefix: char) -> Result<usize, ProviderError> {
    match cursor {
        None => Ok(0),
        Some(value) => value
            .strip_prefix(prefix)
            .and_then(|offset| offset.parse().ok())
            .ok_or_else(|| ProviderError::malformed("invalid cursor for this query")),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    kind: ResourceKind,
    /// The file that identifies the resource.
    file: String,
    /// Resource root: the directory for skills/plugins, else the file.
    root: String,
}

fn detect(entries: &[TreeEntry], scope: &str) -> Vec<Candidate> {
    let in_scope = |path: &str| {
        scope.is_empty()
            || path == scope
            || path
                .strip_prefix(scope)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    let files: Vec<&str> = entries
        .iter()
        .filter(|entry| !entry.is_dir && is_safe_path(&entry.path) && in_scope(&entry.path))
        .filter(|entry| {
            !entry
                .path
                .split('/')
                .any(|part| SKIPPED_DIRS.contains(&part))
        })
        .map(|entry| entry.path.as_str())
        .collect();
    let skill_roots: Vec<&str> = files
        .iter()
        .filter_map(|path| {
            let (parent, name) = split_parent(path);
            (name == "SKILL.md").then_some(parent)
        })
        .collect();
    // Markdown inside a skill directory is that skill's reference material.
    let inside_skill = |path: &str| {
        skill_roots.iter().any(|root| {
            root.is_empty()
                || path
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let mut candidates: Vec<Candidate> = files
        .into_iter()
        .filter_map(|path| {
            let (parent, name) = split_parent(path);
            let parent_dirs: Vec<&str> =
                parent.split('/').filter(|part| !part.is_empty()).collect();
            let markdown = name.ends_with(".md") && !name.eq_ignore_ascii_case("README.md");
            let (kind, root) = if name == "SKILL.md" {
                (ResourceKind::Skill, parent)
            } else if name == "plugin.json"
                && matches!(
                    parent_dirs.last(),
                    Some(&(".claude-plugin" | ".cursor-plugin"))
                )
            {
                (ResourceKind::Plugin, split_parent(parent).0)
            } else if name == "server.json" {
                (ResourceKind::McpServer, path)
            } else if name == "AGENTS.md" || name.ends_with(".mdc") {
                (ResourceKind::Rule, path)
            } else if markdown && !inside_skill(path) && parent_dirs.contains(&"agents") {
                (ResourceKind::Agent, path)
            } else if markdown
                && !inside_skill(path)
                && parent_dirs
                    .iter()
                    .any(|dir| matches!(*dir, "prompts" | "commands"))
            {
                (ResourceKind::Prompt, path)
            } else {
                return None;
            };
            Some(Candidate {
                kind,
                file: path.to_owned(),
                root: root.to_owned(),
            })
        })
        .collect();
    candidates.sort_by(|a, b| {
        kind_rank(a.kind)
            .cmp(&kind_rank(b.kind))
            .then_with(|| a.file.cmp(&b.file))
    });
    candidates
}

#[derive(Default)]
struct FrontMatter {
    name: Option<String>,
    description: Option<String>,
}

fn yaml_str(value: &serde_norway::Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// YAML frontmatter between leading `---` lines; `None` when absent/invalid.
fn front_matter(text: &str) -> Option<FrontMatter> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut yaml = String::new();
    for line in lines {
        if line.trim_end() == "---" {
            let value: serde_norway::Value = serde_norway::from_str(&yaml).ok()?;
            return Some(FrontMatter {
                name: yaml_str(&value, "name"),
                description: yaml_str(&value, "description"),
            });
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    None
}

/// Text of the first `# ` heading, skipping any frontmatter.
fn first_heading(text: &str) -> Option<String> {
    let mut in_front_matter = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "---" && (index == 0 || in_front_matter) {
            in_front_matter = !in_front_matter;
            continue;
        }
        if in_front_matter {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            let heading = heading.trim();
            if !heading.is_empty() {
                return Some(heading.to_owned());
            }
        }
    }
    None
}

fn json_str(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[derive(Default)]
struct Metadata {
    title: Option<String>,
    summary: Option<String>,
    version: Option<String>,
    author: Option<String>,
    package_ref: Option<String>,
}

/// Reads the candidate's identifying file. `None` when the file does not
/// actually hold the recognised format.
fn read_metadata(candidate: &Candidate, text: &str) -> Option<Metadata> {
    let file = candidate.file.as_str();
    match candidate.kind {
        ResourceKind::Skill => {
            let front = front_matter(text).unwrap_or_default();
            Some(Metadata {
                title: front.name,
                summary: front.description,
                ..Metadata::default()
            })
        }
        ResourceKind::Agent => {
            let front = front_matter(text)?;
            if front.name.is_none() && front.description.is_none() {
                return None;
            }
            Some(Metadata {
                title: front.name,
                summary: front.description,
                ..Metadata::default()
            })
        }
        ResourceKind::Prompt | ResourceKind::Rule => {
            let front = front_matter(text).unwrap_or_default();
            Some(Metadata {
                title: front.name,
                summary: front.description.or_else(|| first_heading(text)),
                ..Metadata::default()
            })
        }
        ResourceKind::Plugin => {
            let manifest: Value = serde_json::from_str(text).ok()?;
            let name = json_str(&manifest, "name")?;
            let author =
                json_str(&manifest["author"], "name").or_else(|| json_str(&manifest, "author"));
            Some(Metadata {
                title: json_str(&manifest, "displayName").or(Some(name)),
                summary: json_str(&manifest, "description"),
                version: json_str(&manifest, "version"),
                author,
                package_ref: None,
            })
        }
        ResourceKind::McpServer => {
            let manifest: Value = serde_json::from_str(text).ok()?;
            let name = json_str(&manifest, "name")?;
            let registry_shaped = manifest["packages"].is_array()
                || manifest["remotes"].is_array()
                || manifest["$schema"]
                    .as_str()
                    .is_some_and(|schema| schema.contains("modelcontextprotocol"));
            if !registry_shaped {
                return None;
            }
            Some(Metadata {
                title: json_str(&manifest, "title").or_else(|| Some(name.clone())),
                summary: json_str(&manifest, "description"),
                // Unrendered release templates such as `${VERSION}` are not versions.
                version: json_str(&manifest, "version").filter(|version| !version.contains("${")),
                author: None,
                package_ref: Some(name),
            })
        }
    }
    .map(|mut metadata| {
        if metadata.title.is_none() {
            metadata.title = Some(match candidate.kind {
                ResourceKind::Skill if !candidate.root.is_empty() => {
                    last_segment(&candidate.root).to_owned()
                }
                ResourceKind::Rule if last_segment(file) == "AGENTS.md" => file.to_owned(),
                _ => file_stem(file).to_owned(),
            });
        }
        metadata
    })
}

/// Raw text, `None` for a missing file.
fn optional_raw(
    env: &ProviderEnv,
    repo: &RepoRef,
    path: &str,
) -> Result<Option<String>, ProviderError> {
    match github::raw_file(env, repo, path) {
        Ok(text) => Ok(Some(text)),
        Err(ProviderError::Http { status: 404 }) => Ok(None),
        Err(error) => Err(error),
    }
}

#[derive(Deserialize)]
struct RepoSearch {
    total_count: u64,
    items: Vec<RepoHit>,
}

#[derive(Deserialize)]
struct RepoHit {
    full_name: String,
    description: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    owner: RepoOwner,
}

#[derive(Deserialize)]
struct RepoOwner {
    login: String,
}

impl GithubDirect {
    fn resolve(
        &self,
        target: &RepoTarget,
        request: &SearchRequest,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<SearchPage, ProviderError> {
        let offset = parse_cursor(request.cursor.as_deref(), RESOLVE_CURSOR)?;
        let (entries, _truncated) = github::tree(env, &target.repo)?;
        let candidates: Vec<Candidate> = detect(&entries, &target.path)
            .into_iter()
            .filter(|candidate| request.wants(candidate.kind))
            .collect();
        let window = request.page_size.clamp(1, MAX_RESOLVE_PAGE);
        let fetched_at_unix = now_unix();
        let mut items = Vec::new();
        for candidate in candidates.iter().skip(offset).take(window) {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            let Some(text) = optional_raw(env, &target.repo, &candidate.file)? else {
                continue;
            };
            if let Some(metadata) = read_metadata(candidate, &text) {
                items.push(self.resolved_item(&target.repo, candidate, metadata, fetched_at_unix));
            }
        }
        let next = offset + window;
        Ok(SearchPage {
            items,
            next_cursor: (next < candidates.len()).then(|| format!("{RESOLVE_CURSOR}{next}")),
            total: None,
        })
    }

    fn resolved_item(
        &self,
        repo: &RepoRef,
        candidate: &Candidate,
        metadata: Metadata,
        fetched_at_unix: u64,
    ) -> RemoteItem {
        let directory_based = matches!(candidate.kind, ResourceKind::Skill | ResourceKind::Plugin);
        let canonical_url = match (directory_based, candidate.root.is_empty()) {
            (true, true) => repo.html_url(),
            (true, false) => repo.tree_url(&candidate.root),
            (false, _) => repo.blob_url(&candidate.file),
        };
        RemoteItem {
            source: self.descriptor.id.clone(),
            upstream_id: item_id(candidate.kind, repo, &candidate.file),
            kind: candidate.kind,
            title: metadata.title.unwrap_or_default(),
            summary: metadata.summary.unwrap_or_default(),
            canonical_url: Some(canonical_url),
            repo_url: Some(repo.html_url()),
            resource_path: (!candidate.root.is_empty()).then(|| candidate.root.clone()),
            package_ref: metadata.package_ref,
            version: metadata.version,
            author: metadata.author.or_else(|| Some(repo.owner.clone())),
            categories: Vec::new(),
            actions: RemoteActions {
                view_source: true,
                fetch_files: true,
                install: false,
            },
            fetched_at_unix,
        }
    }

    fn topic_search(
        &self,
        query: &str,
        request: &SearchRequest,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<SearchPage, ProviderError> {
        let offset = parse_cursor(request.cursor.as_deref(), TOPIC_CURSOR)?;
        let wanted: Vec<(ResourceKind, &str)> = KIND_TOPICS
            .iter()
            .copied()
            .filter(|(kind, _)| request.wants(*kind))
            .collect();
        if offset >= wanted.len() {
            return Ok(SearchPage::empty());
        }
        let budget = if env.credentials.get(github::CREDENTIAL_SLOT).is_some() {
            TOKEN_SEARCHES_PER_PAGE
        } else {
            ANONYMOUS_SEARCHES_PER_PAGE
        };
        let end = (offset + budget).min(wanted.len());
        let per_page = request.page_size.clamp(1, MAX_SEARCH_PER_PAGE);
        let fetched_at_unix = now_unix();
        let mut items = Vec::new();
        let mut total = None;
        for (kind, topic) in &wanted[offset..end] {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            let json =
                github::search_repositories(env, &format!("{query} topic:{topic}"), per_page)?;
            let result: RepoSearch = serde_json::from_value(json)
                .map_err(|error| ProviderError::malformed(format!("repository search: {error}")))?;
            if wanted.len() == 1 {
                total = Some(result.total_count);
            }
            for hit in result.items {
                let Some(repo) = RepoRef::parse(&hit.full_name) else {
                    continue;
                };
                items.push(RemoteItem {
                    source: self.descriptor.id.clone(),
                    upstream_id: item_id(*kind, &repo, ""),
                    kind: *kind,
                    title: hit.full_name.clone(),
                    summary: hit.description.unwrap_or_default(),
                    canonical_url: Some(repo.html_url()),
                    repo_url: Some(repo.html_url()),
                    resource_path: None,
                    package_ref: None,
                    version: None,
                    author: Some(hit.owner.login),
                    categories: hit.topics,
                    actions: RemoteActions {
                        view_source: true,
                        fetch_files: false,
                        install: false,
                    },
                    fetched_at_unix,
                });
            }
        }
        Ok(SearchPage {
            items,
            next_cursor: (end < wanted.len()).then(|| format!("{TOPIC_CURSOR}{end}")),
            total,
        })
    }
}

impl MarketplaceProvider for GithubDirect {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus {
        if env.credentials.get(github::CREDENTIAL_SLOT).is_some() {
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
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let query = request.query.trim();
        if query.is_empty() {
            return Ok(SearchPage::empty());
        }
        let Some(target) = RepoTarget::parse(query) else {
            return self.topic_search(query, request, env, cancel);
        };
        match self.resolve(&target, request, env, cancel) {
            // A bare `word/word` that is not a repository is a keyword query.
            Err(ProviderError::Http { status: 404 })
                if !target.explicit_url && request.cursor.is_none() =>
            {
                self.topic_search(query, request, env, cancel)
            }
            result => result,
        }
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let (kind, repo, file) = parse_item_id(&item.upstream_id)
            .ok_or_else(|| ProviderError::malformed("invalid github-direct id"))?;
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let single_file = |path: &str| RemoteFile {
            path: path.to_owned(),
            size: None,
            url: Some(raw_url(&repo, path)),
        };
        let mut detail = RemoteDetail {
            item: item.clone(),
            readme_markdown: None,
            manifest_json: None,
            files: Vec::new(),
            tools: Vec::new(),
        };
        if file.is_empty() {
            detail.readme_markdown = optional_raw(env, &repo, "README.md")?;
            return Ok(detail);
        }
        match kind {
            ResourceKind::Skill | ResourceKind::Plugin => {
                let parent = split_parent(&file).0;
                let root = if kind == ResourceKind::Plugin {
                    split_parent(parent).0
                } else {
                    parent
                };
                let (entries, _truncated) = github::tree(env, &repo)?;
                let under_root = |path: &str| {
                    root.is_empty()
                        || path
                            .strip_prefix(root)
                            .is_some_and(|rest| rest.starts_with('/'))
                };
                detail.files = entries
                    .iter()
                    .filter(|entry| !entry.is_dir && under_root(&entry.path))
                    .take(MAX_DETAIL_FILES)
                    .map(|entry| RemoteFile {
                        path: entry.path.clone(),
                        size: entry.size,
                        url: Some(raw_url(&repo, &entry.path)),
                    })
                    .collect();
                if cancel.is_cancelled() {
                    return Err(ProviderError::Cancelled);
                }
                let main = github::raw_file(env, &repo, &file)?;
                if kind == ResourceKind::Skill {
                    detail.readme_markdown = Some(main);
                } else {
                    detail.manifest_json = Some(main);
                    let readme = if root.is_empty() {
                        "README.md".to_owned()
                    } else {
                        format!("{root}/README.md")
                    };
                    if detail.files.iter().any(|entry| entry.path == readme) {
                        detail.readme_markdown = optional_raw(env, &repo, &readme)?;
                    }
                }
            }
            ResourceKind::McpServer => {
                detail.manifest_json = Some(github::raw_file(env, &repo, &file)?);
                detail.files = vec![single_file(&file)];
            }
            ResourceKind::Agent | ResourceKind::Prompt | ResourceKind::Rule => {
                detail.readme_markdown = Some(github::raw_file(env, &repo, &file)?);
                detail.files = vec![single_file(&file)];
            }
        }
        Ok(detail)
    }
}
