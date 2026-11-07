//! Resolve `ID_OR_NAME` arguments to exactly one record.

use perkolator_core::{ResourceId, ResourceKind, ResourceRecord};
use perkolator_registry::Registry;
use perkolator_search::{Boosts, Query, SearchIndex};

use crate::CliError;

const MAX_SUGGESTIONS: usize = 5;

/// Accepts, in order: a full resource id (`kind:provider:key`), a record key
/// (canonical path), `kind:name`, or a display name (exact, then
/// case-insensitive). A name shared by several records is an error listing
/// their ids; nothing is guessed.
pub fn find<'a>(registry: &'a Registry, query: &str) -> Result<&'a ResourceRecord, CliError> {
    if let Ok(id) = query.parse::<ResourceId>()
        && let Some(record) = registry.get(&id)
    {
        return Ok(record);
    }
    let by_key: Vec<&ResourceRecord> = registry.iter().filter(|r| r.id.key == query).collect();
    if by_key.len() == 1 {
        return Ok(by_key[0]);
    }
    let (kind, name) = split_kind(query);
    let candidates = |exact: bool| -> Vec<&'a ResourceRecord> {
        registry
            .iter()
            .filter(|record| kind.is_none_or(|kind| record.kind() == kind))
            .filter(|record| {
                if exact {
                    record.name == name
                } else {
                    record.name.eq_ignore_ascii_case(name)
                }
            })
            .collect()
    };
    let mut matches = candidates(true);
    if matches.is_empty() {
        matches = candidates(false);
    }
    match matches.as_slice() {
        [record] => Ok(record),
        [] => Err(CliError::NotFound {
            query: query.to_owned(),
            suggestions: suggestions(registry, query),
        }),
        many => Err(CliError::Ambiguous {
            query: query.to_owned(),
            candidates: many.iter().map(|record| describe(record)).collect(),
        }),
    }
}

/// `skill:frontend-design` → (Some(Skill), "frontend-design").
fn split_kind(query: &str) -> (Option<ResourceKind>, &str) {
    match query.split_once(':') {
        Some((prefix, name)) if !name.is_empty() && !name.contains(':') => {
            match prefix.parse::<ResourceKind>() {
                Ok(kind) => (Some(kind), name),
                Err(_) => (None, query),
            }
        }
        _ => (None, query),
    }
}

fn describe(record: &ResourceRecord) -> String {
    format!(
        "{}  ({} \"{}\")",
        record.id,
        record.kind().label(),
        record.name
    )
}

fn suggestions(registry: &Registry, query: &str) -> Vec<String> {
    let index = SearchIndex::from_records(registry.iter());
    index
        .search(&Query::parse(query), &Boosts::default(), MAX_SUGGESTIONS)
        .iter()
        .filter_map(|hit| registry.get(&hit.id))
        .map(describe)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_prefix_is_split_only_for_known_kinds() {
        assert_eq!(
            split_kind("skill:frontend-design"),
            (Some(ResourceKind::Skill), "frontend-design")
        );
        assert_eq!(
            split_kind("mcp:github"),
            (Some(ResourceKind::McpServer), "github")
        );
        assert_eq!(split_kind("notes:x"), (None, "notes:x"));
        assert_eq!(split_kind("skill:a:b"), (None, "skill:a:b"));
        assert_eq!(split_kind("plain"), (None, "plain"));
    }
}
