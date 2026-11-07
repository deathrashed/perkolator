//! Helpers shared by the bundle adapters: query matching, local paging,
//! GitHub URL/path handling, a short-lived tree cache, frontmatter parsing
//! and plugin component discovery. Everything here is inert.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use perkolator_core::{CancelToken, ProviderId, ResourceKind};
use perkolator_marketplace::github::{self, RepoRef, TreeEntry};
use perkolator_marketplace::{
    AuthStatus, AuthStrategy, ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions,
    RemoteItem, SearchPage, SearchRequest, now_unix,
};
use serde_json::{Map, Value};

/// GitHub trees cost one API request (60/h anonymous), so a fetched tree is
/// reused for this long by the adapter that fetched it.
pub(crate) const TREE_TTL_SECS: u64 = 600;
const TREE_CACHE_CAPACITY: usize = 8;
const MAX_PAGE_SIZE: usize = 100;
const REDACTED: &str = "<redacted>";

pub(crate) const HOOKS_NOTE: &str =
    "Includes executable hooks: review before enabling; discovery never runs them";
pub(crate) const LSP_NOTE: &str = "Starts language server binaries when enabled";

pub(crate) fn provider_id(id: &str) -> ProviderId {
    ProviderId::new(id).expect("built-in provider id is valid")
}

pub(crate) fn github_auth() -> AuthStrategy {
    AuthStrategy::OptionalToken {
        slot: github::CREDENTIAL_SLOT.to_owned(),
    }
}

/// Local check only: a stored GitHub token raises quotas, none is required.
pub(crate) fn github_auth_status(env: &ProviderEnv) -> AuthStatus {
    if env.credentials.get(github::CREDENTIAL_SLOT).is_some() {
        AuthStatus::Ready { identity: None }
    } else {
        AuthStatus::NotRequired
    }
}

pub(crate) fn check(cancel: &CancelToken) -> Result<(), ProviderError> {
    if cancel.is_cancelled() {
        Err(ProviderError::Cancelled)
    } else {
        Ok(())
    }
}

/// Kinds both requested and supported; empty means "make no request".
pub(crate) fn requested_kinds(
    descriptor: &ProviderDescriptor,
    request: &SearchRequest,
) -> Vec<ResourceKind> {
    descriptor
        .kinds
        .iter()
        .copied()
        .filter(|kind| request.wants(*kind))
        .collect()
}

pub(crate) fn category_matches(request: &SearchRequest, categories: &[String]) -> bool {
    request.category.as_deref().is_none_or(|wanted| {
        categories
            .iter()
            .any(|category| category.eq_ignore_ascii_case(wanted))
    })
}

/// Case-insensitive AND match of whitespace-separated terms.
pub(crate) struct Query {
    phrase: String,
    terms: Vec<String>,
}

impl Query {
    pub(crate) fn new(text: &str) -> Self {
        let phrase = text.trim().to_lowercase();
        let terms = phrase.split_whitespace().map(str::to_owned).collect();
        Self { phrase, terms }
    }

    /// `None` when a term is missing; title hits outrank other fields.
    pub(crate) fn score(&self, title: &str, fields: &[&str]) -> Option<u32> {
        if self.terms.is_empty() {
            return Some(0);
        }
        let title = title.to_lowercase();
        let fields: Vec<String> = fields.iter().map(|field| field.to_lowercase()).collect();
        let mut score = 0;
        for term in &self.terms {
            if title.contains(term.as_str()) {
                score += 2;
            } else if fields.iter().any(|field| field.contains(term.as_str())) {
                score += 1;
            } else {
                return None;
            }
        }
        if title == self.phrase {
            score += 100;
        } else if title.starts_with(self.phrase.as_str()) {
            score += 20;
        }
        Some(score)
    }
}

/// Rank (stable), then slice by the offset cursor the adapter issued.
pub(crate) fn page(
    mut ranked: Vec<(u32, RemoteItem)>,
    request: &SearchRequest,
) -> Result<SearchPage, ProviderError> {
    ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let offset = match request.cursor.as_deref() {
        None => 0,
        Some(cursor) => cursor
            .parse::<usize>()
            .map_err(|_| ProviderError::unsupported(format!("invalid page cursor `{cursor}`")))?,
    };
    let size = request.page_size.clamp(1, MAX_PAGE_SIZE);
    let matched = ranked.len();
    let items = ranked
        .into_iter()
        .skip(offset)
        .take(size)
        .map(|(_, item)| item)
        .collect();
    Ok(SearchPage {
        items,
        next_cursor: (offset + size < matched).then(|| (offset + size).to_string()),
        total: None,
    })
}

/// A result with every optional field empty; adapters fill what they know.
pub(crate) fn base_item(
    source: &ProviderId,
    kind: ResourceKind,
    upstream_id: String,
    title: String,
    summary: String,
) -> RemoteItem {
    RemoteItem {
        source: source.clone(),
        upstream_id,
        kind,
        title,
        summary,
        canonical_url: None,
        repo_url: None,
        resource_path: None,
        package_ref: None,
        version: None,
        author: None,
        categories: Vec::new(),
        actions: RemoteActions::default(),
        fetched_at_unix: now_unix(),
    }
}

/// Repo-relative path with `.`/empty segments removed; `None` for `..` or
/// absolute paths, which untrusted manifests must not be able to express.
pub(crate) fn normalize_rel(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.starts_with('/') || trimmed.contains('\\') {
        return None;
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

pub(crate) fn join_rel(base: &str, rel: &str) -> Option<String> {
    let base = normalize_rel(base)?;
    let rel = normalize_rel(rel)?;
    Some(match (base.is_empty(), rel.is_empty()) {
        (true, _) => rel,
        (_, true) => base,
        _ => format!("{base}/{rel}"),
    })
}

/// Git refs as they may appear in a URL path.
pub(crate) fn valid_reference(reference: &str) -> bool {
    !reference.is_empty()
        && !reference
            .split('/')
            .any(|part| part.is_empty() || part == "..")
        && reference
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'))
}

pub(crate) struct GithubLocation {
    pub repo: RepoRef,
    pub path: String,
}

/// `https://github.com/owner/repo[/(tree|blob)/ref/path]`; other hosts → `None`.
pub(crate) fn parse_github_url(url: &str) -> Option<GithubLocation> {
    let rest = url
        .trim()
        .strip_prefix("https://")
        .or_else(|| url.trim().strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix("github.com/")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
    let mut repo = RepoRef::parse(&format!("{}/{}", parts.first()?, parts.get(1)?))?;
    let mut path = String::new();
    if let (Some(&("tree" | "blob")), Some(reference)) = (parts.get(2), parts.get(3))
        && valid_reference(reference)
    {
        repo.reference = (*reference).to_owned();
        path = normalize_rel(&parts[4..].join("/"))?;
    }
    Some(GithubLocation { repo, path })
}

pub(crate) fn same_repo(a: &RepoRef, b: &RepoRef) -> bool {
    a.owner.eq_ignore_ascii_case(&b.owner) && a.repo.eq_ignore_ascii_case(&b.repo)
}

#[derive(Clone)]
pub(crate) struct RepoTree {
    pub entries: Arc<Vec<TreeEntry>>,
    pub truncated: bool,
}

struct CachedTree {
    fetched_at: u64,
    tree: RepoTree,
}

/// Per-adapter memo of recursive trees, keyed by `owner/repo@ref`.
#[derive(Default)]
pub(crate) struct TreeCache {
    entries: Mutex<HashMap<String, CachedTree>>,
}

impl TreeCache {
    fn key(repo: &RepoRef) -> String {
        format!(
            "{}/{}@{}",
            repo.owner.to_ascii_lowercase(),
            repo.repo.to_ascii_lowercase(),
            repo.reference
        )
    }

    /// A fresh cached tree, never a request.
    pub(crate) fn peek(&self, repo: &RepoRef) -> Option<RepoTree> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        entries
            .get(&Self::key(repo))
            .filter(|cached| now_unix().saturating_sub(cached.fetched_at) < TREE_TTL_SECS)
            .map(|cached| cached.tree.clone())
    }

    pub(crate) fn get(
        &self,
        env: &ProviderEnv,
        repo: &RepoRef,
        cancel: &CancelToken,
    ) -> Result<RepoTree, ProviderError> {
        if let Some(tree) = self.peek(repo) {
            return Ok(tree);
        }
        check(cancel)?;
        let (entries, truncated) = github::tree(env, repo)?;
        let tree = RepoTree {
            entries: Arc::new(entries),
            truncated,
        };
        let mut cache = self
            .entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if cache.len() >= TREE_CACHE_CAPACITY
            && let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, cached)| cached.fetched_at)
                .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
        cache.insert(
            Self::key(repo),
            CachedTree {
                fetched_at: now_unix(),
                tree: tree.clone(),
            },
        );
        drop(cache);
        check(cancel)?;
        Ok(tree)
    }
}

/// Files (not directories) at or below `root`, paths relative to the repo.
pub(crate) fn files_under<'a>(
    entries: &'a [TreeEntry],
    root: &'a str,
) -> impl Iterator<Item = &'a TreeEntry> + 'a {
    entries
        .iter()
        .filter(move |entry| !entry.is_dir && rel_to(&entry.path, root).is_some())
}

fn rel_to<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if root.is_empty() {
        return Some(path);
    }
    path.strip_prefix(root)?.strip_prefix('/')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ComponentKind {
    Skill,
    Agent,
    Rule,
    Mcp,
    Command,
    Hooks,
    Lsp,
}

impl ComponentKind {
    pub(crate) fn resource_kind(self) -> Option<ResourceKind> {
        match self {
            ComponentKind::Skill => Some(ResourceKind::Skill),
            ComponentKind::Agent => Some(ResourceKind::Agent),
            ComponentKind::Rule => Some(ResourceKind::Rule),
            ComponentKind::Mcp => Some(ResourceKind::McpServer),
            ComponentKind::Command | ComponentKind::Hooks | ComponentKind::Lsp => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Component {
    pub kind: ComponentKind,
    pub name: String,
    /// Repo-relative path: the skill directory or the component file.
    pub path: String,
}

fn stem(file: &str) -> &str {
    let name = file.rsplit('/').next().unwrap_or(file);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn is_readme(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("README.md"))
}

/// Components found by the conventional Claude/Cursor plugin layout.
pub(crate) fn discover_components(entries: &[TreeEntry], root: &str) -> Vec<Component> {
    let mut found = Vec::new();
    for entry in files_under(entries, root) {
        let Some(rel) = rel_to(&entry.path, root) else {
            continue;
        };
        let kind = if rel.starts_with("skills/") && rel.ends_with("/SKILL.md") {
            let dir = entry.path.trim_end_matches("/SKILL.md");
            found.push(Component {
                kind: ComponentKind::Skill,
                name: dir.rsplit('/').next().unwrap_or(dir).to_owned(),
                path: dir.to_owned(),
            });
            continue;
        } else if rel.starts_with("agents/") && rel.ends_with(".md") && !is_readme(rel) {
            ComponentKind::Agent
        } else if rel.starts_with("commands/") && rel.ends_with(".md") && !is_readme(rel) {
            ComponentKind::Command
        } else if rel.starts_with("rules/")
            && (rel.ends_with(".mdc") || rel.ends_with(".md"))
            && !is_readme(rel)
        {
            ComponentKind::Rule
        } else if rel == ".mcp.json" || rel == "mcp.json" {
            ComponentKind::Mcp
        } else if rel == "hooks/hooks.json" || rel == "hooks.json" {
            ComponentKind::Hooks
        } else if rel == ".lsp.json" {
            ComponentKind::Lsp
        } else {
            continue;
        };
        found.push(Component {
            kind,
            name: stem(rel).to_owned(),
            path: entry.path.clone(),
        });
    }
    found.sort_by(|a, b| (a.kind, &a.path).cmp(&(b.kind, &b.path)));
    found.dedup_by(|a, b| a.kind == b.kind && a.path == b.path);
    found
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// "Contains 2 agents, 1 skill, MCP config" (hooks/LSP are separate notes).
pub(crate) fn describe_components(components: &[Component]) -> Option<String> {
    let count = |kind| components.iter().filter(|c| c.kind == kind).count();
    let mut parts = Vec::new();
    for (kind, one, many) in [
        (ComponentKind::Skill, "skill", "skills"),
        (ComponentKind::Agent, "agent", "agents"),
        (ComponentKind::Command, "command", "commands"),
        (ComponentKind::Rule, "rule", "rules"),
    ] {
        let n = count(kind);
        if n > 0 {
            parts.push(plural(n, one, many));
        }
    }
    if count(ComponentKind::Mcp) > 0 {
        parts.push("MCP config".to_owned());
    }
    (!parts.is_empty()).then(|| format!("Contains {}", parts.join(", ")))
}

pub(crate) fn join_summary(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .map(|part| part.trim().trim_end_matches('.').to_owned())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// YAML frontmatter as a JSON object plus the Markdown body. Malformed
/// frontmatter yields `None` and leaves the whole text as body.
pub(crate) fn split_frontmatter(text: &str) -> (Option<Map<String, Value>>, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return match serde_norway::from_str::<Value>(yaml) {
                Ok(Value::Object(map)) => (Some(map), body),
                _ => (None, text),
            };
        }
        offset += line.len();
    }
    (None, text)
}

pub(crate) fn fm_string(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// `tools: Read, Write` or a YAML list.
pub(crate) fn fm_list(map: &Map<String, Value>, key: &str) -> Vec<String> {
    match map.get(key) {
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Replace literal `env`/`headers` values (possible secrets) while keeping
/// `${VAR}` placeholders, which are what manifests are meant to contain.
pub(crate) fn redact_mcp(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if matches!(key.as_str(), "env" | "headers") {
                    if let Value::Object(secrets) = child {
                        for secret in secrets.values_mut() {
                            let keep = secret.as_str().is_some_and(|text| text.contains("${"));
                            if !keep {
                                *secret = Value::String(REDACTED.to_owned());
                            }
                        }
                    }
                } else {
                    redact_mcp(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_mcp),
        _ => {}
    }
}

/// Server names from `{"mcpServers": {..}}` or a bare `{name: {..}}` map.
pub(crate) fn mcp_server_names(value: &Value) -> Vec<String> {
    let servers = value.get("mcpServers").unwrap_or(value);
    servers
        .as_object()
        .map(|map| {
            map.iter()
                .filter(|(_, server)| server.is_object())
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn author_name(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.clone()),
        Value::Object(map) => map.get("name").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
    .map(|name| name.trim().to_owned())
    .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(path: &str) -> TreeEntry {
        TreeEntry {
            path: path.to_owned(),
            is_dir: false,
            size: Some(1),
        }
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        assert_eq!(normalize_rel("./plugins//x/").as_deref(), Some("plugins/x"));
        assert!(normalize_rel("../etc").is_none());
        assert!(normalize_rel("/etc/passwd").is_none());
        assert!(join_rel("skills", "./a/../../b").is_none());
        assert_eq!(join_rel("", "./skills/box").as_deref(), Some("skills/box"));
    }

    #[test]
    fn parses_github_urls() {
        let loc =
            parse_github_url("https://github.com/evmts/tevm/blob/main/.claude/commands/commit.md")
                .unwrap();
        assert_eq!(loc.repo.reference, "main");
        assert_eq!(loc.path, ".claude/commands/commit.md");
        let root = parse_github_url("https://github.com/box/skills.git").unwrap();
        assert_eq!(
            (root.repo.repo.as_str(), root.path.as_str()),
            ("skills", "")
        );
        assert!(parse_github_url("https://gitlab.com/a/b").is_none());
        assert!(parse_github_url("https://github.com/a/b/tree/main/../x").is_none());
    }

    #[test]
    fn query_requires_every_term_and_prefers_titles() {
        let query = Query::new("Code Review");
        assert!(query.score("code-formatter", &["linting"]).is_none());
        let title_hit = query.score("code review", &[]).unwrap();
        let field_hit = query.score("pr-toolkit", &["Code review helpers"]).unwrap();
        assert!(title_hit > field_hit);
        assert_eq!(Query::new("  ").score("anything", &[]), Some(0));
    }

    #[test]
    fn discovers_conventional_components() {
        let entries = [
            blob("plugins/p/skills/a/SKILL.md"),
            blob("plugins/p/skills/a/references/x.md"),
            blob("plugins/p/agents/reviewer.md"),
            blob("plugins/p/agents/README.md"),
            blob("plugins/p/commands/go.md"),
            blob("plugins/p/rules/style.mdc"),
            blob("plugins/p/hooks/hooks.json"),
            blob("plugins/p/.mcp.json"),
            blob("plugins/other/agents/x.md"),
        ];
        let found = discover_components(&entries, "plugins/p");
        let kinds: Vec<_> = found.iter().map(|c| (c.kind, c.name.as_str())).collect();
        assert_eq!(
            kinds,
            [
                (ComponentKind::Skill, "a"),
                (ComponentKind::Agent, "reviewer"),
                (ComponentKind::Rule, "style"),
                (ComponentKind::Mcp, ".mcp"),
                (ComponentKind::Command, "go"),
                (ComponentKind::Hooks, "hooks"),
            ]
        );
        assert_eq!(found[0].path, "plugins/p/skills/a");
        assert_eq!(
            describe_components(&found).as_deref(),
            Some("Contains 1 skill, 1 agent, 1 command, 1 rule, MCP config")
        );
    }

    #[test]
    fn frontmatter_parses_and_tolerates_garbage() {
        let (meta, body) = split_frontmatter("---\nname: a\ntools: Read, Grep\n---\nBody\n");
        let meta = meta.unwrap();
        assert_eq!(fm_string(&meta, "name").as_deref(), Some("a"));
        assert_eq!(fm_list(&meta, "tools"), ["Read", "Grep"]);
        assert_eq!(body, "Body\n");
        let (meta, body) = split_frontmatter("---\n: [\n---\nx");
        assert!(meta.is_none());
        assert!(body.starts_with("---"));
    }

    #[test]
    fn redacts_literal_secrets_but_keeps_placeholders() {
        let mut value = serde_json::json!({
            "mcpServers": {"s": {
                "env": {"API_KEY": "sk-live-123", "HOME": "${HOME}"},
                "headers": {"Authorization": "Bearer ${TOKEN}"}
            }}
        });
        redact_mcp(&mut value);
        let text = value.to_string();
        assert!(!text.contains("sk-live-123"));
        assert!(text.contains("${HOME}") && text.contains("Bearer ${TOKEN}"));
        assert_eq!(mcp_server_names(&value), ["s"]);
    }
}
