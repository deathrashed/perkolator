//! Generic MCP Registry API (v0.1), shared by `mcp-official` and the
//! PulseMCP sub-registry: latest-version listing with cursor pagination,
//! per-version detail, and redaction of package/remote input values.

use perkolator_core::{ProviderId, ResourceKind};
use perkolator_marketplace::{
    ProviderError, RemoteActions, RemoteItem, SearchPage, SearchRequest, now_unix,
};
use serde_json::Value;

use super::common::{REDACTED, encode_component, https_url, is_placeholder_template, text};

/// Server-side maximum for `limit`.
pub(crate) const MAX_LIMIT: usize = 100;

pub(crate) struct RegistryApi {
    pub source: ProviderId,
    /// API root including the version, e.g. `https://host/v0.1`.
    pub base: &'static str,
    /// Shortest trimmed `search` the server accepts (0 = any).
    pub min_search_len: usize,
    /// Version URLs are publicly readable and usable as a canonical URI.
    pub public_canonical: bool,
}

impl RegistryApi {
    /// `None` when the query is shorter than the server accepts; the caller
    /// returns no results instead of a request that is known to fail.
    pub fn list_url(&self, request: &SearchRequest) -> Option<String> {
        let mut url = format!(
            "{}/servers?version=latest&limit={}",
            self.base,
            request.page_size.clamp(1, MAX_LIMIT)
        );
        let query = request.query.trim();
        if !query.is_empty() {
            if query.chars().count() < self.min_search_len {
                return None;
            }
            url.push_str("&search=");
            url.push_str(&encode_component(query));
        }
        if let Some(cursor) = request
            .cursor
            .as_deref()
            .filter(|cursor| !cursor.is_empty())
        {
            url.push_str("&cursor=");
            url.push_str(&encode_component(cursor));
        }
        Some(url)
    }

    /// `version = None` resolves the latest version.
    pub fn version_url(&self, name: &str, version: Option<&str>) -> String {
        format!(
            "{}/servers/{}/versions/{}",
            self.base,
            encode_component(name),
            encode_component(version.unwrap_or("latest"))
        )
    }

    pub fn parse_list(&self, json: &Value) -> Result<SearchPage, ProviderError> {
        let servers = json["servers"]
            .as_array()
            .ok_or_else(|| ProviderError::malformed("registry list without a `servers` array"))?;
        let fetched_at_unix = now_unix();
        let items = servers
            .iter()
            .filter(|entry| !is_deleted(entry))
            .filter_map(|entry| self.item(entry, fetched_at_unix))
            .collect();
        let next_cursor = json["metadata"]["nextCursor"]
            .as_str()
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned);
        Ok(SearchPage {
            items,
            next_cursor,
            total: None,
        })
    }

    /// One `ServerResponse` (`{server, _meta}`); `None` without a name.
    pub fn item(&self, entry: &Value, fetched_at_unix: u64) -> Option<RemoteItem> {
        let server = &entry["server"];
        let name = text(server, "name")?;
        let repository = &server["repository"];
        let repo_url = https_url(repository, "url");
        let resource_path = repo_url
            .as_ref()
            .and_then(|_| text(repository, "subfolder"))
            .map(|path| path.trim_matches('/').to_owned())
            .filter(|path| !path.is_empty() && !path.split('/').any(|part| part == ".."));
        Some(RemoteItem {
            source: self.source.clone(),
            upstream_id: name.clone(),
            kind: ResourceKind::McpServer,
            title: text(server, "title").unwrap_or_else(|| name.clone()),
            summary: text(server, "description").unwrap_or_default(),
            canonical_url: self.public_canonical.then(|| self.version_url(&name, None)),
            actions: RemoteActions {
                view_source: repo_url.is_some(),
                fetch_files: false,
                install: false,
            },
            repo_url,
            resource_path,
            // Reverse-DNS namespace, ownership verified by the registry.
            author: name
                .split_once('/')
                .map(|(namespace, _)| namespace.to_owned()),
            package_ref: Some(name),
            version: text(server, "version"),
            categories: Vec::new(),
            fetched_at_unix,
        })
    }

    /// Item plus the redacted `ServerResponse` for a version detail body.
    pub fn parse_detail(&self, json: &Value) -> Result<(RemoteItem, Value), ProviderError> {
        if !json["server"].is_object() {
            return Err(ProviderError::malformed("registry detail without `server`"));
        }
        let item = self
            .item(json, now_unix())
            .ok_or_else(|| ProviderError::malformed("registry detail without a server name"))?;
        let mut manifest = json.clone();
        redact_server(&mut manifest["server"]);
        Ok((item, manifest))
    }
}

/// Soft-deleted in any registry metadata namespace.
fn is_deleted(entry: &Value) -> bool {
    entry["_meta"]
        .as_object()
        .is_some_and(|meta| meta.values().any(|ext| ext["status"] == "deleted"))
}

/// Blank credential-bearing values in a `server.json`: literal (non-template)
/// environment variable and header values, plus `value`/`default` of any
/// input flagged `isSecret`. Placeholder templates like `Bearer {api_key}`
/// are kept because they describe configuration without holding a secret.
pub(crate) fn redact_server(server: &mut Value) {
    for package in array_mut(server, "packages") {
        for variable in array_mut(package, "environmentVariables") {
            redact_input(variable, true);
        }
        for key in ["packageArguments", "runtimeArguments"] {
            for argument in array_mut(package, key) {
                redact_input(argument, false);
            }
        }
        if let Some(transport) = package.get_mut("transport") {
            redact_transport(transport);
        }
    }
    for remote in array_mut(server, "remotes") {
        redact_transport(remote);
    }
}

fn redact_transport(transport: &mut Value) {
    for header in array_mut(transport, "headers") {
        redact_input(header, true);
    }
    redact_variables(transport);
}

fn redact_input(input: &mut Value, literal_is_sensitive: bool) {
    let secret = input["isSecret"].as_bool().unwrap_or(false);
    if let Some(value) = input.get_mut("value") {
        let template = value.as_str().is_some_and(is_placeholder_template);
        if !template && (secret || literal_is_sensitive) {
            *value = Value::String(REDACTED.into());
        }
    }
    if secret && let Some(default) = input.get_mut("default") {
        *default = Value::String(REDACTED.into());
    }
    redact_variables(input);
}

fn redact_variables(holder: &mut Value) {
    if let Some(variables) = holder.get_mut("variables").and_then(Value::as_object_mut) {
        for variable in variables.values_mut() {
            redact_input(variable, false);
        }
    }
}

fn array_mut<'a>(value: &'a mut Value, key: &str) -> impl Iterator<Item = &'a mut Value> {
    value
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn redaction_keeps_templates_and_public_defaults() {
        let mut server = json!({
            "packages": [{
                "environmentVariables": [
                    {"name": "TOKEN", "value": "sk-live-abc"},
                    {"name": "MODE", "default": "fast"},
                    {"name": "KEY", "isSecret": true, "default": "dflt-secret"}
                ],
                "runtimeArguments": [{"type": "positional", "value": "-y"}],
                "transport": {"type": "stdio"}
            }],
            "remotes": [{
                "type": "sse",
                "headers": [{"name": "Authorization", "value": "Bearer {key}", "isSecret": true,
                             "variables": {"key": {"isSecret": true, "value": "raw-var"}}}]
            }]
        });
        redact_server(&mut server);
        let text = server.to_string();
        for secret in ["sk-live-abc", "dflt-secret", "raw-var"] {
            assert!(!text.contains(secret), "{secret} leaked");
        }
        assert!(text.contains("Bearer {key}"));
        assert!(text.contains("\"fast\""));
        assert!(text.contains("\"-y\""));
    }
}
