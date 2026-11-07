//! Tolerant JSON: accepts `//` and `/* */` comments and trailing commas
//! (JSONC as used by VS Code-style configs) without a JSON5 dependency.

/// Parse strict JSON first, falling back to JSONC normalisation.
pub(crate) fn parse(text: &str) -> Result<serde_json::Value, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_json::from_str(text) {
        Ok(value) => Ok(value),
        Err(strict_error) => serde_json::from_str(&strip_trailing_commas(&strip_comments(text)))
            .map_err(|_| strict_error.to_string()),
    }
}

/// Remove comments outside of string literals. Newlines inside comments are
/// kept so that error line numbers stay meaningful.
pub(crate) fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            match ch {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                }
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (ch, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(ch);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    if next == '\n' {
                        out.push('\n');
                    }
                    previous = next;
                }
                out.push(' ');
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Remove commas that are followed (after whitespace) by `}` or `]`.
/// Expects comments to have been stripped already.
pub(crate) fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            out.push(ch);
            if ch == '\\' {
                if let Some(&escaped) = chars.get(index + 1) {
                    out.push(escaped);
                    index += 1;
                }
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
            out.push(ch);
        } else if ch == ',' {
            let next = chars[index + 1..].iter().find(|c| !c.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_trailing_commas() {
        let text = r#"{
            // line comment with "quote"
            "url": "https://example.invalid/a//b", /* block
            comment */
            "list": [1, 2,],
            "s": "not /* a comment */ \"esc\"",
        }"#;
        let value = parse(text).unwrap();
        assert_eq!(value["url"], "https://example.invalid/a//b");
        assert_eq!(value["list"], serde_json::json!([1, 2]));
        assert_eq!(value["s"], "not /* a comment */ \"esc\"");
    }

    #[test]
    fn invalid_json_reports_error() {
        assert!(parse("{ \"a\": }").is_err());
    }

    #[test]
    fn keeps_commas_inside_strings() {
        let value = parse(r#"{"a": "x,}", }"#).unwrap();
        assert_eq!(value["a"], "x,}");
    }
}
