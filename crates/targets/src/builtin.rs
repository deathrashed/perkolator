//! Built-in agent targets.
//!
//! Skill directories (global + project) and detection markers are copied from
//! vercel-labs/skills `src/agents.ts`, commit
//! `d3701c11b1101251af14c9d148f026e1a47ebe17` (2026-11-06). MCP, rule,
//! agent-profile and prompt paths are NOT covered by that source; they are
//! recorded from vendor documentation as data for later lanes, and each
//! target's `notes` says so. Nothing here is assumed to exist on disk.

use perkolator_core::{ConfigFormat, RuleFormat};

use crate::{AgentTarget, ConfigFileSpec, DirSpec, FileFormat, PathTemplate, TargetOrigin};

const VERIFIED: &str =
    "Skill dirs verified: vercel-labs/skills src/agents.ts @ d3701c1 (2026-11-06).";
const UNVERIFIED_EXTRAS: &str =
    "MCP/rule/profile/prompt paths are unverified data; confirm before writing.";

fn t(template: &str) -> PathTemplate {
    PathTemplate::new(template).expect("built-in template is valid")
}

fn dir(global: Option<&str>, project: Option<&str>) -> DirSpec {
    DirSpec {
        global: global.map(t),
        project: project.map(str::to_owned),
    }
}

fn config(
    global: Option<&str>,
    project: Option<&str>,
    format: ConfigFormat,
    key: &str,
) -> ConfigFileSpec {
    ConfigFileSpec {
        global: global.map(t),
        project: project.map(str::to_owned),
        format: FileFormat::Config(format),
        key: Some(key.to_owned()),
    }
}

fn rule(global: Option<&str>, project: Option<&str>, format: RuleFormat) -> ConfigFileSpec {
    ConfigFileSpec {
        global: global.map(t),
        project: project.map(str::to_owned),
        format: FileFormat::Rule(format),
        key: None,
    }
}

fn base(id: &str, display_name: &str, skills: DirSpec, detect: &[&str]) -> AgentTarget {
    AgentTarget {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        skills: Some(skills),
        mcp_configs: Vec::new(),
        rule_files: Vec::new(),
        agent_profiles: None,
        prompts: None,
        detect: detect.iter().map(|marker| t(marker)).collect(),
        supports_symlinks: true,
        enabled: true,
        notes: vec![VERIFIED.to_owned()],
        origin: TargetOrigin::Builtin,
    }
}

pub(crate) fn builtin_targets() -> Vec<AgentTarget> {
    let claude_home = "${CLAUDE_CONFIG_DIR:-~/.claude}";
    let codex_home = "${CODEX_HOME:-~/.codex}";

    let mut claude = base(
        "claude-code",
        "Claude Code",
        dir(
            Some(&format!("{claude_home}/skills")),
            Some(".claude/skills"),
        ),
        &[claude_home],
    );
    claude.mcp_configs = vec![config(
        Some("~/.claude.json"),
        Some(".mcp.json"),
        ConfigFormat::Json,
        "mcpServers",
    )];
    claude.rule_files = vec![rule(
        Some(&format!("{claude_home}/CLAUDE.md")),
        Some("CLAUDE.md"),
        RuleFormat::ClaudeMd,
    )];
    claude.agent_profiles = Some(dir(
        Some(&format!("{claude_home}/agents")),
        Some(".claude/agents"),
    ));
    claude.prompts = Some(dir(
        Some(&format!("{claude_home}/commands")),
        Some(".claude/commands"),
    ));
    claude.notes.push(UNVERIFIED_EXTRAS.to_owned());

    let mut codex = base(
        "codex",
        "Codex",
        dir(Some("~/.agents/skills"), Some(".agents/skills")),
        &[codex_home, "/etc/codex"],
    );
    codex.mcp_configs = vec![config(
        Some(&format!("{codex_home}/config.toml")),
        None,
        ConfigFormat::Toml,
        "mcp_servers",
    )];
    codex.rule_files = vec![rule(
        Some(&format!("{codex_home}/AGENTS.md")),
        Some("AGENTS.md"),
        RuleFormat::AgentsMd,
    )];
    codex.prompts = Some(dir(Some(&format!("{codex_home}/prompts")), None));
    codex.notes.push(UNVERIFIED_EXTRAS.to_owned());

    let mut opencode = base(
        "opencode",
        "OpenCode",
        dir(
            Some("$XDG_CONFIG_HOME/opencode/skills"),
            Some(".agents/skills"),
        ),
        &["$XDG_CONFIG_HOME/opencode"],
    );
    opencode.mcp_configs = vec![config(
        Some("$XDG_CONFIG_HOME/opencode/opencode.json"),
        Some("opencode.json"),
        ConfigFormat::Json,
        "mcp",
    )];
    opencode.rule_files = vec![rule(
        Some("$XDG_CONFIG_HOME/opencode/AGENTS.md"),
        Some("AGENTS.md"),
        RuleFormat::AgentsMd,
    )];
    opencode.notes.push(UNVERIFIED_EXTRAS.to_owned());

    let mut cursor = base(
        "cursor",
        "Cursor",
        dir(Some("~/.cursor/skills"), Some(".agents/skills")),
        &["~/.cursor"],
    );
    cursor.mcp_configs = vec![config(
        Some("~/.cursor/mcp.json"),
        Some(".cursor/mcp.json"),
        ConfigFormat::Json,
        "mcpServers",
    )];
    cursor.rule_files = vec![rule(None, Some("AGENTS.md"), RuleFormat::AgentsMd)];
    cursor.notes.push(UNVERIFIED_EXTRAS.to_owned());
    cursor.notes.push(
        "Project `.cursor/rules/*.mdc` is a directory of rules, handled by the rules lane."
            .to_owned(),
    );

    let mut gemini = base(
        "gemini-cli",
        "Gemini CLI",
        dir(Some("~/.gemini/skills"), Some(".agents/skills")),
        &["~/.gemini"],
    );
    gemini.mcp_configs = vec![config(
        Some("~/.gemini/settings.json"),
        Some(".gemini/settings.json"),
        ConfigFormat::Json,
        "mcpServers",
    )];
    gemini.rule_files = vec![rule(
        Some("~/.gemini/GEMINI.md"),
        Some("GEMINI.md"),
        RuleFormat::GeminiMd,
    )];
    gemini.notes.push(UNVERIFIED_EXTRAS.to_owned());

    let mut copilot = base(
        "github-copilot",
        "GitHub Copilot",
        dir(Some("~/.copilot/skills"), Some(".agents/skills")),
        &["~/.copilot"],
    );
    copilot.mcp_configs = vec![config(
        None,
        Some(".vscode/mcp.json"),
        ConfigFormat::Json,
        "servers",
    )];
    copilot.rule_files = vec![rule(
        None,
        Some(".github/copilot-instructions.md"),
        RuleFormat::CopilotInstructions,
    )];
    copilot.notes.push(UNVERIFIED_EXTRAS.to_owned());

    let mut universal = base(
        "universal",
        "Universal (.agents)",
        dir(Some("~/.agents/skills"), Some(".agents/skills")),
        &[],
    );
    universal.notes = vec![
        "Shared `.agents/skills` home read by Codex, Cline and others (vercel-labs/skills @ d3701c1)."
            .to_owned(),
        "vercel-labs/skills' own `universal` agent uses $XDG_CONFIG_HOME/agents/skills globally; \
         override via registry JSON if preferred."
            .to_owned(),
    ];

    let mut generic = base("generic", "Custom directory", dir(None, None), &[]);
    generic.notes = vec!["Deploys only to an explicit Scope::Custom directory.".to_owned()];

    vec![
        claude, codex, opencode, cursor, gemini, copilot, universal, generic,
    ]
}
