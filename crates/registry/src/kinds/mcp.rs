//! MCP server entries inside client configuration files. Entries are only
//! parsed; nothing is ever connected to or spawned.

use std::path::{Path, PathBuf};

use perkolator_core::{
    ConfigFormat, EnvBinding, InstallState, LocationRole, McpServerDetails, McpToolDecl,
    McpTransport, ResourceDetails, ResourceId, ResourceKind, ResourceLocation, Scope, SecretString,
    SourceKind, ValidationIssue,
};
use serde_json::{Map, Value};

use crate::files::{self, canonical};
use crate::scan::{RecordDraft, RecordMeta, Scanner, build_record, key_for};
use crate::{McpConfigFormat, ScanRoot, jsonc, text};

/// Gemini CLI uses `url` for SSE and `httpUrl` for streamable HTTP.
const GEMINI_CLIENT: &str = "gemini-cli";

pub(crate) fn scan_config(
    scanner: &mut Scanner,
    root: &ScanRoot,
    client: &str,
    format: McpConfigFormat,
) {
    let path = &root.path;
    let contents = match files::read_text(path, scanner.limits) {
        Ok(contents) => contents,
        Err(message) => return scanner.problem(path, message),
    };
    let value = match format {
        McpConfigFormat::CodexToml => match parse_toml(&contents) {
            Ok(value) => value,
            Err(message) => return scanner.problem(path, message),
        },
        _ => match jsonc::parse(&contents) {
            Ok(value) => value,
            Err(message) => return scanner.problem(path, format!("invalid JSON: {message}")),
        },
    };
    let config = ConfigContext {
        root,
        client,
        format,
        canonical_file: canonical(path),
    };
    let servers_key = match format {
        McpConfigFormat::McpServersJson => "mcpServers",
        McpConfigFormat::VsCodeServersJson => "servers",
        McpConfigFormat::CodexToml => "mcp_servers",
        McpConfigFormat::OpenCodeJson => "mcp",
    };
    if let Some(servers) = value.get(servers_key) {
        emit_entries(scanner, &config, servers, &format!("/{servers_key}"), None);
    }
    if format == McpConfigFormat::McpServersJson
        && let Some(projects) = value.get("projects").and_then(Value::as_object)
    {
        for (project, settings) in projects {
            if let Some(servers) = settings.get("mcpServers") {
                let pointer = format!("/projects/{}/mcpServers", pointer_escape(project));
                emit_entries(scanner, &config, servers, &pointer, Some(project));
            }
        }
    }
}

struct ConfigContext<'a> {
    root: &'a ScanRoot,
    client: &'a str,
    format: McpConfigFormat,
    canonical_file: PathBuf,
}

fn emit_entries(
    scanner: &mut Scanner,
    config: &ConfigContext,
    servers: &Value,
    pointer: &str,
    project: Option<&str>,
) {
    let path = &config.root.path;
    let Some(servers) = servers.as_object() else {
        return scanner.problem(path, format!("`{pointer}` is not an object"));
    };
    for (name, entry) in servers {
        if scanner.cancel.is_cancelled() {
            return;
        }
        match parse_entry(entry, config.client) {
            Ok(parsed) => {
                let record = entry_record(config, name, parsed, pointer, project);
                scanner.registry.insert(record);
            }
            Err(message) => scanner.problem(path, format!("MCP entry `{name}`: {message}")),
        }
    }
}

fn entry_record(
    config: &ConfigContext,
    name: &str,
    parsed: ParsedEntry,
    pointer: &str,
    project: Option<&str>,
) -> perkolator_core::ResourceRecord {
    let file_key = key_for(&config.canonical_file);
    let project_root = project.map(|project| canonical(Path::new(project)));
    let key = match &project_root {
        Some(project) => format!("{file_key}:{}:{name}", project.display()),
        None => format!("{file_key}:{name}"),
    };
    let source = match &project_root {
        Some(project) => SourceKind::AgentHost {
            host: config.client.to_owned(),
            scope: Scope::Project {
                root: project.clone(),
            },
        },
        None => config.root.source.clone(),
    };
    let state = match parsed.enabled {
        Some(true) => InstallState::Enabled,
        Some(false) => InstallState::Disabled,
        None => InstallState::Installed,
    };
    let location = ResourceLocation {
        path: config.root.path.clone(),
        role: LocationRole::ConfigEntry {
            file: config.root.path.clone(),
            entry: format!("{pointer}/{}", pointer_escape(name)),
        },
        state,
    };
    let summary = parsed
        .description
        .as_deref()
        .map(text::truncate)
        .unwrap_or_else(|| transport_summary(&parsed.transport));
    let details = McpServerDetails {
        client: config.client.to_owned(),
        config_file: config.root.path.clone(),
        config_format: match config.format {
            McpConfigFormat::CodexToml => ConfigFormat::Toml,
            _ => ConfigFormat::Json,
        },
        entry: name.to_owned(),
        transport: parsed.transport,
        enabled: parsed.enabled,
        declared_tools: parsed.tools,
    };
    build_record(
        RecordDraft {
            id: ResourceId::new(ResourceKind::McpServer, config.root.provider.clone(), key),
            name: name.to_owned(),
            summary,
            details: ResourceDetails::McpServer(details),
            source,
            locations: vec![location],
        },
        RecordMeta {
            issues: parsed.issues,
            ..RecordMeta::default()
        },
    )
}

#[derive(Debug)]
struct ParsedEntry {
    transport: McpTransport,
    enabled: Option<bool>,
    description: Option<String>,
    tools: Vec<McpToolDecl>,
    issues: Vec<ValidationIssue>,
}

fn parse_entry(entry: &Value, client: &str) -> Result<ParsedEntry, String> {
    let object = entry.as_object().ok_or("entry is not an object")?;
    let mut issues = Vec::new();
    let declared_type = object.get("type").and_then(Value::as_str);
    let http_url = object.get("httpUrl").and_then(Value::as_str);
    let url = object
        .get("url")
        .or_else(|| object.get("serverUrl"))
        .and_then(Value::as_str);
    let headers = bindings(object.get("headers").or_else(|| object.get("http_headers")));

    let stdio = || -> Result<McpTransport, String> {
        let (command, mut args) = match object.get("command") {
            Some(Value::String(command)) => (command.clone(), Vec::new()),
            Some(Value::Array(parts)) => {
                let mut parts = parts.iter().filter_map(scalar_string);
                let command = parts.next().ok_or("`command` array is empty")?;
                (command, parts.collect())
            }
            Some(_) => return Err("`command` is not a string".into()),
            None => return Err("stdio entry has no `command`".into()),
        };
        if let Some(Value::Array(extra)) = object.get("args") {
            args.extend(extra.iter().filter_map(scalar_string));
        }
        Ok(McpTransport::Stdio {
            command,
            args,
            cwd: object.get("cwd").and_then(Value::as_str).map(PathBuf::from),
            env: bindings(object.get("env").or_else(|| object.get("environment"))),
        })
    };
    let remote = |sse: bool, url: Option<&str>| -> Result<McpTransport, String> {
        let url = url.ok_or("remote entry has no `url`")?.to_owned();
        let headers = headers.clone();
        Ok(if sse {
            McpTransport::Sse { url, headers }
        } else {
            McpTransport::StreamableHttp { url, headers }
        })
    };

    let transport = match declared_type {
        Some(kind) => match kind.to_ascii_lowercase().as_str() {
            "stdio" | "local" => stdio()?,
            "http" | "streamable-http" | "streamablehttp" | "remote" => {
                remote(false, http_url.or(url))?
            }
            "sse" => remote(true, url.or(http_url))?,
            _ => McpTransport::Unknown {
                declared_type: Some(kind.to_owned()),
            },
        },
        None if object.contains_key("command") => stdio()?,
        None if http_url.is_some() => remote(false, http_url)?,
        None if url.is_some() => remote(client == GEMINI_CLIENT, url)?,
        None => {
            issues.push(ValidationIssue::warning(
                "entry declares neither `command` nor `url`; transport unknown",
            ));
            McpTransport::Unknown {
                declared_type: None,
            }
        }
    };

    let enabled = match (object.get("enabled"), object.get("disabled")) {
        (Some(Value::Bool(enabled)), _) => Some(*enabled),
        (_, Some(Value::Bool(disabled))) => Some(!disabled),
        _ => None,
    };
    Ok(ParsedEntry {
        transport,
        enabled,
        description: object
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|description| !description.trim().is_empty()),
        tools: declared_tools(object.get("tools")),
        issues,
    })
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn bindings(value: Option<&Value>) -> Vec<EnvBinding> {
    let Some(Value::Object(map)) = value else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, value)| {
            scalar_string(value).map(|value| EnvBinding {
                name: name.clone(),
                value: SecretString::new(value),
            })
        })
        .collect()
}

/// Statically declared tools (`tools: [{name, description, inputSchema}]`).
fn declared_tools(value: Option<&Value>) -> Vec<McpToolDecl> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|tool: &Map<String, Value>| {
            Some(McpToolDecl {
                name: tool.get("name")?.as_str()?.to_owned(),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                input_schema: tool
                    .get("inputSchema")
                    .or_else(|| tool.get("input_schema"))
                    .cloned(),
            })
        })
        .collect()
}

/// Short description of the transport without arguments, query strings or
/// credentials.
fn transport_summary(transport: &McpTransport) -> String {
    match transport {
        McpTransport::Stdio { command, .. } => format!("stdio server · {command}"),
        McpTransport::StreamableHttp { url, .. } => {
            format!("streamable HTTP server · {}", redact_url(url))
        }
        McpTransport::Sse { url, .. } => format!("SSE server · {}", redact_url(url)),
        McpTransport::Unknown {
            declared_type: Some(kind),
        } => format!("MCP server ({kind} transport)"),
        McpTransport::Unknown {
            declared_type: None,
        } => "MCP server (unknown transport)".into(),
    }
}

/// `scheme://host/path` only: drops userinfo, query and fragment.
fn redact_url(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or_default();
    match without_query.split_once("://") {
        Some((scheme, rest)) => {
            let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
            format!("{scheme}://{rest}")
        }
        None => without_query.to_owned(),
    }
}

fn pointer_escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Parse TOML into JSON values. Error messages carry only a line number and
/// the parser's message, never the offending source text.
fn parse_toml(contents: &str) -> Result<Value, String> {
    match toml::from_str::<toml::Table>(contents) {
        Ok(table) => Ok(toml_to_json(toml::Value::Table(table))),
        Err(error) => {
            let line = error.span().map(|span| {
                contents[..span.start.min(contents.len())]
                    .matches('\n')
                    .count()
                    + 1
            });
            Err(match line {
                Some(line) => format!("invalid TOML at line {line}: {}", error.message()),
                None => format!("invalid TOML: {}", error.message()),
            })
        }
    }
}

fn toml_to_json(value: toml::Value) -> Value {
    match value {
        toml::Value::String(text) => Value::String(text),
        toml::Value::Integer(int) => Value::from(int),
        toml::Value::Float(float) => {
            serde_json::Number::from_f64(float).map_or(Value::Null, Value::Number)
        }
        toml::Value::Boolean(flag) => Value::Bool(flag),
        toml::Value::Datetime(datetime) => Value::String(datetime.to_string()),
        toml::Value::Array(items) => Value::Array(items.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => Value::Object(
            table
                .into_iter()
                .map(|(key, value)| (key, toml_to_json(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: Value) -> ParsedEntry {
        parse_entry(&value, "cursor").unwrap()
    }

    #[test]
    fn transport_inference() {
        assert!(matches!(
            parse(json!({"command": "npx", "args": ["-y", "x", 3]})).transport,
            McpTransport::Stdio { ref args, .. } if args == &["-y", "x", "3"]
        ));
        assert!(matches!(
            parse(json!({"url": "https://h/mcp"})).transport,
            McpTransport::StreamableHttp { .. }
        ));
        assert!(matches!(
            parse(json!({"type": "sse", "url": "https://h/sse"})).transport,
            McpTransport::Sse { .. }
        ));
        assert!(matches!(
            parse(json!({"type": "streamableHttp", "url": "https://h"})).transport,
            McpTransport::StreamableHttp { .. }
        ));
        assert!(matches!(
            parse_entry(&json!({"url": "https://h/sse"}), GEMINI_CLIENT)
                .unwrap()
                .transport,
            McpTransport::Sse { .. }
        ));
        let unknown = parse(json!({"type": "websocket"}));
        assert!(matches!(
            unknown.transport,
            McpTransport::Unknown { declared_type: Some(ref kind) } if kind == "websocket"
        ));
    }

    #[test]
    fn opencode_local_command_array_and_environment() {
        let parsed = parse(json!({
            "type": "local",
            "command": ["bun", "x", "server"],
            "environment": {"KEY": "v"},
            "enabled": false
        }));
        assert_eq!(parsed.enabled, Some(false));
        match parsed.transport {
            McpTransport::Stdio {
                command, args, env, ..
            } => {
                assert_eq!(command, "bun");
                assert_eq!(args, vec!["x", "server"]);
                assert_eq!(env[0].name, "KEY");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn disabled_flag_and_bad_entries() {
        assert_eq!(
            parse(json!({"command": "x", "disabled": true})).enabled,
            Some(false)
        );
        assert_eq!(parse(json!({"command": "x"})).enabled, None);
        assert!(parse_entry(&json!("nope"), "cursor").is_err());
        assert!(parse_entry(&json!({"type": "stdio"}), "cursor").is_err());
        assert!(parse_entry(&json!({"type": "http"}), "cursor").is_err());
    }

    #[test]
    fn summaries_never_show_query_or_userinfo() {
        assert_eq!(
            redact_url("https://user:pw@host.example/mcp?token=abc#x"),
            "https://host.example/mcp"
        );
        let summary = transport_summary(&McpTransport::StreamableHttp {
            url: "https://host/mcp?key=secret".into(),
            headers: vec![],
        });
        assert!(!summary.contains("secret"));
    }

    #[test]
    fn toml_errors_do_not_echo_source() {
        let error = parse_toml("[mcp_servers.x]\ntoken = \"SECRET_VALUE\" oops\n").unwrap_err();
        assert!(error.contains("line 2"), "{error}");
        assert!(!error.contains("SECRET_VALUE"), "{error}");
    }

    #[test]
    fn declared_tools_are_read() {
        let parsed = parse(json!({
            "command": "x",
            "tools": [{"name": "search", "description": "Find", "inputSchema": {"type": "object"}}, "bare"]
        }));
        assert_eq!(parsed.tools.len(), 1);
        assert_eq!(parsed.tools[0].name, "search");
    }
}
