mod deploy;
mod inspect;
mod inventory;
mod state;
mod targets;

use crate::cli::Command;
use crate::env::Env;
use crate::{CliError, CommandOutput};

pub fn dispatch(env: &Env, command: &Command) -> Result<CommandOutput, CliError> {
    match command {
        Command::Scan { json } => inventory::scan(env, *json),
        Command::List { kind, json } => inventory::list(env, *kind, *json),
        Command::Search { query, limit, json } => inventory::search(env, query, *limit, *json),
        Command::Validate { strict } => inventory::validate(env, *strict),
        Command::Show { resource } => inspect::show(env, resource),
        Command::Actions { resource } => inspect::actions(env, resource),
        Command::Copy { resource, action } => inspect::copy(env, resource, action),
        Command::Targets { json } => targets::targets(env, *json),
        Command::Deploy(args) => deploy::deploy(env, args),
        Command::Undeploy { destination, apply } => deploy::undeploy(env, destination, *apply),
        Command::Repair {
            destination,
            to,
            apply,
        } => deploy::repair(env, destination, to.as_deref(), *apply),
        Command::Recover => state::recover(env),
        Command::Status => state::status(env),
        Command::Matrix => state::matrix(env),
    }
}
