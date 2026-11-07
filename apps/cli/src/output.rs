//! Text formatting helpers shared by commands.

use std::fmt::Write as _;
use std::time::Duration;

use perkolator_core::{LocationRole, ResourceRecord, Scope};
use perkolator_search::SearchDoc;
use serde_json::Value;

/// What a command produced. `success == false` maps to exit code 1 while
/// still printing the full report (conflicting plan, validation errors).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutput {
    pub text: String,
    pub success: bool,
}

impl CommandOutput {
    pub fn ok(text: String) -> Self {
        Self {
            text,
            success: true,
        }
    }

    pub fn failed(text: String) -> Self {
        Self {
            text,
            success: false,
        }
    }

    pub fn json(value: &Value) -> Self {
        let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
        text.push('\n');
        Self::ok(text)
    }
}

/// The serde (kebab-case) name of a unit enum value, e.g. `copy-config`.
pub fn slug(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

pub fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Source label shared with the launcher ("Library", host id, "Project").
pub fn source_label(record: &ResourceRecord) -> String {
    SearchDoc::from_record(record).source_label
}

/// Distinct location states, e.g. `Installed/Linked`.
pub fn state_summary(record: &ResourceRecord) -> String {
    let mut labels: Vec<&str> = Vec::new();
    for location in &record.locations {
        let label = location.state.label();
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        "-".to_owned()
    } else {
        labels.join("/")
    }
}

pub fn scope_label(scope: &Scope) -> String {
    match scope {
        Scope::Global => "global".to_owned(),
        Scope::Project { root } => format!("project {}", root.display()),
        Scope::Custom { path } => format!("custom {}", path.display()),
    }
}

pub fn role_label(role: &LocationRole) -> String {
    match role {
        LocationRole::Canonical => "canonical".to_owned(),
        LocationRole::Deployed { target, scope } => {
            format!("deployed {target} ({})", scope_label(scope))
        }
        LocationRole::ConfigEntry { entry, .. } => format!("config entry {entry}"),
    }
}

/// Left-aligned columns separated by two spaces; the last column is not padded.
pub fn table(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in rows {
        let mut line = String::new();
        for (column, cell) in row.iter().enumerate() {
            if column + 1 == row.len() {
                line.push_str(cell);
            } else {
                let _ = write!(line, "{cell:<width$}  ", width = widths[column]);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Up to `limit` non-empty lines, each cut to `width` characters.
pub fn first_lines(text: &str, limit: usize, width: usize) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .take(limit)
        .map(|line| truncate(line, width))
        .collect()
}

pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(width.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_pads_all_but_the_last_column() {
        let rows = vec![
            vec!["a".to_owned(), "long".to_owned(), "x".to_owned()],
            vec!["bbb".to_owned(), "c".to_owned(), "y".to_owned()],
        ];
        assert_eq!(table(&rows), "a    long  x\nbbb  c     y\n");
    }

    #[test]
    fn truncate_and_first_lines_respect_limits() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(first_lines("\n a\n\nb\nc\n", 2, 10), vec![" a", "b"]);
    }

    #[test]
    fn slug_uses_serde_names() {
        assert_eq!(
            slug(serde_json::json!(perkolator_core::ActionId::CopyConfig)),
            "copy-config"
        );
    }
}
