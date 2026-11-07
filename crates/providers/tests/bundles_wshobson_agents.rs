//! Offline fixture tests for the `wshobson-agents` adapter (real, trimmed
//! files from wshobson/agents) plus an ignored live smoke.

use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{self, FakeCommands, FixtureHttp, MemoryCredentials};
use perkolator_marketplace::{
    AuthStatus, HttpResponse, ProviderEnv, ProviderError, RemoteItem, SearchRequest,
};
use perkolator_providers::bundles::wshobson_agents;

const RAW: &str = "https://raw.githubusercontent.com/wshobson/agents/HEAD/";
const TREE: &str = "https://api.github.com/repos/wshobson/agents/git/trees/HEAD";

fn index_url() -> String {
    format!("{RAW}.claude-plugin/marketplace.json")
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
        .route_json(
            index_url(),
            include_str!("fixtures/wshobson-agents/marketplace.json"),
        )
        .route_json(TREE, include_str!("fixtures/wshobson-agents/tree.json"))
        .route_json(
            format!("{RAW}plugins/protect-mcp/.claude-plugin/plugin.json"),
            include_str!("fixtures/wshobson-agents/protect-mcp-plugin.json"),
        )
        .route_json(
            format!("{RAW}plugins/protect-mcp/hooks/hooks.json"),
            include_str!("fixtures/wshobson-agents/protect-mcp-hooks.json"),
        )
        .route_json(
            format!("{RAW}plugins/protect-mcp/README.md"),
            include_str!("fixtures/wshobson-agents/protect-mcp-README.md"),
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
fn search_lists_agents_skills_and_bundles() {
    let provider = wshobson_agents::provider();
    let (env, http) = fixture_env(full_http());
    let page = provider
        .search(&request("", &[]), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests().len(), 2);

    let docs = find(&page.items, ResourceKind::Plugin, "code-documentation");
    assert_eq!(docs.version.as_deref(), Some("1.2.2"));
    assert_eq!(docs.author.as_deref(), Some("Seth Hobson"));
    assert_eq!(
        docs.package_ref.as_deref(),
        Some("code-documentation@claude-code-workflows")
    );
    assert!(
        docs.summary.contains("3 agents, 2 commands"),
        "{}",
        docs.summary
    );
    assert_eq!(docs.categories, ["documentation"]);

    let guard = find(&page.items, ResourceKind::Plugin, "protect-mcp");
    assert!(
        guard.summary.contains("executable hooks"),
        "{}",
        guard.summary
    );

    let external = find(&page.items, ResourceKind::Plugin, "pensyve");
    assert_eq!(
        external.repo_url.as_deref(),
        Some("https://github.com/major7apps/pensyve")
    );
    assert_eq!(
        external.resource_path.as_deref(),
        Some("integrations/claude-code")
    );
    assert!(
        !external.summary.contains("Contains"),
        "external entries list no components"
    );

    let agent = find(&page.items, ResourceKind::Agent, "docs-architect");
    assert_eq!(
        agent.resource_path.as_deref(),
        Some("plugins/code-documentation/agents/docs-architect.md")
    );
    assert_eq!(
        agent.canonical_url.as_deref(),
        Some(
            "https://github.com/wshobson/agents/blob/HEAD/plugins/code-documentation/agents/docs-architect.md"
        )
    );
    let skill = find(&page.items, ResourceKind::Skill, "grounded-vault");
    assert_eq!(
        skill.resource_path.as_deref(),
        Some("plugins/documentation-standards/skills/grounded-vault")
    );
    assert!(page.items.iter().all(|item| !item.actions.install));
}

#[test]
fn queries_match_components_by_name_and_bundle() {
    let provider = wshobson_agents::provider();
    let (env, _) = fixture_env(full_http());
    let page = provider
        .search(
            &request("tutorial", &[ResourceKind::Agent]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    // Title hit ranks first; siblings match via the bundle description.
    assert_eq!(page.items[0].title, "tutorial-engineer");
    assert_eq!(page.items.len(), 3);
    assert!(
        page.items
            .iter()
            .all(|item| item.upstream_id.starts_with("code-documentation/agent:"))
    );
}

#[test]
fn unsupported_kinds_make_no_request() {
    let provider = wshobson_agents::provider();
    let (env, http, _) = testing::offline_env();
    let page = provider
        .search(
            &request("", &[ResourceKind::McpServer, ResourceKind::Rule]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    assert!(page.items.is_empty() && http.requests().is_empty());
}

#[test]
fn malformed_and_forbidden() {
    let provider = wshobson_agents::provider();
    let (env, _) = fixture_env(FixtureHttp::new().route_json(index_url(), "[1, 2]"));
    assert!(matches!(
        provider
            .search(&request("", &[]), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Malformed { .. }
    ));
    let forbidden = HttpResponse {
        status: 403,
        body: Vec::new(),
        retry_after_secs: None,
    };
    let (env, _) = fixture_env(
        FixtureHttp::new()
            .route_json(
                index_url(),
                include_str!("fixtures/wshobson-agents/marketplace.json"),
            )
            .route(TREE, forbidden),
    );
    assert_eq!(
        provider
            .search(
                &request("", &[ResourceKind::Skill]),
                &env,
                &CancelToken::new()
            )
            .unwrap_err(),
        ProviderError::Forbidden
    );
}

#[test]
fn auth_status_is_local() {
    let provider = wshobson_agents::provider();
    let (env, http, _) = testing::offline_env();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert!(http.requests().is_empty());
}

#[test]
fn bundle_detail_exposes_hook_scripts_for_review() {
    let provider = wshobson_agents::provider();
    let (env, _) = fixture_env(full_http());
    let cancel = CancelToken::new();
    let page = provider
        .search(&request("protect", &[ResourceKind::Plugin]), &env, &cancel)
        .unwrap();
    let detail = provider.detail(&page.items[0], &env, &cancel).unwrap();
    let manifest = detail.manifest_json.unwrap();
    assert!(manifest.contains("plugins/protect-mcp/hooks/hooks.json"));
    assert!(manifest.contains("evaluate.sh"), "{manifest}");
    assert!(
        detail
            .files
            .iter()
            .any(|file| file.path == "plugins/protect-mcp/hooks/sign.sh")
    );
    assert!(detail.item.summary.contains("executable hooks"));
}

#[test]
#[ignore = "live network: wshobson/agents"]
fn live_search() {
    let provider = wshobson_agents::provider();
    let env = ProviderEnv::system();
    let page = provider
        .search(&request("", &[]), &env, &CancelToken::new())
        .expect("live search");
    let count = |kind| page.items.iter().filter(|item| item.kind == kind).count();
    eprintln!(
        "wshobson-agents live: {} items on first page ({} plugins, {} agents, {} skills), next={:?}",
        page.items.len(),
        count(ResourceKind::Plugin),
        count(ResourceKind::Agent),
        count(ResourceKind::Skill),
        page.next_cursor,
    );
    assert!(count(ResourceKind::Agent) + count(ResourceKind::Skill) > 0);
}
