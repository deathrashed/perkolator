//! Helpers shared by the MCP registry adapters.

use perkolator_core::{CancelToken, McpToolDecl, ProviderId, ResourceKind, SecretString};
use perkolator_marketplace::{HttpRequest, ProviderEnv, ProviderError, SearchRequest};
use serde_json::Value;

pub(crate) const REDACTED: &str = SecretString::REDACTED;

pub(crate) fn provider_id(id: &str) -> ProviderId {
    ProviderId::new(id).expect("built-in provider id is valid")
}

/// Every adapter in this module only serves MCP servers.
pub(crate) fn wants_mcp(request: &SearchRequest) -> bool {
    request.wants(ResourceKind::McpServer)
}

pub(crate) fn check_cancel(cancel: &CancelToken) -> Result<(), ProviderError> {
    if cancel.is_cancelled() {
        Err(ProviderError::Cancelled)
    } else {
        Ok(())
    }
}

/// GET `url` with the env's user agent plus `headers`; returns the JSON body
/// or the typed error for non-2xx statuses. Cancellation is checked on both
/// sides of the request so a late response never reaches the caller.
pub(crate) fn get_json(
    env: &ProviderEnv,
    url: String,
    headers: Vec<(&str, String)>,
    cancel: &CancelToken,
) -> Result<Value, ProviderError> {
    check_cancel(cancel)?;
    let mut request = HttpRequest::get(url).header("User-Agent", env.user_agent.clone());
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response = env.http.get(&request)?;
    check_cancel(cancel)?;
    response.json()
}

/// Percent-encode a URL path segment or query value, keeping only RFC 3986
/// unreserved characters.
pub(crate) fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Non-empty, trimmed string field.
pub(crate) fn text(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// `https://` URLs only; anything else from a registry is not offered.
pub(crate) fn https_url(value: &Value, key: &str) -> Option<String> {
    text(value, key).filter(|url| url.starts_with("https://"))
}

/// Tool declarations as published by a registry (`name`, `description`,
/// `inputSchema`). Entries without a name are dropped; duplicates by name
/// keep the first declaration.
pub(crate) fn tools_from(value: &Value) -> Vec<McpToolDecl> {
    let mut tools: Vec<McpToolDecl> = Vec::new();
    for tool in value.as_array().into_iter().flatten() {
        let Some(name) = text(tool, "name") else {
            continue;
        };
        if tools.iter().any(|known| known.name == name) {
            continue;
        }
        tools.push(McpToolDecl {
            name,
            description: text(tool, "description"),
            input_schema: tool
                .get("inputSchema")
                .filter(|schema| schema.is_object())
                .cloned(),
        });
    }
    tools
}

/// `Bearer {api_key}`-style values: every non-placeholder character is
/// fixed text, so the value carries no credential itself.
pub(crate) fn is_placeholder_template(value: &str) -> bool {
    let mut rest = value;
    let mut has_placeholder = false;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            return false;
        };
        let name = &rest[open + 1..open + close];
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return false;
        }
        has_placeholder = true;
        rest = &rest[open + close + 1..];
    }
    has_placeholder
}

/// Replace every `default` inside a JSON Schema's `properties` map. Config
/// schemas carry no secret flag, so defaults are treated as sensitive.
pub(crate) fn redact_schema_defaults(schema: &mut Value) {
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        for property in properties.values_mut() {
            if let Some(default) = property.get_mut("default") {
                *default = Value::String(REDACTED.into());
            }
            redact_schema_defaults(property);
        }
    }
}

pub(crate) fn pretty(value: &Value) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_recognised() {
        assert!(is_placeholder_template("Bearer {api_key}"));
        assert!(is_placeholder_template("{token}"));
        assert!(!is_placeholder_template("Bearer sk-live-123"));
        assert!(!is_placeholder_template("Bearer {api key}"));
        assert!(!is_placeholder_template("Bearer {unterminated"));
    }

    #[test]
    fn components_are_encoded() {
        assert_eq!(encode_component("io.github.a/b c"), "io.github.a%2Fb%20c");
    }

    #[test]
    fn schema_defaults_are_redacted() {
        let mut schema = serde_json::json!({
            "properties": {"key": {"default": "s3cret"}, "nested": {"properties": {"x": {"default": "1"}}}}
        });
        redact_schema_defaults(&mut schema);
        assert!(!schema.to_string().contains("s3cret"));
        assert_eq!(
            schema["properties"]["nested"]["properties"]["x"]["default"],
            REDACTED
        );
    }
}
