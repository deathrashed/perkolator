//! Deployment state: recover, status, matrix.

use std::fmt::Write as _;
use std::path::PathBuf;

use perkolator_core::{ResourceKind, Scope};
use perkolator_deployment::{Destination, deployment_matrix, status_of};
use serde_json::json;

use super::deploy::{skill_source, write_recovery};
use crate::env::Env;
use crate::output::{scope_label, slug, table};
use crate::{CliError, CommandOutput};

pub fn recover(env: &Env) -> Result<CommandOutput, CliError> {
    let mut manifest = env.load_manifest()?;
    let report = env
        .executor()
        .recover(&mut manifest)
        .map_err(CliError::failed)?;
    let mut out = String::new();
    if report.runs.is_empty() {
        out.push_str("No interrupted runs; nothing to recover.\n");
    } else {
        write_recovery(&mut out, &report);
    }
    Ok(if report.is_clean() {
        CommandOutput::ok(out)
    } else {
        CommandOutput::failed(out)
    })
}

pub fn status(env: &Env) -> Result<CommandOutput, CliError> {
    let manifest = env.load_manifest()?;
    if manifest.entries.is_empty() {
        return Ok(CommandOutput::ok(format!(
            "No managed deployments (manifest {}).\n",
            env.manifest_path().display()
        )));
    }
    let mut entries: Vec<_> = manifest.entries.iter().collect();
    entries.sort_by(|a, b| a.destination.cmp(&b.destination));
    let mut rows = vec![vec![
        "STATUS".to_owned(),
        "TARGET".to_owned(),
        "SCOPE".to_owned(),
        "MODE".to_owned(),
        "DESTINATION".to_owned(),
        "SOURCE".to_owned(),
    ]];
    rows.extend(entries.iter().map(|entry| {
        vec![
            slug(json!(status_of(entry))),
            entry.target_id.clone(),
            scope_label(&entry.scope),
            entry.mode.label().to_owned(),
            entry.destination.display().to_string(),
            entry.source.display().to_string(),
        ]
    }));
    let mut out = table(&rows);
    let _ = writeln!(out, "{} managed deployment(s)", entries.len());
    Ok(CommandOutput::ok(out))
}

/// Rows are skills, columns are enabled targets with a global skills dir.
pub fn matrix(env: &Env) -> Result<CommandOutput, CliError> {
    let manifest = env.load_manifest()?;
    let scanned = env.scan();
    let mut skills: Vec<(String, PathBuf)> = scanned
        .registry
        .by_kind(ResourceKind::Skill)
        .filter_map(|record| skill_source(record).map(|source| (record.name.clone(), source)))
        .collect();
    skills.sort();
    let destinations: Vec<Destination> = env
        .targets
        .iter()
        .filter(|target| target.enabled)
        .filter_map(|target| Destination::for_target(target, Scope::Global, &env.home_env).ok())
        .collect();
    let sources: Vec<PathBuf> = skills.iter().map(|(_, source)| source.clone()).collect();
    let cells = deployment_matrix(&manifest, &sources, &destinations);

    let mut rows = vec![
        std::iter::once("SKILL".to_owned())
            .chain(destinations.iter().map(|d| d.target_id.clone()))
            .collect::<Vec<_>>(),
    ];
    for (column, (name, _)) in skills.iter().enumerate() {
        let mut row = vec![name.clone()];
        row.extend(
            cells
                .iter()
                .map(|per_destination| slug(json!(per_destination[column].status))),
        );
        rows.push(row);
    }
    let mut out = table(&rows);
    out.push_str("Global scope. linked/copy = managed; unmanaged = present but not deployed by Perkolator.\n");
    Ok(CommandOutput::ok(out))
}
