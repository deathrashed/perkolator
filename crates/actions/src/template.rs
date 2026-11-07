//! Offline prompt-template expansion. Pure text substitution: nothing is
//! evaluated or executed, and substituted values are never re-scanned.

use std::collections::BTreeMap;

use perkolator_core::{PromptVariable, TemplateSyntax};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum TemplateError {
    #[error("missing required template values: {}", names.join(", "))]
    MissingRequired { names: Vec<String> },
}

/// Expand `text` with `values`, falling back to declared defaults.
///
/// * `DoubleBrace`: `{{ name }}` (whitespace tolerant). Placeholders that are
///   neither supplied nor declared are left verbatim; declared optional
///   variables without a default become empty.
/// * `Arguments`: `$ARGUMENTS` from `values["ARGUMENTS"]`, `$1`..`$9` from
///   `values["1"]`..`values["9"]`; unsupplied ones become empty.
/// * `Plain`: no substitution.
///
/// Leading YAML frontmatter is stripped from the output in every syntax.
/// Every declared `required` variable without a value or default is an error.
pub fn expand_template(
    text: &str,
    syntax: TemplateSyntax,
    values: &BTreeMap<String, String>,
    variables: &[PromptVariable],
) -> Result<String, TemplateError> {
    let missing: Vec<String> = variables
        .iter()
        .filter(|var| var.required && !values.contains_key(&var.name) && var.default.is_none())
        .map(|var| var.name.clone())
        .collect();
    if !missing.is_empty() {
        return Err(TemplateError::MissingRequired { names: missing });
    }
    let body = strip_frontmatter(text);
    let lookup = |name: &str| -> Option<String> {
        values.get(name).cloned().or_else(|| {
            variables
                .iter()
                .find(|var| var.name == name)
                .map(|var| var.default.clone().unwrap_or_default())
        })
    };
    Ok(match syntax {
        TemplateSyntax::Plain => body.to_owned(),
        TemplateSyntax::DoubleBrace => expand_double_brace(body, lookup),
        TemplateSyntax::Arguments => expand_arguments(body, lookup),
    })
}

/// Remove a leading `---` … `---` (or `...`) YAML block, if well formed.
pub fn strip_frontmatter(text: &str) -> &str {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" || trimmed == "..." {
            return &rest[offset..];
        }
    }
    text
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')
}

fn expand_double_brace(body: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let replaced = after_open.find("}}").and_then(|end| {
            let name = after_open[..end].trim();
            let valid = !name.is_empty() && name.chars().all(is_name_char);
            valid
                .then(|| lookup(name))
                .flatten()
                .map(|value| (value, end))
        });
        match replaced {
            Some((value, end)) => {
                out.push_str(&value);
                rest = &after_open[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after_open;
            }
        }
    }
    out.push_str(rest);
    out
}

fn expand_arguments(body: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    const ARGUMENTS: &str = "ARGUMENTS";
    let continues_word = |tail: &str| {
        tail.chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    };
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(tail) = after
            .strip_prefix(ARGUMENTS)
            .filter(|tail| !continues_word(tail))
        {
            out.push_str(&lookup(ARGUMENTS).unwrap_or_default());
            rest = tail;
        } else if let Some(digit) = after.chars().next().filter(|ch| ('1'..='9').contains(ch)) {
            out.push_str(&lookup(&digit.to_string()).unwrap_or_default());
            rest = &after[1..];
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str, default: Option<&str>, required: bool) -> PromptVariable {
        PromptVariable {
            name: name.into(),
            description: None,
            default: default.map(Into::into),
            required,
        }
    }

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn double_brace_with_defaults_and_whitespace() {
        let text = "---\ndescription: x\n---\nReview {{ file }} for {{lang}} in {{  tone }} tone.";
        let vars = [
            var("file", None, true),
            var("lang", Some("Rust"), false),
            var("tone", None, false),
        ];
        let out = expand_template(
            text,
            TemplateSyntax::DoubleBrace,
            &values(&[("file", "main.rs")]),
            &vars,
        )
        .unwrap();
        assert_eq!(out, "Review main.rs for Rust in  tone.");
    }

    #[test]
    fn missing_required_lists_names_in_declaration_order() {
        let vars = [
            var("b", None, true),
            var("a", None, true),
            var("c", Some("x"), true),
        ];
        let error = expand_template(
            "{{a}}{{b}}",
            TemplateSyntax::DoubleBrace,
            &BTreeMap::new(),
            &vars,
        )
        .unwrap_err();
        assert_eq!(
            error,
            TemplateError::MissingRequired {
                names: vec!["b".into(), "a".into()]
            }
        );
        assert_eq!(error.to_string(), "missing required template values: b, a");
    }

    #[test]
    fn values_are_not_re_expanded() {
        let vars = [var("x", None, true), var("y", Some("SECOND"), false)];
        let out = expand_template(
            "{{x}} / {{y}}",
            TemplateSyntax::DoubleBrace,
            &values(&[("x", "{{y}} $ARGUMENTS")]),
            &vars,
        )
        .unwrap();
        assert_eq!(out, "{{y}} $ARGUMENTS / SECOND");
    }

    #[test]
    fn unknown_and_malformed_placeholders_stay_verbatim() {
        let out = expand_template(
            "{{unknown}} {{ bad name }} {{open",
            TemplateSyntax::DoubleBrace,
            &BTreeMap::new(),
            &[],
        )
        .unwrap();
        assert_eq!(out, "{{unknown}} {{ bad name }} {{open");
    }

    #[test]
    fn arguments_syntax() {
        let out = expand_template(
            "Fix $ARGUMENTS now; first=$1 second=$2 none=$3 price=$ $ARGUMENTSX",
            TemplateSyntax::Arguments,
            &values(&[("ARGUMENTS", "issue 42"), ("1", "a"), ("2", "$1")]),
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            "Fix issue 42 now; first=a second=$1 none= price=$ $ARGUMENTSX"
        );
    }

    #[test]
    fn arguments_required_is_enforced() {
        let vars = [var("ARGUMENTS", None, true)];
        assert!(
            expand_template(
                "$ARGUMENTS",
                TemplateSyntax::Arguments,
                &BTreeMap::new(),
                &vars
            )
            .is_err()
        );
    }

    #[test]
    fn plain_only_strips_frontmatter() {
        let out = expand_template(
            "---\na: 1\n---\nHi {{x}} $1",
            TemplateSyntax::Plain,
            &BTreeMap::new(),
            &[],
        )
        .unwrap();
        assert_eq!(out, "Hi {{x}} $1");
    }

    #[test]
    fn frontmatter_edge_cases() {
        assert_eq!(strip_frontmatter("no front"), "no front");
        assert_eq!(strip_frontmatter("---\nunterminated"), "---\nunterminated");
        assert_eq!(strip_frontmatter("---\r\na: 1\r\n---\r\nbody"), "body");
        assert_eq!(strip_frontmatter("\u{feff}---\na: 1\n...\nbody"), "body");
    }
}
