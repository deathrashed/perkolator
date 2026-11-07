use std::path::{Path, PathBuf};

use perkolator_core::{PluginHost, ProviderId, RuleFormat, Scope, SourceKind};

/// On-disk dialect of an MCP client configuration file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpConfigFormat {
    /// `{"mcpServers": {..}}` (Claude Code, Cursor, Gemini CLI, `.mcp.json`).
    /// Claude's `~/.claude.json` additionally carries project-scoped entries
    /// under `projects.<path>.mcpServers`.
    McpServersJson,
    /// VS Code `{"servers": {..}}`.
    VsCodeServersJson,
    /// Codex `config.toml` `[mcp_servers.<name>]` tables.
    CodexToml,
    /// OpenCode `{"mcp": {..}}` with `local`/`remote` types.
    OpenCodeJson,
}

/// How the contents of a [`ScanRoot`] are organised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootLayout {
    /// Nested category folders; a directory containing `SKILL.md` is a skill
    /// and is not descended into further.
    SkillLibrary,
    /// Flat agent skills directory whose entries may be symlinks.
    SkillDirectory { host: String },
    /// A single MCP client configuration file.
    McpConfig {
        client: String,
        format: McpConfigFormat,
    },
    /// Directory of `*.md` agent profiles with frontmatter.
    AgentProfiles { host: String },
    /// Directory of `*.md` prompts / slash commands.
    Prompts { host: Option<String> },
    /// A single instructions file (`AGENTS.md`, `CLAUDE.md`, ...).
    RuleFile {
        host: Option<String>,
        format: RuleFormat,
        scope: Scope,
    },
    /// Directory of `*.mdc` / `*.md` rules (e.g. `.cursor/rules`).
    RuleDirectory { host: Option<String>, scope: Scope },
    /// Directory whose descendants hold plugin manifests
    /// (`.cursor-plugin/plugin.json`, `.claude-plugin/plugin.json`,
    /// `perkolator-plugin.json`).
    Plugins { host: PluginHost },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanRoot {
    pub path: PathBuf,
    pub layout: RootLayout,
    pub provider: ProviderId,
    pub source: SourceKind,
}

/// Bounds that keep a scan of an arbitrary directory tree predictable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanLimits {
    /// Maximum directory depth below a root or resource directory.
    pub max_depth: usize,
    /// Maximum inventory entries recorded per skill/plugin.
    pub max_files_per_resource: usize,
    /// Larger documents/configs are reported as problems instead of parsed.
    pub max_file_bytes: u64,
    /// Bytes of each executable file folded into a plugin content hash.
    pub max_hash_bytes_per_file: u64,
}

const DEFAULT_MAX_DEPTH: usize = 8;
const DEFAULT_MAX_FILES_PER_RESOURCE: usize = 2_000;
const DEFAULT_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_MAX_HASH_BYTES_PER_FILE: u64 = 4 * 1024 * 1024;

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_files_per_resource: DEFAULT_MAX_FILES_PER_RESOURCE,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_hash_bytes_per_file: DEFAULT_MAX_HASH_BYTES_PER_FILE,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanConfig {
    pub roots: Vec<ScanRoot>,
    pub limits: ScanLimits,
}

fn provider(value: &str) -> ProviderId {
    ProviderId::new(value).expect("static provider ids are valid")
}

fn host_source(host: &str, scope: Scope) -> SourceKind {
    SourceKind::AgentHost {
        host: host.to_owned(),
        scope,
    }
}

impl ScanConfig {
    /// Well-known locations for the current user's home directory, if known.
    pub fn for_current_user(projects: &[PathBuf]) -> Option<Self> {
        dirs::home_dir().map(|home| Self::well_known(&home, projects))
    }

    /// Roots used by the bundled demo fixture (`tests/fixtures/sample-home`).
    pub fn for_fixture_home(home: &Path) -> Self {
        Self::well_known(home, &[home.join("projects/demo-app")])
    }

    /// Conventional locations used by common agent hosts, filtered to the
    /// ones that exist. These are discovery guesses based on each host's
    /// documented or observed defaults, not guarantees: hosts move files
    /// between versions and users relocate them. Users can add custom roots.
    ///
    /// `home` and `projects` are canonicalized so that "reached through a
    /// symlink" can be decided by comparing a found path with its canonical
    /// form; a relative or symlinked home would otherwise look linked.
    pub fn well_known(home: &Path, projects: &[PathBuf]) -> Self {
        let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        let home = home.as_path();
        let projects: Vec<PathBuf> = projects
            .iter()
            .map(|project| project.canonicalize().unwrap_or_else(|_| project.clone()))
            .collect();
        let projects = projects.as_slice();
        let mut roots = Vec::new();
        let global = || Scope::Global;

        let library = |path: PathBuf, provider_id: &str, layout: RootLayout| ScanRoot {
            source: SourceKind::Library { root: path.clone() },
            path,
            layout,
            provider: provider(provider_id),
        };
        let hosted = |path: PathBuf, host: &str, layout: RootLayout, scope: Scope| ScanRoot {
            path,
            layout,
            provider: provider(host),
            source: host_source(host, scope),
        };
        let mcp = |path: PathBuf, client: &str, format: McpConfigFormat| ScanRoot {
            path,
            layout: RootLayout::McpConfig {
                client: client.to_owned(),
                format,
            },
            provider: provider(client),
            source: host_source(client, Scope::Global),
        };
        let rule_file = |path: PathBuf, host: &str, format: RuleFormat| ScanRoot {
            path,
            layout: RootLayout::RuleFile {
                host: Some(host.to_owned()),
                format,
                scope: Scope::Global,
            },
            provider: provider(host),
            source: host_source(host, Scope::Global),
        };
        let skills_dir = |host: &str| RootLayout::SkillDirectory {
            host: host.to_owned(),
        };
        let agents_dir = |host: &str| RootLayout::AgentProfiles {
            host: host.to_owned(),
        };
        let prompts_dir = |host: &str| RootLayout::Prompts {
            host: Some(host.to_owned()),
        };
        let plugins_dir = |host: &str| RootLayout::Plugins {
            host: PluginHost::External {
                name: host.to_owned(),
            },
        };

        // User libraries. `~/.agents/skills` is the shared canonical store used
        // by `npx skills`; host directories commonly symlink into it.
        roots.push(library(
            home.join("Skills"),
            "library",
            RootLayout::SkillLibrary,
        ));
        roots.push(library(
            home.join(".agents/skills"),
            "agents",
            RootLayout::SkillLibrary,
        ));
        roots.push(library(
            home.join("Prompts"),
            "library",
            RootLayout::Prompts { host: None },
        ));

        // Claude Code.
        let claude = home.join(".claude");
        roots.push(hosted(
            claude.join("skills"),
            "claude-code",
            skills_dir("claude-code"),
            global(),
        ));
        roots.push(hosted(
            claude.join("agents"),
            "claude-code",
            agents_dir("claude-code"),
            global(),
        ));
        roots.push(hosted(
            claude.join("commands"),
            "claude-code",
            prompts_dir("claude-code"),
            global(),
        ));
        roots.push(rule_file(
            claude.join("CLAUDE.md"),
            "claude-code",
            RuleFormat::ClaudeMd,
        ));
        roots.push(mcp(
            home.join(".claude.json"),
            "claude-code",
            McpConfigFormat::McpServersJson,
        ));
        roots.push(hosted(
            claude.join("plugins/cache"),
            "claude-code",
            plugins_dir("claude-code"),
            global(),
        ));

        // Cursor.
        let cursor = home.join(".cursor");
        roots.push(hosted(
            cursor.join("skills"),
            "cursor",
            skills_dir("cursor"),
            global(),
        ));
        roots.push(hosted(
            cursor.join("agents"),
            "cursor",
            agents_dir("cursor"),
            global(),
        ));
        roots.push(mcp(
            cursor.join("mcp.json"),
            "cursor",
            McpConfigFormat::McpServersJson,
        ));
        roots.push(hosted(
            cursor.join("rules"),
            "cursor",
            RootLayout::RuleDirectory {
                host: Some("cursor".into()),
                scope: Scope::Global,
            },
            global(),
        ));
        roots.push(hosted(
            cursor.join("plugins/local"),
            "cursor",
            plugins_dir("cursor"),
            global(),
        ));
        roots.push(hosted(
            cursor.join("plugins/cache"),
            "cursor",
            plugins_dir("cursor"),
            global(),
        ));

        // Codex.
        let codex = home.join(".codex");
        roots.push(hosted(
            codex.join("skills"),
            "codex",
            skills_dir("codex"),
            global(),
        ));
        roots.push(hosted(
            codex.join("prompts"),
            "codex",
            prompts_dir("codex"),
            global(),
        ));
        roots.push(mcp(
            codex.join("config.toml"),
            "codex",
            McpConfigFormat::CodexToml,
        ));
        roots.push(rule_file(
            codex.join("AGENTS.md"),
            "codex",
            RuleFormat::AgentsMd,
        ));

        // Gemini CLI.
        let gemini = home.join(".gemini");
        roots.push(rule_file(
            gemini.join("GEMINI.md"),
            "gemini-cli",
            RuleFormat::GeminiMd,
        ));
        roots.push(mcp(
            gemini.join("settings.json"),
            "gemini-cli",
            McpConfigFormat::McpServersJson,
        ));

        // OpenCode.
        let opencode = home.join(".config/opencode");
        roots.push(hosted(
            opencode.join("skills"),
            "opencode",
            skills_dir("opencode"),
            global(),
        ));
        roots.push(hosted(
            opencode.join("agent"),
            "opencode",
            agents_dir("opencode"),
            global(),
        ));
        roots.push(hosted(
            opencode.join("agents"),
            "opencode",
            agents_dir("opencode"),
            global(),
        ));
        roots.push(hosted(
            opencode.join("command"),
            "opencode",
            prompts_dir("opencode"),
            global(),
        ));
        roots.push(hosted(
            opencode.join("commands"),
            "opencode",
            prompts_dir("opencode"),
            global(),
        ));
        roots.push(rule_file(
            opencode.join("AGENTS.md"),
            "opencode",
            RuleFormat::AgentsMd,
        ));
        roots.push(mcp(
            opencode.join("opencode.json"),
            "opencode",
            McpConfigFormat::OpenCodeJson,
        ));

        for project in projects {
            roots.extend(project_roots(project));
        }

        roots.retain(|root| root.path.exists());
        Self {
            roots,
            limits: ScanLimits::default(),
        }
    }
}

/// Conventional per-project locations (unfiltered).
fn project_roots(project: &Path) -> Vec<ScanRoot> {
    let scope = Scope::Project {
        root: project.to_path_buf(),
    };
    let source = SourceKind::Project {
        root: project.to_path_buf(),
    };
    let root = |relative: &str, provider_id: &str, layout: RootLayout| ScanRoot {
        path: project.join(relative),
        layout,
        provider: provider(provider_id),
        source: source.clone(),
    };
    let rule = |host: Option<&str>, format: RuleFormat| RootLayout::RuleFile {
        host: host.map(str::to_owned),
        format,
        scope: scope.clone(),
    };
    let mcp = |client: &str, format: McpConfigFormat| RootLayout::McpConfig {
        client: client.to_owned(),
        format,
    };
    vec![
        root("AGENTS.md", "project", rule(None, RuleFormat::AgentsMd)),
        root(
            "CLAUDE.md",
            "claude-code",
            rule(Some("claude-code"), RuleFormat::ClaudeMd),
        ),
        root(
            "GEMINI.md",
            "gemini-cli",
            rule(Some("gemini-cli"), RuleFormat::GeminiMd),
        ),
        root(
            ".github/copilot-instructions.md",
            "copilot",
            rule(Some("copilot"), RuleFormat::CopilotInstructions),
        ),
        root(
            ".mcp.json",
            "claude-code",
            mcp("claude-code", McpConfigFormat::McpServersJson),
        ),
        root(
            ".cursor/mcp.json",
            "cursor",
            mcp("cursor", McpConfigFormat::McpServersJson),
        ),
        root(
            ".vscode/mcp.json",
            "vscode",
            mcp("vscode", McpConfigFormat::VsCodeServersJson),
        ),
        root(
            ".cursor/rules",
            "cursor",
            RootLayout::RuleDirectory {
                host: Some("cursor".into()),
                scope: scope.clone(),
            },
        ),
        root(
            ".claude/skills",
            "claude-code",
            RootLayout::SkillDirectory {
                host: "claude-code".into(),
            },
        ),
        root(
            ".claude/agents",
            "claude-code",
            RootLayout::AgentProfiles {
                host: "claude-code".into(),
            },
        ),
        root(
            ".claude/commands",
            "claude-code",
            RootLayout::Prompts {
                host: Some("claude-code".into()),
            },
        ),
        root(".agents/skills", "project", RootLayout::SkillLibrary),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_keeps_only_existing_roots() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".claude/skills")).unwrap();
        std::fs::write(home.path().join(".cursor-not-a-dir"), "").unwrap();
        let config = ScanConfig::well_known(home.path(), &[]);
        assert_eq!(config.roots.len(), 1);
        assert_eq!(
            config.roots[0].layout,
            RootLayout::SkillDirectory {
                host: "claude-code".into()
            }
        );
    }

    #[test]
    fn project_roots_use_project_scope() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("p");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("AGENTS.md"), "# Agents\n").unwrap();
        let config = ScanConfig::well_known(home.path(), std::slice::from_ref(&project));
        assert_eq!(config.roots.len(), 1);
        assert_eq!(
            config.roots[0].source,
            SourceKind::Project { root: project }
        );
    }
}
