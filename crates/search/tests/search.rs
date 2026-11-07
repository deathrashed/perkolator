use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use perkolator_core::{
    CapabilitySet, FileKind, InstallState, LocationRole, Provenance, ProviderId, RelPath,
    ResourceDetails, ResourceFile, ResourceId, ResourceKind, ResourceLocation, ResourceRecord,
    SkillDetails, SourceKind,
};
use perkolator_search::{
    Boosts, MatchTier, Query, ScopeFilter, SearchDoc, SearchHit, SearchIndex, reselect, synthetic,
};

fn id(kind: ResourceKind, key: &str) -> ResourceId {
    ResourceId::new(kind, ProviderId::new("test").unwrap(), key)
}

fn doc(kind: ResourceKind, name: &str) -> SearchDoc {
    SearchDoc {
        id: id(kind, name),
        kind,
        name: name.to_owned(),
        publisher: None,
        tags: Vec::new(),
        summary: String::new(),
        extra: Vec::new(),
        installed: false,
        remote: false,
        source_label: "Library".to_owned(),
    }
}

fn names(index: &SearchIndex, hits: &[SearchHit]) -> Vec<String> {
    hits.iter()
        .map(|hit| index.get(&hit.id).unwrap().name.clone())
        .collect()
}

fn search(index: &SearchIndex, input: &str) -> Vec<String> {
    names(
        index,
        &index.search(&Query::parse(input), &Boosts::default(), 50),
    )
}

#[test]
fn subsequence_and_dropped_letter_typos_match() {
    let index = SearchIndex::from_docs([
        doc(ResourceKind::McpServer, "github"),
        doc(ResourceKind::Skill, "frontend-design"),
        doc(ResourceKind::Rule, "python-style"),
    ]);
    assert_eq!(search(&index, "gthb"), ["github"]);
    assert_eq!(search(&index, "githb"), ["github"]);
    assert_eq!(search(&index, "frntend"), ["frontend-design"]);
    assert_eq!(search(&index, "fe des"), ["frontend-design"]);
    assert!(search(&index, "zzqx").is_empty());
}

#[test]
fn field_weights_order_tiers() {
    let mut tagged = doc(ResourceKind::Skill, "alpha");
    tagged.tags = vec!["review".into()];
    let mut summarized = doc(ResourceKind::Skill, "beta");
    summarized.summary = "Helps review pull requests".into();
    let mut extra = doc(ResourceKind::Skill, "gamma");
    extra.extra = vec!["references/review.md".into()];
    let index = SearchIndex::from_docs([
        extra,
        summarized,
        tagged,
        doc(ResourceKind::Skill, "code-review-helper"),
        doc(ResourceKind::Skill, "reviewer"),
        doc(ResourceKind::Skill, "Review"),
    ]);
    let hits = index.search(&Query::text("review"), &Boosts::default(), 10);
    assert_eq!(
        names(&index, &hits),
        [
            "Review",
            "reviewer",
            "code-review-helper",
            "alpha",
            "beta",
            "gamma"
        ]
    );
    let tiers: Vec<MatchTier> = hits.iter().map(|hit| hit.tier).collect();
    assert_eq!(
        tiers,
        [
            MatchTier::NameExact,
            MatchTier::NamePrefix,
            MatchTier::NameFuzzy,
            MatchTier::Secondary,
            MatchTier::Summary,
            MatchTier::Extra
        ]
    );
}

#[test]
fn name_exact_outranks_summary_even_when_summary_doc_is_boosted() {
    let mut summary_doc = doc(ResourceKind::Prompt, "commit-message");
    summary_doc.summary = "pdf helper".into();
    summary_doc.installed = true;
    let index = SearchIndex::from_docs([summary_doc.clone(), doc(ResourceKind::Skill, "pdf")]);
    let mut boosts = Boosts::default();
    boosts.favorites.insert(summary_doc.id.clone());
    boosts.recent.insert(summary_doc.id.clone(), 0);
    let hits = index.search(&Query::text("pdf"), &boosts, 10);
    assert_eq!(names(&index, &hits), ["pdf", "commit-message"]);
}

#[test]
fn multi_word_terms_may_match_different_fields() {
    let mut mcp = doc(ResourceKind::McpServer, "github");
    mcp.tags = vec!["issues".into()];
    let index = SearchIndex::from_docs([mcp, doc(ResourceKind::McpServer, "gitlab")]);
    assert_eq!(search(&index, "git issues"), ["github"]);
}

#[test]
fn negative_atoms_exclude() {
    let index = SearchIndex::from_docs([
        doc(ResourceKind::Skill, "git-commit"),
        doc(ResourceKind::Skill, "git-review"),
    ]);
    assert_eq!(search(&index, "git !review"), ["git-commit"]);
}

#[test]
fn kind_prefix_filters() {
    let index = SearchIndex::from_docs([
        doc(ResourceKind::McpServer, "github"),
        doc(ResourceKind::Skill, "github-actions"),
        doc(ResourceKind::Rule, "git-conventions"),
    ]);
    assert_eq!(search(&index, "mcp: git"), ["github"]);
    assert_eq!(search(&index, "skills:git"), ["github-actions"]);
    assert_eq!(
        search(&index, "rule: skill: git"),
        // Both are name-prefix matches; shorter names rank first.
        ["github-actions", "git-conventions"]
    );
    assert_eq!(search(&index, "plugin: git"), Vec::<String>::new());
}

#[test]
fn scope_filters() {
    let mut installed = doc(ResourceKind::Skill, "deploy-installed");
    installed.installed = true;
    let mut remote = doc(ResourceKind::Skill, "deploy-remote");
    remote.remote = true;
    let index =
        SearchIndex::from_docs([installed, remote, doc(ResourceKind::Skill, "deploy-local")]);
    assert_eq!(search(&index, "@installed deploy"), ["deploy-installed"]);
    assert_eq!(search(&index, "@remote deploy"), ["deploy-remote"]);
    assert_eq!(
        search(&index, "@local deploy"),
        ["deploy-installed", "deploy-local"]
    );
    let query = Query {
        scope: ScopeFilter::All,
        ..Query::text("deploy")
    };
    assert_eq!(index.search(&query, &Boosts::default(), 10).len(), 3);
}

#[test]
fn boosts_reorder_but_never_drop() {
    let index = SearchIndex::from_docs([
        doc(ResourceKind::Skill, "preview-pane"),
        doc(ResourceKind::Skill, "code-reviewer"),
        doc(ResourceKind::Skill, "interview-notes"),
    ]);
    let query = Query::text("view");
    let plain = index.search(&query, &Boosts::default(), 10);
    assert_eq!(plain.len(), 3);
    let last = plain.last().unwrap().id.clone();

    let mut favorite = Boosts::default();
    favorite.favorites.insert(last.clone());
    let boosted = index.search(&query, &favorite, 10);
    assert_eq!(boosted[0].id, last);
    let mut before: Vec<_> = plain.iter().map(|hit| hit.id.clone()).collect();
    let mut after: Vec<_> = boosted.iter().map(|hit| hit.id.clone()).collect();
    before.sort();
    after.sort();
    assert_eq!(before, after, "boosts must not add or drop matches");

    let mut recent = Boosts::default();
    recent.recent.insert(plain[1].id.clone(), 0);
    recent.recent.insert(plain[0].id.clone(), 19);
    let reordered = index.search(&query, &recent, 10);
    assert_eq!(reordered[0].id, plain[1].id);
    assert_eq!(reordered.len(), 3);
}

#[test]
fn deterministic_tie_break_by_name_then_id() {
    let a = SearchDoc {
        id: id(ResourceKind::Skill, "b-key"),
        ..doc(ResourceKind::Skill, "same")
    };
    let b = SearchDoc {
        id: id(ResourceKind::Skill, "a-key"),
        ..doc(ResourceKind::Skill, "same")
    };
    let index = SearchIndex::from_docs([a, b.clone()]);
    let hits = index.search(&Query::text("same"), &Boosts::default(), 10);
    assert_eq!(hits[0].id, b.id);
    assert_eq!(hits[0].score, hits[1].score);
}

#[test]
fn empty_query_lists_installed_then_kind_then_name() {
    let mut installed_rule = doc(ResourceKind::Rule, "zeta-rule");
    installed_rule.installed = true;
    let index = SearchIndex::from_docs([
        doc(ResourceKind::Plugin, "a-plugin"),
        doc(ResourceKind::Skill, "b-skill"),
        doc(ResourceKind::Skill, "a-skill"),
        installed_rule,
    ]);
    assert_eq!(
        search(&index, ""),
        ["zeta-rule", "a-skill", "b-skill", "a-plugin"]
    );
    assert_eq!(search(&index, "plugin:"), ["a-plugin"]);
    assert!(
        index
            .search(&Query::default(), &Boosts::default(), 0)
            .is_empty()
    );
}

#[test]
fn limit_returns_best_hits() {
    let index =
        SearchIndex::from_docs((0..50).map(|n| doc(ResourceKind::Skill, &format!("tool-{n:02}"))));
    let hits = index.search(&Query::text("tool"), &Boosts::default(), 3);
    assert_eq!(names(&index, &hits), ["tool-00", "tool-01", "tool-02"]);
}

#[test]
fn name_match_indices_highlight_name_chars() {
    let index = SearchIndex::from_docs([doc(ResourceKind::Skill, "frontend-design")]);
    let hits = index.search(&Query::text("fd"), &Boosts::default(), 10);
    assert_eq!(hits[0].name_match, [0, 9]);
    let summary_only = {
        let mut d = doc(ResourceKind::Skill, "alpha");
        d.summary = "zebra".into();
        SearchIndex::from_docs([d])
    };
    let hits = summary_only.search(&Query::text("zebra"), &Boosts::default(), 10);
    assert!(hits[0].name_match.is_empty());
}

#[test]
fn stable_reselect_after_reranking() {
    let mut index = SearchIndex::from_docs([
        doc(ResourceKind::Skill, "git-alpha"),
        doc(ResourceKind::Skill, "git-beta"),
        doc(ResourceKind::Skill, "git-gamma"),
    ]);
    let query = Query::text("git");
    let first = index.search(&query, &Boosts::default(), 10);
    let selected = first[1].id.clone();

    // An async update adds a better match and re-ranks the list.
    index.upsert(doc(ResourceKind::Skill, "git"));
    let second = index.search(&query, &Boosts::default(), 10);
    let position = reselect(Some(&selected), &second);
    assert_eq!(second[position].id, selected);
    assert_ne!(position, 1);

    index.remove(&selected);
    let third = index.search(&query, &Boosts::default(), 10);
    assert_eq!(reselect(Some(&selected), &third), 0);
    assert_eq!(reselect(None, &third), 0);
    assert_eq!(reselect(Some(&selected), &[]), 0);
}

#[test]
fn upsert_replaces_and_remove_keeps_positions_consistent() {
    let mut index = SearchIndex::new();
    for name in ["one", "two", "three"] {
        index.upsert(doc(ResourceKind::Skill, name));
    }
    let mut renamed = doc(ResourceKind::Skill, "two");
    renamed.summary = "updated".into();
    index.upsert(renamed);
    assert_eq!(index.len(), 3);
    assert_eq!(
        index.get(&id(ResourceKind::Skill, "two")).unwrap().summary,
        "updated"
    );
    assert!(index.remove(&id(ResourceKind::Skill, "one")).is_some());
    assert!(index.remove(&id(ResourceKind::Skill, "one")).is_none());
    assert_eq!(index.len(), 2);
    assert_eq!(search(&index, "three"), ["three"]);
    assert_eq!(search(&index, "two"), ["two"]);
}

#[test]
fn from_record_indexes_reference_names_and_provenance() {
    let root = PathBuf::from("/lib/skills/dev/pdf-tools");
    let record = ResourceRecord {
        id: id(ResourceKind::Skill, "/lib/skills/dev/pdf-tools"),
        name: "pdf-tools".into(),
        summary: "Work with PDF files".into(),
        publisher: Some("acme".into()),
        tags: vec!["documents".into()],
        version: None,
        provenance: Provenance {
            source: SourceKind::Cache {
                provider: "skills.sh".into(),
                url: "https://example.invalid/pdf".into(),
            },
            origin_url: None,
            fingerprint: None,
        },
        locations: vec![ResourceLocation {
            path: root.clone(),
            role: LocationRole::Canonical,
            state: InstallState::Linked,
        }],
        capabilities: CapabilitySet::new(),
        details: ResourceDetails::Skill(SkillDetails {
            root: root.clone(),
            entry: root.join("SKILL.md"),
            category_path: vec!["dev".into()],
            frontmatter: BTreeMap::new(),
            files: vec![
                ResourceFile {
                    path: RelPath::new("references").unwrap(),
                    kind: FileKind::Directory,
                    size: 0,
                    executable: false,
                },
                ResourceFile {
                    path: RelPath::new("references/forms-guide.md").unwrap(),
                    kind: FileKind::Markdown,
                    size: 10,
                    executable: false,
                },
            ],
        }),
        issues: Vec::new(),
    };
    let doc = SearchDoc::from_record(&record);
    assert!(doc.installed);
    assert!(doc.remote);
    assert_eq!(doc.source_label, "skills.sh (cached)");
    assert_eq!(doc.extra, ["dev", "forms-guide.md"]);

    let index = SearchIndex::from_records([&record]);
    let hits = index.search(&Query::text("formsguide"), &Boosts::default(), 5);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tier, MatchTier::Extra);
}

#[test]
fn index_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SearchIndex>();
}

#[test]
fn ten_thousand_docs_twenty_queries_timing() {
    let started = Instant::now();
    let index = SearchIndex::from_docs(synthetic::docs(10_000));
    let build = started.elapsed();
    let boosts = Boosts::default();
    let mut slowest = Duration::ZERO;
    let mut total = Duration::ZERO;
    for input in synthetic::QUERIES {
        let query = Query::parse(input);
        let started = Instant::now();
        let hits = index.search(&query, &boosts, 50);
        let elapsed = started.elapsed();
        println!("query {input:>20?}: {:>4} hits in {elapsed:?}", hits.len());
        slowest = slowest.max(elapsed);
        total += elapsed;
    }
    println!("built 10k docs in {build:?}; 20 queries total {total:?}, slowest {slowest:?}");
    assert_eq!(index.len(), 10_000);
    // Generous unoptimised-build bound; release numbers come from the bench.
    assert!(
        slowest < Duration::from_secs(2),
        "slowest query took {slowest:?}"
    );
}
