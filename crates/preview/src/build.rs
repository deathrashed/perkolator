use std::fs;
use std::path::Path;

use perkolator_core::{
    AgentProfileDetails, CancelToken, IssueSeverity, LocationRole, McpServerDetails, McpToolDecl,
    McpTransport, PluginDetails, PluginHost, PromptTemplateDetails, RelPath, ResourceDetails,
    ResourceRecord, RuleDetails, RuleFormat, Scope, SkillDetails, SourceKind, TemplateSyntax,
};
use serde_json::Value;

use crate::fs::{read_confined, walk_tree};
use crate::markdown::parse_markdown;
use crate::redact::{mcp_config_snippet, redact_url};
use crate::{Document, Preview, PreviewError, PreviewLimits, PreviewTab, ToolParam, ToolView};

const TOOLS_UNKNOWN_NOTE: &str =
    "Tools are only known after a connection test (requires permission)";
const README_NAMES: &[&str] = &["README.md", "readme.md", "Readme.md", "README.markdown"];

type Pairs = Vec<(String, String)>;

/// Build the inert preview for one record. Blocking I/O: call off the UI
/// thread. Returns [`PreviewError::Cancelled`] as soon as `cancel` fires
/// (checked between every file operation and during tree walks).
pub fn build_preview(
    record: &ResourceRecord,
    generation: u64,
    limits: &PreviewLimits,
    cancel: &CancelToken,
) -> Result<Preview, PreviewError> {
    check(cancel)?;
    let tabs = match &record.details {
        ResourceDetails::Skill(skill) => skill_tabs(record, skill, limits, cancel)?,
        ResourceDetails::McpServer(mcp) => mcp_tabs(record, mcp),
        ResourceDetails::Agent(agent) => agent_tabs(record, agent, limits),
        ResourceDetails::Prompt(prompt) => prompt_tabs(record, prompt, limits),
        ResourceDetails::Rule(rule) => rule_tabs(record, rule, limits),
        ResourceDetails::Plugin(plugin) => plugin_tabs(record, plugin, limits, cancel)?,
    };
    check(cancel)?;
    Ok(Preview {
        generation,
        id: record.id.clone(),
        title: record.name.clone(),
        tabs,
    })
}

fn check(cancel: &CancelToken) -> Result<(), PreviewError> {
    if cancel.is_cancelled() {
        Err(PreviewError::Cancelled)
    } else {
        Ok(())
    }
}

fn pair(key: &str, value: impl Into<String>) -> (String, String) {
    (key.to_owned(), value.into())
}

fn push_list(pairs: &mut Pairs, key: &str, values: &[String]) {
    if !values.is_empty() {
        pairs.push(pair(key, values.join(", ")));
    }
}

fn is_dir(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// Read `file` confined to `root` (which may itself be a symlink).
fn read_in_root(root: &Path, file: &Path, limits: &PreviewLimits) -> Document {
    let rel = RelPath::from_child(root, file).ok().or_else(|| {
        let root = fs::canonicalize(root).ok()?;
        let file = fs::canonicalize(file).ok()?;
        RelPath::from_child(&root, &file).ok()
    });
    match rel {
        Some(rel) => read_confined(root, &rel, limits),
        None => Document::unavailable(format!("{} is outside the resource root", file.display())),
    }
}

/// Read a single-file resource confined to its own directory.
fn read_standalone(file: &Path, limits: &PreviewLimits) -> Document {
    let name = file.file_name().and_then(|name| name.to_str());
    match (file.parent(), name.map(RelPath::new)) {
        (Some(parent), Some(Ok(rel))) => read_confined(parent, &rel, limits),
        _ => Document::unavailable(format!("{} is not a file path", file.display())),
    }
}

fn json_display(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) if items.iter().all(Value::is_string) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

fn scope_label(scope: &Scope) -> String {
    match scope {
        Scope::Global => "Global".to_owned(),
        Scope::Project { root } => format!("Project: {}", root.display()),
        Scope::Custom { path } => format!("Custom: {}", path.display()),
    }
}

fn source_label(source: &SourceKind) -> String {
    match source {
        SourceKind::Library { root } => format!("Library: {}", root.display()),
        SourceKind::AgentHost { host, scope } => format!("{host} ({})", scope_label(scope)),
        SourceKind::Project { root } => format!("Project: {}", root.display()),
        SourceKind::Remote { provider, url } => format!("{provider}: {}", redact_url(url)),
        SourceKind::Cache { provider, url } => format!("{provider} (cached): {}", redact_url(url)),
    }
}

fn common_metadata(record: &ResourceRecord) -> Pairs {
    let mut pairs = vec![pair("Kind", record.kind().label())];
    if let Some(publisher) = &record.publisher {
        pairs.push(pair("Publisher", publisher.as_str()));
    }
    if let Some(version) = &record.version {
        pairs.push(pair("Version", version.as_str()));
    }
    pairs.push(pair("Source", source_label(&record.provenance.source)));
    if let Some(url) = &record.provenance.origin_url {
        pairs.push(pair("Origin", redact_url(url)));
    }
    for location in &record.locations {
        let role = match &location.role {
            LocationRole::Canonical => "canonical".to_owned(),
            LocationRole::Deployed { target, scope } => {
                format!("deployed to {target} ({})", scope_label(scope))
            }
            LocationRole::ConfigEntry { entry, .. } => format!("config entry {entry}"),
        };
        pairs.push(pair(
            "Location",
            format!(
                "{} · {role} · {}",
                location.state.label(),
                location.path.display()
            ),
        ));
    }
    push_list(&mut pairs, "Tags", &record.tags);
    for issue in &record.issues {
        let key = match issue.severity {
            IssueSeverity::Info => "Note",
            IssueSeverity::Warning => "Warning",
            IssueSeverity::Error => "Error",
        };
        pairs.push(pair(key, issue.message.as_str()));
    }
    pairs
}

fn skill_tabs(
    record: &ResourceRecord,
    skill: &SkillDetails,
    limits: &PreviewLimits,
    cancel: &CancelToken,
) -> Result<Vec<PreviewTab>, PreviewError> {
    let mut overview = read_in_root(&skill.root, &skill.entry, limits);
    // The registry's typed frontmatter is authoritative over the shallow view.
    if let Document::Markdown { frontmatter, .. } = &mut overview
        && !skill.frontmatter.is_empty()
    {
        *frontmatter = skill
            .frontmatter
            .iter()
            .map(|(key, value)| (key.clone(), json_display(value)))
            .collect();
    }
    let mut tabs = vec![PreviewTab::Overview(overview)];
    check(cancel)?;
    if is_dir(&skill.root) {
        tabs.push(PreviewTab::Files(walk_tree(
            &skill.root,
            limits,
            Some(cancel),
        )?));
    }
    let mut metadata = common_metadata(record);
    if !skill.category_path.is_empty() {
        metadata.push(pair("Category", skill.category_path.join(" / ")));
    }
    tabs.push(PreviewTab::Metadata(metadata));
    Ok(tabs)
}

fn type_display(value: &Value) -> String {
    match value {
        Value::Array(types) => types
            .iter()
            .map(json_display)
            .collect::<Vec<_>>()
            .join(" | "),
        other => json_display(other),
    }
}

fn tool_view(tool: &McpToolDecl) -> ToolView {
    let schema = tool.input_schema.as_ref();
    let required: Vec<&str> = schema
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let parameters = schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, property)| ToolParam {
                    name: name.clone(),
                    type_name: property.get("type").map(type_display),
                    required: required.contains(&name.as_str()),
                    description: property
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default();
    ToolView {
        name: tool.name.clone(),
        description: tool.description.clone(),
        parameters,
        schema: schema.and_then(|schema| serde_json::to_string_pretty(schema).ok()),
    }
}

fn mcp_tabs(record: &ResourceRecord, mcp: &McpServerDetails) -> Vec<PreviewTab> {
    let (format, text) = mcp_config_snippet(mcp);
    let mut tabs = vec![PreviewTab::Config { format, text }];
    if !mcp.declared_tools.is_empty() {
        tabs.push(PreviewTab::Tools(
            mcp.declared_tools.iter().map(tool_view).collect(),
        ));
    }
    let mut metadata = common_metadata(record);
    metadata.push(pair("Client", mcp.client.as_str()));
    metadata.push(pair("Config file", mcp.config_file.display().to_string()));
    metadata.push(pair("Entry", mcp.entry.as_str()));
    let transport = match &mcp.transport {
        McpTransport::Stdio { .. } => "stdio".to_owned(),
        McpTransport::StreamableHttp { .. } => "Streamable HTTP".to_owned(),
        McpTransport::Sse { .. } => "SSE".to_owned(),
        McpTransport::Unknown { declared_type } => match declared_type {
            Some(declared) => format!("unknown ({declared})"),
            None => "unknown".to_owned(),
        },
    };
    metadata.push(pair("Transport", transport));
    if let Some(enabled) = mcp.enabled {
        metadata.push(pair("Enabled", if enabled { "yes" } else { "no" }));
    }
    if mcp.declared_tools.is_empty() {
        metadata.push(pair("Tools", TOOLS_UNKNOWN_NOTE));
    }
    tabs.push(PreviewTab::Metadata(metadata));
    tabs
}

fn agent_tabs(
    record: &ResourceRecord,
    agent: &AgentProfileDetails,
    limits: &PreviewLimits,
) -> Vec<PreviewTab> {
    let mut metadata = common_metadata(record);
    if let Some(host) = &agent.host {
        metadata.push(pair("Host", host.as_str()));
    }
    if let Some(model) = &agent.model {
        metadata.push(pair("Model", model.as_str()));
    }
    push_list(&mut metadata, "Tools", &agent.tools);
    push_list(&mut metadata, "Skills", &agent.skills);
    push_list(&mut metadata, "MCP servers", &agent.mcp_servers);
    vec![
        PreviewTab::Overview(read_standalone(&agent.file, limits)),
        PreviewTab::Metadata(metadata),
    ]
}

fn prompt_tabs(
    record: &ResourceRecord,
    prompt: &PromptTemplateDetails,
    limits: &PreviewLimits,
) -> Vec<PreviewTab> {
    let overview = read_standalone(&prompt.file, limits);
    let raw = overview.text().map(str::to_owned);
    let mut tabs = vec![PreviewTab::Overview(overview)];
    if raw.is_some() || !prompt.variables.is_empty() {
        tabs.push(PreviewTab::Template {
            variables: prompt.variables.clone(),
            raw: raw.unwrap_or_default(),
        });
    }
    let mut metadata = common_metadata(record);
    if let Some(host) = &prompt.host {
        metadata.push(pair("Host", host.as_str()));
    }
    let syntax = match prompt.syntax {
        TemplateSyntax::DoubleBrace => "{{name}} placeholders",
        TemplateSyntax::Arguments => "$ARGUMENTS / $1 placeholders",
        TemplateSyntax::Plain => "plain text (no variables)",
    };
    metadata.push(pair("Syntax", syntax));
    tabs.push(PreviewTab::Metadata(metadata));
    tabs
}

fn rule_tabs(
    record: &ResourceRecord,
    rule: &RuleDetails,
    limits: &PreviewLimits,
) -> Vec<PreviewTab> {
    let format = match rule.format {
        RuleFormat::AgentsMd => "AGENTS.md",
        RuleFormat::ClaudeMd => "CLAUDE.md",
        RuleFormat::GeminiMd => "GEMINI.md",
        RuleFormat::CursorMdc => "Cursor rule (.mdc)",
        RuleFormat::CopilotInstructions => "Copilot instructions",
        RuleFormat::Generic => "Generic",
    };
    let mut precedence = vec![pair("Scope", scope_label(&rule.scope))];
    if let Some(host) = &rule.host {
        precedence.push(pair("Host", host.as_str()));
    }
    precedence.push(pair("Format", format));
    push_list(&mut precedence, "Globs", &rule.globs);
    if let Some(always) = rule.always_apply {
        precedence.push(pair("Always apply", if always { "yes" } else { "no" }));
    }
    if let Some(order) = rule.precedence {
        precedence.push(pair("Precedence", format!("{order} (lower loads first)")));
    }
    if let Some(description) = &rule.description {
        precedence.push(pair("Description", description.as_str()));
    }
    vec![
        PreviewTab::Overview(read_standalone(&rule.file, limits)),
        PreviewTab::Precedence(precedence),
        PreviewTab::Metadata(common_metadata(record)),
    ]
}

/// The manifest's `description` field, if the manifest parses.
fn manifest_description(document: &Document) -> Option<String> {
    let Document::Code { language, text } = document else {
        return None;
    };
    let description = match language.as_str() {
        "json" => serde_json::from_str::<Value>(text)
            .ok()?
            .get("description")?
            .as_str()?
            .to_owned(),
        "toml" => text
            .parse::<toml::Table>()
            .ok()?
            .get("description")?
            .as_str()?
            .to_owned(),
        _ => return None,
    };
    (!description.trim().is_empty()).then_some(description)
}

fn plugin_tabs(
    record: &ResourceRecord,
    plugin: &PluginDetails,
    limits: &PreviewLimits,
    cancel: &CancelToken,
) -> Result<Vec<PreviewTab>, PreviewError> {
    let mut tabs = Vec::new();
    let readme = README_NAMES.iter().find_map(|name| {
        let rel = RelPath::new(name).ok()?;
        let document = read_confined(&plugin.root, &rel, limits);
        (!matches!(document, Document::Unavailable { .. })).then_some(document)
    });
    check(cancel)?;
    let overview = readme.or_else(|| {
        manifest_description(&read_in_root(&plugin.root, &plugin.manifest, limits))
            .map(|description| parse_markdown(&description, None))
    });
    if let Some(overview) = overview {
        tabs.push(PreviewTab::Overview(overview));
    }
    check(cancel)?;
    if is_dir(&plugin.root) {
        tabs.push(PreviewTab::Files(walk_tree(
            &plugin.root,
            limits,
            Some(cancel),
        )?));
    }
    tabs.push(PreviewTab::Permissions {
        permissions: plugin.permissions.clone(),
        executable_files: plugin.executable_files.clone(),
        content_hash: plugin.content_hash.clone(),
    });
    let mut metadata = common_metadata(record);
    let host = match &plugin.host {
        PluginHost::Perkolator => "Perkolator".to_owned(),
        PluginHost::External { name } => name.clone(),
    };
    metadata.push(pair("Host", host));
    metadata.push(pair("Manifest", plugin.manifest.display().to_string()));
    for component in &plugin.components {
        metadata.push(pair(
            "Component",
            format!("{}: {}", component.kind, component.path),
        ));
    }
    tabs.push(PreviewTab::Metadata(metadata));
    Ok(tabs)
}
