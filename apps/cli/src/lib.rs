//! Headless `perk` CLI over the Perkolator crates.
//!
//! Every command returns a [`CommandOutput`] (text + success flag) so tests
//! drive the real clap parser and command code without spawning processes.
//! Mutating commands (`deploy`, `undeploy`, `repair`) are dry runs unless
//! both `--apply` and `--yes` are given.

pub mod cli;
mod commands;
mod env;
mod error;
mod lookup;
mod output;

pub use cli::Cli;
pub use error::CliError;
pub use output::CommandOutput;

/// Parse-free entry point: run an already parsed command line.
pub fn run(cli: &Cli) -> Result<CommandOutput, CliError> {
    let env = env::Env::new(&cli.global)?;
    commands::dispatch(&env, &cli.command)
}
