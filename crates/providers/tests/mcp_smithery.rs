//! Smithery adapter against responses recorded from registry.smithery.ai
//! on 2026-11-07 (after Smithery's 2026-10-31 API/CLI shutdown).

use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{FakeCommands, FixtureHttp, MemoryCredentials, env};
use perkolator_marketplace::{
    AuthStatus, HttpResponse, MarketplaceProvider, Pagination, ProviderEnv, ProviderError,
    RemoteItem, SearchRequest,
};
use perkolator_providers::mcp::smithery::{API_BASE, Smithery};

const PAGE1: &str = include_str!("fixtures/smithery/list_page1.json");
const PAGE2: &str = include_str!("fixtures/smithery/list_page2.json");
const DETAIL_EXA: &str = include_str!("fixtures/smithery/detail_exa.json");
const ERROR_500: &str = include_str!("fixtures/smithery/error_500_query.json");
const ERROR_404: &str = include_str!("fixtures/smithery/error_404_not_found.json");

fn setup(
    http: FixtureHttp,
    commands: FakeCommands,
) -> (ProviderEnv, Arc<FixtureHttp>, Arc<FakeCommands>) {
    let http = Arc::new(http);
    let commands = Arc::new(commands);
    (
        env(http.clone(), commands.clone(), MemoryCredentials::default()),
        http,
        commands,
    )
}

fn request(query: &str, page_size: usize) -> SearchRequest {
    let mut request = SearchRequest::new(query);
    request.page_size = page_size;
    request
}

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.as_bytes().to_vec(),
        retry_after_secs: None,
    }
}

fn search_err(response: HttpResponse, query: &str) -> ProviderError {
    let (env, _, _) = setup(
        FixtureHttp::new().route(API_BASE, response),
        FakeCommands::new(),
    );
    Smithery::new()
        .search(&request(query, 3), &env, &CancelToken::new())
        .unwrap_err()
}

fn item(name: &str) -> RemoteItem {
    let (env, _, _) = setup(
        FixtureHttp::new().route_json(API_BASE, PAGE1),
        FakeCommands::new(),
    );
    let mut item = Smithery::new()
        .search(&request("", 3), &env, &CancelToken::new())
        .unwrap()
        .items
        .remove(0);
    item.upstream_id = name.into();
    item.package_ref = Some(name.into());
    item
}

#[test]
fn parses_recorded_browse_page() {
    let (env, http, _) = setup(
        FixtureHttp::new().route_json(API_BASE, PAGE1),
        FakeCommands::new(),
    );
    let page = Smithery::new()
        .search(&request("", 3), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [format!("{API_BASE}/servers?page=1&pageSize=3")]
    );
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.next_cursor.as_deref(), Some("2"));
    assert_eq!(page.total, Some(20759));

    let brave = &page.items[0];
    assert_eq!(brave.source.as_str(), "smithery");
    assert_eq!(brave.kind, ResourceKind::McpServer);
    assert_eq!(brave.upstream_id, "brave");
    assert_eq!(brave.title, "Brave Search");
    assert_eq!(brave.package_ref.as_deref(), Some("brave"));
    assert_eq!(brave.author.as_deref(), Some("brave"));
    assert_eq!(
        brave.canonical_url.as_deref(),
        Some("https://smithery.ai/servers/brave")
    );
    assert_eq!(brave.repo_url, None);
    assert_eq!(brave.version, None);
    assert!(!brave.actions.view_source && !brave.actions.install);

    let item_json = serde_json::to_string(&page.items).unwrap();
    assert!(!item_json.contains("useCount") && !item_json.contains("87579"));
}

#[test]
fn page_cursor_round_trips() {
    let (env, http, _) = setup(
        FixtureHttp::new().route_json(API_BASE, PAGE2),
        FakeCommands::new(),
    );
    let mut req = request("", 3);
    req.cursor = Some("2".into());
    let page = Smithery::new()
        .search(&req, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [format!("{API_BASE}/servers?page=2&pageSize=3")]
    );
    assert_eq!(page.next_cursor.as_deref(), Some("3"));

    let last = r#"{"servers":[],"pagination":{"currentPage":5,"pageSize":3,"totalPages":5,"totalCount":15}}"#;
    let (env, _, _) = setup(
        FixtureHttp::new().route_json(API_BASE, last),
        FakeCommands::new(),
    );
    assert_eq!(
        Smithery::new()
            .search(&request("", 3), &env, &CancelToken::new())
            .unwrap()
            .next_cursor,
        None
    );
}

#[test]
fn invalid_cursor_is_rejected_without_a_request() {
    let (env, http, _) = setup(FixtureHttp::new(), FakeCommands::new());
    for cursor in ["abc", "0", "-1"] {
        let mut req = request("", 3);
        req.cursor = Some(cursor.into());
        assert!(matches!(
            Smithery::new().search(&req, &env, &CancelToken::new()),
            Err(ProviderError::Malformed { .. })
        ));
    }
    assert!(http.requests().is_empty());
}

#[test]
fn hidden_entries_are_skipped() {
    let body = r#"{"servers":[
        {"qualifiedName":"a","displayName":"A","unlisted":true},
        {"qualifiedName":"b","displayName":"B","inactive":true},
        {"qualifiedName":"c/d","displayName":"C"},
        {"displayName":"no name"}
    ],"pagination":{"currentPage":1,"totalPages":1,"totalCount":4}}"#;
    let (env, _, _) = setup(
        FixtureHttp::new().route_json(API_BASE, body),
        FakeCommands::new(),
    );
    let page = Smithery::new()
        .search(&request("", 10), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].canonical_url.as_deref(),
        Some("https://smithery.ai/servers/c/d")
    );
}

#[test]
fn text_search_surfaces_the_providers_server_error() {
    let (env, http, _) = setup(
        FixtureHttp::new().route(API_BASE, response(500, ERROR_500)),
        FakeCommands::new(),
    );
    let error = Smithery::new()
        .search(&request("git hub", 3), &env, &CancelToken::new())
        .unwrap_err();
    assert_eq!(error, ProviderError::Http { status: 500 });
    assert!(http.requests()[0].ends_with("&q=git%20hub"));
}

#[test]
fn http_errors_and_malformed_bodies_map_to_typed_errors() {
    assert_eq!(
        search_err(response(401, "{}"), ""),
        ProviderError::NeedsAuth
    );
    assert_eq!(
        search_err(response(403, "{}"), ""),
        ProviderError::Forbidden
    );
    let limited = HttpResponse {
        status: 429,
        body: Vec::new(),
        retry_after_secs: Some(30),
    };
    assert_eq!(
        search_err(limited, ""),
        ProviderError::RateLimited {
            retry_after_secs: Some(30)
        }
    );
    assert!(matches!(
        search_err(HttpResponse::ok(b"not json".to_vec()), ""),
        ProviderError::Malformed { .. }
    ));
    assert!(matches!(
        search_err(HttpResponse::ok(br#"{"error":"x"}"#.to_vec()), ""),
        ProviderError::Malformed { .. }
    ));
}

#[test]
fn never_probes_the_discontinued_cli_or_network_for_status() {
    let (env, http, commands) = setup(
        FixtureHttp::new().route_json(API_BASE, PAGE1),
        FakeCommands::new().binary("smithery", "{}"),
    );
    let provider = Smithery::new();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert!(http.requests().is_empty());
    provider
        .search(&request("", 3), &env, &CancelToken::new())
        .unwrap();
    assert!(commands.calls().is_empty(), "the Smithery CLI is never run");
    assert_eq!(
        provider.descriptor().capabilities.pagination,
        Pagination::Page
    );
}

#[test]
fn other_kinds_and_cancellation_make_no_request() {
    let (env, http, _) = setup(FixtureHttp::new(), FakeCommands::new());
    let mut req = request("", 3);
    req.kinds = BTreeSet::from([ResourceKind::Plugin]);
    assert!(
        Smithery::new()
            .search(&req, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        Smithery::new()
            .search(&request("", 3), &env, &cancel)
            .unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(http.requests().is_empty());
}

#[test]
fn detail_returns_declared_tools() {
    let (env, http, _) = setup(
        FixtureHttp::new().route_json(format!("{API_BASE}/servers/exa"), DETAIL_EXA),
        FakeCommands::new(),
    );
    let detail = Smithery::new()
        .detail(&item("exa"), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), [format!("{API_BASE}/servers/exa")]);
    assert_eq!(detail.item.title, "Exa Search");
    let names: Vec<&str> = detail.tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, ["web_search_exa", "web_fetch_exa"]);
    assert!(detail.tools.iter().all(|tool| tool.input_schema.is_some()));
    assert!(detail.manifest_json.unwrap().contains("exa.run.tools"));
}

#[test]
fn detail_encodes_each_name_segment_and_redacts_config_defaults() {
    let mut body: serde_json::Value = serde_json::from_str(DETAIL_EXA).unwrap();
    body["connections"][0]["configSchema"] = serde_json::json!({
        "type": "object",
        "properties": {"apiKey": {"type": "string", "default": "fake-smithery-default"}}
    });
    let (env, http, _) = setup(
        FixtureHttp::new().route_json(API_BASE, &body.to_string()),
        FakeCommands::new(),
    );
    let detail = Smithery::new()
        .detail(&item("acme team/tool"), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [format!("{API_BASE}/servers/acme%20team/tool")]
    );
    assert!(
        !serde_json::to_string(&detail)
            .unwrap()
            .contains("fake-smithery-default")
    );

    let (env, http, _) = setup(FixtureHttp::new(), FakeCommands::new());
    assert!(matches!(
        Smithery::new().detail(&item("../admin"), &env, &CancelToken::new()),
        Err(ProviderError::Unsupported { .. })
    ));
    assert!(http.requests().is_empty());
}

#[test]
fn detail_not_found_maps_to_http_404() {
    let (env, _, _) = setup(
        FixtureHttp::new().route(API_BASE, response(404, ERROR_404)),
        FakeCommands::new(),
    );
    assert_eq!(
        Smithery::new()
            .detail(&item("zzz-nonexistent-xyz"), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::Http { status: 404 }
    );
}

#[test]
#[ignore = "live: queries registry.smithery.ai anonymously"]
fn live_browse_detail_and_search_outcome() {
    let env = ProviderEnv::system();
    let provider = Smithery::new();
    let cancel = CancelToken::new();
    let page = provider
        .search(&request("", 5), &env, &cancel)
        .expect("live browse");
    assert!(!page.items.is_empty());
    let detail = provider
        .detail(&page.items[0], &env, &cancel)
        .expect("live detail");
    let search = provider.search(&request("github", 5), &env, &cancel);
    eprintln!(
        "live smithery: browse={} next={:?} detail_tools={} search(github)={:?}",
        page.items.len(),
        page.next_cursor,
        detail.tools.len(),
        search.as_ref().map(|page| page.items.len())
    );
}
