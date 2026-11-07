//! Shared adapter for plugin marketplaces kept in a GitHub repository as a
//! `marketplace.json` index (Claude Code `.claude-plugin/` and Cursor
//! `.cursor-plugin/` layouts). Search reads the index from
//! raw.githubusercontent.com once per call; components of plugins hosted in
//! the marketplace repository come from one cached tree request. Hooks, LSP
//! servers and command-based sources are reported, never run.

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::github::{self, RepoRef};
use perkolator_marketplace::{
    AuthStatus, MarketplaceProvider, ProviderDescriptor, ProviderEnv, ProviderError, RemoteActions,
    RemoteDetail, RemoteFile, RemoteItem, SearchPage, SearchRequest, now_unix,
};
use serde_json::{Map, Value};

use super::common::{
    Component, ComponentKind, HOOKS_NOTE, LSP_NOTE, Query, TreeCache, author_name, base_item,
    category_matches, check, describe_components, discover_components, files_under, fm_list,
    fm_string, github_auth_status, join_rel, join_summary, mcp_server_names, normalize_rel, page,
    parse_github_url, redact_mcp, requested_kinds, same_repo, split_frontmatter, valid_reference,
};

const PLUGIN_ID_PREFIX: &str = "plugin:";

/// Where a marketplace lives and which conventions it follows.
pub(crate) struct MarketplaceSpec {
    pub owner: &'static str,
    pub repo: &'static str,
    /// Repo-relative path of the index, e.g. `.claude-plugin/marketplace.json`.
    pub index_path: &'static str,
    /// Plugin-relative manifest path, e.g. `.claude-plugin/plugin.json`.
    pub manifest_path: &'static str,
    /// Emit Claude Code `name@marketplace` install references.
    pub install_refs: bool,
    /// Client key in `minClientVersions` whose `"never"` marks an entry
    /// unavailable for that client.
    pub client: Option<&'static str>,
}

pub(crate) struct PluginMarketplace {
    descriptor: ProviderDescriptor,
    spec: MarketplaceSpec,
    trees: TreeCache,
}

struct Index {
    name: Option<String>,
    plugins: Vec<PluginEntry>,
}

enum PluginSource {
    Github {
        repo: RepoRef,
        path: String,
    },
    /// npm, command or non-GitHub git sources: listed, not fetched.
    External {
        kind: String,
        reference: Option<String>,
    },
}

#[derive(Default)]
struct Declared {
    skills: Vec<String>,
    agents: Vec<String>,
    inline_mcp: Vec<String>,
    mcp_files: Vec<String>,
    commands: bool,
    hooks: bool,
    lsp: bool,
}

struct PluginEntry {
    name: String,
    title: String,
    description: String,
    version: Option<String>,
    author: Option<String>,
    categories: Vec<String>,
    keywords: Vec<String>,
    homepage: Option<String>,
    source: PluginSource,
    declared: Declared,
    client_unavailable: bool,
    raw: Value,
}

#[derive(Clone, Copy, Default)]
struct Flags {
    hooks: bool,
    lsp: bool,
    commands: bool,
}

impl PluginEntry {
    fn repo(&self) -> Option<(&RepoRef, &str)> {
        match &self.source {
            PluginSource::Github { repo, path } => Some((repo, path.as_str())),
            PluginSource::External { .. } => None,
        }
    }

    /// Root inside the marketplace repo at its default branch, if hosted there.
    fn local_root(&self, home: &RepoRef) -> Option<&str> {
        self.repo()
            .filter(|(repo, _)| same_repo(repo, home) && repo.reference == home.reference)
            .map(|(_, path)| path)
    }

    /// Discovered components plus the ones the marketplace entry declares.
    fn components(&self, discovered: &[Component]) -> Vec<Component> {
        let mut all = discovered.to_vec();
        if let Some((_, root)) = self.repo() {
            let leaf = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
            for path in &self.declared.skills {
                all.push(Component {
                    kind: ComponentKind::Skill,
                    name: leaf(path),
                    path: path.clone(),
                });
            }
            for path in self.declared.agents.iter().filter(|p| p.ends_with(".md")) {
                all.push(Component {
                    kind: ComponentKind::Agent,
                    name: leaf(path).trim_end_matches(".md").to_owned(),
                    path: path.clone(),
                });
            }
            for path in self
                .declared
                .mcp_files
                .iter()
                .filter(|p| p.ends_with(".json"))
            {
                all.push(Component {
                    kind: ComponentKind::Mcp,
                    name: leaf(path),
                    path: path.clone(),
                });
            }
            if !self.declared.inline_mcp.is_empty() {
                all.push(Component {
                    kind: ComponentKind::Mcp,
                    name: self.declared.inline_mcp.join(", "),
                    path: root.to_owned(),
                });
            }
        }
        all.sort_by(|a, b| (a.kind, &a.path).cmp(&(b.kind, &b.path)));
        all.dedup_by(|a, b| a.kind == b.kind && a.path == b.path);
        all
    }

    fn flags(&self, components: &[Component]) -> Flags {
        let has = |kind| components.iter().any(|c: &Component| c.kind == kind);
        Flags {
            hooks: self.declared.hooks || has(ComponentKind::Hooks),
            lsp: self.declared.lsp || has(ComponentKind::Lsp),
            commands: self.declared.commands && !has(ComponentKind::Command),
        }
    }
}

fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn present(raw: &Value, key: &str) -> bool {
    raw.get(key).is_some_and(|value| !value.is_null())
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Declared component paths resolved against the plugin root. Directory
/// containers (`./skills/`) are left to tree discovery.
fn declared_paths(value: Option<&Value>, base: &str) -> Vec<String> {
    strings(value)
        .into_iter()
        .filter(|path| !path.trim_end().ends_with('/'))
        .filter_map(|path| join_rel(base, &path))
        .filter(|path| !path.is_empty())
        .collect()
}

fn parse_source(value: &Value, plugin_root: &str, home: &RepoRef) -> PluginSource {
    let external = |kind: &str, reference: Option<String>| PluginSource::External {
        kind: kind.to_owned(),
        reference,
    };
    let map = match value {
        Value::String(path) => {
            return match join_rel(plugin_root, path) {
                Some(path) => PluginSource::Github {
                    repo: home.clone(),
                    path,
                },
                None => external("unsafe path", Some(path.clone())),
            };
        }
        Value::Object(map) => map,
        _ => return external("unknown", None),
    };
    let kind = non_empty(map.get("source")).unwrap_or_else(|| "unknown".to_owned());
    let url = non_empty(map.get("url"));
    let location = match kind.as_str() {
        "github" => non_empty(map.get("repo"))
            .and_then(|repo| RepoRef::parse(&repo))
            .map(|repo| (repo, String::new())),
        "url" | "git" | "git-subdir" => url
            .as_deref()
            .and_then(parse_github_url)
            .map(|location| (location.repo, location.path)),
        _ => None,
    };
    let Some((mut repo, base)) = location else {
        let reference = non_empty(map.get("package"))
            .or(url)
            .or_else(|| non_empty(map.get("command")));
        return external(&kind, reference);
    };
    let pinned = non_empty(map.get("sha")).or_else(|| non_empty(map.get("ref")));
    if let Some(pinned) = pinned {
        if !valid_reference(&pinned) {
            return external(&kind, url);
        }
        repo.reference = pinned;
    }
    let sub = non_empty(map.get("path")).unwrap_or_default();
    match join_rel(&base, &sub) {
        Some(path) => PluginSource::Github { repo, path },
        None => external("unsafe path", Some(sub)),
    }
}

fn parse_entry(
    raw: &Value,
    plugin_root: &str,
    home: &RepoRef,
    client: Option<&str>,
) -> Option<PluginEntry> {
    let name = non_empty(raw.get("name"))?;
    let source = parse_source(raw.get("source")?, plugin_root, home);
    let mut declared = Declared {
        commands: present(raw, "commands"),
        hooks: present(raw, "hooks"),
        lsp: present(raw, "lspServers"),
        ..Declared::default()
    };
    if let PluginSource::Github { path, .. } = &source {
        declared.skills = declared_paths(raw.get("skills"), path);
        declared.agents = declared_paths(raw.get("agents"), path);
        match raw.get("mcpServers") {
            Some(Value::Object(servers)) => {
                declared.inline_mcp = servers.keys().cloned().collect();
            }
            other => declared.mcp_files = declared_paths(other, path),
        }
    }
    let client_unavailable = client.is_some_and(|client| {
        raw.get("minClientVersions")
            .and_then(|versions| versions.get(client))
            .and_then(Value::as_str)
            == Some("never")
    });
    let mut keywords = strings(raw.get("keywords"));
    keywords.extend(strings(raw.get("tags")));
    Some(PluginEntry {
        title: non_empty(raw.get("displayName")).unwrap_or_else(|| name.clone()),
        description: non_empty(raw.get("description")).unwrap_or_default(),
        version: non_empty(raw.get("version")),
        author: raw.get("author").and_then(author_name),
        categories: non_empty(raw.get("category")).into_iter().collect(),
        keywords,
        homepage: non_empty(raw.get("homepage")).filter(|url| url.starts_with("https://")),
        source,
        declared,
        client_unavailable,
        raw: raw.clone(),
        name,
    })
}

fn parse_index(text: &str, home: &RepoRef, client: Option<&str>) -> Result<Index, ProviderError> {
    let json: Value = serde_json::from_str(text)
        .map_err(|error| ProviderError::malformed(format!("marketplace.json: {error}")))?;
    let listed = json
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::malformed("marketplace.json has no `plugins` array"))?;
    let plugin_root = json
        .get("metadata")
        .and_then(|metadata| metadata.get("pluginRoot"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let plugins: Vec<PluginEntry> = listed
        .iter()
        .filter_map(|raw| parse_entry(raw, plugin_root, home, client))
        .collect();
    if plugins.is_empty() && !listed.is_empty() {
        return Err(ProviderError::malformed(
            "marketplace.json lists plugins without a name or source",
        ));
    }
    Ok(Index {
        name: non_empty(json.get("name")),
        plugins,
    })
}

/// Parsed `upstream_id`: `plugin:<name>` or `<name>/<kind>:<path>`.
struct Target {
    plugin: String,
    component: Option<(ResourceKind, String)>,
}

impl Target {
    fn parse(id: &str) -> Option<Self> {
        if let Some(name) = id.strip_prefix(PLUGIN_ID_PREFIX) {
            return Some(Self {
                plugin: name.to_owned(),
                component: None,
            });
        }
        let (plugin, rest) = id.split_once('/')?;
        let (kind, path) = rest.split_once(':')?;
        let path = normalize_rel(path).filter(|normal| normal == path)?;
        Some(Self {
            plugin: plugin.to_owned(),
            component: Some((kind.parse().ok()?, path)),
        })
    }
}

fn source_url(repo: &RepoRef, path: &str, is_dir: bool) -> String {
    match (path.is_empty(), is_dir) {
        (true, _) if repo.reference == "HEAD" => repo.html_url(),
        (true, _) => repo.tree_url("").trim_end_matches('/').to_owned(),
        (false, true) => repo.tree_url(path),
        (false, false) => repo.blob_url(path),
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

impl PluginMarketplace {
    pub(crate) fn new(descriptor: ProviderDescriptor, spec: MarketplaceSpec) -> Self {
        Self {
            descriptor,
            spec,
            trees: TreeCache::default(),
        }
    }

    fn home(&self) -> RepoRef {
        RepoRef::new(self.spec.owner, self.spec.repo)
    }

    fn fetch_index(&self, env: &ProviderEnv, cancel: &CancelToken) -> Result<Index, ProviderError> {
        check(cancel)?;
        let text = github::raw_file(env, &self.home(), self.spec.index_path)?;
        check(cancel)?;
        parse_index(&text, &self.home(), self.spec.client)
    }

    fn fetch_text(
        &self,
        env: &ProviderEnv,
        repo: &RepoRef,
        path: &str,
        cancel: &CancelToken,
    ) -> Result<String, ProviderError> {
        check(cancel)?;
        let text = github::raw_file(env, repo, path)?;
        check(cancel)?;
        Ok(text)
    }

    /// JSON file content; unparsable text is kept verbatim for review.
    fn fetch_json(
        &self,
        env: &ProviderEnv,
        repo: &RepoRef,
        path: &str,
        cancel: &CancelToken,
    ) -> Result<Value, ProviderError> {
        let text = self.fetch_text(env, repo, path, cancel)?;
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    fn plugin_summary(
        &self,
        entry: &PluginEntry,
        components: &[Component],
        flags: Flags,
    ) -> String {
        let mut parts = vec![entry.description.clone()];
        parts.extend(describe_components(components));
        if flags.commands {
            parts.push("Declares commands".to_owned());
        }
        if flags.hooks {
            parts.push(HOOKS_NOTE.to_owned());
        }
        if flags.lsp {
            parts.push(LSP_NOTE.to_owned());
        }
        if let PluginSource::External { kind, reference } = &entry.source {
            let reference = reference.as_deref().unwrap_or("unspecified");
            parts.push(match kind.as_str() {
                "command" => format!(
                    "Installed by running a local command (`{reference}`); Perkolator never runs it"
                ),
                "npm" => format!("Distributed as npm package `{reference}`; not fetched"),
                kind => format!("Source type `{kind}` ({reference}) is not fetched"),
            });
        }
        if entry.client_unavailable {
            let client = self.spec.client.unwrap_or("this client");
            parts.push(format!(
                "Marked unavailable for {client} by the marketplace"
            ));
        }
        join_summary(parts)
    }

    fn plugin_item(
        &self,
        index: &Index,
        entry: &PluginEntry,
        components: &[Component],
        flags: Flags,
    ) -> RemoteItem {
        let mut item = base_item(
            &self.descriptor.id,
            ResourceKind::Plugin,
            format!("{PLUGIN_ID_PREFIX}{}", entry.name),
            entry.title.clone(),
            self.plugin_summary(entry, components, flags),
        );
        match &entry.source {
            PluginSource::Github { repo, path } => {
                item.repo_url = Some(repo.html_url());
                item.resource_path = Some(path.clone());
                item.canonical_url = Some(source_url(repo, path, true));
                item.actions = RemoteActions {
                    view_source: true,
                    fetch_files: true,
                    install: false,
                };
            }
            PluginSource::External { kind, reference } => {
                item.canonical_url = entry.homepage.clone();
                item.actions.view_source = item.canonical_url.is_some();
                if kind == "npm" {
                    item.package_ref = reference.clone();
                }
            }
        }
        if self.spec.install_refs
            && let Some(marketplace) = &index.name
        {
            item.package_ref = Some(format!("{}@{marketplace}", entry.name));
        }
        item.version = entry.version.clone();
        item.author = entry.author.clone();
        item.categories = entry.categories.clone();
        item
    }

    fn component_item(
        &self,
        entry: &PluginEntry,
        repo: &RepoRef,
        component: &Component,
        kind: ResourceKind,
    ) -> RemoteItem {
        let inline = entry.repo().is_some_and(|(_, root)| component.path == root);
        let is_dir = kind == ResourceKind::Skill || inline;
        let (title, summary) = match kind {
            ResourceKind::McpServer => {
                let servers = inline.then(|| format!("Servers: {}", component.name));
                (
                    entry.title.clone(),
                    join_summary(
                        [format!(
                            "MCP server configuration in plugin `{}`",
                            entry.name
                        )]
                        .into_iter()
                        .chain(servers)
                        .chain([entry.description.clone()]),
                    ),
                )
            }
            kind => (
                component.name.clone(),
                format!("{} in plugin `{}`", kind.label(), entry.title),
            ),
        };
        let mut item = base_item(
            &self.descriptor.id,
            kind,
            format!("{}/{}:{}", entry.name, kind.slug(), component.path),
            title,
            summary,
        );
        item.repo_url = Some(repo.html_url());
        item.resource_path = Some(component.path.clone());
        item.canonical_url = Some(source_url(repo, &component.path, is_dir));
        item.author = entry.author.clone();
        item.categories = entry.categories.clone();
        item.actions = RemoteActions {
            view_source: true,
            fetch_files: true,
            install: false,
        };
        item
    }

    fn plugin_detail(
        &self,
        index: &Index,
        entry: &PluginEntry,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let mut wrapper = Map::new();
        wrapper.insert("marketplaceEntry".into(), entry.raw.clone());
        let Some((repo, root)) = entry.repo() else {
            let flags = entry.flags(&[]);
            return Ok(RemoteDetail {
                item: self.plugin_item(index, entry, &[], flags),
                readme_markdown: None,
                manifest_json: Some(pretty(&Value::Object(wrapper))),
                files: Vec::new(),
                tools: Vec::new(),
            });
        };
        let tree = self.trees.get(env, repo, cancel)?;
        let files: Vec<RemoteFile> = files_under(&tree.entries, root)
            .map(|file| RemoteFile {
                path: file.path.clone(),
                size: file.size,
                url: Some(repo.blob_url(&file.path)),
            })
            .collect();
        if files.is_empty() {
            return Err(ProviderError::malformed(format!(
                "plugin source `{root}` has no files in {}",
                repo.html_url()
            )));
        }
        let has_file = |path: &str| files.iter().any(|file| file.path == path);
        let components = entry.components(&discover_components(&tree.entries, root));
        let mut flags = entry.flags(&components);

        let manifest_path = join_rel(root, self.spec.manifest_path).unwrap_or_default();
        if has_file(&manifest_path) {
            let manifest = self.fetch_json(env, repo, &manifest_path, cancel)?;
            flags.hooks |= present(&manifest, "hooks");
            flags.lsp |= present(&manifest, "lspServers");
            wrapper.insert("pluginManifest".into(), manifest);
        }
        let mut hooks = Map::new();
        let mut servers = Map::new();
        for component in &components {
            match component.kind {
                ComponentKind::Hooks => {
                    let value = self.fetch_json(env, repo, &component.path, cancel)?;
                    hooks.insert(component.path.clone(), value);
                }
                ComponentKind::Mcp if component.path.ends_with(".json") => {
                    let mut value = self.fetch_json(env, repo, &component.path, cancel)?;
                    redact_mcp(&mut value);
                    servers.insert(component.path.clone(), value);
                }
                _ => {}
            }
        }
        if !hooks.is_empty() {
            wrapper.insert("hooks".into(), Value::Object(hooks));
        }
        if !servers.is_empty() {
            wrapper.insert("mcpConfigs".into(), Value::Object(servers));
        }
        let readme_path = join_rel(root, "README.md").unwrap_or_default();
        let readme_markdown = if has_file(&readme_path) {
            Some(self.fetch_text(env, repo, &readme_path, cancel)?)
        } else {
            None
        };
        let mut item = self.plugin_item(index, entry, &components, flags);
        if tree.truncated {
            item.summary = join_summary([item.summary, "GitHub truncated the file list".into()]);
        }
        Ok(RemoteDetail {
            item,
            readme_markdown,
            manifest_json: Some(pretty(&Value::Object(wrapper))),
            files,
            tools: Vec::new(),
        })
    }

    fn component_detail(
        &self,
        entry: &PluginEntry,
        kind: ResourceKind,
        path: &str,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let (repo, _) = entry
            .repo()
            .ok_or_else(|| ProviderError::unsupported("plugin source is not on GitHub"))?;
        let mut item = item.clone();
        item.fetched_at_unix = now_unix();
        let single_file = |path: &str, text: &str| {
            vec![RemoteFile {
                path: path.to_owned(),
                size: Some(text.len() as u64),
                url: Some(repo.blob_url(path)),
            }]
        };
        match kind {
            ResourceKind::Skill => {
                let skill_md = join_rel(path, "SKILL.md").unwrap_or_default();
                let text = self.fetch_text(env, repo, &skill_md, cancel)?;
                let (meta, body) = split_frontmatter(&text);
                if let Some(description) = meta.as_ref().and_then(|m| fm_string(m, "description")) {
                    item.summary = description;
                }
                let files = match self.trees.get(env, repo, cancel) {
                    Ok(tree) => files_under(&tree.entries, path)
                        .map(|file| RemoteFile {
                            path: file.path.clone(),
                            size: file.size,
                            url: Some(repo.blob_url(&file.path)),
                        })
                        .collect(),
                    Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                    Err(error) => {
                        item.summary =
                            join_summary([item.summary, format!("File list unavailable: {error}")]);
                        single_file(&skill_md, &text)
                    }
                };
                Ok(RemoteDetail {
                    item,
                    readme_markdown: Some(body.to_owned()),
                    manifest_json: meta.map(|meta| pretty(&Value::Object(meta))),
                    files,
                    tools: Vec::new(),
                })
            }
            ResourceKind::Agent | ResourceKind::Rule => {
                let text = self.fetch_text(env, repo, path, cancel)?;
                let (meta, body) = split_frontmatter(&text);
                if let Some(meta) = &meta {
                    let tools = fm_list(meta, "tools");
                    let tools = (!tools.is_empty()).then(|| format!("Tools: {}", tools.join(", ")));
                    if let Some(description) = fm_string(meta, "description") {
                        item.summary = join_summary([description].into_iter().chain(tools));
                    }
                }
                Ok(RemoteDetail {
                    item,
                    readme_markdown: Some(body.to_owned()),
                    manifest_json: meta.map(|meta| pretty(&Value::Object(meta))),
                    files: single_file(path, &text),
                    tools: Vec::new(),
                })
            }
            ResourceKind::McpServer => {
                let (mut value, files) = if path.ends_with(".json") {
                    let text = self.fetch_text(env, repo, path, cancel)?;
                    let value = serde_json::from_str(&text)
                        .map_err(|error| ProviderError::malformed(format!("{path}: {error}")))?;
                    (value, single_file(path, &text))
                } else {
                    let inline = entry.raw.get("mcpServers").cloned().unwrap_or(Value::Null);
                    (serde_json::json!({ "mcpServers": inline }), Vec::new())
                };
                redact_mcp(&mut value);
                let names = mcp_server_names(&value);
                if !names.is_empty() {
                    item.summary = join_summary([
                        format!("MCP servers: {}", names.join(", ")),
                        entry.description.clone(),
                    ]);
                }
                Ok(RemoteDetail {
                    item,
                    readme_markdown: None,
                    manifest_json: Some(pretty(&value)),
                    files,
                    tools: Vec::new(),
                })
            }
            _ => Err(ProviderError::unsupported("no detail for this component")),
        }
    }
}

impl MarketplaceProvider for PluginMarketplace {
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
        let index = self.fetch_index(env, cancel)?;
        let home = self.home();
        let wants_plugins = kinds.contains(&ResourceKind::Plugin);
        let wants_components = kinds.iter().any(|kind| *kind != ResourceKind::Plugin);
        let tree = if wants_components {
            match self.trees.get(env, &home, cancel) {
                Ok(tree) => Some(tree),
                Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                // Plugins still come from the raw index when the API quota is spent.
                Err(_) if wants_plugins => None,
                Err(error) => return Err(error),
            }
        } else {
            self.trees.peek(&home)
        };
        let query = Query::new(&request.query);
        let mut ranked = Vec::new();
        for entry in &index.plugins {
            if self.descriptor.capabilities.categories
                && !category_matches(request, &entry.categories)
            {
                continue;
            }
            let discovered = match (&tree, entry.local_root(&home)) {
                (Some(tree), Some(root)) => discover_components(&tree.entries, root),
                _ => Vec::new(),
            };
            let components = entry.components(&discovered);
            let mut fields: Vec<&str> = vec![&entry.name, &entry.description];
            fields.extend(entry.categories.iter().map(String::as_str));
            fields.extend(entry.keywords.iter().map(String::as_str));
            fields.extend(entry.author.as_deref());
            if wants_plugins && let Some(score) = query.score(&entry.title, &fields) {
                let flags = entry.flags(&components);
                ranked.push((score, self.plugin_item(&index, entry, &components, flags)));
            }
            let Some((repo, _)) = entry.repo() else {
                continue;
            };
            for component in &components {
                let Some(kind) = component.kind.resource_kind() else {
                    continue;
                };
                if !kinds.contains(&kind) {
                    continue;
                }
                let item = self.component_item(entry, repo, component, kind);
                if let Some(score) = query.score(&item.title, &[&entry.title, &entry.description]) {
                    ranked.push((score, item));
                }
            }
        }
        check(cancel)?;
        page(ranked, request)
    }

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let target = Target::parse(&item.upstream_id)
            .ok_or_else(|| ProviderError::unsupported("item id is not from this source"))?;
        let index = self.fetch_index(env, cancel)?;
        let entry = index
            .plugins
            .iter()
            .find(|entry| entry.name == target.plugin)
            .ok_or_else(|| {
                ProviderError::malformed(format!(
                    "`{}` is no longer listed in the marketplace",
                    target.plugin
                ))
            })?;
        match &target.component {
            None => self.plugin_detail(&index, entry, env, cancel),
            Some((kind, path)) => self.component_detail(entry, *kind, path, item, env, cancel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> RepoRef {
        RepoRef::new("o", "market")
    }

    #[test]
    fn resolves_every_source_shape() {
        let string = parse_source(&"./plugins/a".into(), "", &home());
        assert!(matches!(string, PluginSource::Github { ref path, .. } if path == "plugins/a"));
        let subdir = parse_source(
            &serde_json::json!({"source": "git-subdir", "url": "https://github.com/x/y.git",
                "path": "plugins/p", "ref": "v1", "sha": "abc123"}),
            "",
            &home(),
        );
        let PluginSource::Github { repo, path } = subdir else {
            panic!("expected github source");
        };
        assert_eq!(
            (repo.owner.as_str(), repo.reference.as_str()),
            ("x", "abc123")
        );
        assert_eq!(path, "plugins/p");
        let command = parse_source(
            &serde_json::json!({"source": "command", "command": "xcrun agent plugin path"}),
            "",
            &home(),
        );
        assert!(matches!(command, PluginSource::External { ref kind, .. } if kind == "command"));
        let escape = parse_source(&"../../etc".into(), "", &home());
        assert!(matches!(escape, PluginSource::External { ref kind, .. } if kind == "unsafe path"));
        let bad_ref = parse_source(
            &serde_json::json!({"source": "url", "url": "https://github.com/x/y", "ref": "a b"}),
            "",
            &home(),
        );
        assert!(matches!(bad_ref, PluginSource::External { .. }));
    }

    #[test]
    fn target_ids_round_trip_and_reject_traversal() {
        let plugin = Target::parse("plugin:hookify").unwrap();
        assert_eq!(plugin.plugin, "hookify");
        let component = Target::parse("hookify/agent:plugins/hookify/agents/a.md").unwrap();
        assert_eq!(component.component.unwrap().0, ResourceKind::Agent);
        assert!(Target::parse("x/skill:../../etc").is_none());
        assert!(Target::parse("x/bogus:path").is_none());
    }
}
