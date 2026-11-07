//! Read-only local discovery for skills, MCP servers, agents, prompts, rules
//! and plugins. Owned by the `core` lane (see docs/WORKSTREAMS.md).
//!
//! Scanning is inert: files are read and parsed, never executed, and MCP
//! servers are never connected to or spawned. [`scan`] is synchronous and
//! cooperative-cancellable; callers run it on a background thread.

mod config;
mod files;
mod frontmatter;
mod jsonc;
mod kinds;
mod registry;
mod report;
mod scan;
mod text;

pub use config::{McpConfigFormat, RootLayout, ScanConfig, ScanLimits, ScanRoot};
pub use registry::{InsertOutcome, Registry};
pub use report::{BrokenLink, ScanProblem, ScanReport};
pub use scan::scan;
