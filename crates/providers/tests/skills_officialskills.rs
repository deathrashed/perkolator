use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{
    FakeCommands, FixtureHttp, MemoryCredentials, env, offline_env,
};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, HttpResponse, MarketplaceProvider, ProviderEnv, ProviderError,
    RemoteItem, SearchRequest,
};
use perkolator_providers::skills::OfficialSkills;
use perkolator_providers::skills::officialskills::SITEMAP_URL;

const SITEMAP: &str = include_str!("fixtures/officialskills/sitemap.xml");
const README: &str = include_str!("fixtures/voltagent-skills/README.md");
const README_URL: &str =
    "https://raw.githubusercontent.com/VoltAgent/awesome-agent-skills/HEAD/README.md";

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

fn status(code: u16) -> HttpResponse {
    HttpResponse {
        status: code,
        body: Vec::new(),
        retry_after_secs: None,
    }
}

fn search_all(provider: &OfficialSkills, env: &ProviderEnv, query: &str) -> Vec<RemoteItem> {
    let mut request = SearchRequest::new(query);
    request.page_size = 1000;
    provider
        .search(&request, env, &CancelToken::new())
        .unwrap()
        .items
}

#[test]
fn lists_sitemap_skill_pages_enriched_from_the_index() {
    let (env, http) = fixture_env(
        FixtureHttp::new()
            .route_json(SITEMAP_URL, SITEMAP)
            .route_json(README_URL, README),
    );
    let provider = OfficialSkills::new();
    let items = search_all(&provider, &env, "");
    // 20 skill pages; owner pages, /about and /terms are skipped.
    assert_eq!(items.len(), 20);
    assert!(
        items
            .iter()
            .all(|item| item.source.as_str() == "officialskills"
                && item.kind == ResourceKind::Skill
                && item.actions == Default::default()
                && item.repo_url.is_none())
    );

    let pdf = items
        .iter()
        .find(|item| item.upstream_id == "anthropics/pdf")
        .unwrap();
    assert_eq!(pdf.title, "anthropics/pdf");
    assert_eq!(pdf.summary, "Extract text, create PDFs, and handle forms");
    assert_eq!(pdf.categories, ["Official Claude Skills"]);
    assert_eq!(pdf.author.as_deref(), Some("anthropics"));
    assert_eq!(
        pdf.canonical_url.as_deref(),
        Some("https://officialskills.sh/anthropics/skills/pdf")
    );

    let bare = items
        .iter()
        .find(|item| item.upstream_id == "cloudflare/web-perf")
        .unwrap();
    assert_eq!(bare.title, "cloudflare/web-perf");
    assert_eq!(bare.summary, "", "no description is invented");
    assert!(bare.categories.is_empty());

    assert_eq!(search_all(&provider, &env, "powerpoint").len(), 1);
    assert_eq!(http.requests().len(), 2, "sitemap + README fetched once");
}

#[test]
fn readme_failure_degrades_to_sitemap_only() {
    let (env, _) = fixture_env(
        FixtureHttp::new()
            .route_json(SITEMAP_URL, SITEMAP)
            .route(README_URL, status(503)),
    );
    let items = search_all(&OfficialSkills::new(), &env, "pdf");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].summary, "");
}

#[test]
fn sitemap_errors_map_to_typed_errors() {
    for (code, expected) in [
        (401, ProviderError::NeedsAuth),
        (403, ProviderError::Forbidden),
        (
            429,
            ProviderError::RateLimited {
                retry_after_secs: None,
            },
        ),
    ] {
        let (env, http) = fixture_env(FixtureHttp::new().route(SITEMAP_URL, status(code)));
        assert_eq!(
            OfficialSkills::new()
                .search(&SearchRequest::new(""), &env, &CancelToken::new())
                .unwrap_err(),
            expected
        );
        assert_eq!(http.requests().len(), 1, "README is not fetched");
    }
}

#[test]
fn unrecognised_sitemap_is_malformed() {
    let (env, _) = fixture_env(FixtureHttp::new().route_json(SITEMAP_URL, "{\"json\":true}"));
    assert!(matches!(
        OfficialSkills::new().search(&SearchRequest::new(""), &env, &CancelToken::new()),
        Err(ProviderError::Malformed { .. })
    ));
}

#[test]
fn other_kinds_and_auth_status_make_no_requests() {
    let (env, http, _) = offline_env();
    let provider = OfficialSkills::new();
    let mut request = SearchRequest::new("pdf");
    request.kinds = BTreeSet::from([ResourceKind::McpServer, ResourceKind::Plugin]);
    assert!(
        provider
            .search(&request, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert_eq!(provider.connect_intent(), AuthIntent::NoAuthRequired);
    assert!(!provider.descriptor().capabilities.detail);
    assert!(http.requests().is_empty());
}

#[test]
fn cancellation_stops_before_any_request() {
    let (env, http, _) = offline_env();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        OfficialSkills::new()
            .search(&SearchRequest::new(""), &env, &cancel)
            .unwrap_err(),
        ProviderError::Cancelled
    );
    assert!(http.requests().is_empty());
}

#[test]
#[ignore = "live: fetches officialskills.sh/sitemap.xml and the VoltAgent README"]
fn live_directory_search() {
    let page = OfficialSkills::new()
        .search(
            &SearchRequest::new("pdf"),
            &ProviderEnv::system(),
            &CancelToken::new(),
        )
        .expect("live officialskills directory");
    assert!(!page.items.is_empty());
    let described = page
        .items
        .iter()
        .filter(|item| !item.summary.is_empty())
        .count();
    eprintln!(
        "officialskills live: {} matches for `pdf`, {described} with descriptions",
        page.items.len()
    );
}
