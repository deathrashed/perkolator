use std::collections::BTreeSet;
use std::sync::Arc;

use perkolator_core::{CancelToken, ResourceKind};
use perkolator_marketplace::testing::{
    FakeCommands, FixtureHttp, MemoryCredentials, env, offline_env,
};
use perkolator_marketplace::{
    AuthIntent, AuthStatus, HttpRequest, HttpResponse, MarketplaceProvider, ProviderEnv,
    ProviderError, SearchRequest,
};
use perkolator_providers::skills::SkillsSh;
use perkolator_providers::skills::skillssh::API;

const SEARCH: &str = include_str!("fixtures/skillssh/search.json");
const LEADERBOARD: &str = include_str!("fixtures/skillssh/leaderboard.json");
const DETAIL: &str = include_str!("fixtures/skillssh/detail.json");
const UNAUTHORISED: &str = include_str!("fixtures/skillssh/error_401.json");

fn keyed(http: FixtureHttp) -> (ProviderEnv, Arc<FixtureHttp>) {
    let http = Arc::new(http);
    (
        env(
            http.clone(),
            Arc::new(FakeCommands::new()),
            MemoryCredentials::default().with("skillssh", "oidc-token"),
        ),
        http,
    )
}

#[test]
fn parses_documented_search_shape_and_hides_duplicates() {
    let (env, http) = keyed(FixtureHttp::new().route_json(format!("{API}/search"), SEARCH));
    let page = SkillsSh::new()
        .search(
            &SearchRequest::new("react native"),
            &env,
            &CancelToken::new(),
        )
        .unwrap();

    assert_eq!(page.items.len(), 2, "isDuplicate entries are dropped");
    let github = &page.items[0];
    assert_eq!(github.source.as_str(), "skillssh");
    assert_eq!(github.upstream_id, "expo/skills/react-native");
    assert_eq!(github.title, "React Native");
    assert_eq!(github.summary, "", "search results carry no description");
    assert_eq!(
        github.repo_url.as_deref(),
        Some("https://github.com/expo/skills")
    );
    assert_eq!(github.author.as_deref(), Some("expo"));
    assert_eq!(
        github.canonical_url.as_deref(),
        Some("https://skills.sh/expo/skills/react-native")
    );
    assert!(github.actions.fetch_files && !github.actions.install);

    let well_known = &page.items[1];
    assert_eq!(well_known.upstream_id, "mintlify.com/mintlify");
    assert!(well_known.repo_url.is_none() && well_known.author.is_none());

    assert_eq!(page.total, None);
    assert_eq!(page.next_cursor, None);
    assert_eq!(
        http.requests(),
        vec![format!("{API}/search?q=react+native&limit=25")]
    );
}

#[test]
fn short_queries_page_through_the_leaderboard() {
    let (env, http) = keyed(FixtureHttp::new().route_json(format!("{API}?"), LEADERBOARD));
    let mut request = SearchRequest::new("");
    request.page_size = 10;
    let page = SkillsSh::new()
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(page.items[0].upstream_id, "vercel-labs/skills/find-skills");
    assert_eq!(page.total, Some(8420));
    assert_eq!(page.next_cursor.as_deref(), Some("1"));
    assert_eq!(http.requests(), vec![format!("{API}?page=0&per_page=10")]);
}

#[test]
fn search_without_token_needs_auth_and_makes_no_request() {
    let (env, http, _) = offline_env();
    let provider = SkillsSh::new();
    assert_eq!(
        provider
            .search(&SearchRequest::new("pdf"), &env, &CancelToken::new())
            .unwrap_err(),
        ProviderError::NeedsAuth
    );
    assert_eq!(provider.auth_status(&env), AuthStatus::NotConfigured);
    assert!(http.requests().is_empty());
    assert_eq!(
        provider.connect_intent(),
        AuthIntent::PromptSecret {
            slot: "skillssh".into(),
            docs_url: "https://skills.sh/docs/api#authentication".into(),
        }
    );
}

#[test]
fn auth_status_with_token_is_ready_without_requests() {
    let (env, http) = keyed(FixtureHttp::new());
    assert_eq!(
        SkillsSh::new().auth_status(&env),
        AuthStatus::Ready { identity: None }
    );
    assert!(http.requests().is_empty());
}

#[test]
fn http_errors_map_to_typed_errors() {
    for (code, expected) in [
        (401, ProviderError::NeedsAuth),
        (403, ProviderError::Forbidden),
        (
            429,
            ProviderError::RateLimited {
                retry_after_secs: Some(30),
            },
        ),
    ] {
        let (env, _) = keyed(FixtureHttp::new().route(
            format!("{API}/search"),
            HttpResponse {
                status: code,
                body: UNAUTHORISED.as_bytes().to_vec(),
                retry_after_secs: Some(30),
            },
        ));
        assert_eq!(
            SkillsSh::new()
                .search(&SearchRequest::new("pdf"), &env, &CancelToken::new())
                .unwrap_err(),
            expected
        );
    }
}

#[test]
fn malformed_json_is_reported() {
    for body in ["<html>", r#"{"query":"pdf"}"#] {
        let (env, _) = keyed(FixtureHttp::new().route_json(format!("{API}/search"), body));
        assert!(matches!(
            SkillsSh::new().search(&SearchRequest::new("pdf"), &env, &CancelToken::new()),
            Err(ProviderError::Malformed { .. })
        ));
    }
}

#[test]
fn other_kinds_return_empty_without_requests() {
    let (env, http) = keyed(FixtureHttp::new());
    let mut request = SearchRequest::new("pdf");
    request.kinds = BTreeSet::from([ResourceKind::Agent, ResourceKind::Prompt]);
    assert!(
        SkillsSh::new()
            .search(&request, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    assert!(http.requests().is_empty());
}

#[test]
fn detail_returns_skill_md_and_file_list() {
    let (env, _) = keyed(FixtureHttp::new().route_json(format!("{API}/search"), SEARCH));
    let provider = SkillsSh::new();
    let mut item = provider
        .search(&SearchRequest::new("react"), &env, &CancelToken::new())
        .unwrap()
        .items
        .remove(0);
    item.upstream_id = "vercel-labs/skills/find-skills".into();

    let (env, http) = keyed(FixtureHttp::new().route_json(format!("{API}/vercel-labs"), DETAIL));
    let detail = provider.detail(&item, &env, &CancelToken::new()).unwrap();
    assert!(
        detail
            .readme_markdown
            .as_deref()
            .unwrap()
            .contains("# Find skills")
    );
    let paths: Vec<&str> = detail.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["SKILL.md", "examples/app-router.ts"]);
    assert_eq!(
        http.requests(),
        vec![format!("{API}/vercel-labs/skills/find-skills")]
    );

    item.upstream_id = "../../etc".into();
    assert!(matches!(
        provider.detail(&item, &env, &CancelToken::new()),
        Err(ProviderError::Unsupported { .. })
    ));
}

#[test]
#[ignore = "live: confirms skills.sh still rejects anonymous search"]
fn live_anonymous_search_is_rejected() {
    // No token is configured, so the adapter must refuse before any request;
    // the raw endpoint is probed through the HTTP seam to record the status.
    let env = ProviderEnv::system();
    let response = env
        .http
        .get(&HttpRequest::get(format!("{API}/search?q=pdf&limit=1")))
        .expect("reach skills.sh");
    eprintln!("skills.sh anonymous search status: {}", response.status);
    assert_eq!(response.status, 401);
}
