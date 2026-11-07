//! `perk targets`: built-in agent targets resolved against the chosen home.

use perkolator_core::Scope;
use perkolator_targets::{detect, resolve_skill_dir};
use serde_json::{Value, json};

use crate::env::Env;
use crate::output::{slug, table};
use crate::{CliError, CommandOutput};

struct TargetRow {
    id: String,
    name: String,
    status: String,
    global: Option<String>,
    project: Option<String>,
    supports_symlinks: bool,
}

pub fn targets(env: &Env, json: bool) -> Result<CommandOutput, CliError> {
    let rows: Vec<TargetRow> = env
        .targets
        .iter()
        .map(|target| TargetRow {
            id: target.id.clone(),
            name: target.display_name.clone(),
            status: slug(json!(detect(target, &env.home_env))),
            global: resolve_skill_dir(target, &Scope::Global, &env.home_env)
                .ok()
                .map(|path| path.display().to_string()),
            project: target
                .skills
                .as_ref()
                .and_then(|skills| skills.project.clone()),
            supports_symlinks: target.supports_symlinks,
        })
        .collect();
    if json {
        let items: Vec<Value> = rows
            .iter()
            .map(|row| {
                json!({
                    "id": row.id,
                    "display_name": row.name,
                    "status": row.status,
                    "global_skills_dir": row.global,
                    "project_skills_dir": row.project,
                    "supports_symlinks": row.supports_symlinks,
                })
            })
            .collect();
        return Ok(CommandOutput::json(&Value::Array(items)));
    }
    let mut table_rows = vec![vec![
        "ID".to_owned(),
        "NAME".to_owned(),
        "STATUS".to_owned(),
        "GLOBAL SKILLS DIR".to_owned(),
        "PROJECT DIR".to_owned(),
    ]];
    table_rows.extend(rows.into_iter().map(|row| {
        vec![
            row.id,
            row.name,
            row.status,
            row.global.unwrap_or_else(|| "-".to_owned()),
            row.project.unwrap_or_else(|| "-".to_owned()),
        ]
    }));
    Ok(CommandOutput::ok(table(&table_rows)))
}
