use perkolator_core::{ConfigFormat, EnvBinding, McpServerDetails, McpTransport, SecretString};
use serde_json::{Map, Value, json};

/// Flag names whose value is a credential (`--token x`, `--api-key=x`).
const SECRET_FLAG_WORDS: &[&str] = &[
    "token",
    "password",
    "passwd",
    "secret",
    "apikey",
    "api-key",
    "api_key",
    "credential",
    "private-key",
    "access-key",
    "client-secret",
    "auth",
    "authorization",
    "bearer",
    "key",
    "pat",
];
/// Header / query parameter names whose value is a credential.
const SECRET_NAME_PARTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "api-key",
    "api_key",
    "auth",
    "key",
    "signature",
    "sig",
    "cookie",
    "session",
    "credential",
];
const TOKEN_PREFIXES: &[&str] = &[
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xapp-",
    "AKIA",
    "ASIA",
    "AIza",
    "npm_",
    "pypi-",
    "hf_",
    "eyJ",
];
/// Generic opaque-token heuristic threshold (hex/base64-ish strings).
const MIN_OPAQUE_TOKEN_LEN: usize = 32;
const REDACTED: &str = SecretString::REDACTED;

fn mask(value: &str) -> String {
    SecretString::new(value).display_value().to_owned()
}

fn is_secret_flag(flag: &str) -> bool {
    let name = flag.trim_start_matches('-').to_ascii_lowercase();
    if name.is_empty() || !flag.starts_with('-') {
        return false;
    }
    SECRET_FLAG_WORDS.iter().any(|word| {
        name == *word
            || name.ends_with(&format!("-{word}"))
            || name.ends_with(&format!("_{word}"))
            || (word.len() >= 5 && name.contains(word))
    })
}

fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_NAME_PARTS.iter().any(|part| lower.contains(part))
}

fn is_env_assignment_key(key: &str) -> bool {
    key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True for strings that look like credentials on their own.
fn looks_like_token(value: &str) -> bool {
    if TOKEN_PREFIXES
        .iter()
        .any(|prefix| value.starts_with(prefix) && value.len() > prefix.len() + 6)
    {
        return true;
    }
    value.len() >= MIN_OPAQUE_TOKEN_LEN
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '='))
        && value.chars().any(|c| c.is_ascii_digit())
        && value.chars().any(|c| c.is_ascii_alphabetic())
}

/// Redact one `Name: value` header-style argument if the name is secret-ish.
fn redact_header_arg(arg: &str) -> Option<String> {
    let (name, value) = arg.split_once(':')?;
    let name_is_header =
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    (name_is_header && (is_secret_name(name) || looks_like_token(value.trim())))
        .then(|| format!("{name}: {}", mask(value.trim())))
}

/// Redact credentials in a command line: values after secret flags
/// (`--token x`, `--api-key=x`), `KEY=value` assignments, secret headers
/// (`Authorization: Bearer x`) and token-shaped arguments. References such as
/// `${GITHUB_TOKEN}` stay visible.
pub fn redact_args(args: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            redact_next = false;
            redacted.push(redact_header_arg(arg).unwrap_or_else(|| mask(arg)));
            continue;
        }
        if arg.starts_with('-') {
            if let Some((flag, value)) = arg.split_once('=') {
                if is_secret_flag(flag) || looks_like_token(value) {
                    redacted.push(format!("{flag}={}", mask(value)));
                } else {
                    redacted.push(arg.clone());
                }
            } else {
                redact_next = is_secret_flag(arg);
                redacted.push(arg.clone());
            }
            continue;
        }
        if let Some((key, value)) = arg.split_once('=')
            && is_env_assignment_key(key)
        {
            redacted.push(format!("{key}={}", mask(value)));
            continue;
        }
        if let Some(header) = redact_header_arg(arg) {
            redacted.push(header);
            continue;
        }
        if looks_like_token(arg) {
            redacted.push(mask(arg));
            continue;
        }
        redacted.push(arg.clone());
    }
    redacted
}

/// Redact URL userinfo, secret-named or token-shaped query values and
/// token-shaped path segments.
pub fn redact_url(url: &str) -> String {
    let (base, fragment) = match url.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (url, None),
    };
    let (location, query) = match base.split_once('?') {
        Some((location, query)) => (location, Some(query)),
        None => (base, None),
    };
    let (scheme, rest) = match location.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, location),
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let authority = match authority.rsplit_once('@') {
        Some((_, host)) => format!("{REDACTED}@{host}"),
        None => authority.to_owned(),
    };
    let path: Vec<String> = path
        .split('/')
        .map(|segment| {
            if looks_like_token(segment) {
                REDACTED.to_owned()
            } else {
                segment.to_owned()
            }
        })
        .collect();
    let mut out = String::new();
    if let Some(scheme) = scheme {
        out.push_str(scheme);
        out.push_str("://");
    }
    out.push_str(&authority);
    out.push_str(&path.join("/"));
    if let Some(query) = query {
        let params: Vec<String> = query
            .split('&')
            .map(|param| match param.split_once('=') {
                Some((key, value)) if is_secret_name(key) || looks_like_token(value) => {
                    format!("{key}={}", mask(value))
                }
                _ => param.to_owned(),
            })
            .collect();
        out.push('?');
        out.push_str(&params.join("&"));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(fragment);
    }
    out
}

fn bindings_json(bindings: &[EnvBinding]) -> Value {
    Value::Object(
        bindings
            .iter()
            .map(|binding| {
                (
                    binding.name.clone(),
                    Value::String(binding.value.display_value().to_owned()),
                )
            })
            .collect(),
    )
}

/// Redacted, normalized view of one MCP entry built from typed details (the
/// config file itself is not re-read). Returns `(format, text)`.
pub fn mcp_config_snippet(details: &McpServerDetails) -> (String, String) {
    let mut entry = Map::new();
    match &details.transport {
        McpTransport::Stdio {
            command,
            args,
            cwd,
            env,
        } => {
            let mut command_line = vec![command.clone()];
            command_line.extend(args.iter().cloned());
            let mut command_line = redact_args(&command_line).into_iter();
            entry.insert(
                "command".into(),
                json!(command_line.next().unwrap_or_default()),
            );
            let args: Vec<String> = command_line.collect();
            if !args.is_empty() {
                entry.insert("args".into(), json!(args));
            }
            if let Some(cwd) = cwd {
                entry.insert("cwd".into(), json!(cwd.display().to_string()));
            }
            if !env.is_empty() {
                entry.insert("env".into(), bindings_json(env));
            }
        }
        McpTransport::StreamableHttp { url, headers } | McpTransport::Sse { url, headers } => {
            let kind = if matches!(details.transport, McpTransport::Sse { .. }) {
                "sse"
            } else {
                "http"
            };
            entry.insert("type".into(), json!(kind));
            entry.insert("url".into(), json!(redact_url(url)));
            if !headers.is_empty() {
                entry.insert("headers".into(), bindings_json(headers));
            }
        }
        McpTransport::Unknown { declared_type } => {
            if let Some(declared) = declared_type {
                entry.insert("type".into(), json!(declared));
            }
        }
    }
    if let Some(enabled) = details.enabled {
        entry.insert("enabled".into(), json!(enabled));
    }
    let mut root = Map::new();
    root.insert(details.entry.clone(), Value::Object(entry));
    let root = Value::Object(root);
    match details.config_format {
        ConfigFormat::Json => (
            "json".to_owned(),
            serde_json::to_string_pretty(&root).unwrap_or_default(),
        ),
        ConfigFormat::Toml => {
            let wrapped = json!({ "mcp_servers": root });
            let text = toml::to_string_pretty(&wrapped)
                .unwrap_or_else(|_| serde_json::to_string_pretty(&wrapped).unwrap_or_default());
            ("toml".to_owned(), text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn redacts_flag_values_assignments_and_tokens() {
        let args = strings(&[
            "-y",
            "@modelcontextprotocol/server-github",
            "--token",
            "abc123secretvalue",
            "--api-key=sk-live-000000000000",
            "--port",
            "3000",
            "-e",
            "DB_PASSWORD=hunter2hunter2",
            "--header",
            "Authorization: Bearer opaque-value-1",
            "ghp_abcdefghijklmnop1234",
            "--github-token",
            "${GITHUB_TOKEN}",
        ]);
        let joined = redact_args(&args).join(" ");
        for secret in [
            "abc123secretvalue",
            "sk-live-000000000000",
            "hunter2hunter2",
            "opaque-value-1",
            "ghp_abcdefghijklmnop1234",
        ] {
            assert!(!joined.contains(secret), "{secret} leaked: {joined}");
        }
        assert!(joined.contains("@modelcontextprotocol/server-github"));
        assert!(joined.contains("--port 3000"));
        assert!(joined.contains("${GITHUB_TOKEN}"));
    }

    #[test]
    fn redacts_url_credentials() {
        let url = "https://user:pa55word@mcp.example.invalid/v1/0123456789abcdef0123456789abcdef/sse?api_key=zzz-secret&region=eu#frag";
        let redacted = redact_url(url);
        assert!(!redacted.contains("pa55word"));
        assert!(!redacted.contains("zzz-secret"));
        assert!(!redacted.contains("0123456789abcdef0123456789abcdef"));
        assert!(redacted.contains("mcp.example.invalid"));
        assert!(redacted.contains("region=eu"));
        assert_eq!(
            redact_url("https://mcp.example.invalid/mcp"),
            "https://mcp.example.invalid/mcp"
        );
    }
}
