use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{CoreError, ResourceKind};

/// Identifies a source adapter or host namespace, e.g. `library`,
/// `claude-code`, `cursor`, `github`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            });
        if valid {
            Ok(Self(value))
        } else {
            Err(CoreError::InvalidProviderId(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProviderId {
    type Error = CoreError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProviderId> for String {
    fn from(value: ProviderId) -> Self {
        value.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable identity of a resource: kind + provider + upstream canonical key,
/// plus an optional variant (branch, version, host-specific copy).
///
/// For local files the key is the canonical (symlink-resolved) absolute path,
/// so a library skill and its symlinked agent deployments share one identity,
/// while an independent copy inside an agent directory is a distinct resource.
/// Two resources with the same display name never collapse into one ID.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceId {
    pub kind: ResourceKind,
    pub provider: ProviderId,
    pub key: String,
    pub variant: Option<String>,
}

impl ResourceId {
    pub fn new(kind: ResourceKind, provider: ProviderId, key: impl Into<String>) -> Self {
        Self {
            kind,
            provider,
            key: key.into(),
            variant: None,
        }
    }

    pub fn with_variant(mut self, variant: impl Into<String>) -> Self {
        self.variant = Some(variant.into());
        self
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.kind, self.provider, self.key)?;
        if let Some(variant) = &self.variant {
            write!(f, "#{variant}")?;
        }
        Ok(())
    }
}

impl FromStr for ResourceId {
    type Err = CoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || CoreError::InvalidResourceId(value.to_owned());
        let mut parts = value.splitn(3, ':');
        let kind = parts.next().ok_or_else(invalid)?.parse()?;
        let provider = ProviderId::new(parts.next().ok_or_else(invalid)?)?;
        let rest = parts.next().filter(|rest| !rest.is_empty()).ok_or_else(invalid)?;
        let (key, variant) = match rest.rsplit_once('#') {
            Some((key, variant)) if !key.is_empty() && !variant.is_empty() => {
                (key.to_owned(), Some(variant.to_owned()))
            }
            _ => (rest.to_owned(), None),
        };
        Ok(Self {
            kind,
            provider,
            key,
            variant,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(value: &str) -> ProviderId {
        ProviderId::new(value).unwrap()
    }

    #[test]
    fn id_round_trips_through_display() {
        let id = ResourceId::new(
            ResourceKind::Skill,
            provider("library"),
            "/home/u/Skills/dev/frontend-design",
        )
        .with_variant("main");
        let parsed: ResourceId = id.to_string().parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn keys_may_contain_colons() {
        let parsed: ResourceId = "mcp:cursor:/home/u/.cursor/mcp.json:github".parse().unwrap();
        assert_eq!(parsed.key, "/home/u/.cursor/mcp.json:github");
        assert_eq!(parsed.kind, ResourceKind::McpServer);
    }

    #[test]
    fn same_name_different_provider_is_distinct() {
        let a = ResourceId::new(ResourceKind::Skill, provider("library"), "pdf");
        let b = ResourceId::new(ResourceKind::Skill, provider("claude-code"), "pdf");
        assert_ne!(a, b);
    }

    #[test]
    fn provider_rejects_uppercase_and_spaces() {
        assert!(ProviderId::new("Claude Code").is_err());
        assert!(ProviderId::new("").is_err());
        assert!(ProviderId::new("gemini-cli").is_ok());
    }

    #[test]
    fn malformed_ids_are_rejected() {
        assert!("skill:library".parse::<ResourceId>().is_err());
        assert!("skill:library:".parse::<ResourceId>().is_err());
        assert!("widget:library:x".parse::<ResourceId>().is_err());
    }
}
