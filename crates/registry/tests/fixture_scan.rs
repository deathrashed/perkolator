//! Scans the committed `tests/fixtures/sample-home` demo home.

use std::path::{Path, PathBuf};

use perkolator_core::{
    Capability, FileKind, InstallState, LocationRole, McpTransport, PluginHost, ResourceDetails,
    ResourceKind, ResourceRecord, Scope, TemplateSyntax,
};
use perkolator_registry::{Registry, ScanConfig, ScanReport, scan};

fn fixture_home() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/sample-home")
        .canonicalize()
        .expect("fixture home exists")
}

fn scan_fixture() -> (PathBuf, ScanReport) {
    let home = fixture_home();
    let config = ScanConfig::for_fixture_home(&home);
    let report = scan(&config, &Default::default());
    (home, report)
}

fn named<'a>(report: &'a ScanReport, kind: ResourceKind, name: &str) -> &'a ResourceRecord {
    report
        .records
        .iter()
        .find(|record| record.kind() == kind && record.name == name)
        .unwrap_or_else(|| panic!("no {kind} named {name}"))
}

#[test]
fn all_six_kinds_are_discovered() {
    let (_, report) = scan_fixture();
    assert!(!report.cancelled);
    let registry = Registry::from_report(&report);
    assert_eq!(registry.len(), report.records.len());
    for kind in ResourceKind::ALL {
        assert!(registry.by_kind(kind).count() > 0, "missing {kind}");
    }
    let counts = registry.count_by_kind();
    assert_eq!(counts[&ResourceKind::Skill], 4);
    assert_eq!(counts[&ResourceKind::McpServer], 4);
    assert_eq!(counts[&ResourceKind::Agent], 1);
    assert_eq!(counts[&ResourceKind::Prompt], 2);
    assert_eq!(counts[&ResourceKind::Rule], 3);
    assert_eq!(counts[&ResourceKind::Plugin], 1);
}

#[test]
fn symlinked_skill_is_a_linked_deployment_of_the_library_record() {
    let (home, report) = scan_fixture();
    let skill = named(&report, ResourceKind::Skill, "frontend-design");
    assert_eq!(skill.id.provider.as_str(), "library");
    let library_dir = home.join("Skills/Development/frontend-design");
    assert_eq!(skill.id.key, library_dir.to_string_lossy());
    assert_eq!(skill.canonical_location().unwrap().path, library_dir);
    let deployments: Vec<_> = skill.deployments().collect();
    assert_eq!(deployments.len(), 1);
    assert_eq!(
        deployments[0].path,
        home.join(".claude/skills/frontend-design")
    );
    assert_eq!(deployments[0].state, InstallState::Linked);
    assert_eq!(
        deployments[0].role,
        LocationRole::Deployed {
            target: "claude-code".into(),
            scope: Scope::Global
        }
    );
    assert_eq!(skill.version.as_deref(), Some("1.2.0"));
    assert_eq!(skill.publisher.as_deref(), Some("Perkolator Fixtures"));
    assert_eq!(skill.tags, vec!["design", "ui", "css"]);
    assert!(skill.summary.starts_with("Build restrained"));
    assert!(skill.issues.is_empty(), "{:?}", skill.issues);

    let ResourceDetails::Skill(details) = &skill.details else {
        panic!("not a skill");
    };
    assert_eq!(details.category_path, vec!["Development"]);
    assert_eq!(details.frontmatter["license"], "Apache-2.0");
    assert_eq!(details.frontmatter["metadata"]["audience"], "frontend");
    let file = |path: &str| {
        details
            .files
            .iter()
            .find(|file| file.path.as_str() == path)
            .unwrap_or_else(|| panic!("missing {path}"))
    };
    assert_eq!(file("references/deep/layout.md").kind, FileKind::Markdown);
    assert_eq!(file("assets/logo.svg").kind, FileKind::Image);
    assert!(file("scripts/check.sh").executable);
}

#[test]
fn unmanaged_copy_dangling_link_and_broken_skill() {
    let (home, report) = scan_fixture();
    let legacy = named(&report, ResourceKind::Skill, "legacy-copy");
    assert_eq!(legacy.id.provider.as_str(), "claude-code");
    assert_eq!(legacy.locations.len(), 1);
    assert_eq!(legacy.locations[0].state, InstallState::Unmanaged);

    let dangling = home.join(".claude/skills/dangling");
    assert_eq!(report.broken_links.len(), 1);
    assert_eq!(report.broken_links[0].path, dangling);
    assert_eq!(report.broken_links[0].host.as_deref(), Some("claude-code"));
    assert!(
        report
            .records
            .iter()
            .all(|record| record.name != "dangling")
    );
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert_eq!(report.problems[0].path, dangling);

    let broken = named(&report, ResourceKind::Skill, "broken-skill");
    assert!(broken.has_errors());
    assert!(
        broken
            .issues
            .iter()
            .any(|issue| issue.message.contains("description"))
    );

    let release = named(&report, ResourceKind::Skill, "release-notes");
    assert!(release.capabilities.contains(Capability::SourceUrl));
    assert!(
        release
            .provenance
            .origin_url
            .as_deref()
            .unwrap()
            .starts_with("https://github.com/")
    );
    assert!(
        !named(&report, ResourceKind::Skill, "frontend-design")
            .capabilities
            .contains(Capability::SourceUrl)
    );
}

#[test]
fn mcp_entries_from_three_formats() {
    let (home, report) = scan_fixture();
    let github = named(&report, ResourceKind::McpServer, "github");
    let ResourceDetails::McpServer(details) = &github.details else {
        panic!("not mcp");
    };
    assert_eq!(details.client, "claude-code");
    match &details.transport {
        McpTransport::Stdio { command, env, .. } => {
            assert_eq!(command, "npx");
            assert_eq!(env[0].name, "GITHUB_TOKEN");
            assert_eq!(env[0].value.expose(), "FAKE_TOKEN_FOR_TESTS");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(github.capabilities.contains(Capability::ExecutesCode));

    let postgres = named(&report, ResourceKind::McpServer, "postgres");
    assert!(postgres.id.key.contains("/home/example/projects/demo-app"));
    assert!(postgres.id.key.ends_with(":postgres"));
    assert_ne!(postgres.id, github.id);
    match &postgres.locations[0].role {
        LocationRole::ConfigEntry { entry, .. } => {
            assert_eq!(
                entry,
                "/projects/~1home~1example~1projects~1demo-app/mcpServers/postgres"
            );
        }
        other => panic!("unexpected {other:?}"),
    }

    let docs_search = named(&report, ResourceKind::McpServer, "docs-search");
    let ResourceDetails::McpServer(details) = &docs_search.details else {
        panic!("not mcp");
    };
    assert!(
        matches!(details.transport, McpTransport::StreamableHttp { ref headers, .. } if headers[0].name == "Authorization")
    );
    assert_eq!(details.config_file, home.join(".cursor/mcp.json"));

    let codex = named(&report, ResourceKind::McpServer, "docs");
    let ResourceDetails::McpServer(details) = &codex.details else {
        panic!("not mcp");
    };
    assert_eq!(details.enabled, Some(false));
    assert_eq!(codex.locations[0].state, InstallState::Disabled);
    assert!(codex.capabilities.contains(Capability::ToggleEnabled));
}

#[test]
fn secrets_never_appear_in_debug_or_json() {
    let (_, report) = scan_fixture();
    let debug = format!("{report:?}");
    let json = serde_json::to_string(&report).unwrap();
    for secret in ["FAKE_TOKEN_FOR_TESTS", "FAKE_BEARER_FOR_TESTS"] {
        assert!(!debug.contains(secret), "{secret} leaked into Debug");
        assert!(!json.contains(secret), "{secret} leaked into JSON");
    }
    assert!(json.contains("${DATABASE_URL}"), "references stay visible");
}

#[test]
fn agents_prompts_rules_and_plugin_details() {
    let (home, report) = scan_fixture();

    let agent = named(&report, ResourceKind::Agent, "code-reviewer");
    let ResourceDetails::Agent(details) = &agent.details else {
        panic!("not agent");
    };
    assert_eq!(details.tools, vec!["Read", "Grep", "Glob"]);
    assert_eq!(details.model.as_deref(), Some("sonnet"));
    assert_eq!(details.skills, vec!["frontend-design"]);

    let commit = named(&report, ResourceKind::Prompt, "commit-message");
    let ResourceDetails::Prompt(details) = &commit.details else {
        panic!("not prompt");
    };
    assert_eq!(details.syntax, TemplateSyntax::DoubleBrace);
    let names: Vec<_> = details.variables.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, vec!["type", "scope"]);
    assert_eq!(details.variables[1].default.as_deref(), Some("core"));
    assert!(commit.capabilities.contains(Capability::ExpandVariables));

    let review = named(&report, ResourceKind::Prompt, "review-pr");
    let ResourceDetails::Prompt(details) = &review.details else {
        panic!("not prompt");
    };
    assert_eq!(details.syntax, TemplateSyntax::Arguments);
    assert_eq!(details.host.as_deref(), Some("claude-code"));

    let agents_md = named(&report, ResourceKind::Rule, "AGENTS.md — demo-app");
    assert!(agents_md.summary.starts_with("Rust workspace"));
    named(&report, ResourceKind::Rule, "CLAUDE.md (global)");
    let mdc = named(&report, ResourceKind::Rule, "rust-style");
    let ResourceDetails::Rule(details) = &mdc.details else {
        panic!("not rule");
    };
    assert_eq!(details.globs, vec!["**/*.rs", "Cargo.toml"]);
    assert_eq!(details.always_apply, Some(false));
    assert_eq!(
        details.scope,
        Scope::Project {
            root: home.join("projects/demo-app")
        }
    );

    let plugin = named(&report, ResourceKind::Plugin, "demo-plugin");
    let ResourceDetails::Plugin(details) = &plugin.details else {
        panic!("not plugin");
    };
    assert_eq!(
        details.host,
        PluginHost::External {
            name: "cursor".into()
        }
    );
    let executables: Vec<_> = details
        .executable_files
        .iter()
        .map(|p| p.as_str())
        .collect();
    assert_eq!(executables, vec!["hooks/pre-commit.sh"]);
    assert_eq!(
        details.permissions,
        vec!["filesystem:read", "process:spawn"]
    );
    let kinds: Vec<_> = details.components.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(kinds, vec!["skills", "hooks"]);
    assert_eq!(details.content_hash.as_ref().map(String::len), Some(64));
    assert_eq!(plugin.version.as_deref(), Some("0.3.1"));
    assert_eq!(plugin.publisher.as_deref(), Some("Perkolator Fixtures"));
    assert!(plugin.capabilities.contains(Capability::ExecutesCode));
    assert!(plugin.capabilities.contains(Capability::SourceUrl));
}

#[test]
fn scan_is_deterministic() {
    let (_, first) = scan_fixture();
    let (_, second) = scan_fixture();
    assert_eq!(first, second);
}
