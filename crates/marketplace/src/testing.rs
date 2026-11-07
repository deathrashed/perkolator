//! Offline test doubles: fixture HTTP responses, scripted commands and
//! in-memory credentials. Every adapter test should run against these.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use perkolator_core::SecretString;

use crate::{
    CommandOutput, CommandRunner, CredentialStore, HttpClient, HttpRequest, HttpResponse,
    ProviderEnv, ProviderError,
};

/// Maps URL prefixes to canned responses and records every request.
#[derive(Default)]
pub struct FixtureHttp {
    routes: Mutex<Vec<(String, HttpResponse)>>,
    requests: Mutex<Vec<String>>,
}

impl FixtureHttp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Respond to any URL starting with `prefix` (longest prefix wins).
    pub fn route(self, prefix: impl Into<String>, response: HttpResponse) -> Self {
        self.routes.lock().unwrap().push((prefix.into(), response));
        self
    }

    pub fn route_json(self, prefix: impl Into<String>, body: &str) -> Self {
        self.route(prefix, HttpResponse::ok(body.as_bytes().to_vec()))
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl HttpClient for FixtureHttp {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, ProviderError> {
        self.requests.lock().unwrap().push(request.url.clone());
        let routes = self.routes.lock().unwrap();
        routes
            .iter()
            .filter(|(prefix, _)| request.url.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, response)| response.clone())
            .ok_or_else(|| ProviderError::Network {
                message: format!("no fixture for {}", request.url),
            })
    }
}

/// Scripted binaries: `which` succeeds only for registered names.
#[derive(Default)]
pub struct FakeCommands {
    outputs: Mutex<HashMap<String, CommandOutput>>,
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl FakeCommands {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn binary(self, name: &str, stdout: &str) -> Self {
        self.outputs.lock().unwrap().insert(
            name.to_owned(),
            CommandOutput {
                status: Some(0),
                stdout: stdout.to_owned(),
                stderr: String::new(),
            },
        );
        self
    }

    pub fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for FakeCommands {
    fn which(&self, binary: &str) -> Option<PathBuf> {
        self.outputs
            .lock()
            .unwrap()
            .contains_key(binary)
            .then(|| PathBuf::from(format!("/fake/bin/{binary}")))
    }

    fn run(
        &self,
        program: &Path,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, ProviderError> {
        let name = program
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.calls
            .lock()
            .unwrap()
            .push((name.clone(), args.to_vec()));
        self.outputs
            .lock()
            .unwrap()
            .get(&name)
            .cloned()
            .ok_or(ProviderError::CliMissing { binary: name })
    }
}

#[derive(Default)]
pub struct MemoryCredentials(HashMap<String, String>);

impl MemoryCredentials {
    pub fn with(mut self, slot: &str, value: &str) -> Self {
        self.0.insert(slot.to_owned(), value.to_owned());
        self
    }
}

impl CredentialStore for MemoryCredentials {
    fn get(&self, slot: &str) -> Option<SecretString> {
        self.0.get(slot).map(SecretString::new)
    }
}

/// Build an env from doubles. Keep the `Arc`s to inspect recorded calls.
pub fn env(
    http: Arc<FixtureHttp>,
    commands: Arc<FakeCommands>,
    credentials: MemoryCredentials,
) -> ProviderEnv {
    ProviderEnv {
        http,
        commands,
        credentials: Arc::new(credentials),
        user_agent: "Perkolator/test".into(),
    }
}

/// An env whose HTTP and CLI seams fail loudly: proves "no request made".
pub fn offline_env() -> (ProviderEnv, Arc<FixtureHttp>, Arc<FakeCommands>) {
    let http = Arc::new(FixtureHttp::new());
    let commands = Arc::new(FakeCommands::new());
    (
        env(http.clone(), commands.clone(), MemoryCredentials::default()),
        http,
        commands,
    )
}
