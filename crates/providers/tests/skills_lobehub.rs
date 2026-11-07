use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{
    FakeCommands, FixtureHttp, MemoryCredentials, env, offline_env,
};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, CommandOutput, CommandRunner, MarketplaceProvider, NoCredentials,
    ProviderEnv, ProviderError, SearchRequest,
};
use perkolator_providers::skills::LobeHub;

const SKILLS: &str = include_str!("fixtures/lobehub/skills_search.json");
const MCP: &str = include_str!("fixtures/lobehub/mcp_search.json");

/// `lhm` on PATH whose output depends on the subcommand (`skills`/`mcp`).
struct ScriptedLhm {
    skills: CommandOutput,
    mcp: CommandOutput,
    calls: Mutex<Vec<Vec<String>>>,
}

impl ScriptedLhm {
    fn ok(skills: &str, mcp: &str) -> Self {
        Self::new(success(skills), success(mcp))
    }

    fn new(skills: CommandOutput, mcp: CommandOutput) -> Self {
        Self {
            skills,
            mcp,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

fn success(stdout: &str) -> CommandOutput {
    CommandOutput {
        status: Some(0),
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

fn failure(stderr: &str) -> CommandOutput {
    CommandOutput {
        status: Some(1),
        stdout: String::new(),
        stderr: stderr.to_owned(),
    }
}

impl CommandRunner for ScriptedLhm {
    fn which(&self, binary: &str) -> Option<PathBuf> {
        (binary == "lhm").then(|| PathBuf::from("/fake/bin/lhm"))
    }

    fn run(
        &self,
        program: &Path,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, ProviderError> {
        assert_eq!(program, Path::new("/fake/bin/lhm"));
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(match args.first().map(String::as_str) {
            Some("skills") => self.skills.clone(),
            Some("mcp") => self.mcp.clone(),
            other => panic!("unexpected lhm subcommand {other:?}"),
        })
    }
}

fn cli_env(lhm: ScriptedLhm) -> (ProviderEnv, Arc<ScriptedLhm>, Arc<FixtureHttp>) {
    let lhm = Arc::new(lhm);
    let http = Arc::new(FixtureHttp::new());
    (
        ProviderEnv {
            http: http.clone(),
            commands: lhm.clone(),
            credentials: Arc::new(NoCredentials),
            user_agent: "Perkolator/test".into(),
        },
        lhm,
        http,
    )
}

#[test]
fn searches_skills_and_mcp_with_structured_args() {
    let (env, lhm, http) = cli_env(ScriptedLhm::ok(SKILLS, MCP));
    let mut request = SearchRequest::new("-rf pdf; rm");
    request.page_size = 2;
    let page = LobeHub::new()
        .search(&request, &env, &CancelToken::new())
        .unwrap();

    assert_eq!(
        lhm.calls(),
        vec![
            vec![
                "skills".to_owned(),
                "search".into(),
                "--q=-rf pdf; rm".into(),
                "--page=1".into(),
                "--page-size=2".into(),
                "--output=json".into(),
            ],
            vec![
                "mcp".to_owned(),
                "search".into(),
                "--q=-rf pdf; rm".into(),
                "--page=1".into(),
                "--page-size=2".into(),
                "--output=json".into(),
            ],
        ]
    );
    assert!(http.requests().is_empty(), "no direct HTTP");

    assert_eq!(page.items.len(), 3);
    let skill = &page.items[0];
    assert_eq!(skill.source.as_str(), "lobehub");
    assert_eq!(skill.kind, ResourceKind::Skill);
    assert_eq!(skill.upstream_id, "skill:github.anthropics.skills.pdf");
    assert_eq!(
        skill.package_ref.as_deref(),
        Some("github.anthropics.skills.pdf")
    );
    assert_eq!(skill.version.as_deref(), Some("1.0.0"));
    assert_eq!(
        skill.repo_url.as_deref(),
        Some("https://github.com/anthropics/skills")
    );
    assert_eq!(skill.categories, ["productivity"]);
    assert!(skill.actions.view_source && !skill.actions.install);

    let bare = &page.items[1];
    assert_eq!(bare.repo_url, None);
    assert_eq!(bare.canonical_url, None);
    assert_eq!(bare.author, None);
    assert!(!bare.actions.view_source);

    let mcp = &page.items[2];
    assert_eq!(mcp.kind, ResourceKind::McpServer);
    assert_eq!(mcp.upstream_id, "mcp:filesystem");
    assert_eq!(
        mcp.canonical_url.as_deref(),
        Some("https://modelcontextprotocol.io")
    );
    assert_eq!(mcp.version, None);

    // Skills report page 1 of 2; MCP is complete. Totals are summed.
    assert_eq!(page.next_cursor.as_deref(), Some("2"));
    assert_eq!(page.total, Some(4));
}

#[test]
fn kind_filter_limits_cli_calls() {
    let (env, lhm, _) = cli_env(ScriptedLhm::ok(SKILLS, MCP));
    let mut request = SearchRequest::new("");
    request.kinds = BTreeSet::from([ResourceKind::McpServer]);
    request.category = Some("developer".into());
    request.cursor = Some("3".into());
    let page = LobeHub::new()
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        lhm.calls(),
        vec![vec![
            "mcp".to_owned(),
            "search".into(),
            "--category=developer".into(),
            "--page=3".into(),
            "--page-size=25".into(),
            "--output=json".into(),
        ]]
    );

    request.kinds = BTreeSet::from([ResourceKind::Agent, ResourceKind::Plugin]);
    let (env, lhm, _) = cli_env(ScriptedLhm::ok(SKILLS, MCP));
    assert!(
        LobeHub::new()
            .search(&request, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    assert!(lhm.calls().is_empty());
}

#[test]
fn missing_cli_is_reported_without_running_anything() {
    let (env, http, commands) = offline_env();
    let provider = LobeHub::new();
    assert_eq!(
        provider.auth_status(&env),
        AuthStatus::CliMissing {
            binary: "lhm".into()
        }
    );
    assert_eq!(
        provider
            .search(&SearchRequest::new("pdf"), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::CliMissing {
            binary: "lhm".into()
        }
    );
    assert!(commands.calls().is_empty());
    assert!(http.requests().is_empty());
}

#[test]
fn auth_status_with_cli_present_runs_nothing() {
    let commands = Arc::new(FakeCommands::new().binary("lhm", "{}"));
    let http = Arc::new(FixtureHttp::new());
    let env = env(http.clone(), commands.clone(), MemoryCredentials::default());
    assert_eq!(LobeHub::new().auth_status(&env), AuthStatus::Unknown);
    assert!(commands.calls().is_empty());
    assert!(http.requests().is_empty());
}

#[test]
fn cli_failures_map_to_typed_errors_without_leaking_stderr() {
    for (stderr, expected) in [
        (
            "No credentials found. Run `lhm register` first or set MARKET_CLIENT_ID and MARKET_CLIENT_SECRET.",
            ProviderError::NeedsAuth,
        ),
        ("Search failed: 403 Forbidden", ProviderError::Forbidden),
        (
            "Search failed: 429 Too Many Requests",
            ProviderError::RateLimited {
                retry_after_secs: None,
            },
        ),
        (
            "Search failed: socket hang up token=abc123",
            ProviderError::Network {
                message: "`lhm` exited with status 1".into(),
            },
        ),
    ] {
        let (env, _, _) = cli_env(ScriptedLhm::new(failure(stderr), success(MCP)));
        let error = LobeHub::new()
            .search(&SearchRequest::new("pdf"), &env, &CancelToken::new())
            .unwrap_err();
        assert!(!error.to_string().contains("abc123"));
        assert_eq!(error, expected, "{stderr}");
    }
}

#[test]
fn malformed_output_is_reported() {
    for stdout in ["Showing 1-2 of 3 results", r#"{"currentPage":1}"#] {
        let (env, _, _) = cli_env(ScriptedLhm::ok(stdout, MCP));
        assert!(matches!(
            LobeHub::new().search(&SearchRequest::new("pdf"), &env, &CancelToken::new()),
            Err(ProviderError::Malformed { .. })
        ));
    }
    let (env, lhm, _) = cli_env(ScriptedLhm::ok(SKILLS, MCP));
    let mut request = SearchRequest::new("pdf");
    request.cursor = Some("0".into());
    assert!(matches!(
        LobeHub::new().search(&request, &env, &CancelToken::new()),
        Err(ProviderError::Malformed { .. })
    ));
    assert!(lhm.calls().is_empty());
}

#[test]
fn cancellation_prevents_cli_runs() {
    let (env, lhm, _) = cli_env(ScriptedLhm::ok(SKILLS, MCP));
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        LobeHub::new()
            .search(&SearchRequest::new("pdf"), &env, &cancel)
            .unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(lhm.calls().is_empty());
}

#[test]
fn connect_never_spawns_registration_or_login() {
    let provider = LobeHub::new();
    let intent = provider.connect_intent();
    assert!(
        matches!(intent, AuthIntent::Unsupported { .. }),
        "{intent:?}"
    );
    assert_eq!(
        provider.descriptor().kinds,
        [ResourceKind::Skill, ResourceKind::McpServer]
    );
}

#[test]
#[ignore = "live: needs `lhm` on PATH and M2M credentials; never installs or registers"]
fn live_cli_search() {
    let env = ProviderEnv::system();
    let provider = LobeHub::new();
    match provider.auth_status(&env) {
        AuthStatus::CliMissing { .. } => {
            eprintln!("lobehub live: UNVERIFIED (CLI missing)");
        }
        _ => {
            let result = provider.search(&SearchRequest::new("pdf"), &env, &CancelToken::new());
            eprintln!("lobehub live: {result:?}");
            result.expect("lhm search");
        }
    }
}
