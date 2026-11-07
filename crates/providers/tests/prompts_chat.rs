//! prompts-chat adapter against trimmed real responses from
//! `GET https://prompts.chat/api/prompts` (captured 2026-11-07).

use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{
    FakeCommands, FixtureHttp, MemoryCredentials, env, offline_env,
};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, HttpResponse, MarketplaceProvider, ProviderEnv, ProviderError,
    SearchRequest,
};
use perkolator_providers::prompts::PromptsChat;

const SEARCH_URL: &str = "https://prompts.chat/api/prompts?page=1&perPage=25&q=code+review";
const BOTS_URL: &str = "https://prompts.chat/api/prompts?page=1&perPage=25&q=code+review&type=BOT";
const BOT_ID: &str = "cseed7c35d61e62dca980e3a94f8b";

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/prompts-chat/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn fixture_env(http: FixtureHttp) -> (ProviderEnv, Arc<FixtureHttp>) {
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

fn listing_env() -> (ProviderEnv, Arc<FixtureHttp>) {
    fixture_env(
        FixtureHttp::new()
            .route_json(SEARCH_URL, &fixture("search-prompts.json"))
            .route_json(BOTS_URL, &fixture("search-bots.json")),
    )
}

fn request(kinds: &[ResourceKind]) -> SearchRequest {
    let mut request = SearchRequest::new("code review");
    request.kinds = kinds.iter().copied().collect::<BTreeSet<_>>();
    request
}

#[test]
fn descriptor_is_public_rest_for_prompts_and_agents() {
    let provider = PromptsChat::new();
    let descriptor = provider.descriptor();
    assert_eq!(descriptor.id.as_str(), "prompts-chat");
    assert_eq!(
        descriptor.kinds,
        vec![ResourceKind::Prompt, ResourceKind::Agent]
    );
    assert_eq!(provider.connect_intent(), AuthIntent::NoAuthRequired);
    assert!(!descriptor.capabilities.install);
}

#[test]
fn auth_status_makes_no_request() {
    let (env, http, commands) = offline_env();
    assert_eq!(
        PromptsChat::new().auth_status(&env),
        AuthStatus::NotRequired
    );
    assert!(http.requests().is_empty());
    assert!(commands.calls().is_empty());
}

#[test]
fn search_maps_prompts_and_bots_and_skips_skills() {
    let (env, http) = listing_env();
    let page = PromptsChat::new()
        .search(&request(&[]), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), vec![SEARCH_URL, BOTS_URL]);

    let titles: Vec<(&str, ResourceKind)> = page
        .items
        .iter()
        .map(|item| (item.title.as_str(), item.kind))
        .collect();
    assert_eq!(
        titles,
        vec![
            ("dbt Analytics Project taste", ResourceKind::Prompt),
            ("AI-Generated PLC Code Reviewer", ResourceKind::Prompt),
            ("Tutorial Drafter", ResourceKind::Agent),
            ("Concept Researcher", ResourceKind::Agent),
        ],
        "the SKILL-type entry must be skipped, not relabelled"
    );
    assert_eq!(page.next_cursor.as_deref(), Some("2"));
    assert_eq!(page.total, None, "untyped totals include skipped skills");

    let prompt = &page.items[1];
    assert_eq!(prompt.source.as_str(), "prompts-chat");
    assert_eq!(prompt.upstream_id, "cmvsgb1fm000pjq08qhs7vixm");
    assert_eq!(
        prompt.canonical_url.as_deref(),
        Some(
            "https://prompts.chat/prompts/cmvsgb1fm000pjq08qhs7vixm_ai-generated-plc-code-reviewer"
        )
    );
    assert_eq!(prompt.author.as_deref(), Some("markus864"));
    assert_eq!(prompt.categories, vec!["Coding"]);
    assert!(prompt.fetched_at_unix > 0);
    assert!(prompt.actions.view_source && !prompt.actions.install);

    let taste = &page.items[0];
    assert!(taste.categories.contains(&"SQL".to_owned()));

    let bot = &page.items[3];
    assert_eq!(
        bot.canonical_url.as_deref(),
        Some(&*format!(
            "https://prompts.chat/bots/{BOT_ID}_concept-researcher"
        ))
    );
    assert_eq!(bot.version, None);
    assert_eq!(bot.package_ref, None);
}

#[test]
fn agent_only_search_requests_bots_and_reports_their_total() {
    let (env, http) = listing_env();
    let page = PromptsChat::new()
        .search(&request(&[ResourceKind::Agent]), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), vec![BOTS_URL]);
    assert_eq!(page.total, Some(26));
    assert!(
        page.items
            .iter()
            .all(|item| item.kind == ResourceKind::Agent)
    );
}

#[test]
fn prompt_only_search_skips_the_bot_listing() {
    let (env, http) = listing_env();
    let page = PromptsChat::new()
        .search(&request(&[ResourceKind::Prompt]), &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), vec![SEARCH_URL]);
    assert_eq!(page.items.len(), 2);
}

#[test]
fn unsupported_kinds_make_no_request() {
    let (env, http, _) = offline_env();
    let page = PromptsChat::new()
        .search(
            &request(&[ResourceKind::Skill, ResourceKind::McpServer]),
            &env,
            &CancelToken::new(),
        )
        .unwrap();
    assert!(page.items.is_empty());
    assert!(http.requests().is_empty());
}

#[test]
fn cursor_selects_the_page_and_bad_cursors_are_rejected() {
    let url = "https://prompts.chat/api/prompts?page=3&perPage=25&q=code+review&type=BOT";
    let (env, http) = fixture_env(FixtureHttp::new().route_json(url, &fixture("search-bots.json")));
    let mut paged = request(&[ResourceKind::Agent]);
    paged.cursor = Some("3".into());
    let page = PromptsChat::new()
        .search(&paged, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(http.requests(), vec![url]);
    assert_eq!(page.next_cursor.as_deref(), Some("4"));

    paged.cursor = Some("0".into());
    assert!(matches!(
        PromptsChat::new().search(&paged, &env, &CancelToken::new()),
        Err(ProviderError::Malformed { .. })
    ));
}

#[test]
fn malformed_responses_are_typed() {
    for body in [
        "not json",
        r#"{"prompts": 5}"#,
        r#"{"prompts":[{"id":1}],"total":1,"page":1,"perPage":1,"totalPages":1}"#,
    ] {
        let (env, _) = fixture_env(FixtureHttp::new().route_json(SEARCH_URL, body));
        let error = PromptsChat::new()
            .search(&request(&[ResourceKind::Prompt]), &env, &CancelToken::new())
            .unwrap_err();
        assert!(
            matches!(error, ProviderError::Malformed { .. }),
            "{body}: {error:?}"
        );
    }
}

#[test]
fn http_errors_map_to_typed_errors() {
    let cases = [
        (401, ProviderError::NeedsAuth),
        (403, ProviderError::Forbidden),
        (
            429,
            ProviderError::RateLimited {
                retry_after_secs: Some(30),
            },
        ),
        (503, ProviderError::Http { status: 503 }),
    ];
    for (status, expected) in cases {
        let response = HttpResponse {
            status,
            body: Vec::new(),
            retry_after_secs: Some(30),
        };
        let (env, _) = fixture_env(FixtureHttp::new().route(SEARCH_URL, response));
        let error = PromptsChat::new()
            .search(&request(&[ResourceKind::Prompt]), &env, &CancelToken::new())
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[test]
fn cancelled_search_makes_no_request() {
    let (env, http) = listing_env();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        PromptsChat::new().search(&request(&[]), &env, &cancel),
        Err(ProviderError::Cancelled)
    );
    assert!(http.requests().is_empty());
}

#[test]
fn bot_detail_returns_instructions_and_definition() {
    let detail_url = format!("https://prompts.chat/api/prompts/{BOT_ID}");
    let (env, http) = fixture_env(
        FixtureHttp::new()
            .route_json(BOTS_URL, &fixture("search-bots.json"))
            .route_json(&detail_url, &fixture("detail-bot.json")),
    );
    let provider = PromptsChat::new();
    let page = provider
        .search(&request(&[ResourceKind::Agent]), &env, &CancelToken::new())
        .unwrap();
    let item = page
        .items
        .iter()
        .find(|item| item.upstream_id == BOT_ID)
        .unwrap();
    let detail = provider.detail(item, &env, &CancelToken::new()).unwrap();
    assert_eq!(http.requests().last().unwrap(), &detail_url);
    assert_eq!(detail.item.kind, ResourceKind::Agent);
    assert!(
        detail
            .readme_markdown
            .as_deref()
            .unwrap()
            .starts_with("You are Concept Researcher.")
    );
    let manifest: serde_json::Value =
        serde_json::from_str(detail.manifest_json.as_deref().unwrap()).unwrap();
    assert!(manifest["botPlatforms"].as_array().unwrap().len() > 1);
    assert!(manifest["botConfig"].is_object());
    assert!(detail.files.is_empty() && detail.tools.is_empty());
}

#[test]
fn detail_refuses_unsafe_ids_without_a_request() {
    let (env, http) = listing_env();
    let provider = PromptsChat::new();
    let mut item = provider
        .search(&request(&[ResourceKind::Agent]), &env, &CancelToken::new())
        .unwrap()
        .items
        .remove(0);
    let before = http.requests().len();
    item.upstream_id = "../users/me".into();
    assert!(matches!(
        provider.detail(&item, &env, &CancelToken::new()),
        Err(ProviderError::Malformed { .. })
    ));
    assert_eq!(http.requests().len(), before);
}

#[test]
#[ignore = "live network: prompts.chat public API"]
fn live_public_search() {
    let provider = PromptsChat::new();
    let env = ProviderEnv::system();
    let mut live = SearchRequest::new("code review");
    live.page_size = 5;
    let page = provider.search(&live, &env, &CancelToken::new()).unwrap();
    assert!(!page.items.is_empty());
    assert!(
        page.items
            .iter()
            .any(|item| item.kind == ResourceKind::Prompt)
    );
    assert!(
        page.items
            .iter()
            .any(|item| item.kind == ResourceKind::Agent)
    );
    let agent = page
        .items
        .iter()
        .find(|item| item.kind == ResourceKind::Agent)
        .unwrap();
    let detail = provider.detail(agent, &env, &CancelToken::new()).unwrap();
    assert!(detail.readme_markdown.is_some());
    eprintln!(
        "prompts-chat live: {} items, next={:?}, first={:?}",
        page.items.len(),
        page.next_cursor,
        page.items.first().map(|item| &item.title)
    );
}
