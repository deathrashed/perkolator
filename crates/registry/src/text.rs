//! Small text helpers: summaries and fingerprints.

const SUMMARY_MAX_CHARS: usize = 160;

/// First line of the first non-heading paragraph, skipping code fences,
/// tables, HTML comments and other non-prose lines.
pub(crate) fn first_prose_line(body: &str) -> Option<String> {
    let mut in_fence = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        let skip = in_fence
            || trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('|')
            || trimmed.starts_with("<!--")
            || trimmed.starts_with('<')
            || trimmed.starts_with("---")
            || trimmed.starts_with("===");
        if !skip {
            let line = trimmed.trim_start_matches(['-', '*', '>', ' ']).trim();
            if !line.is_empty() {
                return Some(truncate(line));
            }
        }
    }
    None
}

/// Collapse whitespace and cap at [`SUMMARY_MAX_CHARS`] with an ellipsis.
pub(crate) fn truncate(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SUMMARY_MAX_CHARS {
        return collapsed;
    }
    let mut out: String = collapsed.chars().take(SUMMARY_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

/// Summary from an explicit description, else the document's first prose line.
pub(crate) fn summary(description: Option<&str>, body: &str) -> String {
    description
        .map(truncate)
        .filter(|text| !text.is_empty())
        .or_else(|| first_prose_line(body))
        .unwrap_or_default()
}

pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_headings_fences_and_tables() {
        let body = "# Title\n\n```sh\nnot this\n```\n| a | b |\n\nUse this line.\nNot this.\n";
        assert_eq!(first_prose_line(body).as_deref(), Some("Use this line."));
    }

    #[test]
    fn truncates_long_text() {
        let long = "word ".repeat(100);
        let out = truncate(&long);
        assert_eq!(out.chars().count(), SUMMARY_MAX_CHARS);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn description_wins() {
        assert_eq!(summary(Some("Desc"), "Body"), "Desc");
        assert_eq!(summary(None, "Body"), "Body");
        assert_eq!(summary(Some("  "), "Body"), "Body");
    }
}
