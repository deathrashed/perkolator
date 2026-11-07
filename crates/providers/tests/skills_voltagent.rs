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
use perkolator_providers::skills::VoltAgentSkills;

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

fn readme_env() -> (ProviderEnv, Arc<FixtureHttp>) {
    fixture_env(FixtureHttp::new().route_json(README_URL, README))
}

fn all(provider: &VoltAgentSkills, env: &ProviderEnv, query: &str) -> Vec<RemoteItem> {
    let mut request = SearchRequest::new(query);
    request.page_size = 1000;
    provider
        .search(&request, env, &CancelToken::new())
        .unwrap()
        .items
}

fn find<'a>(items: &'a [RemoteItem], id: &str) -> &'a RemoteItem {
    items
        .iter()
        .find(|item| item.upstream_id == id)
        .unwrap_or_else(|| panic!("{id} missing"))
}

#[test]
fn parses_index_entries_with_provenance() {
    let (env, _) = readme_env();
    let items = all(&VoltAgentSkills::new(), &env, "");
    assert_eq!(items.len(), 28);
    assert!(
        items
            .iter()
            .all(|item| item.source.as_str() == "voltagent-skills"
                && item.kind == ResourceKind::Skill
                && !item.actions.install
                && item.version.is_none())
    );

    let docx = find(&items, "anthropics/docx");
    assert_eq!(docx.summary, "Create, edit, and analyze Word documents");
    assert_eq!(
        docx.canonical_url.as_deref(),
        Some("https://officialskills.sh/anthropics/skills/docx")
    );
    assert_eq!(docx.repo_url, None);
    assert_eq!(docx.author.as_deref(), Some("anthropics"));
    assert_eq!(docx.categories, ["Official Claude Skills"]);
    assert!(!docx.actions.view_source);

    let tree = find(&items, "testmu-ai/api-skill");
    assert_eq!(
        tree.repo_url.as_deref(),
        Some("https://github.com/LambdaTest/agent-skills")
    );
    assert_eq!(tree.resource_path.as_deref(), Some("api-skill"));
    assert!(
        !tree.actions.view_source,
        "directory links are not verified"
    );

    let blob = find(&items, "obra/brainstorming");
    assert_eq!(blob.resource_path.as_deref(), Some("skills/brainstorming"));
    assert!(blob.actions.view_source, "links to a concrete SKILL.md");

    let colon = find(&items, "infrasity-labs/dev-gtm-claude-skills");
    assert!(colon.summary.starts_with("GTM-focused skill collection"));
    assert_eq!(colon.resource_path, None);

    let external = find(&items, "redhat/cve-skillpack");
    assert!(external.repo_url.is_none());
}

#[test]
fn query_and_category_filter_locally() {
    let (env, http) = readme_env();
    let provider = VoltAgentSkills::new();
    let whatsapp = all(&provider, &env, "WhatsApp");
    assert_eq!(whatsapp.len(), 3);

    let mut request = SearchRequest::new("");
    request.category = Some("vector databases".into());
    let vector = provider
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert!(
        vector
            .items
            .iter()
            .any(|item| item.upstream_id == "qdrant/skills")
    );
    assert!(
        vector
            .items
            .iter()
            .all(|item| item.categories == ["Vector Databases"])
    );
    assert_eq!(http.requests().len(), 1, "index is fetched once and cached");
}

#[test]
fn paginates_with_offset_cursor() {
    let (env, _) = readme_env();
    let provider = VoltAgentSkills::new();
    let mut request = SearchRequest::new("");
    request.page_size = 10;
    let first = provider
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(first.items.len(), 10);
    assert_eq!(first.total, Some(28));
    assert_eq!(first.next_cursor.as_deref(), Some("10"));

    request.cursor = Some("20".into());
    let last = provider
        .search(&request, &env, &CancelToken::new())
        .unwrap();
    assert_eq!(last.items.len(), 8);
    assert_eq!(last.next_cursor, None);

    request.cursor = Some("x".into());
    assert!(matches!(
        provider.search(&request, &env, &CancelToken::new()),
        Err(ProviderError::Malformed { .. })
    ));
}

#[test]
fn unrecognised_readme_is_malformed() {
    let (env, _) = fixture_env(FixtureHttp::new().route_json(README_URL, "# nothing here\n"));
    assert!(matches!(
        VoltAgentSkills::new().search(&SearchRequest::new(""), &env, &CancelToken::new()),
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
                retry_after_secs: None,
            },
        ),
        (404, ProviderError::Http { status: 404 }),
    ] {
        let (env, _) = fixture_env(FixtureHttp::new().route(
            README_URL,
            HttpResponse {
                status: code,
                body: Vec::new(),
                retry_after_secs: None,
            },
        ));
        assert_eq!(
            VoltAgentSkills::new()
                .search(&SearchRequest::new(""), &env, &CancelToken::new())
                .unwrap_err(),
            expected
        );
    }
}

#[test]
fn other_kinds_and_auth_status_make_no_requests() {
    let (env, http, _) = offline_env();
    let provider = VoltAgentSkills::new();
    let mut request = SearchRequest::new("pdf");
    request.kinds = BTreeSet::from([ResourceKind::Rule]);
    assert!(
        provider
            .search(&request, &env, &CancelToken::new())
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(provider.auth_status(&env), AuthStatus::NotRequired);
    assert_eq!(provider.connect_intent(), AuthIntent::NoAuthRequired);
    assert!(http.requests().is_empty());
}

#[test]
fn detail_fetches_skill_md_or_falls_back_to_readme() {
    let (env, _) = readme_env();
    let provider = VoltAgentSkills::new();
    let items = all(&provider, &env, "");

    let (blob_env, blob_http) = fixture_env(FixtureHttp::new().route_json(
        "https://raw.githubusercontent.com/obra/superpowers/main/skills/brainstorming/SKILL.md",
        "---\nname: brainstorming\n---\n",
    ));
    let detail = provider
        .detail(
            find(&items, "obra/brainstorming"),
            &blob_env,
            &CancelToken::new(),
        )
        .unwrap();
    assert!(detail.readme_markdown.unwrap().contains("brainstorming"));
    assert!(detail.item.actions.fetch_files);
    assert_eq!(
        detail.files[0].url.as_deref(),
        Some("https://github.com/obra/superpowers/blob/main/skills/brainstorming/SKILL.md")
    );
    assert_eq!(blob_http.requests().len(), 1);

    let (repo_env, repo_http) = fixture_env(
        FixtureHttp::new()
            .route(
                "https://raw.githubusercontent.com/qdrant/skills/HEAD/SKILL.md",
                HttpResponse {
                    status: 404,
                    body: Vec::new(),
                    retry_after_secs: None,
                },
            )
            .route_json(
                "https://raw.githubusercontent.com/qdrant/skills/HEAD/README.md",
                "# Qdrant skills\n",
            ),
    );
    let detail = provider
        .detail(
            find(&items, "qdrant/skills"),
            &repo_env,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(detail.files[0].path, "README.md");
    assert!(!detail.item.actions.fetch_files, "a README is not a skill");
    assert_eq!(repo_http.requests().len(), 2);

    let (none_env, none_http, _) = offline_env();
    assert!(matches!(
        provider.detail(
            find(&items, "anthropics/docx"),
            &none_env,
            &CancelToken::new()
        ),
        Err(ProviderError::Unsupported { .. })
    ));
    assert!(none_http.requests().is_empty());
}

#[test]
#[ignore = "live: fetches the public VoltAgent README from raw.githubusercontent.com"]
fn live_index_search() {
    let page = VoltAgentSkills::new()
        .search(
            &SearchRequest::new("pdf"),
            &ProviderEnv::system(),
            &CancelToken::new(),
        )
        .expect("live VoltAgent index");
    assert!(!page.items.is_empty());
    eprintln!(
        "voltagent-skills live: {} matches for `pdf` (total {:?})",
        page.items.len(),
        page.total
    );
}
