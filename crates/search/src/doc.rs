use std::collections::HashSet;

use perkolator_core::{
    FileKind, PluginHost, ResourceDetails, ResourceId, ResourceKind, ResourceRecord, SourceKind,
};

/// Upper bound on `extra` entries taken from one record, so a plugin with a
/// huge file inventory cannot dominate index memory or match time.
const MAX_EXTRA_TERMS: usize = 48;

/// Searchable projection of a [`ResourceRecord`]. Contains no file contents
/// and no secret values (env/header values are never indexed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchDoc {
    pub id: ResourceId,
    pub kind: ResourceKind,
    pub name: String,
    pub publisher: Option<String>,
    pub tags: Vec<String>,
    pub summary: String,
    /// Low-weight terms: reference file names, categories, hosts, tool names.
    pub extra: Vec<String>,
    pub installed: bool,
    /// True for remote catalogue results and cached copies of them.
    pub remote: bool,
    /// Truthful origin label, e.g. `Library`, `claude-code`, `skills.sh (cached)`.
    pub source_label: String,
}

impl SearchDoc {
    pub fn from_record(record: &ResourceRecord) -> Self {
        let (remote, source_label) = match &record.provenance.source {
            SourceKind::Library { .. } => (false, "Library".to_owned()),
            SourceKind::AgentHost { host, .. } => (false, host.clone()),
            SourceKind::Project { .. } => (false, "Project".to_owned()),
            SourceKind::Remote { provider, .. } => (true, provider.clone()),
            SourceKind::Cache { provider, .. } => (true, format!("{provider} (cached)")),
        };
        Self {
            id: record.id.clone(),
            kind: record.kind(),
            name: record.name.clone(),
            publisher: record.publisher.clone(),
            tags: record.tags.clone(),
            summary: record.summary.clone(),
            extra: extra_terms(&record.details),
            installed: record.is_installed(),
            remote,
            source_label,
        }
    }
}

fn extra_terms(details: &ResourceDetails) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    match details {
        ResourceDetails::Skill(skill) => {
            terms.extend(skill.category_path.iter().cloned());
            terms.extend(
                skill
                    .files
                    .iter()
                    .filter(|file| file.kind != FileKind::Directory)
                    .map(|file| file.path.file_name().to_owned()),
            );
        }
        ResourceDetails::McpServer(mcp) => {
            terms.push(mcp.client.clone());
            terms.push(mcp.entry.clone());
            terms.extend(mcp.declared_tools.iter().map(|tool| tool.name.clone()));
        }
        ResourceDetails::Agent(agent) => {
            terms.extend(agent.host.iter().cloned());
            terms.extend(agent.model.iter().cloned());
            terms.extend(agent.tools.iter().cloned());
            terms.extend(agent.skills.iter().cloned());
            terms.extend(agent.mcp_servers.iter().cloned());
        }
        ResourceDetails::Prompt(prompt) => {
            terms.extend(prompt.host.iter().cloned());
            terms.extend(
                prompt
                    .variables
                    .iter()
                    .map(|variable| variable.name.clone()),
            );
        }
        ResourceDetails::Rule(rule) => {
            terms.extend(rule.host.iter().cloned());
            terms.extend(rule.globs.iter().cloned());
        }
        ResourceDetails::Plugin(plugin) => {
            if let PluginHost::External { name } = &plugin.host {
                terms.push(name.clone());
            }
            terms.extend(
                plugin
                    .components
                    .iter()
                    .map(|component| component.kind.clone()),
            );
            terms.extend(
                plugin
                    .files
                    .iter()
                    .filter(|file| file.kind != FileKind::Directory)
                    .map(|file| file.path.file_name().to_owned()),
            );
        }
    }
    let mut seen = HashSet::new();
    terms.retain(|term| !term.is_empty() && seen.insert(term.clone()));
    terms.truncate(MAX_EXTRA_TERMS);
    terms
}
