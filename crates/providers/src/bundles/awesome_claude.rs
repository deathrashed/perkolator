//! `awesome-claude`: the curated README of
//! https://github.com/ianwieds/awesome-claude-code. Metadata only: entries
//! link to third-party projects that Perkolator does not fetch or install.

use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::github::{self, RepoRef};
use perkolator_marketplace::{
    AuthStatus, MarketplaceProvider, Pagination, ProviderCapabilities, ProviderDescriptor,
    ProviderEnv, ProviderError, RemoteActions, SearchPage, SearchRequest, Transport,
};

use super::common::{
    Query, base_item, category_matches, check, github_auth, github_auth_status, join_summary, page,
    parse_github_url, provider_id, requested_kinds,
};

pub const ID: &str = "awesome-claude";
const OWNER: &str = "ianwieds";
const REPO: &str = "awesome-claude-code";
/// Official documentation pages are listed in resource sections but are not resources.
const DOC_HOSTS: [&str; 3] = ["code.claude.com", "docs.claude.com", "docs.anthropic.com"];
const HOOK_NOTE: &str = "Hook project: runs executable code if installed; review before enabling";

struct Entry {
    kind: ResourceKind,
    title: String,
    url: String,
    description: String,
    category: String,
    hook: bool,
}

/// README section → resource kind. Unlisted sections (guides, tools,
/// marketplaces, skill tooling) are not resources and are skipped.
fn section_kind(h2: &str, h3: Option<&str>) -> Option<(ResourceKind, bool)> {
    match (h2, h3) {
        ("Plugins and marketplaces", Some("Plugins")) => Some((ResourceKind::Plugin, false)),
        ("Skills", Some("Skill tools")) => None,
        ("Skills", _) => Some((ResourceKind::Skill, false)),
        ("Subagents", _) => Some((ResourceKind::Agent, false)),
        ("Slash commands", _) => Some((ResourceKind::Prompt, false)),
        ("Hooks", _) => Some((ResourceKind::Plugin, true)),
        ("MCP servers", _) => Some((ResourceKind::McpServer, false)),
        ("CLAUDE.md files and rules", _) => Some((ResourceKind::Rule, false)),
        _ => None,
    }
}

/// `- [Title](https://...) - Description.`
fn parse_line(line: &str) -> Option<(String, String, String)> {
    let rest = line.strip_prefix("- [")?;
    let (title, rest) = rest.split_once("](")?;
    let (url, rest) = rest.split_once(')')?;
    let description = rest.trim_start().strip_prefix('-').unwrap_or(rest).trim();
    let host = url.strip_prefix("https://")?.split('/').next()?;
    if DOC_HOSTS.contains(&host) || title.trim().is_empty() {
        return None;
    }
    Some((
        title.trim().to_owned(),
        url.to_owned(),
        description.to_owned(),
    ))
}

fn parse_readme(text: &str) -> Result<Vec<Entry>, ProviderError> {
    let mut entries = Vec::new();
    let mut h2: Option<String> = None;
    let mut h3: Option<String> = None;
    for line in text.lines().map(str::trim_end) {
        if let Some(title) = line.strip_prefix("## ") {
            h2 = Some(title.trim().to_owned());
            h3 = None;
        } else if let Some(title) = line.strip_prefix("### ") {
            h3 = Some(title.trim().to_owned());
        } else if let Some(h2) = &h2 {
            let Some((kind, hook)) = section_kind(h2, h3.as_deref()) else {
                continue;
            };
            if let Some((title, url, description)) = parse_line(line) {
                entries.push(Entry {
                    kind,
                    title,
                    url,
                    description,
                    category: h3.clone().unwrap_or_else(|| h2.clone()),
                    hook,
                });
            }
        }
    }
    if entries.is_empty() {
        return Err(ProviderError::malformed(
            "README.md has no entries in the expected resource sections",
        ));
    }
    Ok(entries)
}

pub struct AwesomeClaude {
    descriptor: ProviderDescriptor,
}

pub fn provider() -> Arc<dyn MarketplaceProvider> {
    Arc::new(AwesomeClaude {
        descriptor: ProviderDescriptor {
            id: provider_id(ID),
            display_name: "Awesome Claude Code".into(),
            homepage: "https://github.com/ianwieds/awesome-claude-code".into(),
            docs_url: None,
            kinds: vec![
                ResourceKind::Plugin,
                ResourceKind::Skill,
                ResourceKind::Agent,
                ResourceKind::Prompt,
                ResourceKind::McpServer,
                ResourceKind::Rule,
            ],
            transport: Transport::GithubIndex,
            auth: github_auth(),
            capabilities: ProviderCapabilities {
                search: true,
                detail: false,
                fetch_files: false,
                download: false,
                install: false,
                pagination: Pagination::Page,
                categories: true,
                rate_limit_policy: Some(
                    "README.md via raw.githubusercontent.com once per search".into(),
                ),
                requires_attribution: Some(
                    "List content CC0-1.0 (ianwieds/awesome-claude-code)".into(),
                ),
            },
            notes: vec![
                "Curated link list: results are metadata with a source link (view source only); \
                 nothing is fetched from or installed for linked projects."
                    .into(),
                "Sections map to kinds: Plugins → plugin, Skills → skill, Subagents → agent, \
                 Slash commands → prompt, Hooks → plugin (flagged executable), MCP servers → \
                 MCP, CLAUDE.md files and rules → rule. Official docs links are skipped."
                    .into(),
            ],
        },
    })
}

impl MarketplaceProvider for AwesomeClaude {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus {
        github_auth_status(env)
    }

    fn search(
        &self,
        request: &SearchRequest,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<SearchPage, ProviderError> {
        let kinds = requested_kinds(&self.descriptor, request);
        if kinds.is_empty() {
            return Ok(SearchPage::empty());
        }
        check(cancel)?;
        let text = github::raw_file(env, &RepoRef::new(OWNER, REPO), "README.md")?;
        check(cancel)?;
        let query = Query::new(&request.query);
        let mut ranked = Vec::new();
        for entry in parse_readme(&text)? {
            let categories = vec![entry.category.clone()];
            if !kinds.contains(&entry.kind) || !category_matches(request, &categories) {
                continue;
            }
            let Some(score) = query.score(&entry.title, &[&entry.description, &entry.category])
            else {
                continue;
            };
            let summary = if entry.hook {
                join_summary([entry.description, HOOK_NOTE.to_owned()])
            } else {
                entry.description
            };
            let mut item = base_item(
                &self.descriptor.id,
                entry.kind,
                format!("{}:{}", entry.kind.slug(), entry.url),
                entry.title,
                summary,
            );
            if let Some(location) = parse_github_url(&entry.url) {
                item.repo_url = Some(location.repo.html_url());
                item.resource_path = Some(location.path);
            }
            item.canonical_url = Some(entry.url);
            item.categories = categories;
            item.actions = RemoteActions {
                view_source: true,
                fetch_files: false,
                install: false,
            };
            ranked.push((score, item));
        }
        page(ranked, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_sections_and_skips_docs_and_tooling() {
        let readme = "## Skills\n\n### Skill collections\n\n\
            - [a](https://github.com/x/a) - Skills.\n\n### Skill tools\n\n\
            - [tool](https://github.com/x/tool) - Not a skill.\n\n## Subagents\n\n\
            - [Subagents docs](https://code.claude.com/docs/en/sub-agents) - Docs.\n\
            - [b](https://github.com/x/b) - Agents.\n\n## Guides and tutorials\n\n\
            - [guide](https://example.com/g) - Guide.\n";
        let entries = parse_readme(readme).unwrap();
        let summary: Vec<_> = entries.iter().map(|e| (e.kind, e.title.as_str())).collect();
        assert_eq!(
            summary,
            [(ResourceKind::Skill, "a"), (ResourceKind::Agent, "b")]
        );
        assert!(parse_readme("## Guides\n- [g](https://x.dev) - y").is_err());
    }
}
