//! Data-driven agent host target registry.
//!
//! An [`AgentTarget`] describes where a host application (Claude Code, Codex,
//! Cursor, ...) reads skills, MCP configuration, rules, agent profiles and
//! prompts. Every path is configuration expressed as a [`PathTemplate`] and is
//! never assumed to exist; resolution takes an explicit [`HomeEnv`] so tests
//! and previews never read the real environment.
//!
//! Built-in skill directories were verified against vercel-labs/skills
//! `src/agents.ts` at commit `d3701c11b1101251af14c9d148f026e1a47ebe17`
//! (2026-11-06). Non-skill paths are marked unverified in each target's notes.

mod builtin;
mod path_template;
mod registry;

use std::path::{Path, PathBuf};

use perkolator_core::{ConfigFormat, RelPath, RuleFormat, Scope};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use path_template::{HomeEnv, KNOWN_HOME_VARS, PathTemplate, is_clean_absolute};
pub use registry::{REGISTRY_SCHEMA_VERSION, RegistryFile, TargetRegistry};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TargetError {
    #[error("invalid path template `{template}`: {reason}")]
    InvalidTemplate {
        template: String,
        reason: &'static str,
    },
    #[error("variable `{name}` used by `{template}` is not set")]
    UnsetVariable { name: String, template: String },
    #[error("target `{target}` has no {scope} skills directory")]
    UnsupportedScope { target: String, scope: &'static str },
    #[error("invalid project-relative directory `{0}`")]
    InvalidProjectDir(String),
    #[error("path `{0}` must be absolute and must not contain `.` or `..`")]
    NotCleanAbsolute(PathBuf),
    #[error("invalid target id `{0}`: use lowercase ASCII letters, digits, `-`, `_` or `.`")]
    InvalidId(String),
    #[error("duplicate target id `{0}` in registry file")]
    DuplicateId(String),
    #[error("unknown target `{0}`")]
    UnknownTarget(String),
    #[error("unsupported registry schema version {found} (expected {expected})")]
    UnsupportedVersion { found: u32, expected: u32 },
    #[error("invalid registry JSON: {0}")]
    Json(String),
}

/// A directory a host reads at global scope and/or relative to a project root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DirSpec {
    #[serde(default)]
    pub global: Option<PathTemplate>,
    /// Relative to the project root, e.g. `.claude/skills`. Validated as a
    /// [`RelPath`] at registry load.
    #[serde(default)]
    pub project: Option<String>,
}

pub type SkillDirSpec = DirSpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileFormat {
    Config(ConfigFormat),
    Rule(RuleFormat),
}

/// A configuration or rule file a host reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFileSpec {
    #[serde(default)]
    pub global: Option<PathTemplate>,
    #[serde(default)]
    pub project: Option<String>,
    pub format: FileFormat,
    /// Top-level key holding entries (e.g. `mcpServers`), when applicable.
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TargetOrigin {
    #[default]
    Builtin,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTarget {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub skills: Option<SkillDirSpec>,
    #[serde(default)]
    pub mcp_configs: Vec<ConfigFileSpec>,
    #[serde(default)]
    pub rule_files: Vec<ConfigFileSpec>,
    #[serde(default)]
    pub agent_profiles: Option<DirSpec>,
    #[serde(default)]
    pub prompts: Option<DirSpec>,
    /// Marker paths whose existence means the host is installed (read-only probe).
    #[serde(default)]
    pub detect: Vec<PathTemplate>,
    #[serde(default = "default_true")]
    pub supports_symlinks: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing)]
    pub origin: TargetOrigin,
}

fn default_true() -> bool {
    true
}

impl AgentTarget {
    /// Structural validation run for built-ins (tests) and every JSON load.
    pub fn validate(&self) -> Result<(), TargetError> {
        validate_id(&self.id)?;
        let dirs = [&self.skills, &self.agent_profiles, &self.prompts];
        let project_dirs = dirs
            .into_iter()
            .flatten()
            .filter_map(|spec| spec.project.as_deref())
            .chain(
                self.mcp_configs
                    .iter()
                    .chain(&self.rule_files)
                    .filter_map(|spec| spec.project.as_deref()),
            );
        for dir in project_dirs {
            RelPath::new(dir).map_err(|_| TargetError::InvalidProjectDir(dir.to_owned()))?;
        }
        Ok(())
    }
}

pub fn validate_id(id: &str) -> Result<(), TargetError> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.')
        });
    if valid {
        Ok(())
    } else {
        Err(TargetError::InvalidId(id.to_owned()))
    }
}

fn scope_label(scope: &Scope) -> &'static str {
    match scope {
        Scope::Global => "global",
        Scope::Project { .. } => "project",
        Scope::Custom { .. } => "custom",
    }
}

/// Resolve the skills directory `target` reads for `scope`.
///
/// - `Global`: the target's global template resolved against `home`.
/// - `Project { root }`: `root` (absolute, clean) joined with the target's
///   project-relative directory.
/// - `Custom { path }`: `path` itself (absolute, clean); any target that
///   supports skills accepts an explicit custom directory.
///
/// Purely lexical: nothing is read from disk.
pub fn resolve_skill_dir(
    target: &AgentTarget,
    scope: &Scope,
    home: &HomeEnv,
) -> Result<PathBuf, TargetError> {
    let unsupported = || TargetError::UnsupportedScope {
        target: target.id.clone(),
        scope: scope_label(scope),
    };
    let spec = target.skills.as_ref().ok_or_else(unsupported)?;
    match scope {
        Scope::Global => spec.global.as_ref().ok_or_else(unsupported)?.resolve(home),
        Scope::Project { root } => {
            let relative = spec.project.as_deref().ok_or_else(unsupported)?;
            ensure_clean_absolute(root)?;
            let relative = RelPath::new(relative)
                .map_err(|_| TargetError::InvalidProjectDir(relative.to_owned()))?;
            Ok(relative.to_path(root))
        }
        Scope::Custom { path } => {
            ensure_clean_absolute(path)?;
            Ok(path.clone())
        }
    }
}

fn ensure_clean_absolute(path: &Path) -> Result<(), TargetError> {
    if is_clean_absolute(path) {
        Ok(())
    } else {
        Err(TargetError::NotCleanAbsolute(path.to_path_buf()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetStatus {
    /// A detection marker exists.
    Detected,
    /// No marker, but the user configured this target or its global skills
    /// directory already exists.
    Configured,
    Missing,
    Disabled,
}

/// Read-only probe: only `symlink_metadata` existence checks, never creates
/// or follows anything beyond the marker path itself.
pub fn detect(target: &AgentTarget, home: &HomeEnv) -> TargetStatus {
    if !target.enabled {
        return TargetStatus::Disabled;
    }
    let exists = |path: PathBuf| std::fs::symlink_metadata(path).is_ok();
    if target
        .detect
        .iter()
        .filter_map(|marker| marker.resolve(home).ok())
        .any(exists)
    {
        return TargetStatus::Detected;
    }
    let global_skills_exist = resolve_skill_dir(target, &Scope::Global, home)
        .map(exists)
        .unwrap_or(false);
    if target.origin == TargetOrigin::User || global_skills_exist {
        TargetStatus::Configured
    } else {
        TargetStatus::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> HomeEnv {
        HomeEnv::new("/fake/home")
    }

    #[test]
    fn resolves_global_project_custom_for_several_agents() {
        let registry = TargetRegistry::builtin();
        let project = Scope::Project {
            root: PathBuf::from("/work/repo"),
        };
        let cases = [
            (
                "claude-code",
                "/fake/home/.claude/skills",
                "/work/repo/.claude/skills",
            ),
            (
                "codex",
                "/fake/home/.agents/skills",
                "/work/repo/.agents/skills",
            ),
            (
                "cursor",
                "/fake/home/.cursor/skills",
                "/work/repo/.agents/skills",
            ),
            (
                "opencode",
                "/fake/home/.config/opencode/skills",
                "/work/repo/.agents/skills",
            ),
            (
                "gemini-cli",
                "/fake/home/.gemini/skills",
                "/work/repo/.agents/skills",
            ),
            (
                "universal",
                "/fake/home/.agents/skills",
                "/work/repo/.agents/skills",
            ),
        ];
        for (id, global, proj) in cases {
            let target = registry.get(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(
                resolve_skill_dir(target, &Scope::Global, &home()).unwrap(),
                Path::new(global),
                "{id} global"
            );
            assert_eq!(
                resolve_skill_dir(target, &project, &home()).unwrap(),
                Path::new(proj),
                "{id} project"
            );
            let custom = Scope::Custom {
                path: PathBuf::from("/custom/skills"),
            };
            assert_eq!(
                resolve_skill_dir(target, &custom, &home()).unwrap(),
                Path::new("/custom/skills")
            );
        }
    }

    #[test]
    fn claude_honours_config_dir_override() {
        let registry = TargetRegistry::builtin();
        let env = home().with_var("CLAUDE_CONFIG_DIR", "/alt/claude");
        let target = registry.get("claude-code").unwrap();
        assert_eq!(
            resolve_skill_dir(target, &Scope::Global, &env).unwrap(),
            Path::new("/alt/claude/skills")
        );
    }

    #[test]
    fn generic_target_only_supports_custom() {
        let registry = TargetRegistry::builtin();
        let generic = registry.get("generic").unwrap();
        assert!(matches!(
            resolve_skill_dir(generic, &Scope::Global, &home()),
            Err(TargetError::UnsupportedScope { .. })
        ));
        assert!(
            resolve_skill_dir(
                generic,
                &Scope::Custom {
                    path: PathBuf::from("/x/skills")
                },
                &home()
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_relative_or_dotted_roots() {
        let registry = TargetRegistry::builtin();
        let target = registry.get("claude-code").unwrap();
        for scope in [
            Scope::Project {
                root: PathBuf::from("relative"),
            },
            Scope::Project {
                root: PathBuf::from("/a/../b"),
            },
            Scope::Custom {
                path: PathBuf::from("rel/skills"),
            },
        ] {
            assert!(matches!(
                resolve_skill_dir(target, &scope, &home()),
                Err(TargetError::NotCleanAbsolute(_))
            ));
        }
    }

    #[test]
    fn detect_is_read_only_and_uses_fake_home() {
        let dir = tempfile::tempdir().unwrap();
        let env = HomeEnv::new(dir.path());
        let registry = TargetRegistry::builtin();
        let cursor = registry.get("cursor").unwrap();
        assert_eq!(detect(cursor, &env), TargetStatus::Missing);
        std::fs::create_dir(dir.path().join(".cursor")).unwrap();
        assert_eq!(detect(cursor, &env), TargetStatus::Detected);
        let gemini = registry.get("gemini-cli").unwrap();
        std::fs::create_dir_all(dir.path().join(".gemini/skills")).unwrap();
        assert_eq!(detect(gemini, &env), TargetStatus::Detected);
        let mut disabled = cursor.clone();
        disabled.enabled = false;
        assert_eq!(detect(&disabled, &env), TargetStatus::Disabled);
        // Nothing beyond what the test created.
        let mut names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, [".cursor", ".gemini"]);
    }

    #[test]
    fn configured_when_skills_dir_exists_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let env = HomeEnv::new(dir.path());
        let mut target = TargetRegistry::builtin().get("cursor").unwrap().clone();
        target.detect.clear();
        std::fs::create_dir_all(dir.path().join(".cursor/skills")).unwrap();
        assert_eq!(detect(&target, &env), TargetStatus::Configured);
    }
}
