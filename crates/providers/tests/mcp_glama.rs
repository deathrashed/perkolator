//! Glama adapter. The 401 fixture is a recorded live response; success
//! bodies are synthetic (from Glama's published OpenAPI) because no API key
//! was available.

use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{FakeCommands, FixtureHttp, MemoryCredentials, env};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, AuthStrategy, HttpRequest, HttpResponse, MarketplaceProvider,
    ProviderEnv, ProviderError, RemoteItem, SearchRequest,
};
use perkolator_providers::mcp::glama::{API_BASE, Glama, KEY_SLOT, KEYS_URL};

const LIST: &str = include_str!("fixtures/glama/servers_schema_example.json");
const DETAIL: &str = include_str!("fixtures/glama/server_detail_schema_example.json");
const ERROR_401: &str = include_str!("fixtures/glama/error_401_no_key.json");
const KEY: &str = "glm_fake-test-key";

fn setup(http: FixtureHttp, with_key: bool) -> (ProviderEnv, Arc<FixtureHttp>) {
    let http = Arc::new(http);
    let credentials = if with_key {
        MemoryCredentials::default().with(KEY_SLOT, KEY)
    } else {
        MemoryCredentials::default()
    };
    (
        env(http.clone(), Arc::new(FakeCommands::new()), credentials),
        http,
    )
}

fn request(query: &str) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.page_size = 2;
    request
}

fn search_err(response: HttpResponse) -> ProviderError {
    let (env, _) = setup(FixtureHttp::new().route(API_BASE, response), true);
    Glama::new()
        .search(&request("hn"), &env, &CancelToken::new())
        .unwrap_err()
}

fn listed_item(index: usize) -> RemoteItem {
    let (env, _) = setup(FixtureHttp::new().route_json(API_BASE, LIST), true);
    Glama::new()
        .search(&request(""), &env, &CancelToken::new())
        .unwrap()
        .items
        .remove(index)
}

#[test]
fn without_a_key_nothing_is_requested() {
    let (env, http) = setup(FixtureHttp::new(), false);
    let provider = Glama::new();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotConfigured);
    assert_eq!(
        provider
            .search(&request("hn"), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::NeedsAuth
    );
    assert_eq!(
        provider
            .detail(&listed_item(0), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::NeedsAuth
    );
    assert!(http.requests().is_empty());
}

#[test]
fn descriptor_declares_key_and_attribution() {
    let provider = Glama::new();
    let descriptor = provider.descriptor();
    assert!(matches!(&descriptor.auth, AuthStrategy::ApiKey { slot, .. } if slot == KEY_SLOT));
    assert_eq!(
        provider.connect_intent(),
        AuthIntent::PromptSecret {
            slot: KEY_SLOT.into(),
            docs_url: KEYS_URL.into()
        }
    );
    let attribution = descriptor
        .capabilities
        .requires_attribution
        .as_deref()
        .unwrap();
    assert!(attribution.contains("https://glama.ai"));
    let (env, http) = setup(FixtureHttp::new(), true);
    assert_eq!(
        provider.auth_status(&env),
        AuthStatus::Ready { identity: None }
    );
    assert!(http.requests().is_empty());
}

#[test]
fn parses_list_with_listing_links_and_cursor() {
    let (env, http) = setup(FixtureHttp::new().route_json(API_BASE, LIST), true);
    let page = Glama::new()
        .search(&request("hacker news"), &env, &CancelToken::new())
        .unwrap();
    let url = &http.requests()[0];
    assert_eq!(
        url,
        &format!("{API_BASE}/servers?first=2&sort=added-at%3Adesc&query=hacker%20news")
    );
    assert!(!url.contains(KEY));
    assert_eq!(
        page.next_cursor.as_deref(),
        Some("eyJjcmVhdGVkQXQiOjE3NDA2OTcyNDM3MzUsImlkIjoiNlFPQmZsY1lwbSJ9")
    );
    assert_eq!(page.total, None);
    let playwright = &page.items[0];
    assert_eq!(playwright.source.as_str(), "glama");
    assert_eq!(playwright.kind, ResourceKind::McpServer);
    assert_eq!(playwright.upstream_id, "oge85xl22f");
    assert_eq!(playwright.title, "Playwright MCP Server");
    assert_eq!(
        playwright.canonical_url.as_deref(),
        Some("https://glama.ai/mcp/servers/microsoft/playwright-mcp")
    );
    assert_eq!(
        playwright.repo_url.as_deref(),
        Some("https://github.com/microsoft/playwright-mcp")
    );
    assert_eq!(playwright.author.as_deref(), Some("microsoft"));
    assert_eq!(
        playwright.categories,
        ["author:official", "hosting:local-only"]
    );
    assert_eq!(playwright.version, None);
    assert!(playwright.actions.view_source);
    assert!(!serde_json::to_string(&page).unwrap().contains(KEY));
}

#[test]
fn cursor_is_sent_as_after_and_last_page_has_none() {
    let last = r#"{"pageInfo":{"endCursor":"zzz","hasNextPage":false,"hasPreviousPage":true,"startCursor":"a"},"servers":[]}"#;
    let (env, http) = setup(FixtureHttp::new().route_json(API_BASE, last), true);
    let mut req = request("");
    req.cursor = Some("abc=/+".into());
    let page = Glama::new()
        .search(&req, &env, &CancelToken::new())
        .unwrap();
    assert!(http.requests()[0].ends_with("&after=abc%3D%2F%2B"));
    assert_eq!(page.next_cursor, None);
}

#[test]
fn http_errors_map_to_typed_errors() {
    let unauthorized = HttpResponse {
        status: 401,
        body: ERROR_401.as_bytes().to_vec(),
        retry_after_secs: None,
    };
    assert_eq!(search_err(unauthorized), ProviderError::NeedsAuth);
    let banned = HttpResponse {
        status: 403,
        body: Vec::new(),
        retry_after_secs: None,
    };
    assert_eq!(search_err(banned), ProviderError::Forbidden);
    let limited = HttpResponse {
        status: 429,
        body: Vec::new(),
        retry_after_secs: Some(1),
    };
    assert_eq!(
        search_err(limited),
        ProviderError::RateLimited {
            retry_after_secs: Some(1)
        }
    );
    assert!(matches!(
        search_err(HttpResponse::ok(b"{".to_vec())),
        ProviderError::Malformed { .. }
    ));
    assert!(matches!(
        search_err(HttpResponse::ok(br#"{"pageInfo":{}}"#.to_vec())),
        ProviderError::Malformed { .. }
    ));
}

#[test]
fn other_kinds_and_cancellation_make_no_request() {
    let (env, http) = setup(FixtureHttp::new(), true);
    let mut req = request("hn");
    req.kinds = BTreeSet::from([ResourceKind::Agent]);
    assert!(
        Glama::new()
            .search(&req, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        Glama::new()
            .search(&request("hn"), &env, &cancel)
            .unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(http.requests().is_empty());
}

#[test]
fn detail_returns_tools_and_redacts_env_defaults() {
    let (env, http) = setup(FixtureHttp::new().route_json(API_BASE, DETAIL), true);
    let detail = Glama::new()
        .detail(&listed_item(1), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), [format!("{API_BASE}/servers/6qobflcypm")]);
    assert_eq!(detail.item.categories, ["hosting:remote-capable"]);
    assert_eq!(detail.tools.len(), 2);
    assert_eq!(detail.tools[0].name, "get_stories");
    assert_eq!(detail.tools[1].description, None);
    assert!(detail.tools.iter().all(|tool| tool.input_schema.is_some()));
    let serialized = serde_json::to_string(&detail).unwrap();
    assert!(!serialized.contains("fake-glama-default-secret"));
    assert!(!serialized.contains(KEY));
    assert!(detail.manifest_json.unwrap().contains("HN_TOKEN"));
}

#[test]
#[ignore = "live: confirms Glama still rejects anonymous directory reads"]
fn live_directory_requires_a_key() {
    let env = ProviderEnv::system();
    let response = env
        .http
        .get(
            &HttpRequest::get(format!("{API_BASE}/servers?first=1"))
                .header("User-Agent", env.user_agent.clone()),
        )
        .expect("network");
    assert_eq!(response.status, 401, "Glama now allows anonymous reads");
    if env.credentials.get(KEY_SLOT).is_none() {
        assert_eq!(Glama::new().auth_status(&env), AuthStatus::NotConfigured);
    }
}
