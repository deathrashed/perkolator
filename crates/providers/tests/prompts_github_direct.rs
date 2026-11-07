//! github-direct adapter against trimmed real GitHub responses (captured
//! 2026-11-07): repository search, recursive trees of `wshobson/agents`,
//! `cursor/plugins` and `github/github-mcp-server`, and raw manifests.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{
    FakeCommands, FixtureHttp, MemoryCredentials, env, offline_env,
};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, HttpClient, HttpRequest, HttpResponse, MarketplaceProvider,
    ProviderEnv, ProviderError, RemoteItem, SearchPage, SearchRequest,
};
use perkolator_providers::prompts::{GithubDirect, RepoTarget};

const SEARCH_PREFIX: &str = "https://api.github.com/search/repositories?q=";
const TOKEN: &str = "ghp_FIXTURE_TOKEN_must_not_leak_123";
const CREATE_PLUGIN_URL: &str = "https://github.com/cursor/plugins/tree/main/create-plugin";

fn fixture_dir() -> String {
    format!(
        "{}/tests/fixtures/github-direct",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn fixture(name: &str) -> String {
    let path = format!("{}/{name}", fixture_dir());
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn tree_url(owner: &str, repo: &str, reference: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{reference}?recursive=1")
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Serves `tree-<owner>-<repo>.json` and every file under `raw/<owner>/<repo>`.
fn with_repo(http: FixtureHttp, owner: &str, repo: &str, reference: &str) -> FixtureHttp {
    let mut http = http.route_json(
        tree_url(owner, repo, reference),
        &fixture(&format!("tree-{owner}-{repo}.json")),
    );
    let root = format!("{}/raw/{owner}/{repo}", fixture_dir());
    let mut files = Vec::new();
    collect_files(Path::new(&root), &mut files);
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        http = http.route(
            format!("https://raw.githubusercontent.com/{owner}/{repo}/{reference}/{relative}"),
            HttpResponse::ok(std::fs::read(&file).unwrap()),
        );
    }
    http
}

fn all_repos() -> FixtureHttp {
    let http = with_repo(FixtureHttp::new(), "wshobson", "agents", "HEAD");
    let http = with_repo(http, "cursor", "plugins", "main");
    let http = with_repo(http, "github", "github-mcp-server", "HEAD");
    http.route_json(SEARCH_PREFIX, &fixture("search-repositories.json"))
}

fn anonymous(http: FixtureHttp) -> (ProviderEnv, Arc<FixtureHttp>) {
    let http = Arc::new(http);
    (
        env(
            http.clone(),
            Arc::new(FakeCommands::new()),
            MemoryCredentials::default(),
        ),
        http,
    )
}

fn request(query: &str, kinds: &[ResourceKind]) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.kinds = kinds.iter().copied().collect::<BTreeSet<_>>();
    request
}

fn search(env: &ProviderEnv, request: &SearchRequest) -> Result<SearchPage, ProviderError> {
    GithubDirect::new().search(request, env, &CancelToken::new())
}

fn summary(items: &[RemoteItem]) -> Vec<(ResourceKind, &str)> {
    items
        .iter()
        .map(|item| (item.kind, item.title.as_str()))
        .collect()
}

/// One request: URL plus exposed header name/value pairs.
type SeenRequest = (String, Vec<(String, String)>);

/// Records header values so the test can prove the token is sent.
struct RecordingHttp {
    inner: FixtureHttp,
    seen: Mutex<Vec<SeenRequest>>,
}

impl HttpClient for RecordingHttp {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, ProviderError> {
        let headers = request
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.expose().to_owned()))
            .collect();
        self.seen
            .lock()
            .unwrap()
            .push((request.url.clone(), headers));
        self.inner.get(request)
    }
}

fn token_env(http: FixtureHttp) -> (ProviderEnv, Arc<RecordingHttp>) {
    let recording = Arc::new(RecordingHttp {
        inner: http,
        seen: Mutex::new(Vec::new()),
    });
    let env = ProviderEnv {
        http: recording.clone(),
        commands: Arc::new(FakeCommands::new()),
        credentials: Arc::new(MemoryCredentials::default().with("github", TOKEN)),
        user_agent: "Perkolator/test".into(),
    };
    (env, recording)
}

#[test]
fn parses_repository_locations() {
    let bare = RepoTarget::parse("wshobson/agents").unwrap();
    assert_eq!(
        (
            bare.repo.owner.as_str(),
            bare.repo.repo.as_str(),
            bare.repo.reference.as_str()
        ),
        ("wshobson", "agents", "HEAD")
    );
    assert_eq!(bare.path, "");
    assert!(!bare.explicit_url);

    let nested = RepoTarget::parse("anthropics/skills/skills/pdf").unwrap();
    assert_eq!(nested.path, "skills/pdf");

    let url = RepoTarget::parse(&format!("{CREATE_PLUGIN_URL}?tab=readme")).unwrap();
    assert_eq!(url.repo.reference, "main");
    assert_eq!(url.path, "create-plugin");
    assert!(url.explicit_url);

    let blob = RepoTarget::parse("github.com/o/r/blob/v1.2/agents/a.md").unwrap();
    assert_eq!(
        (blob.repo.reference.as_str(), blob.path.as_str()),
        ("v1.2", "agents/a.md")
    );
    assert_eq!(
        RepoTarget::parse("https://github.com/o/r.git")
            .unwrap()
            .repo
            .repo,
        "r"
    );

    for rejected in [
        "pdf",
        "pdf skills",
        "owner/repo extra",
        "https://example.com/o/r",
        "o/r/../../etc",
        "https://github.com/o/r/tree/%2e%2e/x",
        "",
    ] {
        assert!(RepoTarget::parse(rejected).is_none(), "{rejected}");
    }
}

#[test]
fn descriptor_covers_all_kinds_with_an_optional_token() {
    let provider = GithubDirect::new();
    let descriptor = provider.descriptor();
    assert_eq!(descriptor.id.as_str(), "github-direct");
    assert_eq!(
        descriptor.kinds.iter().copied().collect::<BTreeSet<_>>(),
        ResourceKind::ALL.into_iter().collect()
    );
    assert!(matches!(
        provider.connect_intent(),
        AuthIntent::PromptSecret { slot, .. } if slot == "github"
    ));
}

#[test]
fn auth_status_reflects_the_token_without_requests() {
    let (env, http, _) = offline_env();
    assert_eq!(
        GithubDirect::new().auth_status(&env),
        AuthStatus::NotRequired
    );
    let (token, recording) = token_env(FixtureHttp::new());
    assert_eq!(
        GithubDirect::new().auth_status(&token),
        AuthStatus::Ready { identity: None }
    );
    assert!(http.requests().is_empty());
    assert!(recording.seen.lock().unwrap().is_empty());
}

#[test]
fn empty_query_makes_no_request() {
    let (env, http, _) = offline_env();
    assert!(search(&env, &request("  ", &[])).unwrap().items.is_empty());
    assert!(http.requests().is_empty());
}

#[test]
fn anonymous_keyword_search_spends_two_topic_queries_per_page() {
    let (env, http) = anonymous(all_repos());
    let page = search(&env, &request("pdf", &[])).unwrap();
    assert_eq!(
        http.requests(),
        vec![
            "https://api.github.com/search/repositories?q=pdf+topic%3Aagent-skills&per_page=25",
            "https://api.github.com/search/repositories?q=pdf+topic%3Amcp-server&per_page=25",
        ]
    );
    assert_eq!(page.next_cursor.as_deref(), Some("t2"));
    assert_eq!(page.total, None);
    assert_eq!(page.items.len(), 4);
    let first = &page.items[0];
    assert_eq!(first.kind, ResourceKind::Skill);
    assert_eq!(first.upstream_id, "skill:virgiliojr94/book-to-skill:HEAD:");
    assert_eq!(first.title, "virgiliojr94/book-to-skill");
    assert_eq!(first.author.as_deref(), Some("virgiliojr94"));
    assert_eq!(
        first.repo_url.as_deref(),
        Some("https://github.com/virgiliojr94/book-to-skill")
    );
    assert!(first.categories.contains(&"agent-skills".to_owned()));
    assert!(!first.actions.fetch_files && !first.actions.install);
    assert_eq!(page.items[2].kind, ResourceKind::McpServer);
    assert_eq!(
        page.items[2].upstream_id,
        "mcp:virgiliojr94/book-to-skill:HEAD:"
    );
}

#[test]
fn keyword_cursor_walks_the_remaining_kinds() {
    let (env, http) = anonymous(all_repos());
    let mut paged = request("pdf", &[]);
    paged.cursor = Some("t2".into());
    let page = search(&env, &paged).unwrap();
    assert!(http.requests()[0].contains("topic%3Aclaude-code-plugin"));
    assert!(http.requests()[1].contains("topic%3Aclaude-code-subagents"));
    assert_eq!(page.next_cursor.as_deref(), Some("t4"));

    paged.cursor = Some("t4".into());
    let page = search(&env, &paged).unwrap();
    assert!(http.requests()[2].contains("topic%3Acursor-rules"));
    assert!(http.requests()[3].contains("topic%3Aprompts"));
    assert_eq!(page.next_cursor, None);

    paged.cursor = Some("t6".into());
    assert!(search(&env, &paged).unwrap().items.is_empty());
    assert_eq!(http.requests().len(), 4);

    paged.cursor = Some("r0".into());
    assert!(matches!(
        search(&env, &paged),
        Err(ProviderError::Malformed { .. })
    ));
}

#[test]
fn single_kind_keyword_search_reports_the_total() {
    let (env, http) = anonymous(all_repos());
    let page = search(&env, &request("pdf", &[ResourceKind::Plugin])).unwrap();
    assert_eq!(http.requests().len(), 1);
    assert!(http.requests()[0].contains("topic%3Aclaude-code-plugin"));
    assert_eq!(page.total, Some(617));
    assert!(
        page.items
            .iter()
            .all(|item| item.kind == ResourceKind::Plugin)
    );
}

#[test]
fn resolves_plugins_agents_rules_and_prompts_in_a_repository() {
    let (env, http) = anonymous(all_repos());
    let page = search(&env, &request("wshobson/agents", &[])).unwrap();
    assert_eq!(
        summary(&page.items),
        vec![
            (ResourceKind::Plugin, "git-pr-workflows"),
            (ResourceKind::Agent, "git-pr-workflows-code-reviewer"),
            (ResourceKind::Rule, "AGENTS.md"),
            (ResourceKind::Prompt, "git-workflow"),
            (ResourceKind::Prompt, "onboard"),
            (ResourceKind::Prompt, "pr-enhance"),
        ]
    );
    assert_eq!(page.next_cursor, None);
    assert!(
        !http.requests().iter().any(|url| url.contains("/search/")),
        "resolution must not spend search quota"
    );
    assert!(
        !http
            .requests()
            .iter()
            .any(|url| url.ends_with("marketplace.json")),
        "unrecognised files are not fetched"
    );

    let plugin = &page.items[0];
    assert_eq!(
        plugin.upstream_id,
        "plugin:wshobson/agents:HEAD:plugins/git-pr-workflows/.claude-plugin/plugin.json"
    );
    assert_eq!(
        plugin.resource_path.as_deref(),
        Some("plugins/git-pr-workflows")
    );
    assert_eq!(
        plugin.canonical_url.as_deref(),
        Some("https://github.com/wshobson/agents/tree/HEAD/plugins/git-pr-workflows")
    );
    assert_eq!(plugin.version.as_deref(), Some("1.3.2"));
    assert_eq!(plugin.author.as_deref(), Some("Seth Hobson"));
    assert!(plugin.actions.fetch_files && !plugin.actions.install);

    let rule = &page.items[2];
    assert_eq!(
        rule.summary,
        "Agents, skills, and commands for seven coding tools"
    );
    assert_eq!(
        rule.canonical_url.as_deref(),
        Some("https://github.com/wshobson/agents/blob/HEAD/AGENTS.md")
    );
    let prompt = &page.items[3];
    assert_eq!(
        prompt.summary,
        "Orchestrate git workflow from code review through PR creation with quality gates"
    );
    assert_eq!(prompt.source.as_str(), "github-direct");
}

#[test]
fn resolves_a_url_with_ref_and_path_to_skills_plugin_agent_and_rule() {
    let (env, http) = anonymous(all_repos());
    let page = search(&env, &request(CREATE_PLUGIN_URL, &[])).unwrap();
    assert_eq!(http.requests()[0], tree_url("cursor", "plugins", "main"));
    assert_eq!(
        summary(&page.items),
        vec![
            (ResourceKind::Skill, "create-plugin-scaffold"),
            (ResourceKind::Skill, "review-plugin-submission"),
            (ResourceKind::Plugin, "Create Plugin"),
            (ResourceKind::Agent, "plugin-architect"),
            (ResourceKind::Rule, "plugin-quality-gates"),
        ]
    );
    let skill = &page.items[0];
    assert_eq!(
        skill.canonical_url.as_deref(),
        Some(
            "https://github.com/cursor/plugins/tree/main/create-plugin/skills/create-plugin-scaffold"
        )
    );
    assert_eq!(
        skill.resource_path.as_deref(),
        Some("create-plugin/skills/create-plugin-scaffold")
    );
    assert!(
        skill
            .summary
            .starts_with("Create a new Cursor plugin scaffold")
    );
    assert_eq!(page.items[2].version.as_deref(), Some("1.0.0"));
    assert_eq!(page.items[2].author.as_deref(), Some("Cursor"));
    assert_eq!(
        page.items[4].summary,
        "Keep Cursor plugin manifests, paths, and component metadata valid during plugin authoring."
    );
}

#[test]
fn resolves_mcp_server_manifests_without_template_versions() {
    let (env, _) = anonymous(all_repos());
    let page = search(&env, &request("github/github-mcp-server", &[])).unwrap();
    assert_eq!(
        summary(&page.items),
        vec![
            (ResourceKind::McpServer, "GitHub"),
            (ResourceKind::Agent, "go-sdk-tool-migrator"),
        ]
    );
    let mcp = &page.items[0];
    assert_eq!(
        mcp.package_ref.as_deref(),
        Some("io.github.github/github-mcp-server")
    );
    assert_eq!(mcp.version, None, "`${{VERSION}}` is a template");
    assert_eq!(mcp.resource_path.as_deref(), Some("server.json"));
}

#[test]
fn kind_filter_limits_raw_reads() {
    let (env, http) = anonymous(all_repos());
    let page = search(&env, &request(CREATE_PLUGIN_URL, &[ResourceKind::Skill])).unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(
        page.items
            .iter()
            .all(|item| item.kind == ResourceKind::Skill)
    );
    assert_eq!(http.requests().len(), 3, "1 tree + 2 SKILL.md reads");
}

#[test]
fn resolution_pages_with_offset_cursors() {
    let (env, _) = anonymous(all_repos());
    let mut paged = request("wshobson/agents", &[]);
    paged.page_size = 4;
    let first = search(&env, &paged).unwrap();
    assert_eq!(first.items.len(), 4);
    assert_eq!(first.next_cursor.as_deref(), Some("r4"));
    paged.cursor = first.next_cursor;
    let second = search(&env, &paged).unwrap();
    assert_eq!(
        summary(&second.items),
        vec![
            (ResourceKind::Prompt, "onboard"),
            (ResourceKind::Prompt, "pr-enhance"),
        ]
    );
    assert_eq!(second.next_cursor, None);
}

#[test]
fn bare_pair_that_is_not_a_repository_falls_back_to_keyword_search() {
    let not_found = HttpResponse {
        status: 404,
        body: br#"{"message":"Not Found"}"#.to_vec(),
        retry_after_secs: None,
    };
    let http = FixtureHttp::new()
        .route(tree_url("ci", "cd", "HEAD"), not_found.clone())
        .route_json(SEARCH_PREFIX, &fixture("search-repositories.json"));
    let (env, http) = anonymous(http);
    let page = search(&env, &request("ci/cd", &[ResourceKind::Skill])).unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(http.requests()[1].starts_with(SEARCH_PREFIX));

    let (env, _) = anonymous(FixtureHttp::new().route(tree_url("ci", "cd", "HEAD"), not_found));
    assert_eq!(
        search(&env, &request("https://github.com/ci/cd", &[])),
        Err(ProviderError::Http { status: 404 })
    );
}

#[test]
fn malformed_responses_are_typed() {
    let (env, _) = anonymous(FixtureHttp::new().route_json(SEARCH_PREFIX, r#"{"items": 3}"#));
    assert!(matches!(
        search(&env, &request("pdf", &[ResourceKind::Skill])),
        Err(ProviderError::Malformed { .. })
    ));
    let (env, _) = anonymous(FixtureHttp::new().route_json(SEARCH_PREFIX, "<html>"));
    assert!(matches!(
        search(&env, &request("pdf", &[ResourceKind::Skill])),
        Err(ProviderError::Malformed { .. })
    ));
    let (env, _) =
        anonymous(FixtureHttp::new().route_json(tree_url("o", "r", "HEAD"), r#"{"sha":"x"}"#));
    assert!(matches!(
        search(&env, &request("https://github.com/o/r", &[])),
        Err(ProviderError::Malformed { .. })
    ));
}

#[test]
fn http_errors_map_to_typed_errors() {
    let cases = [
        (401, ProviderError::NeedsAuth),
        (403, ProviderError::Forbidden),
        (
            429,
            ProviderError::RateLimited {
                retry_after_secs: Some(60),
            },
        ),
    ];
    for (status, expected) in cases {
        let response = HttpResponse {
            status,
            body: br#"{"message":"API rate limit exceeded"}"#.to_vec(),
            retry_after_secs: Some(60),
        };
        let (env, _) = anonymous(
            FixtureHttp::new()
                .route(SEARCH_PREFIX, response.clone())
                .route(tree_url("o", "r", "HEAD"), response),
        );
        assert_eq!(
            search(&env, &request("pdf", &[ResourceKind::Skill])).unwrap_err(),
            expected
        );
        assert_eq!(
            search(&env, &request("o/r", &[])).unwrap_err(),
            expected,
            "tree status {status}"
        );
    }
}

#[test]
fn token_is_sent_but_never_exposed() {
    let forbidden = HttpResponse {
        status: 403,
        body: Vec::new(),
        retry_after_secs: None,
    };
    let (env, recording) = token_env(all_repos().route(tree_url("o", "r", "HEAD"), forbidden));
    let provider = GithubDirect::new();
    let cancel = CancelToken::new();

    let keyword = provider
        .search(&request("pdf", &[]), &env, &cancel)
        .unwrap();
    assert_eq!(keyword.next_cursor, None, "a token allows all six topics");
    let resolved = provider
        .search(&request("wshobson/agents", &[]), &env, &cancel)
        .unwrap();
    let detail = provider.detail(&resolved.items[0], &env, &cancel).unwrap();
    let error = provider
        .search(&request("https://github.com/o/r", &[]), &env, &cancel)
        .unwrap_err();

    let seen = recording.seen.lock().unwrap();
    assert_eq!(
        seen.iter()
            .filter(|(url, _)| url.contains("/search/"))
            .count(),
        6
    );
    for (url, headers) in seen.iter() {
        assert!(
            headers.iter().any(|(name, value)| name == "Authorization"
                && value == &format!("Bearer {TOKEN}")),
            "missing token on {url}"
        );
        assert!(!url.contains(TOKEN));
    }
    let exposed = [
        serde_json::to_string(&keyword.items).unwrap(),
        serde_json::to_string(&resolved.items).unwrap(),
        serde_json::to_string(&detail).unwrap(),
        format!("{keyword:?}{resolved:?}{detail:?}"),
        format!("{error}{error:?}"),
        serde_json::to_string(provider.descriptor()).unwrap(),
        format!("{:?}", provider.auth_status(&env)),
    ];
    for text in exposed {
        assert!(!text.contains(TOKEN), "token leaked: {text}");
    }
}

#[test]
fn detail_lists_skill_files_and_reads_the_skill() {
    let (env, http) = anonymous(all_repos());
    let provider = GithubDirect::new();
    let page = search(
        &env,
        &request(
            CREATE_PLUGIN_URL,
            &[ResourceKind::Skill, ResourceKind::Plugin],
        ),
    )
    .unwrap();
    let skill = &page.items[0];
    let detail = provider.detail(skill, &env, &CancelToken::new()).unwrap();
    assert_eq!(detail.item, *skill);
    assert!(
        detail
            .readme_markdown
            .unwrap()
            .contains("name: create-plugin-scaffold")
    );
    assert_eq!(
        detail
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["create-plugin/skills/create-plugin-scaffold/SKILL.md"]
    );
    assert_eq!(
        detail.files[0].url.as_deref(),
        Some(
            "https://raw.githubusercontent.com/cursor/plugins/main/create-plugin/skills/create-plugin-scaffold/SKILL.md"
        )
    );

    let plugin = page
        .items
        .iter()
        .find(|item| item.kind == ResourceKind::Plugin)
        .unwrap();
    let detail = provider.detail(plugin, &env, &CancelToken::new()).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(detail.manifest_json.as_deref().unwrap()).unwrap();
    assert_eq!(manifest["name"], "create-plugin");
    assert!(detail.readme_markdown.is_some());
    assert!(
        detail
            .files
            .iter()
            .any(|file| file.path == "create-plugin/rules/plugin-quality-gates.mdc")
    );
    assert!(
        http.requests()
            .iter()
            .all(|url| url.starts_with("https://"))
    );
}

#[test]
fn detail_of_a_keyword_hit_reads_the_readme() {
    let readme = "https://raw.githubusercontent.com/virgiliojr94/book-to-skill/HEAD/README.md";
    let (env, http) =
        anonymous(all_repos().route(readme, HttpResponse::ok(b"# Book to skill\n".to_vec())));
    let item = search(&env, &request("pdf", &[ResourceKind::Skill]))
        .unwrap()
        .items
        .remove(0);
    let detail = GithubDirect::new()
        .detail(&item, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(detail.readme_markdown.as_deref(), Some("# Book to skill\n"));
    assert!(detail.files.is_empty());
    assert_eq!(http.requests().last().unwrap(), readme);
}

#[test]
fn detail_refuses_malformed_or_traversing_ids_without_requests() {
    let (env, http, _) = offline_env();
    let (listing_env, _) = anonymous(all_repos());
    let mut item = search(&listing_env, &request("pdf", &[ResourceKind::Skill]))
        .unwrap()
        .items
        .remove(0);
    for id in [
        "skill:o/r:HEAD:../../etc/passwd",
        "skill:o/r:a b:SKILL.md",
        "nonsense",
        "widget:o/r:HEAD:",
    ] {
        item.upstream_id = id.into();
        assert!(
            matches!(
                GithubDirect::new().detail(&item, &env, &CancelToken::new()),
                Err(ProviderError::Malformed { .. })
            ),
            "{id}"
        );
    }
    assert!(http.requests().is_empty());
}

#[test]
fn cancelled_search_makes_no_request() {
    let (env, http) = anonymous(all_repos());
    let cancel = CancelToken::new();
    cancel.cancel();
    for query in ["pdf", "wshobson/agents"] {
        assert_eq!(
            GithubDirect::new().search(&request(query, &[]), &env, &cancel),
            Err(ProviderError::Cancelled)
        );
    }
    assert!(http.requests().is_empty());
}

#[test]
#[ignore = "live network: anonymous GitHub API (1 tree + 1 search request)"]
fn live_anonymous_resolution_and_search() {
    let provider = GithubDirect::new();
    let env = ProviderEnv::system();
    let resolved = provider
        .search(&request(CREATE_PLUGIN_URL, &[]), &env, &CancelToken::new())
        .unwrap();
    eprintln!("github-direct live resolve: {:?}", summary(&resolved.items));
    let kinds: BTreeSet<ResourceKind> = resolved.items.iter().map(|item| item.kind).collect();
    assert!(kinds.contains(&ResourceKind::Skill));
    assert!(kinds.contains(&ResourceKind::Plugin));

    let keyword = provider
        .search(
            &request("pdf", &[ResourceKind::Skill]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    eprintln!(
        "github-direct live search: {} items, total={:?}",
        keyword.items.len(),
        keyword.total
    );
    assert!(!keyword.items.is_empty());
}
