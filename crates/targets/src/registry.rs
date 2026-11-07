use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{AgentTarget, TargetError, TargetOrigin, builtin::builtin_targets};

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// Versioned user registry file. Unknown fields are rejected so typos never
/// silently fall back to built-in paths.
///
/// ```json
/// { "version": 1, "targets": [ { "id": "my-agent", "display_name": "My Agent",
///   "skills": { "global": "~/.my-agent/skills" } } ], "disable": ["cursor"] }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryFile {
    pub version: u32,
    /// Additions, or full replacements of a built-in with the same id.
    #[serde(default)]
    pub targets: Vec<AgentTarget>,
    /// Ids to keep but mark disabled.
    #[serde(default)]
    pub disable: Vec<String>,
}

/// Ordered collection of targets; ids are unique.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetRegistry {
    targets: Vec<AgentTarget>,
}

impl TargetRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn builtin() -> Self {
        Self {
            targets: builtin_targets(),
        }
    }

    /// Built-ins with user overrides/additions from a versioned JSON document.
    pub fn from_json(json: &str) -> Result<Self, TargetError> {
        let mut registry = Self::builtin();
        registry.apply_json(json)?;
        Ok(registry)
    }

    /// Apply a registry file. All-or-nothing: on error `self` is unchanged.
    pub fn apply_json(&mut self, json: &str) -> Result<(), TargetError> {
        let file: RegistryFile =
            serde_json::from_str(json).map_err(|error| TargetError::Json(error.to_string()))?;
        if file.version != REGISTRY_SCHEMA_VERSION {
            return Err(TargetError::UnsupportedVersion {
                found: file.version,
                expected: REGISTRY_SCHEMA_VERSION,
            });
        }
        let mut seen = BTreeSet::new();
        for target in &file.targets {
            target.validate()?;
            if !seen.insert(target.id.as_str()) {
                return Err(TargetError::DuplicateId(target.id.clone()));
            }
        }
        let mut next = self.clone();
        for mut target in file.targets {
            target.origin = TargetOrigin::User;
            next.upsert(target);
        }
        for id in &file.disable {
            let target = next
                .targets
                .iter_mut()
                .find(|target| &target.id == id)
                .ok_or_else(|| TargetError::UnknownTarget(id.clone()))?;
            target.enabled = false;
        }
        *self = next;
        Ok(())
    }

    /// Insert or replace by id, preserving position for replacements.
    pub fn upsert(&mut self, target: AgentTarget) {
        match self
            .targets
            .iter_mut()
            .find(|existing| existing.id == target.id)
        {
            Some(existing) => *existing = target,
            None => self.targets.push(target),
        }
    }

    pub fn get(&self, id: &str) -> Option<&AgentTarget> {
        self.targets.iter().find(|target| target.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &AgentTarget> {
        self.targets.iter()
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use perkolator_core::Scope;

    use super::*;
    use crate::{HomeEnv, resolve_skill_dir};

    #[test]
    fn builtins_are_valid_and_unique() {
        let registry = TargetRegistry::builtin();
        let mut ids = BTreeSet::new();
        for target in registry.iter() {
            target.validate().unwrap();
            assert!(ids.insert(target.id.clone()), "duplicate {}", target.id);
        }
        for id in [
            "claude-code",
            "codex",
            "opencode",
            "cursor",
            "gemini-cli",
            "github-copilot",
            "universal",
            "generic",
        ] {
            assert!(registry.get(id).is_some(), "missing {id}");
        }
    }

    #[test]
    fn json_overrides_adds_and_disables() {
        let json = r#"{
            "version": 1,
            "targets": [
                { "id": "cursor", "display_name": "Cursor (custom)",
                  "skills": { "global": "~/alt/cursor-skills", "project": ".cursor/skills" } },
                { "id": "my-agent", "display_name": "My Agent",
                  "skills": { "global": "$XDG_CONFIG_HOME/my-agent/skills" },
                  "supports_symlinks": false }
            ],
            "disable": ["codex"]
        }"#;
        let registry = TargetRegistry::from_json(json).unwrap();
        let home = HomeEnv::new("/h");
        let cursor = registry.get("cursor").unwrap();
        assert_eq!(cursor.origin, TargetOrigin::User);
        assert_eq!(
            resolve_skill_dir(cursor, &Scope::Global, &home).unwrap(),
            Path::new("/h/alt/cursor-skills")
        );
        let mine = registry.get("my-agent").unwrap();
        assert!(!mine.supports_symlinks);
        assert_eq!(
            resolve_skill_dir(mine, &Scope::Global, &home).unwrap(),
            Path::new("/h/.config/my-agent/skills")
        );
        assert!(!registry.get("codex").unwrap().enabled);
        assert_eq!(registry.len(), TargetRegistry::builtin().len() + 1);
    }

    #[test]
    fn json_rejects_unknown_fields_bad_versions_and_unsafe_paths() {
        let cases = [
            r#"{ "version": 2, "targets": [] }"#,
            r#"{ "version": 1, "targets": [], "extra": true }"#,
            r#"{ "version": 1, "targets": [ { "id": "x", "display_name": "X", "skils": {} } ] }"#,
            r#"{ "version": 1, "targets": [ { "id": "Bad Id", "display_name": "X" } ] }"#,
            r#"{ "version": 1, "targets": [ { "id": "x", "display_name": "X", "skills": { "global": "~/../etc" } } ] }"#,
            r#"{ "version": 1, "targets": [ { "id": "x", "display_name": "X", "skills": { "project": "../up" } } ] }"#,
            r#"{ "version": 1, "targets": [ { "id": "x", "display_name": "X" }, { "id": "x", "display_name": "Y" } ] }"#,
            r#"{ "version": 1, "disable": ["nope"] }"#,
        ];
        for json in cases {
            assert!(TargetRegistry::from_json(json).is_err(), "accepted {json}");
        }
    }

    #[test]
    fn failed_apply_leaves_registry_unchanged() {
        let mut registry = TargetRegistry::builtin();
        let before = registry.clone();
        let json = r#"{ "version": 1, "targets": [ { "id": "new", "display_name": "N" } ], "disable": ["nope"] }"#;
        assert!(registry.apply_json(json).is_err());
        assert_eq!(registry, before);
    }
}
