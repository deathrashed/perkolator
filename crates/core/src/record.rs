use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{CapabilitySet, ResourceDetails, ResourceId, ResourceKind};

/// Where a deployment or rule applies.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "kebab-case")]
pub enum Scope {
    Global,
    Project { root: PathBuf },
    Custom { path: PathBuf },
}

/// How Perkolator learned about a resource. Labels must be truthful.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum SourceKind {
    /// A user library root such as `~/Skills`.
    Library { root: PathBuf },
    /// Found in a host application's own directories/config.
    AgentHost { host: String, scope: Scope },
    /// Found inside a project checkout.
    Project { root: PathBuf },
    /// Remote catalogue result (not local).
    Remote { provider: String, url: String },
    /// Cached copy of a remote result.
    Cache { provider: String, url: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: SourceKind,
    /// Upstream URL (repository, registry page) when actually known.
    pub origin_url: Option<String>,
    /// Content fingerprint (blake3 hex) when computed.
    pub fingerprint: Option<String>,
}

/// Distinct lifecycle states; these are deliberately not interchangeable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallState {
    Available,
    Installed,
    Enabled,
    Disabled,
    Running,
    Linked,
    Copy,
    Outdated,
    Unmanaged,
    Broken,
    Unknown,
}

impl InstallState {
    pub fn label(self) -> &'static str {
        match self {
            InstallState::Available => "Available",
            InstallState::Installed => "Installed",
            InstallState::Enabled => "Enabled",
            InstallState::Disabled => "Disabled",
            InstallState::Running => "Running",
            InstallState::Linked => "Linked",
            InstallState::Copy => "Copy",
            InstallState::Outdated => "Outdated",
            InstallState::Unmanaged => "Unmanaged",
            InstallState::Broken => "Broken",
            InstallState::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "kebab-case")]
pub enum LocationRole {
    /// The canonical source of truth (library entry, original file).
    Canonical,
    /// A deployment into a host target (symlink or copy).
    Deployed { target: String, scope: Scope },
    /// An entry inside a configuration file (MCP servers).
    ConfigEntry { file: PathBuf, entry: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLocation {
    pub path: PathBuf,
    pub role: LocationRole,
    pub state: InstallState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
}

/// Strict validation errors are kept separate from lenient browsing warnings
/// by severity; resources with warnings remain browsable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
}

impl ValidationIssue {
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            message: message.into(),
        }
    }
}

/// The normalised, provider-independent view of any resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceRecord {
    pub id: ResourceId,
    pub name: String,
    pub summary: String,
    pub publisher: Option<String>,
    pub tags: Vec<String>,
    /// Version only from real metadata; never invented.
    pub version: Option<String>,
    pub provenance: Provenance,
    pub locations: Vec<ResourceLocation>,
    pub capabilities: CapabilitySet,
    pub details: ResourceDetails,
    pub issues: Vec<ValidationIssue>,
}

impl ResourceRecord {
    pub fn kind(&self) -> ResourceKind {
        self.id.kind
    }

    pub fn canonical_location(&self) -> Option<&ResourceLocation> {
        self.locations
            .iter()
            .find(|location| location.role == LocationRole::Canonical)
    }

    /// Path suitable for "Copy path": canonical location, else first location.
    pub fn local_path(&self) -> Option<&PathBuf> {
        self.canonical_location()
            .or_else(|| self.locations.first())
            .map(|location| &location.path)
    }

    pub fn deployments(&self) -> impl Iterator<Item = &ResourceLocation> {
        self.locations
            .iter()
            .filter(|location| matches!(location.role, LocationRole::Deployed { .. }))
    }

    pub fn is_installed(&self) -> bool {
        self.locations.iter().any(|location| {
            !matches!(
                location.state,
                InstallState::Available | InstallState::Unknown | InstallState::Broken
            )
        })
    }

    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == IssueSeverity::Error)
    }
}
