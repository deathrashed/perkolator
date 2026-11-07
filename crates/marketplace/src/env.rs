use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use perkolator_core::SecretString;

use crate::ProviderError;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(6);
pub const MAX_BODY_BYTES: u64 = 8 * 1024 * 1024;

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub url: String,
    /// Header values may hold credentials and are never logged.
    pub headers: Vec<(String, SecretString)>,
    pub timeout: Duration,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .push((name.into(), SecretString::new(value.into())));
        self
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after_secs: Option<u64>,
}

impl HttpResponse {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            retry_after_secs: None,
        }
    }

    /// Typed error for non-2xx, else the body as JSON.
    pub fn json(&self) -> Result<serde_json::Value, ProviderError> {
        if !(200..300).contains(&self.status) {
            return Err(ProviderError::from_status(
                self.status,
                self.retry_after_secs,
            ));
        }
        serde_json::from_slice(&self.body)
            .map_err(|error| ProviderError::malformed(format!("invalid JSON: {error}")))
    }

    pub fn text(&self) -> Result<String, ProviderError> {
        if !(200..300).contains(&self.status) {
            return Err(ProviderError::from_status(
                self.status,
                self.retry_after_secs,
            ));
        }
        String::from_utf8(self.body.clone())
            .map_err(|_| ProviderError::malformed("response is not UTF-8"))
    }
}

/// HTTP GET seam. Non-2xx statuses are returned as responses, not errors.
pub trait HttpClient: Send + Sync {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, ProviderError>;
}

#[derive(Clone, Debug)]
pub struct CommandOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Subprocess seam. Arguments are passed as a vector; nothing goes through
/// a shell, so user queries can never be interpolated into a command line.
pub trait CommandRunner: Send + Sync {
    fn which(&self, binary: &str) -> Option<PathBuf>;
    fn run(
        &self,
        program: &Path,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, ProviderError>;
}

/// Read-only credential lookup for adapters. Writing credentials is the
/// settings layer's job (OS keychain / Secret Service).
pub trait CredentialStore: Send + Sync {
    fn get(&self, slot: &str) -> Option<SecretString>;
}

pub struct NoCredentials;

impl CredentialStore for NoCredentials {
    fn get(&self, _slot: &str) -> Option<SecretString> {
        None
    }
}

/// Reads `PERKOLATOR_TOKEN_<SLOT>` (slot upper-cased, `-` → `_`), and
/// `GITHUB_TOKEN` for the `github` slot. Interim until secure storage lands.
pub struct EnvCredentials;

impl CredentialStore for EnvCredentials {
    fn get(&self, slot: &str) -> Option<SecretString> {
        let name = format!(
            "PERKOLATOR_TOKEN_{}",
            slot.to_ascii_uppercase().replace('-', "_")
        );
        std::env::var(&name)
            .ok()
            .or_else(|| {
                (slot == "github")
                    .then(|| std::env::var("GITHUB_TOKEN").ok())
                    .flatten()
            })
            .filter(|value| !value.is_empty())
            .map(SecretString::new)
    }
}

/// Everything an adapter may touch.
#[derive(Clone)]
pub struct ProviderEnv {
    pub http: Arc<dyn HttpClient>,
    pub commands: Arc<dyn CommandRunner>,
    pub credentials: Arc<dyn CredentialStore>,
    pub user_agent: String,
}

impl ProviderEnv {
    pub fn system() -> Self {
        Self {
            http: Arc::new(UreqClient::new()),
            commands: Arc::new(SystemCommands),
            credentials: Arc::new(EnvCredentials),
            user_agent: format!("Perkolator/{}", env!("CARGO_PKG_VERSION")),
        }
    }

    /// GET with the env's user agent; JSON body or typed error.
    pub fn get_json(&self, url: &str) -> Result<serde_json::Value, ProviderError> {
        self.http
            .get(&HttpRequest::get(url).header("User-Agent", self.user_agent.clone()))?
            .json()
    }
}

pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(DEFAULT_TIMEOUT))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for UreqClient {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, ProviderError> {
        if !request.url.starts_with("https://") {
            return Err(ProviderError::unsupported("only https URLs are fetched"));
        }
        let mut builder = self
            .agent
            .get(&request.url)
            .config()
            .timeout_global(Some(request.timeout))
            .build();
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.expose());
        }
        let mut response = builder.call().map_err(|error| match error {
            ureq::Error::Timeout(_) => ProviderError::Timeout,
            other => ProviderError::Network {
                message: other.to_string(),
            },
        })?;
        let status = response.status().as_u16();
        let retry_after_secs = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok());
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_vec()
            .map_err(|error| ProviderError::Network {
                message: error.to_string(),
            })?;
        Ok(HttpResponse {
            status,
            body,
            retry_after_secs,
        })
    }
}

pub struct SystemCommands;

const MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

impl CommandRunner for SystemCommands {
    fn which(&self, binary: &str) -> Option<PathBuf> {
        if binary.contains('/') {
            return None;
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(binary))
            .find(|candidate| is_executable(candidate))
    }

    fn run(
        &self,
        program: &Path,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, ProviderError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1")
            .spawn()
            .map_err(|error| ProviderError::Network {
                message: format!("cannot start {}: {error}", program.display()),
            })?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let out_reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = (&mut stdout)
                .take(MAX_OUTPUT_BYTES)
                .read_to_end(&mut buffer);
            buffer
        });
        let err_reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = (&mut stderr)
                .take(MAX_OUTPUT_BYTES)
                .read_to_end(&mut buffer);
            buffer
        });
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|error| ProviderError::Network {
                message: error.to_string(),
            })? {
                break status;
            }
            if started.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProviderError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Ok(CommandOutput {
            status: status.code(),
            stdout: String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned(),
            stderr: String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned(),
        })
    }
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_2xx_maps_to_typed_errors() {
        let response = |status| HttpResponse {
            status,
            body: Vec::new(),
            retry_after_secs: Some(7),
        };
        assert_eq!(response(401).json().unwrap_err(), ProviderError::NeedsAuth);
        assert_eq!(response(403).json().unwrap_err(), ProviderError::Forbidden);
        assert_eq!(
            response(429).json().unwrap_err(),
            ProviderError::RateLimited {
                retry_after_secs: Some(7)
            }
        );
    }

    #[test]
    fn request_headers_never_print_values() {
        let request =
            HttpRequest::get("https://x.invalid").header("Authorization", "Bearer s3cret");
        assert!(!format!("{request:?}").contains("s3cret"));
    }

    #[test]
    fn plain_http_is_refused() {
        let error = UreqClient::new()
            .get(&HttpRequest::get("http://example.invalid"))
            .unwrap_err();
        assert!(matches!(error, ProviderError::Unsupported { .. }));
    }

    #[test]
    fn commands_run_without_a_shell_and_time_out() {
        let runner = SystemCommands;
        let echo = runner.which("echo").expect("echo on PATH");
        let output = runner
            .run(&echo, &["a; rm -rf /".into()], Duration::from_secs(5))
            .unwrap();
        assert_eq!(output.stdout.trim(), "a; rm -rf /");
        let sleep = runner.which("sleep").expect("sleep on PATH");
        assert_eq!(
            runner
                .run(&sleep, &["5".into()], Duration::from_millis(100))
                .unwrap_err(),
            ProviderError::Timeout
        );
        assert!(runner.which("../bin/sh").is_none());
    }
}
