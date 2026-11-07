//! Prompt templates and slash commands: `*.md` with optional frontmatter.

use perkolator_core::{
    PromptTemplateDetails, PromptVariable, ResourceDetails, ResourceId, ResourceKind,
    TemplateSyntax, ValidationIssue,
};
use serde_json::Value;

use crate::ScanRoot;
use crate::files::{self, canonical, file_stem};
use crate::frontmatter::{self, Fields};
use crate::scan::{
    RecordDraft, RecordMeta, Scanner, build_record, file_locations, key_for, scope_for,
};
use crate::text;

const ARGUMENTS_VARIABLE: &str = "ARGUMENTS";

pub(crate) fn scan(scanner: &mut Scanner, root: &ScanRoot, host: Option<&str>) {
    let scope = scope_for(&root.source);
    for found in scanner.document_files(&root.path, &["md"], host) {
        if scanner.cancel.is_cancelled() {
            return;
        }
        let contents = match files::read_text(&found, scanner.limits) {
            Ok(contents) => contents,
            Err(message) => {
                scanner.problem(&found, message);
                continue;
            }
        };
        let mut issues = Vec::new();
        let document = frontmatter::parse(&contents);
        let fields = match document.fields {
            Some(Ok(fields)) => fields,
            Some(Err(message)) => {
                scanner.problem(&found, message.clone());
                issues.push(ValidationIssue::error(message));
                Fields::new()
            }
            None => Fields::new(),
        };
        let description = frontmatter::string(&fields, "description");
        let hint = frontmatter::string(&fields, "argument-hint");
        let (syntax, variables) = template(&fields, document.body, hint.as_deref());
        let canonical_file = canonical(&found);
        let details = PromptTemplateDetails {
            file: found.clone(),
            host: host.map(str::to_owned),
            syntax,
            variables,
        };
        let summary = text::summary(description.as_deref().or(hint.as_deref()), document.body);
        let record = build_record(
            RecordDraft {
                id: ResourceId::new(
                    ResourceKind::Prompt,
                    root.provider.clone(),
                    key_for(&canonical_file),
                ),
                name: frontmatter::string(&fields, "name").unwrap_or_else(|| file_stem(&found)),
                summary,
                details: ResourceDetails::Prompt(details),
                source: root.source.clone(),
                locations: file_locations(&found, &canonical_file, host, &scope),
            },
            RecordMeta {
                tags: frontmatter::string_list(fields.get("tags")),
                fingerprint: Some(text::fingerprint(contents.as_bytes())),
                issues,
                ..RecordMeta::default()
            },
        );
        scanner.upsert(Some(&canonical_file), record);
    }
}

/// Template syntax and variables: `{{var}}` placeholders (body order, merged
/// with frontmatter definitions) win over `$ARGUMENTS`/`$1` style.
pub(crate) fn template(
    fields: &Fields,
    body: &str,
    hint: Option<&str>,
) -> (TemplateSyntax, Vec<PromptVariable>) {
    let definitions = variable_definitions(fields.get("variables"));
    let placeholders = double_brace_names(body);
    if !placeholders.is_empty() || !definitions.is_empty() {
        let mut variables: Vec<PromptVariable> = placeholders
            .iter()
            .map(|name| {
                definitions
                    .iter()
                    .find(|definition| &definition.name == name)
                    .cloned()
                    .unwrap_or_else(|| PromptVariable {
                        name: name.clone(),
                        description: None,
                        default: None,
                        required: true,
                    })
            })
            .collect();
        for definition in definitions {
            if !placeholders.contains(&definition.name) {
                variables.push(definition);
            }
        }
        return (TemplateSyntax::DoubleBrace, variables);
    }
    let arguments = argument_variables(body, hint);
    if arguments.is_empty() {
        (TemplateSyntax::Plain, Vec::new())
    } else {
        (TemplateSyntax::Arguments, arguments)
    }
}

fn variable_definitions(value: Option<&Value>) -> Vec<PromptVariable> {
    let from_object = |name: String, spec: &Value| {
        let default = spec.get("default").and_then(frontmatter::value_string);
        PromptVariable {
            name,
            description: spec.get("description").and_then(frontmatter::value_string),
            required: spec
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(default.is_none()),
            default,
        }
    };
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(name) => Some(PromptVariable {
                    name: name.clone(),
                    description: None,
                    default: None,
                    required: true,
                }),
                Value::Object(_) => {
                    let name = item.get("name").and_then(frontmatter::value_string)?;
                    Some(from_object(name, item))
                }
                _ => None,
            })
            .collect(),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(name, spec)| match spec {
                Value::String(description) => PromptVariable {
                    name: name.clone(),
                    description: Some(description.clone()),
                    default: None,
                    required: true,
                },
                other => from_object(name.clone(), other),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Distinct `{{ name }}` placeholders in order of first appearance. Block
/// helpers (`{{#if}}`, `{{/if}}`) and expressions with spaces are ignored.
fn double_brace_names(body: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let inner = after[..end].trim();
        let valid = inner
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && inner
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
        if valid && !names.iter().any(|name| name == inner) {
            names.push(inner.to_owned());
        }
        rest = &after[end + 2..];
    }
    names
}

/// `$ARGUMENTS` and positional `$1`..`$9`-style placeholders.
fn argument_variables(body: &str, hint: Option<&str>) -> Vec<PromptVariable> {
    let mut variables = Vec::new();
    if body.contains("$ARGUMENTS") {
        variables.push(PromptVariable {
            name: ARGUMENTS_VARIABLE.into(),
            description: hint.map(str::to_owned),
            default: None,
            required: false,
        });
    }
    let bytes = body.as_bytes();
    let mut positions: Vec<u32> = Vec::new();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'$' {
            continue;
        }
        let digits: String = bytes[index + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .map(|&byte| byte as char)
            .collect();
        if let Ok(position) = digits.parse::<u32>()
            && position > 0
            && !positions.contains(&position)
        {
            positions.push(position);
        }
    }
    positions.sort_unstable();
    variables.extend(positions.into_iter().map(|position| PromptVariable {
        name: position.to_string(),
        description: None,
        default: None,
        required: false,
    }));
    variables
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(yaml: &str) -> Fields {
        frontmatter::parse(&format!("---\n{yaml}\n---\n"))
            .fields
            .unwrap()
            .unwrap()
    }

    #[test]
    fn double_brace_order_and_merge() {
        let defs = fields(
            "variables:\n  - name: scope\n    description: Area\n    default: core\n  - name: extra\n",
        );
        let (syntax, variables) =
            template(&defs, "{{type}}({{ scope }}): {{type}} {{#if x}}", None);
        assert_eq!(syntax, TemplateSyntax::DoubleBrace);
        let names: Vec<_> = variables.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["type", "scope", "extra"]);
        assert!(variables[0].required);
        assert_eq!(variables[1].default.as_deref(), Some("core"));
        assert!(!variables[1].required);
    }

    #[test]
    fn arguments_syntax() {
        let (syntax, variables) = template(
            &Fields::new(),
            "Review $ARGUMENTS then $2 and $1",
            Some("<pr>"),
        );
        assert_eq!(syntax, TemplateSyntax::Arguments);
        let names: Vec<_> = variables.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["ARGUMENTS", "1", "2"]);
        assert_eq!(variables[0].description.as_deref(), Some("<pr>"));
    }

    #[test]
    fn plain_prompt() {
        let (syntax, variables) = template(&Fields::new(), "Just text, costs $ 5.", None);
        assert_eq!(syntax, TemplateSyntax::Plain);
        assert!(variables.is_empty());
    }

    #[test]
    fn map_style_definitions() {
        let defs = fields("variables:\n  lang: Programming language\n");
        let (_, variables) = template(&defs, "Write {{lang}}", None);
        assert_eq!(
            variables[0].description.as_deref(),
            Some("Programming language")
        );
    }
}
