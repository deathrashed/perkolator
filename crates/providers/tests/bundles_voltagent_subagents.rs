//! Offline fixture tests for the `voltagent-subagents` adapter (real,
//! trimmed README and agent file) plus an ignored live smoke.

use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{self, FakeCommands, FixtureHttp, MemoryCredentials};
use perkolator_marketplace::{AuthStatus, HttpResponse, ProviderEnv, ProviderError, SearchRequest};
use perkolator_providers::bundles::voltagent_subagents;

const RAW: &str = "https://raw.githubusercontent.com/VoltAgent/awesome-claude-code-subagents/HEAD/";
const README: &str = include_str!("fixtures/voltagent-subagents/README.md");

fn readme_url() -> String {
    format!("{RAW}README.md")
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
        .route_json(readme_url(), README)
        .route_json(
            format!("{RAW}categories/01-core-development/api-designer.md"),
            include_str!("fixtures/voltagent-subagents/api-designer.md"),
        )
}

fn request(query: &str) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.page_size = 100;
    request
}

#[test]
fn search_parses_categories_with_one_raw_request() {
    let provider = voltagent_subagents::provider();
    let (env, http) = fixture_env(full_http());
    let page = provider
        .search(&request(""), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), [readme_url()]);
    let expected = README
        .lines()
        .filter(|line| line.starts_with("- [**"))
        .count();
    assert_eq!(page.items.len(), expected);
    let api = &page.items[0];
    assert_eq!(api.title, "api-designer");
    assert_eq!(api.kind, ResourceKind::Agent);
    assert_eq!(api.summary, "REST and GraphQL API architect");
    assert_eq!(api.categories, ["Core Development"]);
    assert_eq!(
        api.repo_url.as_deref(),
        Some("https://github.com/VoltAgent/awesome-claude-code-subagents")
    );
    assert_eq!(
        api.resource_path.as_deref(),
        Some("categories/01-core-development/api-designer.md")
    );
    assert_eq!(api.version, None);
    assert!(
        page.items
            .iter()
            .all(|item| item.source.as_str() == voltagent_subagents::ID)
    );
}

#[test]
fn query_and_category_filters() {
    let provider = voltagent_subagents::provider();
    let (env, _) = fixture_env(full_http());
    let page = provider
        .search(&request("rust"), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(page.items[0].title, "rust-engineer");
    let mut languages = request("");
    languages.category = Some("language specialists".into());
    let page = provider
        .search(&languages, &env, &CancelToken::new())
        .unwrap();
    assert!(!page.items.is_empty());
    assert!(
        page.items
            .iter()
            .all(|item| item.categories == ["Language Specialists"])
    );
}

#[test]
fn non_agent_kinds_make_no_request() {
    let provider = voltagent_subagents::provider();
    let (env, http, _) = testing::offline_env();
    let mut skills = request("");
    skills.kinds = [ResourceKind::Skill, ResourceKind::Plugin]
        .into_iter()
        .collect();
    assert!(
        provider
            .search(&skills, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    assert!(http.requests().is_empty());
}

#[test]
fn malformed_readme_and_rate_limit() {
    let provider = voltagent_subagents::provider();
    let (env, _) =
        fixture_env(FixtureHttp::new().route_json(readme_url(), "# Moved\n\nSee elsewhere."));
    assert!(matches!(
        provider
            .search(&request(""), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Malformed { .. }
    ));
    let limited = HttpResponse {
        status: 429,
        body: Vec::new(),
        retry_after_secs: Some(60),
    };
    let (env, _) = fixture_env(FixtureHttp::new().route(readme_url(), limited));
    assert_eq!(
        provider
            .search(&request(""), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::RateLimited {
            retry_after_secs: Some(60)
        }
    );
}

#[test]
fn auth_status_is_local() {
    let provider = voltagent_subagents::provider();
    let (env, http, _) = testing::offline_env();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert!(http.requests().is_empty());
}

#[test]
fn detail_parses_agent_frontmatter() {
    let provider = voltagent_subagents::provider();
    let (env, http) = fixture_env(full_http());
    let cancel = CancelToken::new();
    let page = provider
        .search(&request("api-designer"), &env, &cancel)
        .unwrap();
    let detail = provider.detail(&page.items[0], &env, &cancel).unwrap();
    assert!(
        detail
            .item
            .summary
            .starts_with("Use this agent when designing new APIs")
    );
    assert!(
        detail
            .item
            .summary
            .contains("Tools: Read, Write, Edit, Bash, Glob, Grep")
    );
    assert!(detail.item.summary.contains("Model: sonnet"));
    let manifest = detail.manifest_json.unwrap();
    assert!(manifest.contains("\"name\": \"api-designer\""));
    assert!(
        detail
            .readme_markdown
            .unwrap()
            .trim_start()
            .starts_with("You are a senior API designer")
    );
    assert_eq!(detail.files.len(), 1);
    assert_eq!(http.requests().len(), 2);
}

#[test]
fn detail_rejects_paths_outside_the_index() {
    let provider = voltagent_subagents::provider();
    let (env, http) = fixture_env(full_http());
    let cancel = CancelToken::new();
    let mut item = provider.search(&request(""), &env, &cancel).unwrap().items[0].clone();
    item.resource_path = Some("categories/../../secrets.md".into());
    assert!(matches!(
        provider.detail(&item, &env, &cancel).unwrap_err(),
        ProviderError::Unsupported { .. }
    ));
    assert_eq!(http.requests().len(), 1);
}

#[test]
#[ignore = "live network: VoltAgent/awesome-claude-code-subagents"]
fn live_search_and_detail() {
    let provider = voltagent_subagents::provider();
    let env = ProviderEnv::system();
    let cancel = CancelToken::new();
    let mut everything = request("");
    everything.page_size = 100;
    let page = provider
        .search(&everything, &env, &cancel)
        .expect("live search");
    eprintln!(
        "voltagent-subagents live: {} agents on first page, next={:?}",
        page.items.len(),
        page.next_cursor
    );
    assert!(!page.items.is_empty());
    let detail = provider
        .detail(&page.items[0], &env, &cancel)
        .expect("live detail");
    eprintln!("{}: {}", detail.item.title, detail.item.summary);
    assert!(
        detail.manifest_json.is_some(),
        "frontmatter missing in live file"
    );
}
