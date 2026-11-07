use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{McpTransport, ResourceDetails, TemplateSyntax};

/// What a specific resource genuinely supports. Actions are offered only
/// when their required capabilities are present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// Has a real local filesystem path (enables copy path / reveal).
    LocalPath,
    /// Has a verified upstream URL.
    SourceUrl,
    /// Primary document is Markdown that can be rendered.
    PreviewMarkdown,
    /// Has a browsable file tree (references/scripts/assets).
    PreviewFiles,
    /// Has a configuration entry to show (redacted).
    PreviewConfig,
    /// Declares tools with schemas.
    PreviewTools,
    /// Template with variables to expand.
    PreviewTemplate,
    /// Has scope/precedence information.
    PreviewPrecedence,
    /// Declares permissions/executable content.
    PreviewPermissions,
    /// Text content can be copied.
    CopyContent,
    /// A configuration snippet can be copied.
    CopyConfig,
    /// Can be referenced in a composed query to an agent.
    Compose,
    /// Template variables can be expanded offline.
    ExpandVariables,
    /// Directory can be deployed to agent targets via symlink/copy.
    DeployFiles,
    /// Can be written into a host client's configuration.
    ConfigureClient,
    /// Has a reversible enable/disable state in its host.
    ToggleEnabled,
    /// Contains code that could run; needs explicit trust before execution.
    ExecutesCode,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, capability: Capability) -> Self {
        self.0.insert(capability);
        self
    }

    pub fn insert(&mut self, capability: Capability) {
        self.0.insert(capability);
    }

    pub fn contains(&self, capability: Capability) -> bool {
        self.0.contains(&capability)
    }

    pub fn contains_all(&self, capabilities: &[Capability]) -> bool {
        capabilities.iter().all(|capability| self.contains(*capability))
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    /// Capabilities that follow from the typed details alone. Providers add
    /// `SourceUrl` etc. only when they know them to be true.
    pub fn for_details(details: &ResourceDetails) -> Self {
        use Capability::*;
        let mut set = CapabilitySet::new().with(LocalPath).with(CopyContent);
        match details {
            ResourceDetails::Skill(_) => {
                set.insert(PreviewMarkdown);
                set.insert(PreviewFiles);
                set.insert(Compose);
                set.insert(DeployFiles);
            }
            ResourceDetails::McpServer(mcp) => {
                set.insert(PreviewConfig);
                set.insert(CopyConfig);
                set.insert(ConfigureClient);
                if mcp.enabled.is_some() {
                    set.insert(ToggleEnabled);
                }
                if !mcp.declared_tools.is_empty() {
                    set.insert(PreviewTools);
                }
                if matches!(mcp.transport, McpTransport::Stdio { .. }) {
                    set.insert(ExecutesCode);
                }
            }
            ResourceDetails::Agent(_) => {
                set.insert(PreviewMarkdown);
                set.insert(Compose);
            }
            ResourceDetails::Prompt(prompt) => {
                set.insert(PreviewMarkdown);
                set.insert(PreviewTemplate);
                set.insert(Compose);
                if prompt.syntax != TemplateSyntax::Plain {
                    set.insert(ExpandVariables);
                }
            }
            ResourceDetails::Rule(rule) => {
                set.insert(PreviewMarkdown);
                set.insert(PreviewPrecedence);
                if rule.always_apply.is_some() {
                    set.insert(ToggleEnabled);
                }
            }
            ResourceDetails::Plugin(plugin) => {
                set.insert(PreviewFiles);
                set.insert(PreviewPermissions);
                if !plugin.executable_files.is_empty() {
                    set.insert(ExecutesCode);
                }
            }
        }
        set
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{ConfigFormat, McpServerDetails, PromptTemplateDetails};

    #[test]
    fn mcp_entry_is_not_deployable_as_files() {
        let details = ResourceDetails::McpServer(McpServerDetails {
            client: "cursor".into(),
            config_file: PathBuf::from("/tmp/mcp.json"),
            config_format: ConfigFormat::Json,
            entry: "github".into(),
            transport: McpTransport::StreamableHttp {
                url: "https://example.invalid/mcp".into(),
                headers: vec![],
            },
            enabled: None,
            declared_tools: vec![],
        });
        let caps = CapabilitySet::for_details(&details);
        assert!(caps.contains(Capability::CopyConfig));
        assert!(!caps.contains(Capability::DeployFiles));
        assert!(!caps.contains(Capability::ToggleEnabled));
        assert!(!caps.contains(Capability::ExecutesCode));
    }

    #[test]
    fn plain_prompt_does_not_expand() {
        let details = ResourceDetails::Prompt(PromptTemplateDetails {
            file: PathBuf::from("/tmp/p.md"),
            host: None,
            syntax: TemplateSyntax::Plain,
            variables: vec![],
        });
        let caps = CapabilitySet::for_details(&details);
        assert!(caps.contains(Capability::Compose));
        assert!(!caps.contains(Capability::ExpandVariables));
    }
}
