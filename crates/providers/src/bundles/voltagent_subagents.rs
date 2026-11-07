//! `voltagent-subagents`: the curated README index of
//! https://github.com/VoltAgent/awesome-claude-code-subagents. Search reads
//! README.md once; `detail` fetches the agent file and parses frontmatter.

use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::github::{self, RepoRef};
use perkolator_marketplace::{
    AuthStatus, MarketplaceProvider, Pagination, ProviderCapabilities, ProviderDescriptor,
    ProviderEnv, ProviderError, RemoteActions, RemoteDetail, RemoteFile, RemoteItem, SearchPage,
    SearchRequest, Transport, now_unix,
};
use serde_json::Value;

use super::common::{
    Query, base_item, category_matches, check, fm_list, fm_string, github_auth, github_auth_status,
    join_summary, normalize_rel, page, provider_id, requested_kinds, split_frontmatter,
};

pub const ID: &str = "voltagent-subagents";
const OWNER: &str = "VoltAgent";
const REPO: &str = "awesome-claude-code-subagents";
const AGENT_ROOT: &str = "categories/";

struct Entry {
    name: String,
    path: String,
    description: String,
    category: String,
    plugin: Option<String>,
}

/// `### [01. Core Development](categories/...)` → `Core Development`.
fn category_title(line: &str) -> Option<String> {
    let label = line.strip_prefix("### [")?.split_once(']')?.0;
    let label = match label.split_once(". ") {
        Some((number, rest)) if number.chars().all(|ch| ch.is_ascii_digit()) => rest,
        _ => label,
    };
    Some(label.trim().to_owned())
}

/// `- [**name**](categories/x/name.md) - Description`.
fn parse_line(line: &str) -> Option<(String, String, String)> {
    let rest = line.strip_prefix("- [")?;
    let (label, rest) = rest.split_once("](")?;
    let (link, rest) = rest.split_once(')')?;
    let name = label.trim().trim_matches('*').trim().to_owned();
    let description = rest.trim_start().strip_prefix('-').unwrap_or(rest).trim();
    let path = normalize_rel(link)?;
    (path.starts_with(AGENT_ROOT) && path.ends_with(".md") && !name.is_empty())
        .then(|| (name, path, description.to_owned()))
}

fn parse_readme(text: &str) -> Result<Vec<Entry>, ProviderError> {
    let mut entries = Vec::new();
    let mut category: Option<String> = None;
    let mut plugin = None;
    for line in text.lines().map(str::trim_end) {
        if line.starts_with("### [") {
            category = category_title(line);
            plugin = None;
        } else if line.starts_with("## ") || line.starts_with("# ") {
            category = None;
        } else if let Some(rest) = line.strip_prefix("**Plugin:**") {
            plugin = Some(rest.trim().trim_matches('`').to_owned()).filter(|p| !p.is_empty());
        } else if let (Some(category), Some((name, path, description))) =
            (&category, parse_line(line))
        {
            entries.push(Entry {
                name,
                path,
                description,
                category: category.clone(),
                plugin: plugin.clone(),
            });
        }
    }
    if entries.is_empty() {
        return Err(ProviderError::malformed(
            "README.md has no `categories/*.md` subagent entries",
        ));
    }
    Ok(entries)
}

pub struct VoltAgentSubagents {
    descriptor: ProviderDescriptor,
}

pub fn provider() -> Arc<dyn MarketplaceProvider> {
    Arc::new(VoltAgentSubagents {
        descriptor: ProviderDescriptor {
            id: provider_id(ID),
            display_name: "VoltAgent subagents".into(),
            homepage: "https://github.com/VoltAgent/awesome-claude-code-subagents".into(),
            docs_url: None,
            kinds: vec![ResourceKind::Agent],
            transport: Transport::GithubIndex,
            auth: github_auth(),
            capabilities: ProviderCapabilities {
                search: true,
                detail: true,
                fetch_files: true,
                download: true,
                install: false,
                pagination: Pagination::Page,
                categories: true,
                rate_limit_policy: Some(
                    "README.md and agent files via raw.githubusercontent.com; no GitHub API calls"
                        .into(),
                ),
                requires_attribution: None,
            },
            notes: vec![
                "Agents are listed from the README category index; each file is fetched and \
                 its frontmatter (name, description, tools, model) verified only in detail."
                    .into(),
            ],
        },
    })
}

impl VoltAgentSubagents {
    fn repo() -> RepoRef {
        RepoRef::new(OWNER, REPO)
    }

    fn item(&self, entry: &Entry) -> RemoteItem {
        let repo = Self::repo();
        let mut item = base_item(
            &self.descriptor.id,
            ResourceKind::Agent,
            format!("{OWNER}/{REPO}:{}", entry.path),
            entry.name.clone(),
            entry.description.clone(),
        );
        item.canonical_url = Some(repo.blob_url(&entry.path));
        item.repo_url = Some(repo.html_url());
        item.resource_path = Some(entry.path.clone());
        item.categories = vec![entry.category.clone()];
        item.actions = RemoteActions {
            view_source: true,
            fetch_files: true,
            install: false,
        };
        item
    }
}

impl MarketplaceProvider for VoltAgentSubagents {
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
        if requested_kinds(&self.descriptor, request).is_empty() {
            return Ok(SearchPage::empty());
        }
        check(cancel)?;
        let text = github::raw_file(env, &Self::repo(), "README.md")?;
        check(cancel)?;
        let query = Query::new(&request.query);
        let ranked = parse_readme(&text)?
            .iter()
            .filter(|entry| category_matches(request, std::slice::from_ref(&entry.category)))
            .filter_map(|entry| {
                let fields = [
                    entry.description.as_str(),
                    entry.category.as_str(),
                    entry.plugin.as_deref().unwrap_or(""),
                ];
                query
                    .score(&entry.name, &fields)
                    .map(|score| (score, self.item(entry)))
            })
            .collect();
        page(ranked, request)
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let path = item
            .resource_path
            .as_deref()
            .and_then(normalize_rel)
            .filter(|path| path.starts_with(AGENT_ROOT) && path.ends_with(".md"))
            .ok_or_else(|| ProviderError::unsupported("not a VoltAgent subagent file"))?;
        check(cancel)?;
        let repo = Self::repo();
        let text = github::raw_file(env, &repo, &path)?;
        check(cancel)?;
        let (meta, body) = split_frontmatter(&text);
        let mut item = item.clone();
        item.fetched_at_unix = now_unix();
        if let Some(meta) = &meta {
            let tools = fm_list(meta, "tools");
            let extras = [
                (!tools.is_empty()).then(|| format!("Tools: {}", tools.join(", "))),
                fm_string(meta, "model").map(|model| format!("Model: {model}")),
            ];
            if let Some(description) = fm_string(meta, "description") {
                item.summary = join_summary(
                    [description]
                        .into_iter()
                        .chain(extras.into_iter().flatten()),
                );
            }
        } else {
            item.summary =
                join_summary([item.summary, "No frontmatter found in agent file".into()]);
        }
        Ok(RemoteDetail {
            item,
            readme_markdown: Some(body.to_owned()),
            manifest_json: meta
                .map(|meta| serde_json::to_string_pretty(&Value::Object(meta)).unwrap_or_default()),
            files: vec![RemoteFile {
                url: Some(repo.blob_url(&path)),
                size: Some(text.len() as u64),
                path,
            }],
            tools: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_category_entries_and_rejects_outside_links() {
        let readme = "## 📚 Categories\n\n### [01. Core Development](categories/01-core-development/)\n\
            **Plugin:** `voltagent-core-dev`\n\n\
            - [**api-designer**](categories/01-core-development/api-designer.md) - REST architect\n\
            - [**evil**](../../etc/passwd.md) - nope\n\
            - [**ext**](https://example.com/x.md) - nope\n\n## 🤖 Understanding\n\
            - [**after**](categories/x/after.md) - outside section\n";
        let entries = parse_readme(readme).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "api-designer");
        assert_eq!(entries[0].category, "Core Development");
        assert_eq!(entries[0].plugin.as_deref(), Some("voltagent-core-dev"));
        assert!(parse_readme("# Nothing here").is_err());
    }
}
