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
use perkolator_providers::skills::SkillsMp;
use perkolator_providers::skills::skillsmp::SEARCH_URL;

const SEARCH: &str = include_str!("fixtures/skillsmp/search_pdf.json");
const INVALID_KEY: &str = include_str!("fixtures/skillsmp/error_invalid_key.json");

fn with_http(http: FixtureHttp, credentials: MemoryCredentials) -> (ProviderEnv, Arc<FixtureHttp>) {
    let http = Arc::new(http);
    (
        env(http.clone(), Arc::new(FakeCommands::new()), credentials),
        http,
    )
}

fn status(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.as_bytes().to_vec(),
        retry_after_secs: Some(60),
    }
}

#[test]
fn parses_real_anonymous_search_response() {
    let (env, http) = with_http(
        FixtureHttp::new().route_json(SEARCH_URL, SEARCH),
        MemoryCredentials::default(),
    );
    let mut request = SearchRequest::new("pdf");
    request.page_size = 3;
    let page = SkillsMp::new()
        .search(&request, &env, &CancelToken::new())
        .unwrap();

    assert_eq!(page.items.len(), 3);
    let first = &page.items[0];
    assert_eq!(first.source.as_str(), "skillsmp");
    assert_eq!(
        first.upstream_id,
        "openclaw-openclaw-skills-nano-pdf-skill-md"
    );
    assert_eq!(first.kind, ResourceKind::Skill);
    assert_eq!(first.title, "nano-pdf");
    assert_eq!(first.author.as_deref(), Some("openclaw"));
    assert_eq!(
        first.repo_url.as_deref(),
        Some("https://github.com/openclaw/openclaw")
    );
    assert_eq!(first.resource_path.as_deref(), Some("skills/nano-pdf"));
    assert_eq!(
        first.canonical_url.as_deref(),
        Some("https://skillsmp.com/creators/openclaw/openclaw/skills-nano-pdf")
    );
    assert!(first.actions.view_source && !first.actions.install);
    assert!(first.version.is_none());
    assert!(first.fetched_at_unix > 0);
    // `totalIsExact: false` → the approximate total is not reported.
    assert_eq!(page.total, None);
    assert_eq!(page.next_cursor.as_deref(), Some("2"));

    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0], format!("{SEARCH_URL}?q=pdf&page=1&limit=3"));
}

#[test]
fn category_is_sent_only_with_an_api_key() {
    let mut request = SearchRequest::new("seo tools");
    request.category = Some("data-ai".into());

    let (anonymous, http) = with_http(
        FixtureHttp::new().route_json(SEARCH_URL, SEARCH),
        MemoryCredentials::default(),
    );
    SkillsMp::new()
        .search(&request, &anonymous, &CancelToken::new())
        .unwrap();
    assert!(!http.requests()[0].contains("category="));
    assert!(http.requests()[0].contains("q=seo+tools"));

    let (keyed, http) = with_http(
        FixtureHttp::new().route_json(SEARCH_URL, SEARCH),
        MemoryCredentials::default().with("skillsmp", "sk_live_test"),
    );
    SkillsMp::new()
        .search(&request, &keyed, &CancelToken::new())
        .unwrap();
    let url = &http.requests()[0];
    assert!(url.contains("&category=data-ai"));
    assert!(!url.contains("sk_live_test"), "key must not leak into URLs");
}

#[test]
fn cursor_selects_page() {
    let (env, http) = with_http(
        FixtureHttp::new().route_json(SEARCH_URL, SEARCH),
        MemoryCredentials::default(),
    );
    let mut request = SearchRequest::new("pdf");
    request.cursor = Some("2".into());
    SkillsMp::new()
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert!(http.requests()[0].contains("&page=2&"));

    request.cursor = Some("zero".into());
    assert!(matches!(
        SkillsMp::new().search(&request, &env, &CancelToken::new()),
        Err(ProviderError::Malformed { .. })
    ));
}

#[test]
fn malformed_json_is_reported() {
    let (env, _) = with_http(
        FixtureHttp::new().route_json(SEARCH_URL, "{not json"),
        MemoryCredentials::default(),
    );
    let error = SkillsMp::new()
        .search(&SearchRequest::new("pdf"), &env, &CancelToken::new())
        .unwrap_err();
    assert!(matches!(error, ProviderError::Malformed { .. }));

    let (env, _) = with_http(
        FixtureHttp::new().route_json(SEARCH_URL, r#"{"success":true,"data":{}}"#),
        MemoryCredentials::default(),
    );
    assert!(matches!(
        SkillsMp::new().search(&SearchRequest::new("pdf"), &env, &CancelToken::new()),
        Err(ProviderError::Malformed { .. })
    ));
}

#[test]
fn http_errors_map_to_typed_errors() {
    for (code, expected) in [
        (401, ProviderError::NeedsAuth),
        (403, ProviderError::Forbidden),
        (
            429,
            ProviderError::RateLimited {
                retry_after_secs: Some(60),
            },
        ),
        (503, ProviderError::Http { status: 503 }),
    ] {
        let (env, _) = with_http(
            FixtureHttp::new().route(SEARCH_URL, status(code, INVALID_KEY)),
            MemoryCredentials::default().with("skillsmp", "sk_live_bad"),
        );
        let error = SkillsMp::new()
            .search(&SearchRequest::new("pdf"), &env, &CancelToken::new())
            .unwrap_err();
        assert_eq!(error, expected, "status {code}");
    }
}

#[test]
fn skips_requests_for_other_kinds_and_unsearchable_queries() {
    let (env, http, _) = offline_env();
    let provider = SkillsMp::new();
    let mut request = SearchRequest::new("pdf");
    request.kinds = BTreeSet::from([ResourceKind::McpServer]);
    assert!(
        provider
            .search(&request, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    for query in ["", "   ", "*", "--"] {
        assert!(
            provider
                .search(&SearchRequest::new(query), &env, &CancelToken::new())
                .unwrap()
                .items
                .is_empty()
        );
    }
    assert!(http.requests().is_empty());
}

#[test]
fn auth_status_is_local_and_reflects_optional_key() {
    let (env, http, _) = offline_env();
    let provider = SkillsMp::new();
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    let (keyed, keyed_http) = with_http(
        FixtureHttp::new(),
        MemoryCredentials::default().with("skillsmp", "sk_live_x"),
    );
    assert_eq!(
        provider.auth_status(&keyed),
        AuthStatus::Ready { identity: None }
    );
    assert!(http.requests().is_empty() && keyed_http.requests().is_empty());
    assert_eq!(
        provider.connect_intent(),
        AuthIntent::PromptSecret {
            slot: "skillsmp".into(),
            docs_url: "https://skillsmp.com/docs/api".into(),
        }
    );
}

#[test]
fn cancelled_search_makes_no_request() {
    let (env, http, _) = offline_env();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        SkillsMp::new()
            .search(&SearchRequest::new("pdf"), &env, &cancel)
            .unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(http.requests().is_empty());
}

#[test]
fn detail_reads_skill_md_from_github() {
    let (env, _) = with_http(
        FixtureHttp::new().route_json(SEARCH_URL, SEARCH),
        MemoryCredentials::default(),
    );
    let provider = SkillsMp::new();
    let item = provider
        .search(&SearchRequest::new("pdf"), &env, &CancelToken::new())
        .unwrap()
        .items
        .remove(0);

    let (env, http) = with_http(
        FixtureHttp::new().route_json(
            "https://raw.githubusercontent.com/openclaw/openclaw/HEAD/skills/nano-pdf/SKILL.md",
            "---\nname: nano-pdf\n---\n",
        ),
        MemoryCredentials::default(),
    );
    let detail = provider.detail(&item, &env, &CancelToken::new()).unwrap();
    assert_eq!(
        detail.readme_markdown.as_deref(),
        Some("---\nname: nano-pdf\n---\n")
    );
    assert_eq!(detail.files[0].path, "skills/nano-pdf/SKILL.md");
    assert_eq!(http.requests().len(), 1);
}

#[test]
#[ignore = "live: anonymous SkillsMP search (uses 1 of 50 daily anonymous searches)"]
fn live_anonymous_search() {
    let page = SkillsMp::new()
        .search(
            &SearchRequest::new("terraform"),
            &ProviderEnv::system(),
            &CancelToken::new(),
        )
        .expect("live SkillsMP search");
    assert!(!page.items.is_empty());
    eprintln!(
        "skillsmp live: {} items, first = {}",
        page.items.len(),
        page.items[0].title
    );
}
