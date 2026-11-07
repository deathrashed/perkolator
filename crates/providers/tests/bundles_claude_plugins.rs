//! Offline fixture tests for the `claude-plugins` adapter (real, trimmed
//! files from anthropics/claude-plugins-official) plus an ignored live smoke.

use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{self, FakeCommands, FixtureHttp, MemoryCredentials};
use perkolator_marketplace::{
    AuthStatus, HttpResponse, ProviderEnv, ProviderError, RemoteItem, SearchRequest,
};
use perkolator_providers::bundles::claude_plugins;

const RAW: &str = "https://raw.githubusercontent.com/anthropics/claude-plugins-official/HEAD/";
const TREE: &str = "https://api.github.com/repos/anthropics/claude-plugins-official/git/trees/HEAD";
const MARKETPLACE: &str = include_str!("fixtures/claude-plugins/marketplace.json");
const TREE_JSON: &str = include_str!("fixtures/claude-plugins/tree.json");

fn index_url() -> String {
    format!("{RAW}.claude-plugin/marketplace.json")
}

fn status(code: u16, retry: Option<u64>) -> HttpResponse {
    HttpResponse {
        status: code,
        body: Vec::new(),
        retry_after_secs: retry,
    }
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
        .route_json(TREE, TREE_JSON)
        .route_json(
            format!("{RAW}plugins/hookify/.claude-plugin/plugin.json"),
            include_str!("fixtures/claude-plugins/hookify-plugin.json"),
        )
        .route_json(
            format!("{RAW}plugins/hookify/hooks/hooks.json"),
            include_str!("fixtures/claude-plugins/hookify-hooks.json"),
        )
        .route_json(
            format!("{RAW}plugins/hookify/README.md"),
            include_str!("fixtures/claude-plugins/hookify-README.md"),
        )
        .route_json(
            format!("{RAW}external_plugins/github/.mcp.json"),
            include_str!("fixtures/claude-plugins/github-mcp.json"),
        )
        .route_json(
            format!("{RAW}plugins/feature-dev/agents/code-explorer.md"),
            include_str!("fixtures/claude-plugins/code-explorer.md"),
        )
}

fn all(query: &str) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.page_size = 100;
    request
}

fn only(query: &str, kinds: &[ResourceKind]) -> SearchRequest {
    let mut request = all(query);
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
fn search_lists_plugins_components_and_flags_executables() {
    let provider = claude_plugins::provider();
    let (env, http) = fixture_env(full_http());
    let page = provider
        .search(&all(""), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [index_url(), format!("{TREE}?recursive=1")]
    );

    let hookify = find(&page.items, ResourceKind::Plugin, "hookify");
    assert!(
        hookify.summary.contains("executable hooks"),
        "{}",
        hookify.summary
    );
    assert!(
        hookify.summary.contains("4 commands"),
        "{}",
        hookify.summary
    );
    assert_eq!(
        hookify.package_ref.as_deref(),
        Some("hookify@claude-plugins-official")
    );
    assert_eq!(
        hookify.version, None,
        "marketplace entry declares no version"
    );
    assert_eq!(hookify.resource_path.as_deref(), Some("plugins/hookify"));
    assert!(!hookify.actions.install);

    let lsp = find(&page.items, ResourceKind::Plugin, "rust-analyzer-lsp");
    assert!(lsp.summary.contains("language server"), "{}", lsp.summary);

    let xcode = find(&page.items, ResourceKind::Plugin, "Xcode");
    assert!(xcode.summary.contains("never runs it"), "{}", xcode.summary);
    assert!(!xcode.actions.fetch_files && xcode.repo_url.is_none());
    assert_eq!(
        xcode.canonical_url.as_deref(),
        Some("https://developer.apple.com/xcode/")
    );

    let pinned = find(
        &page.items,
        ResourceKind::Plugin,
        "42crunch-api-security-testing",
    );
    assert_eq!(
        pinned.repo_url.as_deref(),
        Some("https://github.com/42Crunch-AI/claude-plugins")
    );
    assert!(
        pinned
            .canonical_url
            .as_deref()
            .unwrap()
            .contains("faf5305385de")
    );

    let agent = find(&page.items, ResourceKind::Agent, "conversation-analyzer");
    assert_eq!(
        agent.resource_path.as_deref(),
        Some("plugins/hookify/agents/conversation-analyzer.md")
    );
    find(&page.items, ResourceKind::Skill, "writing-rules");
    let mcp = page
        .items
        .iter()
        .find(|item| item.kind == ResourceKind::McpServer)
        .unwrap();
    assert_eq!(
        mcp.resource_path.as_deref(),
        Some("external_plugins/github/.mcp.json")
    );

    let declared = find(&page.items, ResourceKind::Skill, "local-ai-use");
    assert_eq!(
        declared.repo_url.as_deref(),
        Some("https://github.com/amd/skills")
    );
    assert_eq!(
        declared.resource_path.as_deref(),
        Some("skills/local-ai-use")
    );
    find(&page.items, ResourceKind::Skill, "box-legal-workflows");

    let mut ids = BTreeSet::new();
    for item in &page.items {
        assert_eq!(item.source.as_str(), claude_plugins::ID);
        assert!(
            ids.insert(&item.upstream_id),
            "duplicate id {}",
            item.upstream_id
        );
        if item.actions.fetch_files {
            assert!(item.repo_url.is_some() && item.resource_path.is_some());
        }
        assert!(item.fetched_at_unix > 0);
    }
}

#[test]
fn plugin_only_query_needs_one_raw_request() {
    let provider = claude_plugins::provider();
    let (env, http) = fixture_env(full_http());
    let page = provider
        .search(
            &only("hook", &[ResourceKind::Plugin]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(http.requests(), [index_url()]);
    assert_eq!(page.items[0].title, "hookify");
    assert!(
        page.items
            .iter()
            .all(|item| item.kind == ResourceKind::Plugin)
    );
}

#[test]
fn unsupported_kinds_make_no_request() {
    let provider = claude_plugins::provider();
    let (env, http, _) = testing::offline_env();
    let page = provider
        .search(
            &only("x", &[ResourceKind::Rule, ResourceKind::Prompt]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    assert!(page.items.is_empty());
    assert!(http.requests().is_empty());
}

#[test]
fn malformed_index_is_reported() {
    let provider = claude_plugins::provider();
    for body in [
        "{not json",
        r#"{"plugins": 3}"#,
        r#"{"plugins": [{"description": "x"}]}"#,
    ] {
        let (env, _) = fixture_env(FixtureHttp::new().route_json(index_url(), body));
        let error = provider
            .search(&all(""), &env, &CancelToken::new())
            .unwrap_err();
        assert!(
            matches!(error, ProviderError::Malformed { .. }),
            "{body}: {error:?}"
        );
    }
}

#[test]
fn rate_limits_map_to_typed_errors() {
    let provider = claude_plugins::provider();
    let (env, _) = fixture_env(FixtureHttp::new().route(index_url(), status(429, Some(30))));
    assert_eq!(
        provider
            .search(&all(""), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::RateLimited {
            retry_after_secs: Some(30)
        }
    );
    let (env, _) = fixture_env(FixtureHttp::new().route(index_url(), status(403, None)));
    assert_eq!(
        provider
            .search(&all(""), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Forbidden
    );
}

#[test]
fn exhausted_api_quota_keeps_plugins_but_fails_component_only_search() {
    let provider = claude_plugins::provider();
    let http = FixtureHttp::new()
        .route_json(index_url(), MARKETPLACE)
        .route(TREE, status(403, None));
    let (env, _) = fixture_env(http);
    let page = provider
        .search(&all(""), &env, &CancelToken::new())
        .unwrap();
    assert!(!page.items.is_empty());
    assert!(
        page.items
            .iter()
            .any(|item| item.kind == ResourceKind::Plugin)
    );
    assert_eq!(
        provider
            .search(&only("", &[ResourceKind::Agent]), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Forbidden
    );
}

#[test]
fn auth_status_is_local_and_token_aware() {
    let provider = claude_plugins::provider();
    let (env, http, commands) = testing::offline_env();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert!(http.requests().is_empty() && commands.calls().is_empty());
    let with_token = testing::env(
        Arc::new(FixtureHttp::new()),
        Arc::new(FakeCommands::new()),
        MemoryCredentials::default().with("github", "ghp_test"),
    );
    assert_eq!(
        provider.auth_status(&with_token),
        AuthStatus::Ready { identity: None }
    );
}

#[test]
fn cancelled_search_makes_no_request() {
    let provider = claude_plugins::provider();
    let (env, http) = fixture_env(full_http());
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        provider.search(&all(""), &env, &cancel).unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(http.requests().is_empty());
}

#[test]
fn pages_with_offset_cursor_and_filters_categories() {
    let provider = claude_plugins::provider();
    let (env, _) = fixture_env(full_http());
    let mut request = only("", &[ResourceKind::Plugin]);
    request.page_size = 3;
    let first = provider
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(first.items.len(), 3);
    assert_eq!(first.next_cursor.as_deref(), Some("3"));
    assert_eq!(first.total, None);
    request.cursor = first.next_cursor.clone();
    let second = provider
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert!(second.items.iter().all(|item| !first.items.contains(item)));
    request.cursor = Some("nope".into());
    assert!(matches!(
        provider
            .search(&request, &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Unsupported { .. }
    ));

    let mut security = all("");
    security.category = Some("Security".into());
    let page = provider
        .search(&security, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title, "42crunch-api-security-testing");
}

#[test]
fn plugin_detail_lists_files_manifest_and_hook_commands() {
    let provider = claude_plugins::provider();
    let (env, _) = fixture_env(full_http());
    let cancel = CancelToken::new();
    let page = provider
        .search(&only("hookify", &[ResourceKind::Plugin]), &env, &cancel)
        .unwrap();
    let detail = provider.detail(&page.items[0], &env, &cancel).unwrap();
    let manifest = detail.manifest_json.unwrap();
    assert!(manifest.contains("pluginManifest") && manifest.contains("\"0.1.1\""));
    assert!(manifest.contains("PreToolUse") && manifest.contains("pretooluse.py"));
    assert!(detail.item.summary.contains("executable hooks"));
    assert!(detail.readme_markdown.unwrap().contains("Hookify"));
    assert!(
        detail
            .files
            .iter()
            .any(|file| file.path == "plugins/hookify/hooks/pretooluse.py")
    );
    assert!(
        detail
            .files
            .iter()
            .all(|file| file.path.starts_with("plugins/hookify/"))
    );
    assert!(detail.tools.is_empty());
}

#[test]
fn component_details_parse_frontmatter_and_redact_mcp() {
    let provider = claude_plugins::provider();
    let (env, _) = fixture_env(full_http());
    let cancel = CancelToken::new();
    let page = provider.search(&all(""), &env, &cancel).unwrap();

    let agent = find(&page.items, ResourceKind::Agent, "code-explorer");
    let detail = provider.detail(agent, &env, &cancel).unwrap();
    assert!(
        detail.item.summary.starts_with("Deeply analyzes"),
        "{}",
        detail.item.summary
    );
    assert!(detail.item.summary.contains("Tools: Glob, Grep"));
    assert!(
        detail
            .manifest_json
            .unwrap()
            .contains("\"model\": \"sonnet\"")
    );
    assert!(!detail.readme_markdown.unwrap().starts_with("---"));

    let mcp = page
        .items
        .iter()
        .find(|item| item.kind == ResourceKind::McpServer)
        .unwrap();
    let detail = provider.detail(mcp, &env, &cancel).unwrap();
    assert!(detail.item.summary.starts_with("MCP servers: github"));
    assert!(
        detail
            .manifest_json
            .unwrap()
            .contains("Bearer ${GITHUB_PERSONAL_ACCESS_TOKEN}")
    );
}

#[test]
#[ignore = "live network: anthropics/claude-plugins-official"]
fn live_search_and_detail() {
    let provider = claude_plugins::provider();
    let env = ProviderEnv::system();
    let cancel = CancelToken::new();
    let page = provider
        .search(&all(""), &env, &cancel)
        .expect("live search");
    let count = |kind| page.items.iter().filter(|item| item.kind == kind).count();
    eprintln!(
        "claude-plugins live: {} items on first page ({} plugins, {} skills, {} agents, {} mcp), next={:?}",
        page.items.len(),
        count(ResourceKind::Plugin),
        count(ResourceKind::Skill),
        count(ResourceKind::Agent),
        count(ResourceKind::McpServer),
        page.next_cursor
    );
    assert!(!page.items.is_empty());
    let hookify = provider
        .search(&only("hookify", &[ResourceKind::Plugin]), &env, &cancel)
        .expect("live plugin search");
    let detail = provider
        .detail(&hookify.items[0], &env, &cancel)
        .expect("live detail");
    eprintln!(
        "hookify detail: {} files; {}",
        detail.files.len(),
        detail.item.summary
    );
    assert!(!detail.files.is_empty());
}
