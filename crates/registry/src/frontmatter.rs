//! YAML frontmatter between leading `---` lines, converted to JSON values so
//! unknown keys can be preserved verbatim in typed details.

use std::collections::BTreeMap;

use serde_json::Value;

pub(crate) type Fields = BTreeMap<String, Value>;

/// Result of reading a Markdown document's optional frontmatter.
pub(crate) struct Document<'a> {
    /// `None` when the document has no frontmatter block at all.
    pub fields: Option<Result<Fields, String>>,
    pub body: &'a str,
}

/// Split a document into its frontmatter block (without fences) and body.
fn split(text: &str) -> (Option<Result<&str, String>>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" || trimmed == "..." {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return (Some(Ok(yaml)), body);
        }
        offset += line.len();
    }
    (
        Some(Err("frontmatter is not terminated by `---`".into())),
        text,
    )
}

/// Strict YAML frontmatter parsing (Agent Skills, agents, prompts).
pub(crate) fn parse(text: &str) -> Document<'_> {
    let (block, body) = split(text);
    let fields = block.map(|block| block.and_then(parse_yaml_mapping));
    Document { fields, body }
}

/// Frontmatter parsing that falls back to `key: value` lines when the block
/// is not valid YAML (Cursor `.mdc` files commonly contain unquoted globs
/// such as `globs: *.rs`). Returns whether the fallback was used.
pub(crate) fn parse_lenient(text: &str) -> (Document<'_>, bool) {
    let (block, body) = split(text);
    match block {
        Some(Ok(yaml)) => match parse_yaml_mapping(yaml) {
            Ok(fields) => (
                Document {
                    fields: Some(Ok(fields)),
                    body,
                },
                false,
            ),
            Err(_) => (
                Document {
                    fields: Some(Ok(parse_key_values(yaml))),
                    body,
                },
                true,
            ),
        },
        other => (
            Document {
                fields: other.map(|block| block.map(|_| Fields::new())),
                body,
            },
            false,
        ),
    }
}

fn parse_yaml_mapping(yaml: &str) -> Result<Fields, String> {
    if yaml.trim().is_empty() {
        return Ok(Fields::new());
    }
    let value: serde_norway::Value = serde_norway::from_str(yaml)
        .map_err(|error| format!("invalid YAML frontmatter: {error}"))?;
    match yaml_to_json(value) {
        Value::Object(map) => Ok(map.into_iter().collect()),
        Value::Null => Ok(Fields::new()),
        _ => Err("frontmatter is not a key/value mapping".into()),
    }
}

fn parse_key_values(block: &str) -> Fields {
    block
        .lines()
        .filter(|line| !line.starts_with([' ', '\t', '#', '-']))
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| {
            let value = value.trim().trim_matches(['"', '\'']);
            let value = match value {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "" => Value::Null,
                other => Value::String(other.to_owned()),
            };
            (key.trim().to_owned(), value)
        })
        .collect()
}

pub(crate) fn yaml_to_json(value: serde_norway::Value) -> Value {
    use serde_norway::Value as Yaml;
    match value {
        Yaml::Null => Value::Null,
        Yaml::Bool(value) => Value::Bool(value),
        Yaml::Number(number) => {
            if let Some(int) = number.as_i64() {
                Value::from(int)
            } else if let Some(uint) = number.as_u64() {
                Value::from(uint)
            } else {
                number
                    .as_f64()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(Value::Null, Value::Number)
            }
        }
        Yaml::String(value) => Value::String(value),
        Yaml::Sequence(items) => Value::Array(items.into_iter().map(yaml_to_json).collect()),
        Yaml::Mapping(mapping) => Value::Object(
            mapping
                .into_iter()
                .map(|(key, value)| (yaml_key(key), yaml_to_json(value)))
                .collect(),
        ),
        Yaml::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

fn yaml_key(key: serde_norway::Value) -> String {
    match yaml_to_json(key) {
        Value::String(key) => key,
        other => other.to_string(),
    }
}

/// Non-empty string field.
pub(crate) fn string(fields: &Fields, key: &str) -> Option<String> {
    fields.get(key).and_then(value_string)
}

/// Strings and numbers rendered as text; other types are not coerced.
pub(crate) fn value_string(value: &Value) -> Option<String> {
    let text = match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// A list given either as a YAML/JSON array or as a comma-separated string.
pub(crate) fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Value::Array(items)) => items.iter().filter_map(value_string).collect(),
        _ => Vec::new(),
    }
}

/// Look up `key`, falling back to `metadata.<key>`.
pub(crate) fn field_or_metadata<'a>(fields: &'a Fields, key: &str) -> Option<&'a Value> {
    fields.get(key).or_else(|| {
        fields
            .get("metadata")
            .and_then(|metadata| metadata.get(key))
    })
}

/// An http(s) URL, the only form accepted as a verified source link.
pub(crate) fn http_url(value: Option<&Value>) -> Option<String> {
    let url = match value? {
        Value::String(url) => url.trim(),
        Value::Object(map) => map.get("url")?.as_str()?.trim(),
        _ => return None,
    };
    (url.starts_with("https://") || url.starts_with("http://")).then(|| url.to_owned())
}

/// Author given as a string or `{ name = .. }`.
pub(crate) fn author(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Object(map) => map.get("name").and_then(value_string),
        other => value_string(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let doc = parse("---\nname: x\ntags: [a, b]\nn: 3\n---\n# Body\n");
        let fields = doc.fields.unwrap().unwrap();
        assert_eq!(string(&fields, "name").as_deref(), Some("x"));
        assert_eq!(string_list(fields.get("tags")), vec!["a", "b"]);
        assert_eq!(fields["n"], 3);
        assert_eq!(doc.body, "# Body\n");
    }

    #[test]
    fn no_frontmatter_is_none() {
        let doc = parse("# Title\n---\n");
        assert!(doc.fields.is_none());
        assert_eq!(doc.body, "# Title\n---\n");
    }

    #[test]
    fn unterminated_and_invalid_are_errors() {
        assert!(parse("---\nname: x\n").fields.unwrap().is_err());
        assert!(
            parse("---\nname: [unclosed\n---\n")
                .fields
                .unwrap()
                .is_err()
        );
        assert!(parse("---\n- a\n- b\n---\n").fields.unwrap().is_err());
    }

    #[test]
    fn crlf_and_non_string_keys() {
        let doc = parse("---\r\n1: one\r\nok: true\r\n---\r\nbody");
        let fields = doc.fields.unwrap().unwrap();
        assert_eq!(fields["1"], "one");
        assert_eq!(fields["ok"], true);
        assert_eq!(doc.body, "body");
    }

    #[test]
    fn lenient_parses_unquoted_globs() {
        let (doc, fallback) = parse_lenient("---\nglobs: *.rs, *.toml\nalwaysApply: true\n---\n");
        assert!(fallback);
        let fields = doc.fields.unwrap().unwrap();
        assert_eq!(string_list(fields.get("globs")), vec!["*.rs", "*.toml"]);
        assert_eq!(fields["alwaysApply"], true);
    }
}
