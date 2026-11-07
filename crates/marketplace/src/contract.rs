use std::collections::BTreeSet;

use perkolator_core::{CancelToken, McpToolDecl, ProviderId, ResourceKind};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ProviderEnv;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    Cli,
    Rest,
    Mcp,
    GithubIndex,
    CuratedDirectory,
    Github,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Cli => "CLI",
            Transport::Rest => "API",
            Transport::Mcp => "MCP",
            Transport::GithubIndex => "GitHub index",
            Transport::CuratedDirectory => "Directory",
            Transport::Github => "GitHub",
        }
    }
}

/// How a source authenticates, as documented by the provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "strategy", rename_all = "kebab-case")]
pub enum AuthStrategy {
    /// Public access.
    None,
    /// Works anonymously; a token in `slot` raises quotas.
    OptionalToken { slot: String },
    /// User-supplied API key kept in secure storage under `slot`.
    ApiKey { slot: String, docs_url: String },
    /// Auth is owned by an external CLI (its own login/registration).
    ExternalCli { binary: String },
    /// OAuth with PKCE via a provider-documented URL.
    OAuthPkce { docs_url: String },
    /// Partner/approved access only.
    PartnerAccess { docs_url: String },
}

/// Connection/auth state shown per source. Distinct from [`ProviderHealth`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum AuthStatus {
    NotRequired,
    NotConfigured,
    CliMissing { binary: String },
    Ready { identity: Option<String> },
    Expired,
    RateLimited,
    NeedsAccess,
    Unsupported { reason: String },
    Unknown,
}

impl AuthStatus {
    pub fn label(&self) -> &'static str {
        match self {
            AuthStatus::NotRequired => "No auth required",
            AuthStatus::NotConfigured => "Needs setup",
            AuthStatus::CliMissing { .. } => "CLI missing",
            AuthStatus::Ready { .. } => "Ready",
            AuthStatus::Expired => "Expired",
            AuthStatus::RateLimited => "Rate limited",
            AuthStatus::NeedsAccess => "Needs access",
            AuthStatus::Unsupported { .. } => "Unsupported",
            AuthStatus::Unknown => "Not checked",
        }
    }

    /// Whether a search request may be attempted at all.
    pub fn allows_search(&self) -> bool {
        matches!(
            self,
            AuthStatus::NotRequired | AuthStatus::Ready { .. } | AuthStatus::Unknown
        )
    }
}

/// What "Connect" should do. Adapters return only provider-documented flows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "kebab-case")]
pub enum AuthIntent {
    NoAuthRequired,
    OpenExternalUrl {
        url: String,
    },
    /// Run an interactive CLI flow in a terminal the user sees.
    SpawnCliFlow {
        program: String,
        args: Vec<String>,
    },
    /// Ask the user for a secret to store under `slot`.
    PromptSecret {
        slot: String,
        docs_url: String,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "health", rename_all = "kebab-case")]
pub enum ProviderHealth {
    Unknown,
    Ok { latency_ms: u64 },
    Degraded { reason: String },
    Unavailable { reason: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pagination {
    #[default]
    None,
    Page,
    Cursor,
}

/// Declared abilities. Anything false must not be offered in the UI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub search: bool,
    pub detail: bool,
    pub fetch_files: bool,
    /// A package or source tree can be downloaded for staging.
    pub download: bool,
    /// The provider itself offers an install flow (Perkolator still stages).
    pub install: bool,
    pub pagination: Pagination,
    pub categories: bool,
    pub rate_limit_policy: Option<String>,
    pub requires_attribution: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub display_name: String,
    pub homepage: String,
    pub docs_url: Option<String>,
    pub kinds: Vec<ResourceKind>,
    pub transport: Transport,
    pub auth: AuthStrategy,
    pub capabilities: ProviderCapabilities,
    /// Honest caveats shown in the source detail drawer.
    pub notes: Vec<String>,
}

impl ProviderDescriptor {
    pub fn supports_kind(&self, kind: ResourceKind) -> bool {
        self.kinds.contains(&kind)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    /// Empty = all kinds the provider supports.
    pub kinds: BTreeSet<ResourceKind>,
    pub category: Option<String>,
    pub page_size: usize,
    /// Opaque, provider-specific (page number or cursor).
    pub cursor: Option<String>,
}

impl SearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            kinds: BTreeSet::new(),
            category: None,
            page_size: 25,
            cursor: None,
        }
    }

    pub fn wants(&self, kind: ResourceKind) -> bool {
        self.kinds.is_empty() || self.kinds.contains(&kind)
    }
}

/// What can be done with a remote item without guessing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteActions {
    pub view_source: bool,
    pub fetch_files: bool,
    pub install: bool,
}

/// One normalised search result. All fields come from provider data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteItem {
    pub source: ProviderId,
    /// Stable id within the source.
    pub upstream_id: String,
    pub kind: ResourceKind,
    pub title: String,
    pub summary: String,
    pub canonical_url: Option<String>,
    /// `https://github.com/owner/repo` when known.
    pub repo_url: Option<String>,
    /// Path of the resource inside `repo_url` (e.g. `skills/pdf`).
    pub resource_path: Option<String>,
    /// Package reference such as an npm name or registry server name.
    pub package_ref: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub categories: Vec<String>,
    pub actions: RemoteActions,
    pub fetched_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    pub items: Vec<RemoteItem>,
    pub next_cursor: Option<String>,
    /// Only when the provider reports it.
    pub total: Option<u64>,
}

impl SearchPage {
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
            total: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFile {
    pub path: String,
    pub size: Option<u64>,
    pub url: Option<String>,
}

/// Read-only detail for preview. Text is untrusted and must be rendered inert.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteDetail {
    pub item: RemoteItem,
    pub readme_markdown: Option<String>,
    pub manifest_json: Option<String>,
    pub files: Vec<RemoteFile>,
    pub tools: Vec<McpToolDecl>,
}

#[derive(Clone, Debug, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "kebab-case")]
pub enum ProviderError {
    #[error("`{binary}` is not installed")]
    CliMissing { binary: String },
    #[error("sign-in or API key required")]
    NeedsAuth,
    #[error("access denied by the provider")]
    Forbidden,
    #[error("rate limited")]
    RateLimited { retry_after_secs: Option<u64> },
    #[error("timed out")]
    Timeout,
    #[error("HTTP {status}")]
    Http { status: u16 },
    #[error("network error: {message}")]
    Network { message: String },
    #[error("unexpected response: {message}")]
    Malformed { message: String },
    #[error("not supported: {message}")]
    Unsupported { message: String },
    #[error("cancelled")]
    Cancelled,
}

impl ProviderError {
    pub fn malformed(message: impl Into<String>) -> Self {
        ProviderError::Malformed {
            message: message.into(),
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        ProviderError::Unsupported {
            message: message.into(),
        }
    }

    /// Map an HTTP status to the closest typed error.
    pub fn from_status(status: u16, retry_after_secs: Option<u64>) -> Self {
        match status {
            401 => ProviderError::NeedsAuth,
            403 => ProviderError::Forbidden,
            429 => ProviderError::RateLimited { retry_after_secs },
            status => ProviderError::Http { status },
        }
    }
}

/// A marketplace source. Implementations must be inert (no code execution),
/// bounded (respect `cancel` and the env's timeouts) and truthful (no
/// invented fields, counts or capabilities).
pub trait MarketplaceProvider: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;

    /// Cheap local check (credential present, CLI on PATH). Must not make a
    /// network request.
    fn auth_status(&self, env: &ProviderEnv) -> AuthStatus;

    /// What "Connect" does for this source.
    fn connect_intent(&self) -> AuthIntent {
        match &self.descriptor().auth {
            AuthStrategy::None => AuthIntent::NoAuthRequired,
            AuthStrategy::OptionalToken { slot } => AuthIntent::PromptSecret {
                slot: slot.clone(),
                docs_url: self
                    .descriptor()
                    .docs_url
                    .clone()
                    .unwrap_or_else(|| self.descriptor().homepage.clone()),
            },
            AuthStrategy::ApiKey { slot, docs_url } => AuthIntent::PromptSecret {
                slot: slot.clone(),
                docs_url: docs_url.clone(),
            },
            AuthStrategy::OAuthPkce { docs_url } | AuthStrategy::PartnerAccess { docs_url } => {
                AuthIntent::OpenExternalUrl {
                    url: docs_url.clone(),
                }
            }
            AuthStrategy::ExternalCli { binary } => AuthIntent::Unsupported {
                reason: format!("Sign in with `{binary}` in a terminal"),
            },
        }
    }

    fn search(
        &self,
        request: &SearchRequest,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<SearchPage, ProviderError>;

    fn detail(
        &self,
        item: &RemoteItem,
        env: &ProviderEnv,
        cancel: &CancelToken,
    ) -> Result<RemoteDetail, ProviderError> {
        let _ = (item, env, cancel);
        Err(ProviderError::unsupported(
            "detail is not available for this source",
        ))
    }
}
