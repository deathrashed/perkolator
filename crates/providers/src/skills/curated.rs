//! Parser for the VoltAgent `awesome-agent-skills` README, the curated index
//! behind both `voltagent-skills` and the descriptions on officialskills.sh.
//!
//! Entries look like `- **[publisher/skill](url)** - description` (some use
//! `:` instead of `-`), grouped under `###` headings or
//! `<summary><h3 ...>Section</h3></summary>` blocks.

use perkolator_core::CancelToken;
use perkolator_marketplace::github::{self, RepoRef};
use perkolator_marketplace::{ProviderEnv, ProviderError};

use super::common::ensure_active;

pub(crate) const INDEX_OWNER: &str = "VoltAgent";
pub(crate) const INDEX_REPO: &str = "awesome-agent-skills";
pub(crate) const INDEX_FILE: &str = "README.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexEntry {
    /// Link text, conventionally `publisher/skill`.
    pub name: String,
    pub url: String,
    pub description: String,
    pub section: Option<String>,
}

impl IndexEntry {
    /// The publisher half of a `publisher/skill` name.
    pub fn publisher(&self) -> Option<&str> {
        self.name
            .split_once('/')
            .map(|(publisher, _)| publisher)
            .filter(|publisher| !publisher.is_empty())
    }
}

/// Fetch and parse the README from raw.githubusercontent.com.
pub(crate) fn fetch_index(
    env: &ProviderEnv,
    cancel: &CancelToken,
) -> Result<Vec<IndexEntry>, ProviderError> {
    ensure_active(cancel)?;
    let markdown = github::raw_file(env, &RepoRef::new(INDEX_OWNER, INDEX_REPO), INDEX_FILE)?;
    ensure_active(cancel)?;
    let entries = parse_index(&markdown);
    if entries.is_empty() {
        return Err(ProviderError::malformed(
            "awesome-agent-skills README has no recognisable skill entries",
        ));
    }
    Ok(entries)
}

pub(crate) fn parse_index(markdown: &str) -> Vec<IndexEntry> {
    let mut section: Option<String> = None;
    let mut entries = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(heading) = heading_text(trimmed) {
            section = Some(heading);
        } else if let Some(entry) = parse_entry(trimmed, section.as_deref()) {
            entries.push(entry);
        }
    }
    entries
}

fn heading_text(line: &str) -> Option<String> {
    let text = if let Some(rest) = line.strip_prefix("<summary>") {
        let start = rest.find('>')? + 1;
        let end = rest.find("</h")?;
        rest.get(start..end)?
    } else if line.starts_with("## ") || line.starts_with("### ") {
        line.trim_start_matches('#')
    } else {
        return None;
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn parse_entry(line: &str, section: Option<&str>) -> Option<IndexEntry> {
    let rest = line.strip_prefix("- **[")?;
    let (name, rest) = rest.split_once("](")?;
    let (url, rest) = rest.split_once(")**")?;
    let name = name.trim();
    let url = url.trim();
    if name.is_empty() || !url.starts_with("https://") || url.contains(char::is_whitespace) {
        return None;
    }
    let description = rest
        .trim_start()
        .trim_start_matches(['-', '–', '—', ':'])
        .trim();
    Some(IndexEntry {
        name: name.to_owned(),
        url: url.to_owned(),
        description: description.to_owned(),
        section: section.map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_with_sections_and_separators() {
        let markdown = "\
### Core Skills

- **[microsoft/mcp-builder](https://officialskills.sh/microsoft/skills/mcp-builder)** - MCP server creation guide
<summary><h3 style=\"display:inline\">Marketing</h3></summary>
- **[infra/gtm](https://github.com/infra/gtm)**: GTM-focused skill collection
- [Not an entry](https://example.com)
- **[bad](http://insecure.example)** - skipped
";
        let entries = parse_index(markdown);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].section.as_deref(), Some("Core Skills"));
        assert_eq!(entries[0].description, "MCP server creation guide");
        assert_eq!(entries[1].section.as_deref(), Some("Marketing"));
        assert_eq!(entries[1].description, "GTM-focused skill collection");
        assert_eq!(entries[1].publisher(), Some("infra"));
    }
}
