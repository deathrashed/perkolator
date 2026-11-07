//! PulseMCP adapter. The 401 fixture is a recorded live response; success
//! bodies are synthetic (from PulseMCP's v0.1 docs/OpenAPI) because partner
//! credentials were not available.

use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{FakeCommands, FixtureHttp, MemoryCredentials, env};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, HttpRequest, HttpResponse, MarketplaceProvider, ProviderEnv,
    ProviderError, RemoteItem, SearchRequest,
};
use perkolator_providers::mcp::pulsemcp::{API_BASE, DOCS_URL, KEY_SLOT, PulseMcp, TENANT_SLOT};

const LIST: &str = include_str!("fixtures/pulsemcp/servers_docs_example.json");
const DETAIL: &str = include_str!("fixtures/pulsemcp/detail_docs_example.json");
const ERROR_401: &str = include_str!("fixtures/pulsemcp/error_401_no_key.json");
const KEY: &str = "fake-pulse-key";
const TENANT: &str = "fake-tenant";

fn setup(http: FixtureHttp, credentials: MemoryCredentials) -> (ProviderEnv, Arc<FixtureHttp>) {
    let http = Arc::new(http);
    (
        env(http.clone(), Arc::new(FakeCommands::new()), credentials),
        http,
    )
}

fn partner() -> MemoryCredentials {
    MemoryCredentials::default()
        .with(KEY_SLOT, KEY)
        .with(TENANT_SLOT, TENANT)
}

fn request(query: &str) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.page_size = 10;
    request
}

fn listed_item() -> RemoteItem {
    let (env, _) = setup(FixtureHttp::new().route_json(API_BASE, LIST), partner());
    PulseMcp::new()
        .search(&request(""), &env, &CancelToken::new())
        .unwrap()
        .items
        .remove(0)
}

fn search_err(response: HttpResponse) -> ProviderError {
    let (env, _) = setup(FixtureHttp::new().route(API_BASE, response), partner());
    PulseMcp::new()
        .search(&request("acme"), &env, &CancelToken::new())
        .unwrap_err()
}

#[test]
fn without_partner_credentials_access_is_needed_and_nothing_is_requested() {
    let provider = PulseMcp::new();
    for credentials in [
        MemoryCredentials::default(),
        MemoryCredentials::default().with(KEY_SLOT, KEY),
        MemoryCredentials::default().with(TENANT_SLOT, TENANT),
    ] {
        let (env, http) = setup(FixtureHttp::new(), credentials);
        assert_eq!(provider.auth_status(&env), AuthStatus::NeedsAccess);
        assert!(!provider.auth_status(&env).allows_search());
        assert_eq!(
            provider
                .search(&request("acme"), &env, &CancelToken::new())
                .unwrap_err(),
            ProviderError::NeedsAuth
        );
        assert_eq!(
            provider
                .detail(&listed_item(), &env, &CancelToken::new())
                .unwrap_err(),
            ProviderError::NeedsAuth
        );
        assert!(http.requests().is_empty());
    }
    assert_eq!(
        provider.connect_intent(),
        AuthIntent::OpenExternalUrl {
            url: DOCS_URL.into()
        }
    );
}

#[test]
fn partner_search_parses_registry_list() {
    let (env, http) = setup(FixtureHttp::new().route_json(API_BASE, LIST), partner());
    let provider = PulseMcp::new();
    assert_eq!(
        provider.auth_status(&env),
        AuthStatus::Ready { identity: None }
    );
    let page = provider
        .search(&request("ac"), &env, &CancelToken::new())
        .unwrap();
    let url = &http.requests()[0];
    assert_eq!(
        url,
        &format!("{API_BASE}/servers?version=latest&limit=10&search=ac")
    );
    assert!(!url.contains(KEY) && !url.contains(TENANT));
    assert_eq!(page.items.len(), 1, "soft-deleted entries are dropped");
    assert_eq!(page.next_cursor.as_deref(), Some("com.acme/mcp:1.2.0"));
    let acme = &page.items[0];
    assert_eq!(acme.source.as_str(), "pulsemcp");
    assert_eq!(acme.upstream_id, "com.acme/mcp");
    assert_eq!(acme.version.as_deref(), Some("1.2.0"));
    assert_eq!(
        acme.repo_url.as_deref(),
        Some("https://github.com/acme/acme-mcp")
    );
    assert_eq!(acme.canonical_url, None, "the API is not publicly readable");
    let serialized = serde_json::to_string(&page).unwrap();
    assert!(
        !serialized.contains("12450"),
        "visitor estimates are not surfaced"
    );
    assert!(!serialized.contains(KEY));
}

#[test]
fn cursor_is_forwarded() {
    let (env, http) = setup(FixtureHttp::new().route_json(API_BASE, LIST), partner());
    let mut req = request("");
    req.cursor = Some("com.acme/mcp:1.2.0".into());
    PulseMcp::new()
        .search(&req, &env, &CancelToken::new())
        .unwrap();
    assert!(http.requests()[0].ends_with("&cursor=com.acme%2Fmcp%3A1.2.0"));
}

#[test]
fn http_errors_map_to_typed_errors() {
    let unauthorized = HttpResponse {
        status: 401,
        body: ERROR_401.as_bytes().to_vec(),
        retry_after_secs: None,
    };
    assert_eq!(search_err(unauthorized), ProviderError::NeedsAuth);
    let wrong_tenant = HttpResponse {
        status: 403,
        body: Vec::new(),
        retry_after_secs: None,
    };
    assert_eq!(search_err(wrong_tenant), ProviderError::Forbidden);
    let limited = HttpResponse {
        status: 429,
        body: Vec::new(),
        retry_after_secs: Some(60),
    };
    assert_eq!(
        search_err(limited),
        ProviderError::RateLimited {
            retry_after_secs: Some(60)
        }
    );
    assert!(matches!(
        search_err(HttpResponse::ok(b"[]".to_vec())),
        ProviderError::Malformed { .. }
    ));
}

#[test]
fn other_kinds_and_cancellation_make_no_request() {
    let (env, http) = setup(FixtureHttp::new(), partner());
    let mut req = request("acme");
    req.kinds = BTreeSet::from([ResourceKind::Rule]);
    assert!(
        PulseMcp::new()
            .search(&req, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        PulseMcp::new()
            .search(&request("acme"), &env, &cancel)
            .unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(http.requests().is_empty());
}

#[test]
fn detail_reads_premium_tools_and_redacts_secrets() {
    let (env, http) = setup(FixtureHttp::new().route_json(API_BASE, DETAIL), partner());
    let detail = PulseMcp::new()
        .detail(&listed_item(), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [format!("{API_BASE}/servers/com.acme%2Fmcp/versions/1.2.0")]
    );
    let names: Vec<&str> = detail.tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names.len(), 2, "duplicate tool names collapse: {names:?}");
    assert!(names.contains(&"read_file") && names.contains(&"list_directory"));
    let serialized = serde_json::to_string(&detail).unwrap();
    assert!(!serialized.contains("fake-pulse-literal-secret"));
    assert!(!serialized.contains(KEY) && !serialized.contains(TENANT));
}

#[test]
#[ignore = "live: confirms PulseMCP still requires partner credentials"]
fn live_api_requires_partner_credentials() {
    let env = ProviderEnv::system();
    let response = env
        .http
        .get(
            &HttpRequest::get(format!("{API_BASE}/servers?limit=1"))
                .header("User-Agent", env.user_agent.clone()),
        )
        .expect("network");
    assert_eq!(response.status, 401, "PulseMCP now allows anonymous reads");
    if env.credentials.get(KEY_SLOT).is_none() {
        assert_eq!(PulseMcp::new().auth_status(&env), AuthStatus::NeedsAccess);
    }
}
