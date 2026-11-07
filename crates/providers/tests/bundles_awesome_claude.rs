//! Offline fixture tests for the metadata-only `awesome-claude` adapter
//! (real, trimmed README) plus an ignored live smoke.

use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{self, FakeCommands, FixtureHttp, MemoryCredentials};
use perkolator_marketplace::{
    AuthStatus, HttpResponse, ProviderEnv, ProviderError, RemoteItem, SearchRequest,
};
use perkolator_providers::bundles::awesome_claude;

const README_URL: &str =
    "https://raw.githubusercontent.com/ianwieds/awesome-claude-code/HEAD/README.md";
const README: &str = include_str!("fixtures/awesome-claude/README.md");

fn fixture_env(http: FixtureHttp) -> (ProviderEnv, Arc<FixtureHttp>) {
    let http = Arc::new(http);
    let env = testing::env(
        http.clone(),
        Arc::new(FakeCommands::new()),
        MemoryCredentials::default(),
    );
    (env, http)
}

fn request(query: &str, kinds: &[ResourceKind]) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.page_size = 100;
    request.kinds = kinds.iter().copied().collect();
    request
}

fn titles(items: &[RemoteItem], kind: ResourceKind) -> Vec<&str> {
    items
        .iter()
        .filter(|item| item.kind == kind)
        .map(|item| item.title.as_str())
        .collect()
}

#[test]
fn sections_map_to_kinds_with_source_links_only() {
    let provider = awesome_claude::provider();
    let (env, http) = fixture_env(FixtureHttp::new().route_json(README_URL, README));
    let page = provider
        .search(&request("", &[]), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), [README_URL]);

    assert_eq!(
        titles(&page.items, ResourceKind::Plugin).len(),
        4 + 3,
        "plugins + hooks"
    );
    assert_eq!(
        titles(&page.items, ResourceKind::Skill).len(),
        4,
        "skill tools skipped"
    );
    assert_eq!(
        titles(&page.items, ResourceKind::Agent).len(),
        4,
        "docs link skipped"
    );
    assert_eq!(titles(&page.items, ResourceKind::Prompt).len(), 3);
    assert_eq!(titles(&page.items, ResourceKind::McpServer).len(), 3);
    assert_eq!(titles(&page.items, ResourceKind::Rule).len(), 5);
    assert!(!titles(&page.items, ResourceKind::Plugin).contains(&"AgentStore"));
    assert!(!titles(&page.items, ResourceKind::Agent).contains(&"Subagents docs"));

    for item in &page.items {
        assert!(item.actions.view_source && !item.actions.fetch_files && !item.actions.install);
        assert_eq!(item.package_ref, None);
        assert_eq!(item.version, None);
        assert!(item.repo_url.is_some(), "every fixture link is on GitHub");
        assert!(item.resource_path.is_some());
    }

    let commit = page
        .items
        .iter()
        .find(|item| item.title == "/commit")
        .unwrap();
    assert_eq!(
        commit.repo_url.as_deref(),
        Some("https://github.com/evmts/tevm")
    );
    assert_eq!(
        commit.resource_path.as_deref(),
        Some(".claude/commands/commit.md")
    );
    assert_eq!(commit.categories, ["Slash commands"]);

    let bouncer = page
        .items
        .iter()
        .find(|item| item.title == "Bouncer")
        .unwrap();
    assert!(
        bouncer.summary.contains("runs executable code"),
        "{}",
        bouncer.summary
    );
    assert_eq!(bouncer.categories, ["Hooks"]);
}

#[test]
fn kind_and_category_filters() {
    let provider = awesome_claude::provider();
    let (env, _) = fixture_env(FixtureHttp::new().route_json(README_URL, README));
    let rules = provider
        .search(
            &request("agents.md", &[ResourceKind::Rule]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    assert!(!rules.items.is_empty());
    assert!(
        rules
            .items
            .iter()
            .all(|item| item.kind == ResourceKind::Rule)
    );

    let mut hooks = request("", &[]);
    hooks.category = Some("hooks".into());
    let page = provider.search(&hooks, &env, &CancelToken::new()).unwrap();
    assert_eq!(page.items.len(), 3);
}

#[test]
fn detail_is_not_offered_and_makes_no_request() {
    let provider = awesome_claude::provider();
    assert!(!provider.descriptor().capabilities.detail);
    let (env, http) = fixture_env(FixtureHttp::new().route_json(README_URL, README));
    let page = provider
        .search(&request("bouncer", &[]), &env, &CancelToken::new())
        .unwrap();
    assert!(matches!(
        provider
            .detail(&page.items[0], &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Unsupported { .. }
    ));
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn malformed_and_forbidden() {
    let provider = awesome_claude::provider();
    let (env, _) = fixture_env(FixtureHttp::new().route_json(README_URL, "<html>moved</html>"));
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
    let (env, _) = fixture_env(FixtureHttp::new().route(README_URL, forbidden));
    assert_eq!(
        provider
            .search(&request("", &[]), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Forbidden
    );
}

#[test]
fn auth_status_is_local() {
    let provider = awesome_claude::provider();
    let (env, http, _) = testing::offline_env();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert!(http.requests().is_empty());
}

#[test]
#[ignore = "live network: ianwieds/awesome-claude-code"]
fn live_search() {
    let provider = awesome_claude::provider();
    let env = ProviderEnv::system();
    let mut everything = request("", &[]);
    everything.page_size = 100;
    let page = provider
        .search(&everything, &env, &CancelToken::new())
        .expect("live search");
    let count = |kind| page.items.iter().filter(|item| item.kind == kind).count();
    eprintln!(
        "awesome-claude live: {} items on first page ({} plugins, {} skills, {} agents, {} prompts, {} mcp, {} rules), next={:?}",
        page.items.len(),
        count(ResourceKind::Plugin),
        count(ResourceKind::Skill),
        count(ResourceKind::Agent),
        count(ResourceKind::Prompt),
        count(ResourceKind::McpServer),
        count(ResourceKind::Rule),
        page.next_cursor,
    );
    assert!(!page.items.is_empty());
}
