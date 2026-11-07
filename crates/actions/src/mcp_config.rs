//! Per-entry MCP configuration snippets for copying.
//!
//! Secrets are redacted unless the caller passes `include_secrets` from an
//! explicit, user-confirmed UI path. Redaction covers env/header values
//! (via [`SecretString::display_value`]), URL credentials and query strings,
//! and arguments that follow or embed token/key/secret/password flags.

use perkolator_core::{ConfigFormat, EnvBinding, McpServerDetails, McpTransport, SecretString};
use serde_json::{Map, Value, json};

const REDACTED: &str = SecretString::REDACTED;
const SENSITIVE_FLAG_WORDS: &[&str] = &["token", "key", "secret", "password", "passwd", "auth"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigSnippet {
    pub text: String,
    /// True when at least one value was replaced with a redaction marker.
    pub redacted: bool,
}

/// `None` when the transport is unknown and no faithful snippet exists.
pub fn mcp_config_snippet(mcp: &McpServerDetails, include_secrets: bool) -> Option<ConfigSnippet> {
    let mut redactor = Redactor {
        include_secrets,
        redacted: false,
    };
    let server = ServerFields::from_transport(&mcp.transport, &mut redactor)?;
    let text = match mcp.config_format {
        ConfigFormat::Json => json_snippet(&mcp.entry, &server),
        ConfigFormat::Toml => toml_snippet(&mcp.entry, &server),
    };
    Some(ConfigSnippet {
        text,
        redacted: redactor.redacted,
    })
}

struct Redactor {
    include_secrets: bool,
    redacted: bool,
}

impl Redactor {
    fn secret(&mut self, value: &SecretString) -> String {
        if self.include_secrets {
            return value.expose().to_owned();
        }
        if !value.is_reference() {
            self.redacted = true;
        }
        value.display_value().to_owned()
    }

    fn bindings(&mut self, bindings: &[EnvBinding]) -> Vec<(String, String)> {
        bindings
            .iter()
            .map(|binding| (binding.name.clone(), self.secret(&binding.value)))
            .collect()
    }

    fn url(&mut self, url: &str) -> String {
        if self.include_secrets {
            return url.to_owned();
        }
        let mut out = url.to_owned();
        if let Some(scheme_end) = out.find("://") {
            let authority_start = scheme_end + 3;
            let authority_end = out[authority_start..]
                .find(['/', '?', '#'])
                .map_or(out.len(), |offset| authority_start + offset);
            if let Some(at) = out[authority_start..authority_end].rfind('@') {
                out.replace_range(authority_start..authority_start + at, REDACTED);
                self.redacted = true;
            }
        }
        if let Some(query_start) = out.find('?') {
            let query_end = out[query_start..]
                .find('#')
                .map_or(out.len(), |offset| query_start + offset);
            if query_end > query_start + 1 {
                out.replace_range(query_start + 1..query_end, REDACTED);
                self.redacted = true;
            }
        }
        out
    }

    fn args(&mut self, args: &[String]) -> Vec<String> {
        if self.include_secrets {
            return args.to_vec();
        }
        let is_sensitive_flag = |flag: &str| {
            let lower = flag.trim_start_matches('-').to_ascii_lowercase();
            flag.starts_with('-') && SENSITIVE_FLAG_WORDS.iter().any(|word| lower.contains(word))
        };
        let mut out = Vec::with_capacity(args.len());
        let mut redact_next = false;
        for arg in args {
            if redact_next {
                out.push(REDACTED.to_owned());
                self.redacted = true;
                redact_next = false;
                continue;
            }
            match arg.split_once('=') {
                Some((flag, value)) if is_sensitive_flag(flag) && !value.is_empty() => {
                    out.push(format!("{flag}={REDACTED}"));
                    self.redacted = true;
                }
                None if is_sensitive_flag(arg) => {
                    out.push(arg.clone());
                    redact_next = true;
                }
                _ => out.push(arg.clone()),
            }
        }
        out
    }
}

enum ServerFields {
    Stdio {
        command: String,
        args: Vec<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
    },
    Remote {
        json_type: &'static str,
        url: String,
        headers: Vec<(String, String)>,
    },
}

impl ServerFields {
    fn from_transport(transport: &McpTransport, redactor: &mut Redactor) -> Option<Self> {
        Some(match transport {
            McpTransport::Stdio {
                command,
                args,
                cwd,
                env,
            } => ServerFields::Stdio {
                command: command.clone(),
                args: redactor.args(args),
                cwd: cwd.as_ref().map(|cwd| cwd.display().to_string()),
                env: redactor.bindings(env),
            },
            McpTransport::StreamableHttp { url, headers } => ServerFields::Remote {
                json_type: "http",
                url: redactor.url(url),
                headers: redactor.bindings(headers),
            },
            McpTransport::Sse { url, headers } => ServerFields::Remote {
                json_type: "sse",
                url: redactor.url(url),
                headers: redactor.bindings(headers),
            },
            McpTransport::Unknown { .. } => return None,
        })
    }
}

fn string_map(pairs: &[(String, String)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect(),
    )
}

/// `{"mcpServers": {"<entry>": {...}}}` — the shape used by Claude Code,
/// Cursor, Claude Desktop and most JSON clients.
fn json_snippet(entry: &str, server: &ServerFields) -> String {
    let mut fields = Map::new();
    match server {
        ServerFields::Stdio {
            command,
            args,
            cwd,
            env,
        } => {
            fields.insert("command".into(), json!(command));
            fields.insert("args".into(), json!(args));
            if let Some(cwd) = cwd {
                fields.insert("cwd".into(), json!(cwd));
            }
            if !env.is_empty() {
                fields.insert("env".into(), string_map(env));
            }
        }
        ServerFields::Remote {
            json_type,
            url,
            headers,
        } => {
            fields.insert("type".into(), json!(json_type));
            fields.insert("url".into(), json!(url));
            if !headers.is_empty() {
                fields.insert("headers".into(), string_map(headers));
            }
        }
    }
    let document = json!({ "mcpServers": { entry: Value::Object(fields) } });
    serde_json::to_string_pretty(&document).expect("JSON values always serialise")
}

fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if bare {
        key.to_owned()
    } else {
        toml_string(key)
    }
}

fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn toml_table(out: &mut String, header: &str, pairs: &[(String, String)]) {
    if pairs.is_empty() {
        return;
    }
    out.push_str(&format!("\n[{header}]\n"));
    for (key, value) in pairs {
        out.push_str(&format!("{} = {}\n", toml_key(key), toml_string(value)));
    }
}

/// Codex `config.toml` shape: `[mcp_servers.<entry>]`.
fn toml_snippet(entry: &str, server: &ServerFields) -> String {
    let table = format!("mcp_servers.{}", toml_key(entry));
    let mut out = format!("[{table}]\n");
    match server {
        ServerFields::Stdio {
            command,
            args,
            cwd,
            env,
        } => {
            out.push_str(&format!("command = {}\n", toml_string(command)));
            let args: Vec<String> = args.iter().map(|arg| toml_string(arg)).collect();
            out.push_str(&format!("args = [{}]\n", args.join(", ")));
            if let Some(cwd) = cwd {
                out.push_str(&format!("cwd = {}\n", toml_string(cwd)));
            }
            toml_table(&mut out, &format!("{table}.env"), env);
        }
        ServerFields::Remote { url, headers, .. } => {
            out.push_str(&format!("url = {}\n", toml_string(url)));
            toml_table(&mut out, &format!("{table}.http_headers"), headers);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn stdio(format: ConfigFormat) -> McpServerDetails {
        McpServerDetails {
            client: "cursor".into(),
            config_file: PathBuf::from("/tmp/mcp.json"),
            config_format: format,
            entry: "github".into(),
            transport: McpTransport::Stdio {
                command: "npx".into(),
                args: vec![
                    "-y".into(),
                    "server-github".into(),
                    "--api-key".into(),
                    "sk-argsecret".into(),
                    "--token=tok-inline".into(),
                ],
                cwd: None,
                env: vec![
                    EnvBinding {
                        name: "GITHUB_TOKEN".into(),
                        value: SecretString::new("ghp_literalsecret"),
                    },
                    EnvBinding {
                        name: "FROM_ENV".into(),
                        value: SecretString::new("${OTHER_TOKEN}"),
                    },
                ],
            },
            enabled: None,
            declared_tools: vec![],
        }
    }

    #[test]
    fn json_snippet_redacts_by_default() {
        let snippet = mcp_config_snippet(&stdio(ConfigFormat::Json), false).unwrap();
        assert!(snippet.redacted);
        for secret in ["ghp_literalsecret", "sk-argsecret", "tok-inline"] {
            assert!(!snippet.text.contains(secret), "{}", snippet.text);
        }
        assert!(snippet.text.contains("${OTHER_TOKEN}"));
        let value: Value = serde_json::from_str(&snippet.text).unwrap();
        let server = &value["mcpServers"]["github"];
        assert_eq!(server["command"], "npx");
        assert_eq!(server["env"]["GITHUB_TOKEN"], REDACTED);
        assert_eq!(server["args"][3], REDACTED);
        assert_eq!(server["args"][4], format!("--token={REDACTED}"));
    }

    #[test]
    fn include_secrets_is_explicit() {
        let snippet = mcp_config_snippet(&stdio(ConfigFormat::Json), true).unwrap();
        assert!(!snippet.redacted);
        assert!(snippet.text.contains("ghp_literalsecret"));
        assert!(snippet.text.contains("sk-argsecret"));
    }

    #[test]
    fn toml_snippet_for_codex() {
        let snippet = mcp_config_snippet(&stdio(ConfigFormat::Toml), false).unwrap();
        assert!(
            snippet
                .text
                .starts_with("[mcp_servers.github]\ncommand = \"npx\"\n")
        );
        assert!(
            snippet
                .text
                .contains("[mcp_servers.github.env]\nGITHUB_TOKEN = \"<redacted>\"\n")
        );
        assert!(!snippet.text.contains("ghp_literalsecret"));
    }

    #[test]
    fn remote_url_credentials_and_query_are_redacted() {
        let mut mcp = stdio(ConfigFormat::Json);
        mcp.entry = "my server".into();
        mcp.transport = McpTransport::StreamableHttp {
            url: "https://user:pw@example.invalid/mcp?api_key=abc123#frag".into(),
            headers: vec![EnvBinding {
                name: "Authorization".into(),
                value: SecretString::new("Bearer xyz"),
            }],
        };
        let json = mcp_config_snippet(&mcp, false).unwrap().text;
        assert!(json.contains("https://<redacted>@example.invalid/mcp?<redacted>#frag"));
        assert!(!json.contains("abc123") && !json.contains("pw@") && !json.contains("xyz"));
        mcp.config_format = ConfigFormat::Toml;
        let toml = mcp_config_snippet(&mcp, false).unwrap().text;
        assert!(toml.starts_with("[mcp_servers.\"my server\"]\nurl = "));
        assert!(toml.contains(".http_headers]\nAuthorization = \"<redacted>\""));
    }

    #[test]
    fn unknown_transport_has_no_snippet() {
        let mut mcp = stdio(ConfigFormat::Json);
        mcp.transport = McpTransport::Unknown {
            declared_type: Some("ws".into()),
        };
        assert!(mcp_config_snippet(&mcp, false).is_none());
    }
}
