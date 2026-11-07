//! Mutating commands: deploy, undeploy, repair. Dry run unless `--apply --yes`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use perkolator_core::{Capability, ResourceDetails, ResourceKind, ResourceRecord, Scope};
use perkolator_deployment::{
    ApplyReport, ApplyStatus, DeployError, DeployRequest, Destination, Manifest, ManifestEntry,
    Plan, RecoveryReport, plan_deploy, plan_remove, plan_repair,
};

use crate::cli::{ApplyFlags, DeployArgs, ScopeArg};
use crate::env::{Env, absolute};
use crate::lookup;
use crate::{CliError, CommandOutput};

pub fn deploy(env: &Env, args: &DeployArgs) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let record = lookup::find(&scanned.registry, &args.resource)?;
    let source = deployable_source(record)?;
    let name = match &args.name {
        Some(name) => name.clone(),
        None => source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| CliError::usage("source has no directory name; pass --name"))?,
    };
    let scopes: Vec<Scope> = match args.scope {
        ScopeArg::Global => vec![Scope::Global],
        ScopeArg::Project if env.projects.is_empty() => {
            return Err(CliError::usage(
                "--scope project needs at least one --project DIR",
            ));
        }
        ScopeArg::Project => env
            .projects
            .iter()
            .map(|root| Scope::Project { root: root.clone() })
            .collect(),
    };
    let mut destinations = Vec::new();
    for target_id in &args.targets {
        let target = env.targets.get(target_id).ok_or_else(|| {
            let known: Vec<&str> = env.targets.iter().map(|t| t.id.as_str()).collect();
            CliError::usage(format!(
                "unknown target `{target_id}`; known targets: {}",
                known.join(", ")
            ))
        })?;
        for scope in &scopes {
            destinations.push(
                Destination::for_target(target, scope.clone(), &env.home_env)
                    .map_err(CliError::failed)?,
            );
        }
    }
    let managed_root = args.managed_root.as_deref().map(absolute).transpose()?;
    let request = DeployRequest {
        source,
        name,
        mode: args.mode.into(),
        destinations,
        managed_root,
        approved_roots: Vec::new(),
    };
    execute(env, args.apply, |manifest| plan_deploy(&request, manifest))
}

pub fn undeploy(
    env: &Env,
    destination: &Path,
    flags: ApplyFlags,
) -> Result<CommandOutput, CliError> {
    let destination = absolute(destination)?;
    execute(env, flags, |manifest| plan_remove(&destination, manifest))
}

pub fn repair(
    env: &Env,
    destination: &Path,
    to: Option<&Path>,
    flags: ApplyFlags,
) -> Result<CommandOutput, CliError> {
    let destination = absolute(destination)?;
    let to = to.map(absolute).transpose()?;
    execute(env, flags, |manifest| {
        plan_repair(&destination, manifest, to.as_deref())
    })
}

/// The directory a deployment links to or copies from: the canonical
/// location when the registry knows one, else the skill directory itself.
pub(super) fn skill_source(record: &ResourceRecord) -> Option<PathBuf> {
    let ResourceDetails::Skill(skill) = &record.details else {
        return None;
    };
    record
        .canonical_location()
        .map(|location| location.path.clone())
        .or_else(|| skill.root.canonicalize().ok())
}

fn deployable_source(record: &ResourceRecord) -> Result<PathBuf, CliError> {
    if record.kind() != ResourceKind::Skill
        || !record.capabilities.contains(Capability::DeployFiles)
    {
        return Err(CliError::usage(format!(
            "{} `{}` cannot be deployed as files; only skills support file deployment",
            record.kind().label(),
            record.name
        )));
    }
    if record.has_errors() {
        return Err(CliError::usage(format!(
            "skill `{}` has validation errors; fix them first (see `perk validate`)",
            record.name
        )));
    }
    skill_source(record)
        .ok_or_else(|| CliError::usage(format!("skill `{}` has no local directory", record.name)))
}

/// Shared plan → confirm → recover → re-plan → apply flow.
///
/// Dry runs and refusals only read the manifest; recovery (which may roll
/// back interrupted runs) happens only on an explicit `--apply --yes`, and the
/// plan is rebuilt afterwards so it reflects the recovered state.
fn execute(
    env: &Env,
    flags: ApplyFlags,
    make_plan: impl Fn(&Manifest) -> Plan,
) -> Result<CommandOutput, CliError> {
    let mut manifest = env.load_manifest()?;
    let mut out = String::new();
    if !(flags.apply && flags.yes) {
        let plan = make_plan(&manifest);
        out.push_str(&plan.summary());
        if !plan.is_applicable() {
            out.push_str("Plan has conflicts; nothing was changed.\n");
            return Ok(CommandOutput::failed(out));
        }
        if flags.apply {
            out.push_str("Refusing to apply without --yes; nothing was changed.\n");
            return Ok(CommandOutput::failed(out));
        }
        out.push_str("Dry run: nothing was changed. Re-run with --apply --yes to apply.\n");
        return Ok(CommandOutput::ok(out));
    }

    let executor = env.executor();
    let recovery = executor.recover(&mut manifest).map_err(CliError::failed)?;
    write_recovery(&mut out, &recovery);
    let plan = make_plan(&manifest);
    out.push_str(&plan.summary());
    if !plan.is_applicable() {
        let _ = writeln!(
            out,
            "Refusing to apply: {} conflict(s); nothing was changed.",
            plan.conflicts.len()
        );
        return Ok(CommandOutput::failed(out));
    }
    if nothing_to_apply(&plan, &manifest) {
        out.push_str("Nothing to apply: everything is already in place.\n");
        return Ok(CommandOutput::ok(out));
    }
    match executor.apply(&plan, &mut manifest) {
        Ok(report) => {
            write_report(&mut out, &report);
            Ok(CommandOutput::ok(out))
        }
        Err(DeployError::OperationFailed(report)) => {
            write_report(&mut out, &report);
            Ok(CommandOutput::failed(out))
        }
        Err(error) => Err(CliError::failed(error)),
    }
}

/// No filesystem change and no manifest change other than re-stamping.
fn nothing_to_apply(plan: &Plan, manifest: &Manifest) -> bool {
    plan.is_noop()
        && plan.manifest_removals.is_empty()
        && plan.manifest_upserts.iter().all(|upsert| {
            manifest
                .entry(&upsert.destination)
                .is_some_and(|existing| same_entry(existing, upsert))
        })
}

fn same_entry(a: &ManifestEntry, b: &ManifestEntry) -> bool {
    a.destination == b.destination
        && a.source == b.source
        && a.mode == b.mode
        && a.target_id == b.target_id
        && a.scope == b.scope
        && a.link_target == b.link_target
        && a.fingerprint == b.fingerprint
}

pub(super) fn write_recovery(out: &mut String, recovery: &RecoveryReport) {
    for run in &recovery.runs {
        let outcome = run
            .outcome
            .map(|outcome| format!("{outcome:?}"))
            .unwrap_or_else(|| "unfinished".to_owned());
        let _ = writeln!(out, "Recovered run {}: {outcome}", run.run_id);
        for issue in &run.issues {
            let _ = writeln!(out, "  ! {issue}");
        }
    }
}

fn write_report(out: &mut String, report: &ApplyReport) {
    let status = match &report.status {
        ApplyStatus::Applied => "applied".to_owned(),
        ApplyStatus::RolledBack {
            failed_index,
            error,
        } => format!("step {failed_index} failed ({error}); rolled back"),
        ApplyStatus::RollbackIncomplete {
            failed_index,
            error,
        } => format!(
            "step {failed_index} failed ({error}); rollback incomplete — run `perk recover`"
        ),
    };
    let _ = writeln!(
        out,
        "Run {}: {status}; {} step(s) applied, {} undone",
        report.run_id, report.steps_applied, report.steps_undone
    );
    for issue in &report.issues {
        let _ = writeln!(out, "  ! {issue}");
    }
    let _ = writeln!(out, "Journal: {}", report.journal.display());
}
