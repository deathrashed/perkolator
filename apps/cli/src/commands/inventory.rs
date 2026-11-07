//! Read-only inventory commands: scan, list, search, validate.

use std::fmt::Write as _;
use std::time::Instant;

use perkolator_core::{IssueSeverity, ResourceKind, ResourceRecord};
use perkolator_search::{Boosts, Query, SearchIndex};
use serde_json::{Value, json};

use crate::env::Env;
use crate::output::{millis, source_label, state_summary, table};
use crate::{CliError, CommandOutput};

pub fn scan(env: &Env, json: bool) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let counts = scanned.registry.count_by_kind();
    let count = |kind: ResourceKind| counts.get(&kind).copied().unwrap_or(0);
    let report = &scanned.report;
    if json {
        let by_kind: serde_json::Map<String, Value> = ResourceKind::ALL
            .iter()
            .map(|kind| (kind.slug().to_owned(), json!(count(*kind))))
            .collect();
        return Ok(CommandOutput::json(&json!({
            "home": env.home,
            "projects": env.projects,
            "total": scanned.registry.len(),
            "counts": by_kind,
            "problems": report.problems,
            "broken_links": report.broken_links,
            "cancelled": report.cancelled,
            "elapsed_ms": millis(scanned.elapsed),
        })));
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Scanned {} in {:.1} ms: {} resource(s)",
        env.home.display(),
        millis(scanned.elapsed),
        scanned.registry.len()
    );
    let rows: Vec<Vec<String>> = ResourceKind::ALL
        .iter()
        .map(|kind| {
            vec![
                format!("  {}", kind.plural_label()),
                count(*kind).to_string(),
            ]
        })
        .collect();
    out.push_str(&table(&rows));
    let _ = writeln!(out, "Problems: {}", report.problems.len());
    for problem in &report.problems {
        let _ = writeln!(out, "  {}: {}", problem.path.display(), problem.message);
    }
    let _ = writeln!(out, "Broken links: {}", report.broken_links.len());
    for link in &report.broken_links {
        let _ = writeln!(
            out,
            "  {} -> {}{}",
            link.path.display(),
            link.target.display(),
            link.host
                .as_deref()
                .map(|host| format!(" ({host})"))
                .unwrap_or_default()
        );
    }
    if report.cancelled {
        out.push_str("Scan was cancelled; results are incomplete.\n");
    }
    Ok(CommandOutput::ok(out))
}

pub fn list(env: &Env, kind: Option<ResourceKind>, json: bool) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let mut records: Vec<&ResourceRecord> = scanned
        .registry
        .iter()
        .filter(|record| kind.is_none_or(|kind| record.kind() == kind))
        .collect();
    records.sort_by(|a, b| {
        (a.kind(), a.name.to_lowercase(), a.id.to_string()).cmp(&(
            b.kind(),
            b.name.to_lowercase(),
            b.id.to_string(),
        ))
    });
    if json {
        let items: Vec<Value> = records
            .iter()
            .map(|record| {
                json!({
                    "id": record.id.to_string(),
                    "name": record.name,
                    "kind": record.kind().slug(),
                    "source": source_label(record),
                    "states": record.locations.iter().map(|l| l.state).collect::<Vec<_>>(),
                    "installed": record.is_installed(),
                })
            })
            .collect();
        return Ok(CommandOutput::json(&Value::Array(items)));
    }
    let mut rows = vec![vec![
        "KIND".to_owned(),
        "NAME".to_owned(),
        "SOURCE".to_owned(),
        "STATE".to_owned(),
        "ID".to_owned(),
    ]];
    rows.extend(records.iter().map(|record| {
        vec![
            record.kind().slug().to_owned(),
            record.name.clone(),
            source_label(record),
            state_summary(record),
            record.id.to_string(),
        ]
    }));
    let mut out = table(&rows);
    let _ = writeln!(out, "{} resource(s)", records.len());
    Ok(CommandOutput::ok(out))
}

pub fn search(env: &Env, input: &str, limit: usize, json: bool) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let build_started = Instant::now();
    let index = SearchIndex::from_records(scanned.registry.iter());
    let build = build_started.elapsed();
    let query = Query::parse(input);
    let search_started = Instant::now();
    let hits = index.search(&query, &Boosts::default(), limit);
    let elapsed = search_started.elapsed();

    let rows: Vec<(usize, &str, ResourceKind, String, String)> = hits
        .iter()
        .enumerate()
        .filter_map(|(rank, hit)| {
            index.get(&hit.id).map(|doc| {
                (
                    rank + 1,
                    doc.name.as_str(),
                    doc.kind,
                    hit.id.to_string(),
                    format!("{:?}", hit.tier),
                )
            })
        })
        .collect();
    if json {
        let results: Vec<Value> = rows
            .iter()
            .map(|(rank, name, kind, id, tier)| {
                json!({ "rank": rank, "name": name, "kind": kind.slug(), "id": id, "tier": tier })
            })
            .collect();
        return Ok(CommandOutput::json(&json!({
            "query": {
                "text": query.text,
                "kinds": query.kinds.iter().map(|kind| kind.slug()).collect::<Vec<_>>(),
                "scope": format!("{:?}", query.scope),
            },
            "docs": index.len(),
            "index_build_ms": millis(build),
            "search_ms": millis(elapsed),
            "results": results,
        })));
    }
    let mut table_rows = vec![vec![
        "#".to_owned(),
        "NAME".to_owned(),
        "KIND".to_owned(),
        "ID".to_owned(),
    ]];
    table_rows.extend(rows.iter().map(|(rank, name, kind, id, _)| {
        vec![
            rank.to_string(),
            (*name).to_owned(),
            kind.slug().to_owned(),
            id.clone(),
        ]
    }));
    let mut out = if rows.is_empty() {
        String::from("No matches.\n")
    } else {
        table(&table_rows)
    };
    let _ = writeln!(
        out,
        "{} result(s) in {:.3} ms over {} docs (index build {:.3} ms)",
        rows.len(),
        millis(elapsed),
        index.len(),
        millis(build)
    );
    Ok(CommandOutput::ok(out))
}

pub fn validate(env: &Env, strict: bool) -> Result<CommandOutput, CliError> {
    let scanned = env.scan();
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    let mut out = String::new();
    let mut records: Vec<&ResourceRecord> = scanned
        .registry
        .iter()
        .filter(|record| !record.issues.is_empty())
        .collect();
    records.sort_by(|a, b| (a.kind(), &a.name).cmp(&(b.kind(), &b.name)));
    for record in records {
        for issue in &record.issues {
            let label = match issue.severity {
                IssueSeverity::Error => {
                    errors += 1;
                    "error"
                }
                IssueSeverity::Warning => {
                    warnings += 1;
                    "warning"
                }
                IssueSeverity::Info => {
                    infos += 1;
                    "info"
                }
            };
            let _ = writeln!(
                out,
                "{label:<7}  {} {}: {}  [{}]",
                record.kind().slug(),
                record.name,
                issue.message,
                record
                    .local_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| record.id.to_string())
            );
        }
    }
    let report = &scanned.report;
    for problem in &report.problems {
        warnings += 1;
        let _ = writeln!(
            out,
            "warning  scan: {}  [{}]",
            problem.message,
            problem.path.display()
        );
    }
    for link in &report.broken_links {
        if report
            .problems
            .iter()
            .any(|problem| problem.path == link.path)
        {
            continue;
        }
        warnings += 1;
        let _ = writeln!(
            out,
            "warning  broken link -> {}  [{}]",
            link.target.display(),
            link.path.display()
        );
    }
    let failed = errors > 0 || (strict && warnings > 0);
    let _ = writeln!(
        out,
        "{errors} error(s), {warnings} warning(s), {infos} info{}",
        if failed {
            if errors > 0 {
                "; validation failed"
            } else {
                "; validation failed (--strict)"
            }
        } else {
            ""
        }
    );
    Ok(if failed {
        CommandOutput::failed(out)
    } else {
        CommandOutput::ok(out)
    })
}
