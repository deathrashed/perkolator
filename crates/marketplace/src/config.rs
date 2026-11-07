//! Source settings using the `[marketplaces]` schema from
//! `config/marketplace-providers.example.toml`. Settings hold no credentials.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use perkolator_core::{ProviderId, ResourceKind};
use serde::Deserialize;
use thiserror::Error;

/// Defaults ship with the repository's example declarations.
pub const DEFAULT_SOURCES_TOML: &str =
    include_str!("../../../config/marketplace-providers.example.toml");

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid marketplace settings: {0}")]
    Parse(String),
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    AllEnabled,
    SelectedOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSettings {
    pub enabled_for_search: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketplaceSettings {
    /// Master opt-in for any remote request. Off until the user turns it on,
    /// so installing Perkolator never causes surprise network traffic.
    pub remote_search_enabled: bool,
    pub mode: SearchMode,
    pub default_kinds: BTreeSet<ResourceKind>,
    pub local_index_always_enabled: bool,
    pub remote_timeout: Duration,
    pub max_concurrent_sources: usize,
    pub stale_cache_ok: bool,
    pub sources: BTreeMap<ProviderId, SourceSettings>,
}

#[derive(Deserialize)]
struct FileRoot {
    marketplaces: FileMarketplaces,
}

#[derive(Deserialize)]
struct FileMarketplaces {
    remote_search_enabled: Option<bool>,
    mode: Option<String>,
    default_resource_types: Option<Vec<String>>,
    local_index_always_enabled: Option<bool>,
    remote_timeout_ms: Option<u64>,
    max_concurrent_sources: Option<usize>,
    stale_cache_ok: Option<bool>,
    #[serde(default)]
    sources: BTreeMap<String, FileSource>,
}

/// Unknown per-source keys (transport, url, repo, cli) are descriptive and
/// ignored here: providers own their real endpoints.
#[derive(Deserialize)]
struct FileSource {
    enabled: Option<bool>,
}

impl MarketplaceSettings {
    pub fn defaults() -> Self {
        Self::from_toml(DEFAULT_SOURCES_TOML).expect("bundled marketplace defaults are valid")
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let root: FileRoot =
            toml::from_str(text).map_err(|error| ConfigError::Parse(error.to_string()))?;
        let file = root.marketplaces;
        let mode = match file.mode.as_deref() {
            None | Some("all-enabled") => SearchMode::AllEnabled,
            Some("selected-only") => SearchMode::SelectedOnly,
            Some(other) => return Err(ConfigError::Parse(format!("unknown mode `{other}`"))),
        };
        let default_kinds = match file.default_resource_types {
            None => ResourceKind::ALL.into_iter().collect(),
            Some(kinds) => kinds
                .iter()
                .map(|kind| {
                    kind.parse()
                        .map_err(|_| ConfigError::Parse(format!("unknown kind `{kind}`")))
                })
                .collect::<Result<_, _>>()?,
        };
        let mut sources = BTreeMap::new();
        for (id, source) in file.sources {
            let id = ProviderId::new(id).map_err(|error| ConfigError::Parse(error.to_string()))?;
            sources.insert(
                id,
                SourceSettings {
                    enabled_for_search: source.enabled.unwrap_or(false),
                },
            );
        }
        Ok(Self {
            remote_search_enabled: file.remote_search_enabled.unwrap_or(false),
            mode,
            default_kinds,
            local_index_always_enabled: file.local_index_always_enabled.unwrap_or(true),
            remote_timeout: Duration::from_millis(
                file.remote_timeout_ms.unwrap_or(6000).clamp(500, 60_000),
            ),
            max_concurrent_sources: file.max_concurrent_sources.unwrap_or(6).clamp(1, 16),
            stale_cache_ok: file.stale_cache_ok.unwrap_or(true),
            sources,
        })
    }

    /// Bundled defaults overlaid by the user's file when it exists.
    pub fn load(user_file: &Path) -> Result<Self, ConfigError> {
        let mut settings = Self::defaults();
        match std::fs::read_to_string(user_file) {
            Ok(text) => settings.overlay(&Self::from_toml(&text)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ConfigError::Io {
                    path: user_file.display().to_string(),
                    message: error.to_string(),
                });
            }
        }
        Ok(settings)
    }

    fn overlay(&mut self, user: &MarketplaceSettings) {
        self.remote_search_enabled = user.remote_search_enabled;
        self.mode = user.mode;
        self.default_kinds = user.default_kinds.clone();
        self.local_index_always_enabled = user.local_index_always_enabled;
        self.remote_timeout = user.remote_timeout;
        self.max_concurrent_sources = user.max_concurrent_sources;
        self.stale_cache_ok = user.stale_cache_ok;
        for (id, source) in &user.sources {
            self.sources.insert(id.clone(), source.clone());
        }
    }

    pub fn is_enabled(&self, id: &ProviderId) -> bool {
        self.sources
            .get(id)
            .is_some_and(|source| source.enabled_for_search)
    }

    pub fn set_enabled(&mut self, id: ProviderId, enabled: bool) {
        self.sources.insert(
            id,
            SourceSettings {
                enabled_for_search: enabled,
            },
        );
    }

    /// Serialise only user-controlled state (no endpoints, no secrets).
    pub fn to_toml(&self) -> String {
        let mut out = String::from(
            "# Perkolator marketplace source settings. No credentials here.\n[marketplaces]\n",
        );
        out.push_str(&format!(
            "remote_search_enabled = {}\n",
            self.remote_search_enabled
        ));
        out.push_str(&format!(
            "mode = \"{}\"\n",
            match self.mode {
                SearchMode::AllEnabled => "all-enabled",
                SearchMode::SelectedOnly => "selected-only",
            }
        ));
        let kinds: Vec<String> = self
            .default_kinds
            .iter()
            .map(|kind| format!("\"{}\"", kind.slug()))
            .collect();
        out.push_str(&format!(
            "default_resource_types = [{}]\n",
            kinds.join(", ")
        ));
        out.push_str(&format!(
            "local_index_always_enabled = {}\nremote_timeout_ms = {}\nmax_concurrent_sources = {}\nstale_cache_ok = {}\n",
            self.local_index_always_enabled,
            self.remote_timeout.as_millis(),
            self.max_concurrent_sources,
            self.stale_cache_ok
        ));
        for (id, source) in &self.sources {
            out.push_str(&format!(
                "\n[marketplaces.sources.{id}]\nenabled = {}\n",
                source.enabled_for_search
            ));
        }
        out
    }

    /// Atomic write (temp file + rename) of the user's settings file.
    pub fn save(&self, user_file: &Path) -> Result<(), ConfigError> {
        let io = |error: std::io::Error| ConfigError::Io {
            path: user_file.display().to_string(),
            message: error.to_string(),
        };
        if let Some(parent) = user_file.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let temp = user_file.with_extension("toml.tmp");
        std::fs::write(&temp, self.to_toml()).map_err(io)?;
        std::fs::rename(&temp, user_file).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> ProviderId {
        ProviderId::new(value).unwrap()
    }

    #[test]
    fn bundled_defaults_parse_with_opt_in_sources_disabled() {
        let settings = MarketplaceSettings::defaults();
        assert!(!settings.remote_search_enabled, "remote search is opt-in");
        assert_eq!(settings.sources.len(), 16);
        assert!(settings.is_enabled(&id("mcp-official")));
        assert!(!settings.is_enabled(&id("glama")), "glama is opt-in");
        assert!(!settings.is_enabled(&id("pulsemcp")), "pulsemcp is opt-in");
        assert!(!settings.is_enabled(&id("smithery")), "smithery search is down");
        assert!(!settings.is_enabled(&id("skillssh")), "skills.sh needs OIDC");
        assert_eq!(settings.default_kinds.len(), 6);
        assert_eq!(settings.remote_timeout, Duration::from_millis(6000));
    }

    #[test]
    fn toggles_persist_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("marketplaces.toml");
        let mut settings = MarketplaceSettings::load(&file).unwrap();
        settings.set_enabled(id("smithery"), false);
        settings.set_enabled(id("glama"), true);
        settings.remote_search_enabled = true;
        settings.save(&file).unwrap();
        let reloaded = MarketplaceSettings::load(&file).unwrap();
        assert!(reloaded.remote_search_enabled);
        assert!(!reloaded.is_enabled(&id("smithery")));
        assert!(reloaded.is_enabled(&id("glama")));
        assert!(reloaded.is_enabled(&id("mcp-official")));
        assert!(!std::fs::read_to_string(&file).unwrap().contains("url"));
    }

    #[test]
    fn rejects_bad_values() {
        assert!(MarketplaceSettings::from_toml("[marketplaces]\nmode = \"everything\"").is_err());
        assert!(
            MarketplaceSettings::from_toml("[marketplaces]\ndefault_resource_types = [\"model\"]")
                .is_err()
        );
    }
}
