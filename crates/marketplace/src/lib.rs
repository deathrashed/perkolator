//! Federated marketplace integration: the provider contract every source
//! adapter implements, transport seams (HTTP, CLI, credentials) that make
//! adapters testable offline, and shared GitHub access.
//!
//! Discovery is read-only and inert: no adapter may execute downloaded code,
//! start MCP servers or run hooks. Installation goes through the deployment
//! engine, never through a provider.

pub mod config;
mod contract;
pub mod dedup;
mod env;
pub mod federated;
pub mod github;
pub mod testing;

pub use contract::{
    AuthIntent, AuthStatus, AuthStrategy, MarketplaceProvider, Pagination, ProviderCapabilities,
    ProviderDescriptor, ProviderError, ProviderHealth, RemoteActions, RemoteDetail, RemoteFile,
    RemoteItem, SearchPage, SearchRequest, Transport,
};
pub use env::{
    CommandOutput, CommandRunner, CredentialStore, EnvCredentials, HttpClient, HttpRequest,
    HttpResponse, NoCredentials, ProviderEnv, SystemCommands, UreqClient, now_unix,
};
