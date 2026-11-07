//! Official MCP Registry adapter against recorded registry responses
//! (fixtures captured from registry.modelcontextprotocol.io on 2026-11-07).

use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{FakeCommands, FixtureHttp, MemoryCredentials, env};
use perkolator_marketplace::{
    AuthStatus, HttpResponse, MarketplaceProvider, ProviderEnv, ProviderError, RemoteItem,
    SearchRequest,
};
use perkolator_providers::mcp::official::{API_BASE, McpOfficial};

const PAGE1: &str = include_str!("fixtures/mcp-official/search_filesystem_page1.json");
const PAGE2: &str = include_str!("fixtures/mcp-official/search_filesystem_page2.json");
const DETAIL: &str = include_str!("fixtures/mcp-official/detail_mcp_filesystem.json");
const DETAIL_SECRET_HEADER: &str =
    include_str!("fixtures/mcp-official/detail_remote_secret_header.json");
const ERROR_422: &str = include_str!("fixtures/mcp-official/error_422_short_search.json");
const LITERAL_SECRETS: &str = include_str!("fixtures/mcp-official/synthetic_literal_secrets.json");
const FAKE_SECRETS: [&str; 5] = [
    "fake-header-literal-secret",
    "fake-env-literal-secret",
    "fake-secret-default",
    "fake-arg-literal-secret",
    "fake-variable-secret",
];

fn setup(http: FixtureHttp) -> (ProviderEnv, Arc<FixtureHttp>, Arc<FakeCommands>) {
    let http = Arc::new(http);
    let commands = Arc::new(FakeCommands::new());
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

fn status(code: u16, retry_after_secs: Option<u64>) -> HttpResponse {
    HttpResponse {
        status: code,
        body: b"{}".to_vec(),
        retry_after_secs,
    }
}

fn search_err(response: HttpResponse) -> ProviderError {
    let (env, _, _) = setup(FixtureHttp::new().route(API_BASE, response));
    McpOfficial::new()
        .search(&request("filesystem", 5), &env, &CancelToken::new())
        .unwrap_err()
}

fn first_item(fixture: &str) -> RemoteItem {
    let (env, _, _) = setup(FixtureHttp::new().route_json(API_BASE, fixture));
    McpOfficial::new()
        .search(&request("filesystem", 3), &env, &CancelToken::new())
        .unwrap()
        .items
        .remove(0)
}

#[test]
fn parses_recorded_search_page() {
    let (env, http, commands) = setup(FixtureHttp::new().route_json(API_BASE, PAGE1));
    let page = McpOfficial::new()
        .search(&request("filesystem", 3), &env, &CancelToken::new())
        .unwrap();

    assert_eq!(
        http.requests(),
        [format!(
            "{API_BASE}/servers?version=latest&limit=3&search=filesystem"
        )]
    );
    assert!(commands.calls().is_empty());
    assert_eq!(page.items.len(), 3);
    assert_eq!(
        page.next_cursor.as_deref(),
        Some("io.github.Evozim/chroot-filesystem-jail-mcp:2.0.0")
    );
    assert_eq!(
        page.total, None,
        "registry `count` is a page count, not a total"
    );

    let first = &page.items[0];
    assert_eq!(first.source.as_str(), "mcp-official");
    assert_eq!(first.kind, ResourceKind::McpServer);
    assert_eq!(first.upstream_id, "com.pulsemcp/remote-filesystem");
    assert_eq!(
        first.package_ref.as_deref(),
        Some("com.pulsemcp/remote-filesystem")
    );
    assert_eq!(first.title, "com.pulsemcp/remote-filesystem");
    assert_eq!(first.version.as_deref(), Some("0.1.5"));
    assert_eq!(first.author.as_deref(), Some("com.pulsemcp"));
    assert_eq!(
        first.repo_url.as_deref(),
        Some("https://github.com/pulsemcp/mcp-servers")
    );
    assert_eq!(
        first.resource_path.as_deref(),
        Some("experimental/remote-filesystem")
    );
    assert_eq!(
        first.canonical_url.as_deref(),
        Some(
            "https://registry.modelcontextprotocol.io/v0.1/servers/com.pulsemcp%2Fremote-filesystem/versions/latest"
        )
    );
    assert!(first.actions.view_source);
    assert!(!first.actions.install && !first.actions.fetch_files);
    assert!(first.categories.is_empty());
    assert!(first.fetched_at_unix > 0);

    let remote_only = &page.items[2];
    assert_eq!(remote_only.title, "Execute chroot-filesystem-jail-mcp");
    assert_eq!(remote_only.repo_url, None);
    assert!(!remote_only.actions.view_source);
}

#[test]
fn forwards_the_cursor_and_reads_the_next_one() {
    let (env, http, _) = setup(FixtureHttp::new().route_json(API_BASE, PAGE2));
    let mut req = request("filesystem", 3);
    req.cursor = Some("io.github.Evozim/chroot-filesystem-jail-mcp:2.0.0".into());
    let page = McpOfficial::new()
        .search(&req, &env, &CancelToken::new())
        .unwrap();
    assert!(http.requests()[0].ends_with(
        "&search=filesystem&cursor=io.github.Evozim%2Fchroot-filesystem-jail-mcp%3A2.0.0"
    ));
    assert_eq!(
        page.items[0].upstream_id,
        "io.github.Oncorporation/filesystem-server"
    );
    assert_eq!(
        page.next_cursor.as_deref(),
        Some("io.github.domdomegg/filesystem-mcp:1.3.0")
    );
}

#[test]
fn empty_query_browses_and_short_query_makes_no_request() {
    let (env, http, _) = setup(FixtureHttp::new().route_json(API_BASE, PAGE1));
    let provider = McpOfficial::new();
    provider
        .search(&request("  ", 200), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [format!("{API_BASE}/servers?version=latest&limit=100")]
    );

    let (env, http, _) = setup(FixtureHttp::new());
    let page = provider
        .search(&request("ab", 10), &env, &CancelToken::new())
        .unwrap();
    assert!(page.items.is_empty() && page.next_cursor.is_none());
    assert!(
        http.requests().is_empty(),
        "registry answers 422 for <3 chars"
    );
}

#[test]
fn query_is_percent_encoded() {
    let (env, http, _) = setup(FixtureHttp::new().route_json(API_BASE, PAGE1));
    McpOfficial::new()
        .search(&request("a&b=c d", 3), &env, &CancelToken::new())
        .unwrap();
    assert!(http.requests()[0].ends_with("&search=a%26b%3Dc%20d"));
}

#[test]
fn http_errors_map_to_typed_errors() {
    assert_eq!(search_err(status(401, None)), ProviderError::NeedsAuth);
    assert_eq!(search_err(status(403, None)), ProviderError::Forbidden);
    assert_eq!(
        search_err(status(429, Some(9))),
        ProviderError::RateLimited {
            retry_after_secs: Some(9)
        }
    );
    let unprocessable = HttpResponse {
        status: 422,
        body: ERROR_422.as_bytes().to_vec(),
        retry_after_secs: None,
    };
    assert_eq!(
        search_err(unprocessable),
        ProviderError::Http { status: 422 }
    );
}

#[test]
fn malformed_bodies_are_reported() {
    assert!(matches!(
        search_err(HttpResponse::ok(b"<html>".to_vec())),
        ProviderError::Malformed { .. }
    ));
    assert!(matches!(
        search_err(HttpResponse::ok(br#"{"items": []}"#.to_vec())),
        ProviderError::Malformed { .. }
    ));
}

#[test]
fn other_kinds_and_cancellation_make_no_request() {
    let (env, http, commands) = setup(FixtureHttp::new());
    let provider = McpOfficial::new();
    let mut req = request("filesystem", 5);
    req.kinds = BTreeSet::from([ResourceKind::Skill, ResourceKind::Prompt]);
    assert!(
        provider
            .search(&req, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );

    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        provider
            .search(&request("filesystem", 5), &env, &cancel)
            .unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(http.requests().is_empty());
    assert!(commands.calls().is_empty());
}

#[test]
fn auth_status_is_local_and_not_required() {
    let (env, http, commands) = setup(FixtureHttp::new());
    let provider = McpOfficial::new();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert!(http.requests().is_empty() && commands.calls().is_empty());
    assert_eq!(
        provider.connect_intent(),
        perkolator_marketplace::AuthIntent::NoAuthRequired
    );
}

#[test]
fn detail_fetches_the_listed_version_without_tools() {
    let item = {
        let (env, _, _) = setup(FixtureHttp::new().route_json(API_BASE, PAGE1));
        McpOfficial::new()
            .search(&request("filesystem", 3), &env, &CancelToken::new())
            .unwrap()
            .items
            .remove(1)
    };
    let (env, http, _) = setup(FixtureHttp::new().route_json(API_BASE, DETAIL));
    let detail = McpOfficial::new()
        .detail(&item, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(
        http.requests(),
        [format!(
            "{API_BASE}/servers/io.github.Digital-Defiance%2Fmcp-filesystem/versions/0.1.9"
        )]
    );
    assert_eq!(
        detail.item.upstream_id,
        "io.github.Digital-Defiance/mcp-filesystem"
    );
    assert!(
        detail.tools.is_empty(),
        "the registry publishes no tool schemas"
    );
    assert!(detail.readme_markdown.is_none() && detail.files.is_empty());
    let manifest = detail.manifest_json.unwrap();
    assert!(manifest.contains("@ai-capabilities-suite/mcp-filesystem"));
    assert!(manifest.contains("WORKSPACE_ROOT"));
}

#[test]
fn literal_secrets_never_reach_items_or_previews() {
    let list = format!(r#"{{"servers":[{LITERAL_SECRETS}],"metadata":{{"count":1}}}}"#);
    let item = first_item(&list);
    let item_json = serde_json::to_string(&item).unwrap();

    let (env, _, _) = setup(FixtureHttp::new().route_json(API_BASE, LITERAL_SECRETS));
    let detail = McpOfficial::new()
        .detail(&item, &env, &CancelToken::new())
        .unwrap();
    let manifest = detail.manifest_json.clone().unwrap();
    let detail_json = serde_json::to_string(&detail).unwrap();
    for secret in FAKE_SECRETS {
        assert!(!item_json.contains(secret), "{secret} in item");
        assert!(!detail_json.contains(secret), "{secret} in detail");
    }
    assert!(manifest.contains("<redacted>"));
    assert!(manifest.contains("Key {key}"), "templates stay readable");
    assert!(manifest.contains("\"info\""), "non-secret defaults stay");
}

#[test]
fn placeholder_header_templates_are_kept() {
    let item = first_item(&format!(
        r#"{{"servers":[{DETAIL_SECRET_HEADER}],"metadata":{{"count":1}}}}"#
    ));
    let (env, _, _) = setup(FixtureHttp::new().route_json(API_BASE, DETAIL_SECRET_HEADER));
    let manifest = McpOfficial::new()
        .detail(&item, &env, &CancelToken::new())
        .unwrap()
        .manifest_json
        .unwrap();
    assert!(manifest.contains("Bearer {apify_token}"));
}

#[test]
fn detail_rejects_items_from_other_sources() {
    let mut item = first_item(PAGE1);
    item.source = perkolator_core::ProviderId::new("glama").unwrap();
    let (env, http, _) = setup(FixtureHttp::new());
    assert!(matches!(
        McpOfficial::new().detail(&item, &env, &CancelToken::new()),
        Err(ProviderError::Unsupported { .. })
    ));
    assert!(http.requests().is_empty());
}

#[test]
#[ignore = "live: queries registry.modelcontextprotocol.io anonymously"]
fn live_search_paginates_and_fetches_detail() {
    let env = ProviderEnv::system();
    let provider = McpOfficial::new();
    let cancel = CancelToken::new();
    let page = provider
        .search(&request("filesystem", 5), &env, &cancel)
        .expect("live search");
    assert!(!page.items.is_empty(), "no results for `filesystem`");
    assert!(
        page.items
            .iter()
            .all(|item| item.source.as_str() == "mcp-official")
    );
    let cursor = page
        .next_cursor
        .clone()
        .expect("more than 5 filesystem servers");

    let mut next = request("filesystem", 5);
    next.cursor = Some(cursor);
    let second = provider.search(&next, &env, &cancel).expect("live page 2");
    assert!(
        second.items.iter().all(|item| !page
            .items
            .iter()
            .any(|seen| seen.upstream_id == item.upstream_id)),
        "page 2 repeats page 1"
    );

    let detail = provider
        .detail(&page.items[0], &env, &cancel)
        .expect("live detail");
    assert_eq!(detail.item.upstream_id, page.items[0].upstream_id);
    assert!(detail.manifest_json.is_some());
    eprintln!(
        "live mcp-official: page1={} page2={} first={}",
        page.items.len(),
        second.items.len(),
        page.items[0].upstream_id
    );
}
