//! Offline fixture tests for the `cursor-plugins` adapter (real, trimmed
//! files from cursor/plugins) plus an ignored live smoke.

use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{self, FakeCommands, FixtureHttp, MemoryCredentials};
use perkolator_marketplace::{
    AuthStatus, HttpResponse, ProviderEnv, ProviderError, RemoteItem, SearchRequest,
};
use perkolator_providers::bundles::cursor_plugins;

const RAW: &str = "https://raw.githubusercontent.com/cursor/plugins/HEAD/";
const TREE: &str = "https://api.github.com/repos/cursor/plugins/git/trees/HEAD";
const MARKETPLACE: &str = include_str!("fixtures/cursor-plugins/marketplace.json");

fn index_url() -> String {
    format!("{RAW}.cursor-plugin/marketplace.json")
}

fn fixture_env(http: FixtureHttp) -> (ProviderEnv, Arc<FixtureHttp>) {
    let http = Arc::new(http);
    let env = testing::env(
        http.clone(),
        Arc::new(FakeCommands::new()),
        MemoryCredentials::default(),
    );
    (env, http)
}

fn full_http() -> FixtureHttp {
    FixtureHttp::new()
        .route_json(index_url(), MARKETPLACE)
        .route_json(TREE, include_str!("fixtures/cursor-plugins/tree.json"))
        .route_json(
            format!("{RAW}continual-learning/.cursor-plugin/plugin.json"),
            include_str!("fixtures/cursor-plugins/continual-learning-plugin.json"),
        )
        .route_json(
            format!("{RAW}continual-learning/hooks/hooks.json"),
            include_str!("fixtures/cursor-plugins/continual-learning-hooks.json"),
        )
        .route_json(
            format!("{RAW}continual-learning/README.md"),
            include_str!("fixtures/cursor-plugins/continual-learning-README.md"),
        )
        .route_json(
            format!("{RAW}cursor-team-kit/rules/no-inline-imports.mdc"),
            include_str!("fixtures/cursor-plugins/no-inline-imports.mdc"),
        )
        .route_json(
            format!("{RAW}third_party/github/mcp.json"),
            include_str!("fixtures/cursor-plugins/github-mcp.json"),
        )
}

fn request(query: &str, kinds: &[ResourceKind]) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.page_size = 100;
    request.kinds = kinds.iter().copied().collect();
    request
}

fn find<'a>(items: &'a [RemoteItem], kind: ResourceKind, title: &str) -> &'a RemoteItem {
    items
        .iter()
        .find(|item| item.kind == kind && item.title == title)
        .unwrap_or_else(|| panic!("no {kind} `{title}` in results"))
}

#[test]
fn search_lists_plugins_rules_skills_agents_and_mcp() {
    let provider = cursor_plugins::provider();
    let (env, http) = fixture_env(full_http());
    let page = provider
        .search(&request("", &[]), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [index_url(), format!("{TREE}?recursive=1")]
    );

    let kit = find(&page.items, ResourceKind::Plugin, "cursor-team-kit");
    assert!(
        kit.summary.contains("18 skills, 2 agents, 2 rules"),
        "{}",
        kit.summary
    );
    assert_eq!(kit.package_ref, None);
    assert_eq!(
        kit.canonical_url.as_deref(),
        Some("https://github.com/cursor/plugins/tree/HEAD/cursor-team-kit")
    );

    let learning = find(&page.items, ResourceKind::Plugin, "continual-learning");
    assert!(
        learning.summary.contains("executable hooks"),
        "{}",
        learning.summary
    );

    let finance = find(&page.items, ResourceKind::Plugin, "finance");
    assert!(
        finance.summary.contains("unavailable for cursor"),
        "{}",
        finance.summary
    );

    let rule = find(&page.items, ResourceKind::Rule, "no-inline-imports");
    assert_eq!(
        rule.resource_path.as_deref(),
        Some("cursor-team-kit/rules/no-inline-imports.mdc")
    );
    assert_eq!(
        rule.repo_url.as_deref(),
        Some("https://github.com/cursor/plugins")
    );
    find(&page.items, ResourceKind::Skill, "deslop");
    find(&page.items, ResourceKind::Agent, "agents-memory-updater");
    let mcp = find(&page.items, ResourceKind::McpServer, "github");
    assert_eq!(
        mcp.resource_path.as_deref(),
        Some("third_party/github/mcp.json")
    );
    assert!(
        page.items
            .iter()
            .all(|item| item.source.as_str() == cursor_plugins::ID)
    );
}

#[test]
fn kind_filter_limits_results_and_skips_unsupported_kinds() {
    let provider = cursor_plugins::provider();
    let (env, http) = fixture_env(full_http());
    let rules = provider
        .search(
            &request("", &[ResourceKind::Rule]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(rules.items.len(), 2);
    assert!(
        rules
            .items
            .iter()
            .all(|item| item.kind == ResourceKind::Rule)
    );

    let (offline, offline_http, _) = testing::offline_env();
    let none = provider
        .search(
            &request("", &[ResourceKind::Prompt]),
            &offline,
            &CancelToken::new(),
        )
        .unwrap();
    assert!(none.items.is_empty() && offline_http.requests().is_empty());
    assert_eq!(http.requests().len(), 2);
}

#[test]
fn category_filter_is_ignored_because_index_has_none() {
    let provider = cursor_plugins::provider();
    assert!(!provider.descriptor().capabilities.categories);
    let (env, _) = fixture_env(full_http());
    let mut filtered = request("", &[ResourceKind::Plugin]);
    filtered.category = Some("developer-tools".into());
    let page = provider
        .search(&filtered, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(page.items.len(), 4);
}

#[test]
fn malformed_and_rate_limited_index() {
    let provider = cursor_plugins::provider();
    let (env, _) = fixture_env(FixtureHttp::new().route_json(index_url(), r#"{"name": "x"}"#));
    assert!(matches!(
        provider
            .search(&request("", &[]), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Malformed { .. }
    ));
    let limited = HttpResponse {
        status: 429,
        body: Vec::new(),
        retry_after_secs: None,
    };
    let (env, _) = fixture_env(FixtureHttp::new().route(index_url(), limited));
    assert_eq!(
        provider
            .search(&request("", &[]), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::RateLimited {
            retry_after_secs: None
        }
    );
}

#[test]
fn auth_status_is_local() {
    let provider = cursor_plugins::provider();
    let (env, http, _) = testing::offline_env();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert!(http.requests().is_empty());
    let token = testing::env(
        Arc::new(FixtureHttp::new()),
        Arc::new(FakeCommands::new()),
        MemoryCredentials::default().with("github", "ghp_test"),
    );
    assert!(matches!(
        provider.auth_status(&token),
        AuthStatus::Ready { .. }
    ));
}

#[test]
fn plugin_detail_shows_hook_commands_without_running_them() {
    let provider = cursor_plugins::provider();
    let (env, http) = fixture_env(full_http());
    let cancel = CancelToken::new();
    let page = provider
        .search(
            &request("continual", &[ResourceKind::Plugin]),
            &env,
            &cancel,
        )
        .unwrap();
    let detail = provider.detail(&page.items[0], &env, &cancel).unwrap();
    let manifest = detail.manifest_json.unwrap();
    assert!(
        manifest.contains("continual-learning-stop.ts"),
        "{manifest}"
    );
    assert!(manifest.contains("\"displayName\": \"Continual Learning\""));
    assert!(detail.item.summary.contains("executable hooks"));
    assert!(detail.item.summary.contains("1 skill, 1 agent"));
    assert!(detail.readme_markdown.is_some());
    assert!(
        detail
            .files
            .iter()
            .any(|file| file.path == "continual-learning/hooks/continual-learning-stop.ts")
    );
    // Only fetches; nothing is executed (no command runner is even reachable).
    assert!(
        http.requests()
            .iter()
            .all(|url| url.starts_with("https://"))
    );
}

#[test]
fn rule_and_mcp_details() {
    let provider = cursor_plugins::provider();
    let (env, _) = fixture_env(full_http());
    let cancel = CancelToken::new();
    let page = provider.search(&request("", &[]), &env, &cancel).unwrap();

    let rule = find(&page.items, ResourceKind::Rule, "no-inline-imports");
    let detail = provider.detail(rule, &env, &cancel).unwrap();
    assert_eq!(
        detail.item.summary,
        "Keep imports at top of file and avoid inline imports"
    );
    assert!(
        detail
            .manifest_json
            .unwrap()
            .contains("\"alwaysApply\": true")
    );
    assert!(
        detail
            .readme_markdown
            .unwrap()
            .contains("# No inline imports")
    );

    let mcp = find(&page.items, ResourceKind::McpServer, "github");
    let detail = provider.detail(mcp, &env, &cancel).unwrap();
    assert!(detail.item.summary.starts_with("MCP servers: github"));
    assert!(
        detail
            .manifest_json
            .unwrap()
            .contains("${GITHUB_PERSONAL_ACCESS_TOKEN}")
    );
}

#[test]
#[ignore = "live network: cursor/plugins"]
fn live_search_and_detail() {
    let provider = cursor_plugins::provider();
    let env = ProviderEnv::system();
    let cancel = CancelToken::new();
    let page = provider
        .search(&request("", &[]), &env, &cancel)
        .expect("live search");
    let count = |kind| page.items.iter().filter(|item| item.kind == kind).count();
    eprintln!(
        "cursor-plugins live: {} items on first page ({} plugins, {} rules, {} skills, {} agents, {} mcp)",
        page.items.len(),
        count(ResourceKind::Plugin),
        count(ResourceKind::Rule),
        count(ResourceKind::Skill),
        count(ResourceKind::Agent),
        count(ResourceKind::McpServer),
    );
    assert!(!page.items.is_empty());
    let rules = provider
        .search(&request("", &[ResourceKind::Rule]), &env, &cancel)
        .expect("live rule search");
    let detail = provider
        .detail(&rules.items[0], &env, &cancel)
        .expect("live rule detail");
    eprintln!(
        "rule detail `{}`: {}",
        detail.item.title, detail.item.summary
    );
}
