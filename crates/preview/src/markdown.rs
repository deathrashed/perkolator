use std::collections::HashMap;

use perkolator_core::RelPath;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::{Document, Heading, LinkRef};

/// Split a leading `---` frontmatter block from the body.
pub(crate) fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (None, text);
    };
    if first.trim_end() != "---" {
        return (None, text);
    }
    let start = first.len();
    let mut offset = start;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            return (Some(&text[start..offset]), &text[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

/// Shallow `key: value` view of YAML frontmatter for display. Nested lines
/// and list items are folded into their parent key's value; the registry's
/// typed frontmatter stays authoritative.
pub(crate) fn frontmatter_pairs(frontmatter: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in frontmatter.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let nested = line.starts_with([' ', '\t']) || line.starts_with('-');
        if !nested && let Some((key, value)) = line.split_once(':') {
            let value = match value.trim() {
                "|" | ">" | "|-" | ">-" => "",
                other => other,
            };
            pairs.push((key.trim().to_owned(), unquote(value).to_owned()));
            continue;
        }
        if let Some((_, value)) = pairs.last_mut() {
            let item = line.trim().trim_start_matches("- ").trim();
            let separator = if line.trim_start().starts_with('-') {
                ", "
            } else {
                " "
            };
            if !value.is_empty() {
                value.push_str(separator);
            }
            value.push_str(unquote(item));
        }
    }
    pairs
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// Parse Markdown for preview: frontmatter split out, headings with
/// GitHub-style anchors, and links resolved against `doc_path`'s directory
/// within the browse root. The body text is kept verbatim for rendering.
pub fn parse_markdown(text: &str, doc_path: Option<&RelPath>) -> Document {
    let (frontmatter, body) = split_frontmatter(text);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES;
    let mut headings = Vec::new();
    let mut links = Vec::new();
    let mut anchors = Anchors::default();
    let mut open_heading: Option<(u8, Option<String>, String)> = None;
    for event in Parser::new_ext(body, options) {
        match event {
            Event::Start(Tag::Heading { level, id, .. }) => {
                open_heading = Some((level as u8, id.map(|id| id.to_string()), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, id, text)) = open_heading.take() {
                    let anchor = anchors.unique(id.unwrap_or_else(|| slugify(&text)));
                    headings.push(Heading {
                        level,
                        text: text.trim().to_owned(),
                        anchor,
                    });
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, heading)) = open_heading.as_mut() {
                    heading.push_str(&text);
                }
            }
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                links.push(resolve_link(&dest_url, doc_path));
            }
            _ => {}
        }
    }
    Document::Markdown {
        text: body.to_owned(),
        frontmatter: frontmatter.map(frontmatter_pairs).unwrap_or_default(),
        headings,
        links,
    }
}

#[derive(Default)]
struct Anchors(HashMap<String, usize>);

impl Anchors {
    fn unique(&mut self, slug: String) -> String {
        let count = self.0.entry(slug.clone()).or_insert(0);
        let anchor = if *count == 0 {
            slug
        } else {
            format!("{slug}-{count}")
        };
        *count += 1;
        anchor
    }
}

/// GitHub-style heading slug: lowercase, spaces to `-`, punctuation dropped.
fn slugify(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn has_scheme(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    // Single letters are Windows drive prefixes, not schemes.
    scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn is_absolute_like(target: &str) -> bool {
    target.starts_with(['/', '\\'])
        || (target.len() >= 2
            && target.as_bytes()[1] == b':'
            && target.as_bytes()[0].is_ascii_alphabetic())
}

/// Lexically resolve a link destination relative to `doc_path`'s directory.
/// Absolute paths, schemes and anything escaping the root stay unresolved.
pub(crate) fn resolve_link(target: &str, doc_path: Option<&RelPath>) -> LinkRef {
    let unresolved = |external| LinkRef {
        target: target.to_owned(),
        resolved: None,
        external,
    };
    let trimmed = target.trim();
    if has_scheme(trimmed) {
        return unresolved(true);
    }
    if trimmed.is_empty() || trimmed.starts_with('#') || is_absolute_like(trimmed) {
        return unresolved(false);
    }
    let path = trimmed
        .split(['#', '?'])
        .next()
        .map(percent_decode)
        .unwrap_or_default();
    let mut parts: Vec<String> = doc_path
        .and_then(RelPath::parent)
        .map(|dir| dir.as_str().split('/').map(str::to_owned).collect())
        .unwrap_or_default();
    for segment in path.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return unresolved(false);
                }
            }
            segment => parts.push(segment.to_owned()),
        }
    }
    LinkRef {
        target: target.to_owned(),
        resolved: RelPath::new(parts.join("/")).ok(),
        external: false,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(value: &str) -> RelPath {
        RelPath::new(value).unwrap()
    }

    #[test]
    fn splits_frontmatter_and_folds_lists() {
        let text = "---\nname: pdf\ndescription: \"Work with PDFs\"\nallowed-tools:\n  - Read\n  - Bash\n---\n# Title\n";
        let (frontmatter, body) = split_frontmatter(text);
        assert_eq!(body, "# Title\n");
        assert_eq!(
            frontmatter_pairs(frontmatter.unwrap()),
            [
                ("name".to_owned(), "pdf".to_owned()),
                ("description".to_owned(), "Work with PDFs".to_owned()),
                ("allowed-tools".to_owned(), "Read, Bash".to_owned()),
            ]
        );
        assert_eq!(
            split_frontmatter("# no fm\n---\n"),
            (None, "# no fm\n---\n")
        );
        assert_eq!(
            split_frontmatter("---\nunterminated"),
            (None, "---\nunterminated")
        );
    }

    #[test]
    fn heading_anchors_are_github_style_and_unique() {
        let Document::Markdown { headings, .. } = parse_markdown(
            "# Hello, World!\n## Setup `cargo`\n## Setup cargo\n### Custom {#my-id}\n",
            None,
        ) else {
            panic!("expected markdown");
        };
        let anchors: Vec<&str> = headings.iter().map(|h| h.anchor.as_str()).collect();
        assert_eq!(
            anchors,
            ["hello-world", "setup-cargo", "setup-cargo-1", "my-id"]
        );
        assert_eq!(headings[1].text, "Setup cargo");
        assert_eq!(headings[0].level, 1);
    }

    #[test]
    fn links_resolve_inside_root_only() {
        let doc = rel("references/guide.md");
        let resolved = |target: &str| {
            resolve_link(target, Some(&doc))
                .resolved
                .map(|p| p.to_string())
        };
        assert_eq!(resolved("forms.md").as_deref(), Some("references/forms.md"));
        assert_eq!(
            resolved("../scripts/run.sh#top").as_deref(),
            Some("scripts/run.sh")
        );
        assert_eq!(
            resolved("./my%20file.md").as_deref(),
            Some("references/my file.md")
        );
        assert_eq!(resolved("../../outside.md"), None);
        assert_eq!(resolved("..\\..\\outside.md"), None);
        assert_eq!(resolved("/etc/passwd"), None);
        assert_eq!(resolved("C:/Windows/x"), None);
        assert_eq!(resolved("#section"), None);
        let external = resolve_link("https://example.invalid/x", Some(&doc));
        assert!(external.external && external.resolved.is_none());
        assert!(resolve_link("mailto:a@example.invalid", None).external);
    }
}
