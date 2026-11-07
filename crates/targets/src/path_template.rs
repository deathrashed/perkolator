use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use perkolator_core::RelPath;
use serde::{Deserialize, Serialize};

use crate::TargetError;

/// The environment a [`PathTemplate`] is resolved against.
///
/// Resolution never reads process-global state: callers build a `HomeEnv`
/// explicitly (tests inject a fake home, the app calls
/// [`HomeEnv::from_process_env`] once at startup).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeEnv {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`; `None` means the XDG default `~/.config`.
    pub xdg_config_home: Option<PathBuf>,
    /// Extra named variables usable as `${NAME}` / `${NAME:-fallback}`.
    /// Only absolute paths are honoured; empty or relative values are ignored
    /// exactly like an unset variable.
    pub vars: BTreeMap<String, PathBuf>,
}

/// Environment variables that agent hosts document as overriding their home.
/// Mirrors vercel-labs/skills `src/agents.ts` (commit d3701c1, 2026-11-06).
pub const KNOWN_HOME_VARS: [&str; 3] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "COPILOT_HOME"];

impl HomeEnv {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            xdg_config_home: None,
            vars: BTreeMap::new(),
        }
    }

    pub fn with_xdg_config_home(mut self, path: impl Into<PathBuf>) -> Self {
        self.xdg_config_home = Some(path.into());
        self
    }

    pub fn with_var(mut self, name: impl Into<String>, value: impl Into<PathBuf>) -> Self {
        self.vars.insert(name.into(), value.into());
        self
    }

    /// Snapshot the real process environment. The only place this crate reads
    /// global state; never call it from tests.
    pub fn from_process_env() -> Option<Self> {
        let mut env = Self::new(dirs::home_dir()?);
        env.xdg_config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        for name in KNOWN_HOME_VARS {
            if let Some(value) = std::env::var_os(name).map(PathBuf::from) {
                env.vars.insert(name.to_owned(), value);
            }
        }
        Some(env)
    }

    pub fn config_home(&self) -> PathBuf {
        match &self.xdg_config_home {
            Some(path) if path.is_absolute() => path.clone(),
            _ => self.home.join(".config"),
        }
    }

    fn var(&self, name: &str) -> Option<PathBuf> {
        match name {
            "HOME" => Some(self.home.clone()),
            "XDG_CONFIG_HOME" => Some(self.config_home()),
            _ => self
                .vars
                .get(name)
                .filter(|path| path.is_absolute())
                .cloned(),
        }
    }
}

/// A host path expressed as data, e.g. `~/.cursor/skills`,
/// `$XDG_CONFIG_HOME/opencode/skills` or `${CLAUDE_CONFIG_DIR:-~/.claude}/skills`.
///
/// Grammar: `<base>[/<relative>]` where `<base>` is `~`, `$HOME`,
/// `$XDG_CONFIG_HOME`, `${NAME}`, `${NAME:-<base>}` or an absolute path, and
/// `<relative>` is a [`RelPath`] (no `..`). Paths are configuration, never
/// assumed to exist or be valid on the current machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PathTemplate(String);

impl PathTemplate {
    pub fn new(template: impl Into<String>) -> Result<Self, TargetError> {
        let template = template.into();
        parse(&template)?;
        Ok(Self(template))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn resolve(&self, env: &HomeEnv) -> Result<PathBuf, TargetError> {
        let parsed = parse(&self.0)?;
        let base = resolve_base(&parsed.base, env, &self.0)?;
        Ok(join_rest(base, &parsed.rest))
    }
}

impl TryFrom<String> for PathTemplate {
    type Error = TargetError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PathTemplate> for String {
    fn from(value: PathTemplate) -> Self {
        value.0
    }
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug)]
enum Base {
    Home,
    Var {
        name: String,
        fallback: Option<Box<(Base, Option<RelPath>)>>,
    },
    Absolute(PathBuf),
}

struct Parsed {
    base: Base,
    rest: Option<RelPath>,
}

fn invalid(template: &str, reason: &'static str) -> TargetError {
    TargetError::InvalidTemplate {
        template: template.to_owned(),
        reason,
    }
}

fn parse(template: &str) -> Result<Parsed, TargetError> {
    if template.contains('\0') {
        return Err(invalid(template, "contains NUL byte"));
    }
    let (base, rest) = parse_base(template, template)?;
    let rest = parse_rest(rest, template)?;
    Ok(Parsed { base, rest })
}

fn parse_rest(rest: &str, template: &str) -> Result<Option<RelPath>, TargetError> {
    match rest.trim_start_matches('/') {
        "" => Ok(None),
        rest => RelPath::new(rest)
            .map(Some)
            .map_err(|_| invalid(template, "unsafe relative part")),
    }
}

fn join_rest(base: PathBuf, rest: &Option<RelPath>) -> PathBuf {
    match rest {
        Some(rest) => rest.to_path(&base),
        None => base,
    }
}

/// Returns the base and the unparsed remainder (starting with `/` or empty).
fn parse_base<'a>(input: &'a str, template: &str) -> Result<(Base, &'a str), TargetError> {
    let ensure_boundary = |rest: &'a str| {
        if rest.is_empty() || rest.starts_with('/') {
            Ok(rest)
        } else {
            Err(invalid(template, "base must be followed by `/`"))
        }
    };
    if let Some(rest) = input.strip_prefix('~') {
        return Ok((Base::Home, ensure_boundary(rest)?));
    }
    if let Some(braced) = input.strip_prefix("${") {
        let close =
            find_matching_brace(braced).ok_or_else(|| invalid(template, "unclosed `${`"))?;
        let inner = &braced[..close];
        let rest = ensure_boundary(&braced[close + 1..])?;
        let (name, fallback) = match inner.split_once(":-") {
            Some((name, fallback)) => {
                let (fallback_base, fallback_rest) = parse_base(fallback, template)?;
                let fallback_rest = parse_rest(fallback_rest, template)?;
                (name, Some(Box::new((fallback_base, fallback_rest))))
            }
            None => (inner, None),
        };
        validate_var_name(name, template)?;
        return Ok((
            Base::Var {
                name: name.to_owned(),
                fallback,
            },
            rest,
        ));
    }
    if let Some(after) = input.strip_prefix('$') {
        let end = after.find('/').unwrap_or(after.len());
        let name = &after[..end];
        validate_var_name(name, template)?;
        return Ok((
            Base::Var {
                name: name.to_owned(),
                fallback: None,
            },
            &after[end..],
        ));
    }
    if input.starts_with('/') {
        // The whole string is an absolute path; still forbid `..`.
        let relative = input.trim_start_matches('/');
        if !relative.is_empty() {
            RelPath::new(relative).map_err(|_| invalid(template, "unsafe absolute path"))?;
        }
        return Ok((Base::Absolute(PathBuf::from("/")), input));
    }
    Err(invalid(
        template,
        "must start with `~`, `$VAR`, `${VAR}` or `/`",
    ))
}

fn find_matching_brace(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(index),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn validate_var_name(name: &str, template: &str) -> Result<(), TargetError> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
    if valid {
        Ok(())
    } else {
        Err(invalid(template, "variable names are UPPER_SNAKE_CASE"))
    }
}

fn resolve_base(base: &Base, env: &HomeEnv, template: &str) -> Result<PathBuf, TargetError> {
    match base {
        Base::Home => Ok(env.home.clone()),
        Base::Absolute(root) => Ok(root.clone()),
        Base::Var { name, fallback } => match (env.var(name), fallback) {
            (Some(value), _) => Ok(value),
            (None, Some(fallback)) => {
                let (fallback_base, fallback_rest) = fallback.as_ref();
                Ok(join_rest(
                    resolve_base(fallback_base, env, template)?,
                    fallback_rest,
                ))
            }
            (None, None) => Err(TargetError::UnsetVariable {
                name: name.clone(),
                template: template.to_owned(),
            }),
        },
    }
}

/// `true` when `path` is absolute and contains no `..` / `.` components.
pub fn is_clean_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> HomeEnv {
        HomeEnv::new("/fake/home")
    }

    #[test]
    fn resolves_home_and_xdg() {
        let t = PathTemplate::new("~/.cursor/skills").unwrap();
        assert_eq!(
            t.resolve(&env()).unwrap(),
            Path::new("/fake/home/.cursor/skills")
        );
        let t = PathTemplate::new("$XDG_CONFIG_HOME/opencode/skills").unwrap();
        assert_eq!(
            t.resolve(&env()).unwrap(),
            Path::new("/fake/home/.config/opencode/skills")
        );
        let custom = env().with_xdg_config_home("/xdg");
        assert_eq!(
            t.resolve(&custom).unwrap(),
            Path::new("/xdg/opencode/skills")
        );
    }

    #[test]
    fn variable_with_fallback() {
        let t = PathTemplate::new("${CLAUDE_CONFIG_DIR:-~/.claude}/skills").unwrap();
        assert_eq!(
            t.resolve(&env()).unwrap(),
            Path::new("/fake/home/.claude/skills")
        );
        let overridden = env().with_var("CLAUDE_CONFIG_DIR", "/alt/claude");
        assert_eq!(
            t.resolve(&overridden).unwrap(),
            Path::new("/alt/claude/skills")
        );
        let relative = env().with_var("CLAUDE_CONFIG_DIR", "relative");
        assert_eq!(
            t.resolve(&relative).unwrap(),
            Path::new("/fake/home/.claude/skills")
        );
    }

    #[test]
    fn unset_variable_without_fallback_errors() {
        let t = PathTemplate::new("${NOPE}/skills").unwrap();
        assert!(matches!(
            t.resolve(&env()),
            Err(TargetError::UnsetVariable { .. })
        ));
    }

    #[test]
    fn rejects_unsafe_templates() {
        for bad in [
            "",
            "relative/path",
            "~/../etc",
            "~foo/x",
            "/abs/../x",
            "${lower}/x",
            "${X:-~/a}/b/../c",
            "${X",
        ] {
            assert!(PathTemplate::new(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn absolute_template() {
        let t = PathTemplate::new("/etc/codex").unwrap();
        assert_eq!(t.resolve(&env()).unwrap(), Path::new("/etc/codex"));
    }

    #[test]
    fn clean_absolute() {
        assert!(is_clean_absolute(Path::new("/a/b")));
        assert!(!is_clean_absolute(Path::new("a/b")));
        assert!(!is_clean_absolute(Path::new("/a/../b")));
    }
}
