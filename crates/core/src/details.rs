use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{RelPath, ResourceFile, ResourceKind, Scope, SecretString};

/// Kind-specific typed payload. Each variant carries only facts that the
/// resource's native format actually provides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResourceDetails {
    Skill(SkillDetails),
    McpServer(McpServerDetails),
    Agent(AgentProfileDetails),
    Prompt(PromptTemplateDetails),
    Rule(RuleDetails),
    Plugin(PluginDetails),
}

impl ResourceDetails {
    pub fn kind(&self) -> ResourceKind {
        match self {
            ResourceDetails::Skill(_) => ResourceKind::Skill,
            ResourceDetails::McpServer(_) => ResourceKind::McpServer,
            ResourceDetails::Agent(_) => ResourceKind::Agent,
            ResourceDetails::Prompt(_) => ResourceKind::Prompt,
            ResourceDetails::Rule(_) => ResourceKind::Rule,
            ResourceDetails::Plugin(_) => ResourceKind::Plugin,
        }
    }

    /// The primary on-disk document to preview, when one exists.
    pub fn primary_file(&self) -> Option<&PathBuf> {
        match self {
            ResourceDetails::Skill(details) => Some(&details.entry),
            ResourceDetails::McpServer(details) => Some(&details.config_file),
            ResourceDetails::Agent(details) => Some(&details.file),
            ResourceDetails::Prompt(details) => Some(&details.file),
            ResourceDetails::Rule(details) => Some(&details.file),
            ResourceDetails::Plugin(details) => Some(&details.manifest),
        }
    }

    /// The root directory whose file tree can be browsed, when one exists.
    pub fn browse_root(&self) -> Option<&PathBuf> {
        match self {
            ResourceDetails::Skill(details) => Some(&details.root),
            ResourceDetails::Plugin(details) => Some(&details.root),
            _ => None,
        }
    }
}

/// An Agent Skills directory (`SKILL.md` plus references/scripts/assets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillDetails {
    /// Skill directory as found (may be a symlink in an agent root).
    pub root: PathBuf,
    /// Absolute path to `SKILL.md`.
    pub entry: PathBuf,
    /// Physical category folders between the library root and the skill.
    pub category_path: Vec<String>,
    /// Full frontmatter including unknown keys, preserved verbatim.
    pub frontmatter: BTreeMap<String, serde_json::Value>,
    /// Inventory of files (not following external symlinks).
    pub files: Vec<ResourceFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigFormat {
    Json,
    Toml,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvBinding {
    pub name: String,
    pub value: SecretString,
}

/// Transport declared for an MCP server. stdio and remote transports are
/// never conflated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        env: Vec<EnvBinding>,
    },
    StreamableHttp {
        url: String,
        headers: Vec<EnvBinding>,
    },
    Sse {
        url: String,
        headers: Vec<EnvBinding>,
    },
    Unknown {
        declared_type: Option<String>,
    },
}

/// A tool declared statically (registry metadata/manifest). Tools discovered
/// through a live handshake are not stored here; that requires explicit
/// user permission.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpToolDecl {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

/// One MCP server entry inside one client's configuration file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpServerDetails {
    /// Host client that owns the config (e.g. `claude-code`, `cursor`).
    pub client: String,
    pub config_file: PathBuf,
    pub config_format: ConfigFormat,
    /// Entry name / key inside the config.
    pub entry: String,
    pub transport: McpTransport,
    /// Explicit enabled flag if the client format has one.
    pub enabled: Option<bool>,
    pub declared_tools: Vec<McpToolDecl>,
}

/// A reusable agent profile / subagent definition (not the host application).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfileDetails {
    pub host: Option<String>,
    pub file: PathBuf,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateSyntax {
    /// `{{name}}` placeholders.
    DoubleBrace,
    /// `$ARGUMENTS`, `$1`.. style (slash-command prompts).
    Arguments,
    /// No variables.
    Plain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptVariable {
    pub name: String,
    pub description: Option<String>,
    pub default: Option<String>,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptTemplateDetails {
    pub file: PathBuf,
    pub host: Option<String>,
    pub syntax: TemplateSyntax,
    pub variables: Vec<PromptVariable>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleFormat {
    AgentsMd,
    ClaudeMd,
    GeminiMd,
    CursorMdc,
    CopilotInstructions,
    Generic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDetails {
    pub file: PathBuf,
    pub format: RuleFormat,
    pub host: Option<String>,
    pub scope: Scope,
    pub description: Option<String>,
    pub always_apply: Option<bool>,
    pub globs: Vec<String>,
    /// Lower numbers load first; only set when the host defines an order.
    pub precedence: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "host", rename_all = "kebab-case")]
pub enum PluginHost {
    /// Extends Perkolator itself.
    Perkolator,
    /// Installed into another application (Cursor, Claude Code, ...).
    External { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginComponent {
    /// e.g. `skills`, `rules`, `agents`, `commands`, `hooks`, `mcpServers`.
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDetails {
    pub manifest: PathBuf,
    pub root: PathBuf,
    pub host: PluginHost,
    pub components: Vec<PluginComponent>,
    pub permissions: Vec<String>,
    /// Files that could execute (scripts, binaries, hooks). Never run on preview.
    pub executable_files: Vec<RelPath>,
    /// blake3 over manifest + executable files; approval invalidates on change.
    pub content_hash: Option<String>,
    pub files: Vec<ResourceFile>,
}
