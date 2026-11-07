//! Command-line surface of `perk` (clap derive). Parsing only; behaviour
//! lives in [`crate::commands`].

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use perkolator_core::{CoreError, ResourceKind};
use perkolator_deployment::DeploymentMode;

#[derive(Debug, Parser)]
#[command(
    name = "perk",
    version,
    about = "Headless Perkolator: discover, search, inspect and safely deploy AI resources."
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// Home directory to scan and deploy into (default: the real home).
    #[arg(long, global = true, value_name = "DIR")]
    pub home: Option<PathBuf>,
    /// Project root to include (repeatable).
    #[arg(long = "project", global = true, value_name = "DIR")]
    pub projects: Vec<PathBuf>,
    /// Journal + manifest directory (default: `<home>/.local/state/perkolator`).
    #[arg(long, global = true, value_name = "DIR")]
    pub state_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scan local roots: counts by kind, problems and broken links.
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// List discovered resources.
    List {
        #[arg(long, value_parser = parse_kind)]
        kind: Option<ResourceKind>,
        #[arg(long)]
        json: bool,
    },
    /// Fuzzy search with launcher query syntax (`mcp: git`, `@installed`, `^pre`).
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Metadata, locations, capabilities, issues and preview summary.
    Show {
        #[arg(value_name = "ID_OR_NAME")]
        resource: String,
    },
    /// Report validation issues; exits 1 on errors (or warnings with --strict).
    Validate {
        #[arg(long)]
        strict: bool,
    },
    /// List actions offered for a resource, with disabled reasons.
    Actions {
        #[arg(value_name = "ID_OR_NAME")]
        resource: String,
    },
    /// Print an action's clipboard payload to stdout (no clipboard access).
    Copy {
        #[arg(value_name = "ID_OR_NAME")]
        resource: String,
        /// Action id, e.g. `copy-path`, `copy-config`, `copy-invocation`.
        action: String,
    },
    /// Built-in agent targets with resolved global skills dir and detection.
    Targets {
        #[arg(long)]
        json: bool,
    },
    /// Plan (and with --apply --yes, apply) a skill deployment.
    Deploy(DeployArgs),
    /// Plan (and with --apply --yes, apply) removal of a managed deployment.
    Undeploy {
        #[arg(value_name = "DEST_PATH")]
        destination: PathBuf,
        #[command(flatten)]
        apply: ApplyFlags,
    },
    /// Plan (and with --apply --yes, apply) repair of a managed link.
    Repair {
        #[arg(value_name = "DEST_PATH")]
        destination: PathBuf,
        /// Re-point the link to this directory instead of its recorded target.
        #[arg(long, value_name = "DIR")]
        to: Option<PathBuf>,
        #[command(flatten)]
        apply: ApplyFlags,
    },
    /// Finish or roll back interrupted deployment runs (idempotent).
    Recover,
    /// Managed deployments from the manifest with their on-disk status.
    Status,
    /// Skills × targets deployment matrix (global scope).
    Matrix,
}

#[derive(Debug, Args)]
pub struct DeployArgs {
    #[arg(value_name = "ID_OR_NAME")]
    pub resource: String,
    /// Target id (repeatable), see `perk targets`.
    #[arg(long = "target", required = true, value_name = "T")]
    pub targets: Vec<String>,
    #[arg(long, value_enum, default_value_t = ScopeArg::Global)]
    pub scope: ScopeArg,
    #[arg(long, value_enum, default_value_t = ModeArg::Symlink)]
    pub mode: ModeArg,
    /// Required for managed-copy and link-source.
    #[arg(long, value_name = "DIR")]
    pub managed_root: Option<PathBuf>,
    /// Directory name in each skills dir (default: the source dir name).
    #[arg(long)]
    pub name: Option<String>,
    #[command(flatten)]
    pub apply: ApplyFlags,
}

/// Mutations are dry runs unless both flags are given.
#[derive(Clone, Copy, Debug, Default, Args)]
pub struct ApplyFlags {
    /// Apply the plan instead of only printing it.
    #[arg(long)]
    pub apply: bool,
    /// Confirm the apply non-interactively.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ScopeArg {
    Global,
    Project,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ModeArg {
    Symlink,
    ManagedCopy,
    LinkSource,
    IndependentCopy,
}

impl From<ModeArg> for DeploymentMode {
    fn from(mode: ModeArg) -> Self {
        match mode {
            ModeArg::Symlink => DeploymentMode::Symlink,
            ModeArg::ManagedCopy => DeploymentMode::ManagedCopy,
            ModeArg::LinkSource => DeploymentMode::LinkSource,
            ModeArg::IndependentCopy => DeploymentMode::IndependentCopy,
        }
    }
}

fn parse_kind(value: &str) -> Result<ResourceKind, String> {
    value
        .to_ascii_lowercase()
        .parse()
        .map_err(|error: CoreError| error.to_string())
}
