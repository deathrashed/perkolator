use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// The six mandatory first-class resource classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Skill,
    McpServer,
    Agent,
    Prompt,
    Rule,
    Plugin,
}

impl ResourceKind {
    pub const ALL: [ResourceKind; 6] = [
        ResourceKind::Skill,
        ResourceKind::McpServer,
        ResourceKind::Agent,
        ResourceKind::Prompt,
        ResourceKind::Rule,
        ResourceKind::Plugin,
    ];

    /// Stable machine identifier used in IDs, query prefixes and storage.
    pub fn slug(self) -> &'static str {
        match self {
            ResourceKind::Skill => "skill",
            ResourceKind::McpServer => "mcp",
            ResourceKind::Agent => "agent",
            ResourceKind::Prompt => "prompt",
            ResourceKind::Rule => "rule",
            ResourceKind::Plugin => "plugin",
        }
    }

    /// Singular human label.
    pub fn label(self) -> &'static str {
        match self {
            ResourceKind::Skill => "Skill",
            ResourceKind::McpServer => "MCP Server",
            ResourceKind::Agent => "Agent",
            ResourceKind::Prompt => "Prompt",
            ResourceKind::Rule => "Rule",
            ResourceKind::Plugin => "Plugin",
        }
    }

    /// Plural label used for navigation sections and filters.
    pub fn plural_label(self) -> &'static str {
        match self {
            ResourceKind::Skill => "Skills",
            ResourceKind::McpServer => "MCP Servers",
            ResourceKind::Agent => "Agents",
            ResourceKind::Prompt => "Prompts",
            ResourceKind::Rule => "Rules",
            ResourceKind::Plugin => "Plugins",
        }
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

impl FromStr for ResourceKind {
    type Err = CoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "skill" | "skills" => Ok(ResourceKind::Skill),
            "mcp" | "mcp-server" | "mcps" => Ok(ResourceKind::McpServer),
            "agent" | "agents" => Ok(ResourceKind::Agent),
            "prompt" | "prompts" => Ok(ResourceKind::Prompt),
            "rule" | "rules" => Ok(ResourceKind::Rule),
            "plugin" | "plugins" => Ok(ResourceKind::Plugin),
            other => Err(CoreError::UnknownKind(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_round_trips_for_every_kind() {
        for kind in ResourceKind::ALL {
            assert_eq!(kind.slug().parse::<ResourceKind>().unwrap(), kind);
        }
    }

    #[test]
    fn unknown_kind_is_rejected() {
        assert!("model".parse::<ResourceKind>().is_err());
    }
}
