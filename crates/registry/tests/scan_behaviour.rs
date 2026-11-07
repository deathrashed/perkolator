//! Behavioural scan tests against temporary directories only.

use std::fs;
use std::path::{Path, PathBuf};

use perkolator_core::{
    CancelToken, FileKind, InstallState, LocationRole, McpTransport, PluginHost, ProviderId,
    ResourceDetails, ResourceKind, ResourceRecord, RuleFormat, Scope, SourceKind,
};
use perkolator_registry::{
    McpConfigFormat, RootLayout, ScanConfig, ScanLimits, ScanReport, ScanRoot, scan,
};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn skill_md(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
}

fn root(path: &Path, layout: RootLayout, provider: &str) -> ScanRoot {
    ScanRoot {
        path: path.to_path_buf(),
        layout,
        provider: ProviderId::new(provider).unwrap(),
        source: SourceKind::Library {
            root: path.to_path_buf(),
        },
    }
}

fn host_root(path: &Path, layout: RootLayout, host: &str) -> ScanRoot {
    ScanRoot {
        path: path.to_path_buf(),
        layout,
        provider: ProviderId::new(host).unwrap(),
        source: SourceKind::AgentHost {
            host: host.into(),
            scope: Scope::Global,
        },
    }
}

fn run(roots: Vec<ScanRoot>) -> ScanReport {
    let config = ScanConfig {
        roots,
        limits: ScanLimits::default(),
    };
    scan(&config, &CancelToken::new())
}

fn of_kind(report: &ScanReport, kind: ResourceKind) -> Vec<&ResourceRecord> {
    report
        .records
        .iter()
        .filter(|record| record.kind() == kind)
        .collect()
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

#[test]
fn nested_categories_and_no_descent_into_skills() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Skills");
    write(
        &library.join("A/B/C/deep-skill/SKILL.md"),
        &skill_md("deep-skill", "Deep."),
    );
    // A SKILL.md nested inside a skill is part of that skill, not a new one.
    write(
        &library.join("A/B/C/deep-skill/examples/inner/SKILL.md"),
        &skill_md("inner", "Inner."),
    );
    write(&library.join("top/SKILL.md"), &skill_md("top", "Top."));
    let report = run(vec![root(&library, RootLayout::SkillLibrary, "library")]);
    let skills = of_kind(&report, ResourceKind::Skill);
    assert_eq!(
        skills.len(),
        2,
        "{:?}",
        skills.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    let deep = skills.iter().find(|s| s.name == "deep-skill").unwrap();
    let ResourceDetails::Skill(details) = &deep.details else {
        panic!()
    };
    assert_eq!(details.category_path, vec!["A", "B", "C"]);
    let top = skills.iter().find(|s| s.name == "top").unwrap();
    let ResourceDetails::Skill(details) = &top.details else {
        panic!()
    };
    assert!(details.category_path.is_empty());
}

#[test]
fn unknown_frontmatter_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("lib/custom/SKILL.md"),
        "---\nname: custom\ndescription: Has extras.\nx-team:\n  owners: [ana, bo]\nallowed-tools: Bash(git:*)\n---\nBody\n",
    );
    let report = run(vec![root(
        &dir.path().join("lib"),
        RootLayout::SkillLibrary,
        "library",
    )]);
    let ResourceDetails::Skill(details) = &report.records[0].details else {
        panic!()
    };
    assert_eq!(details.frontmatter["x-team"]["owners"][1], "bo");
    assert_eq!(details.frontmatter["allowed-tools"], "Bash(git:*)");
    assert!(report.records[0].issues.is_empty());
}

#[test]
fn malformed_and_invalid_skills_are_listed_with_errors() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    write(
        &lib.join("no-frontmatter/SKILL.md"),
        "# Just markdown\n\nSome text.\n",
    );
    write(
        &lib.join("bad-yaml/SKILL.md"),
        "---\nname: [unclosed\n---\nBody\n",
    );
    write(
        &lib.join("Bad_Name/SKILL.md"),
        &skill_md("Bad_Name", "Invalid name."),
    );
    write(
        &lib.join("mismatch/SKILL.md"),
        &skill_md("other-name", "Mismatch."),
    );
    let report = run(vec![root(&lib, RootLayout::SkillLibrary, "library")]);
    assert_eq!(report.records.len(), 4);
    let by_dir = |dir: &str| {
        report
            .records
            .iter()
            .find(|record| record.id.key.ends_with(dir))
            .unwrap()
    };
    assert!(by_dir("no-frontmatter").has_errors());
    assert_eq!(by_dir("no-frontmatter").summary, "Some text.");
    assert!(by_dir("bad-yaml").has_errors());
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.path.ends_with("bad-yaml/SKILL.md"))
    );
    let bad_name = by_dir("Bad_Name");
    assert!(bad_name.has_errors());
    assert_eq!(bad_name.name, "Bad_Name");
    let mismatch = by_dir("mismatch");
    assert!(!mismatch.has_errors());
    assert!(
        mismatch
            .issues
            .iter()
            .any(|i| i.message.contains("does not match"))
    );
}

#[cfg(unix)]
#[test]
fn unreadable_skill_is_a_problem_not_a_panic() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("lib/locked/SKILL.md");
    write(&entry, &skill_md("locked", "Locked."));
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&entry).is_ok() {
        // Running with privileges that bypass permissions; nothing to test.
        return;
    }
    let report = run(vec![root(
        &dir.path().join("lib"),
        RootLayout::SkillLibrary,
        "library",
    )]);
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(report.records.len(), 1);
    assert!(report.records[0].has_errors());
    assert!(report.problems.iter().any(|p| p.path == entry));
}

#[cfg(unix)]
#[test]
fn symlink_loops_do_not_hang() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    write(
        &lib.join("cat/looped/SKILL.md"),
        &skill_md("looped", "Loop."),
    );
    symlink("..", lib.join("cat/looped/up")).unwrap();
    symlink(".", lib.join("cat/self")).unwrap();
    symlink(&lib, lib.join("cat/root")).unwrap();
    let report = run(vec![root(&lib, RootLayout::SkillLibrary, "library")]);
    assert_eq!(of_kind(&report, ResourceKind::Skill).len(), 1);
    let ResourceDetails::Skill(details) = &report.records[0].details else {
        panic!()
    };
    assert!(details.files.iter().any(|file| {
        file.path.as_str() == "up" && matches!(file.kind, FileKind::Symlink { broken: false, .. })
    }));
}

#[test]
fn duplicate_names_in_two_roots_stay_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("one");
    let second = dir.path().join("two");
    write(&first.join("pdf/SKILL.md"), &skill_md("pdf", "First."));
    write(&second.join("pdf/SKILL.md"), &skill_md("pdf", "Second."));
    let report = run(vec![
        root(&first, RootLayout::SkillLibrary, "library"),
        root(&second, RootLayout::SkillLibrary, "library"),
    ]);
    assert_eq!(report.records.len(), 2);
    assert_ne!(report.records[0].id, report.records[1].id);
    assert!(report.records.iter().all(|record| record.name == "pdf"));
}

#[cfg(unix)]
#[test]
fn link_to_unscanned_target_creates_host_record() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("elsewhere/ext-skill");
    write(
        &outside.join("SKILL.md"),
        &skill_md("ext-skill", "External."),
    );
    let agent_dir = dir.path().join(".claude/skills");
    fs::create_dir_all(&agent_dir).unwrap();
    symlink(&outside, agent_dir.join("ext-skill")).unwrap();
    let report = run(vec![host_root(
        &agent_dir,
        RootLayout::SkillDirectory {
            host: "claude-code".into(),
        },
        "claude-code",
    )]);
    assert_eq!(report.records.len(), 1);
    let record = &report.records[0];
    assert_eq!(record.id.provider.as_str(), "claude-code");
    assert_eq!(record.id.key, canonical(&outside).to_string_lossy());
    assert_eq!(
        record.canonical_location().unwrap().path,
        canonical(&outside)
    );
    let deployment = record.deployments().next().unwrap();
    assert_eq!(deployment.state, InstallState::Linked);
    assert_eq!(deployment.path, agent_dir.join("ext-skill"));
}

#[test]
fn mcp_json_jsonc_toml_and_vscode() {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("mcp.json");
    write(
        &json,
        r#"{"mcpServers": {"a": {"command": "a-bin"}, "bad": 42, "b": {"url": "https://h/b", "disabled": true}}}"#,
    );
    let jsonc = dir.path().join("settings.jsonc");
    write(
        &jsonc,
        "{\n // comment\n \"mcpServers\": {\n  \"c\": {\"type\": \"sse\", \"url\": \"https://h/c\"}, /* x */\n },\n}\n",
    );
    let toml = dir.path().join("config.toml");
    write(
        &toml,
        "[mcp_servers.d]\ncommand = \"d-bin\"\nargs = [\"--x\"]\n[mcp_servers.d.env]\nKEY = \"value\"\n[mcp_servers.e]\nurl = \"https://h/e\"\nenabled = true\n",
    );
    let vscode = dir.path().join(".vscode/mcp.json");
    write(
        &vscode,
        r#"{"servers": {"f": {"type": "stdio", "command": "f-bin"}, "g": {"type": "http", "url": "https://h/g"}}, "inputs": []}"#,
    );
    let config = |path: &Path, client: &str, format| {
        host_root(
            path,
            RootLayout::McpConfig {
                client: client.into(),
                format,
            },
            client,
        )
    };
    let report = run(vec![
        config(&json, "cursor", McpConfigFormat::McpServersJson),
        config(&jsonc, "gemini-cli", McpConfigFormat::McpServersJson),
        config(&toml, "codex", McpConfigFormat::CodexToml),
        config(&vscode, "vscode", McpConfigFormat::VsCodeServersJson),
    ]);
    let names: Vec<_> = report.records.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c", "d", "e", "f", "g"]);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(report.problems[0].message.contains("`bad`"));

    let transport = |name: &str| {
        let record = report.records.iter().find(|r| r.name == name).unwrap();
        let ResourceDetails::McpServer(details) = &record.details else {
            panic!()
        };
        (details.transport.clone(), details.enabled)
    };
    assert!(matches!(transport("a").0, McpTransport::Stdio { .. }));
    assert_eq!(transport("b").1, Some(false));
    assert!(matches!(transport("c").0, McpTransport::Sse { .. }));
    match transport("d").0 {
        McpTransport::Stdio { env, args, .. } => {
            assert_eq!(args, vec!["--x"]);
            assert_eq!(env[0].name, "KEY");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        transport("e").0,
        McpTransport::StreamableHttp { .. }
    ));
    assert_eq!(transport("e").1, Some(true));
    assert!(matches!(
        transport("g").0,
        McpTransport::StreamableHttp { .. }
    ));
    let ids: std::collections::HashSet<_> = report.records.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids.len(), report.records.len());
}

#[test]
fn invalid_mcp_file_is_one_problem() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    write(&path, "{ not json");
    let report = run(vec![host_root(
        &path,
        RootLayout::McpConfig {
            client: "cursor".into(),
            format: McpConfigFormat::McpServersJson,
        },
        "cursor",
    )]);
    assert!(report.records.is_empty());
    assert_eq!(report.problems.len(), 1);
}

#[test]
fn project_scoped_claude_entries_are_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".claude.json");
    write(
        &path,
        r#"{"mcpServers": {"db": {"command": "g"}},
            "projects": {"/p/one": {"mcpServers": {"db": {"command": "x"}}},
                         "/p/two": {"mcpServers": {"db": {"command": "y"}}},
                         "/p/none": {"allowedTools": []}}}"#,
    );
    let report = run(vec![host_root(
        &path,
        RootLayout::McpConfig {
            client: "claude-code".into(),
            format: McpConfigFormat::McpServersJson,
        },
        "claude-code",
    )]);
    assert_eq!(report.records.len(), 3);
    let scopes: Vec<_> = report
        .records
        .iter()
        .map(|record| match &record.provenance.source {
            SourceKind::AgentHost { scope, .. } => scope.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(scopes[0], Scope::Global);
    assert_eq!(
        scopes[1],
        Scope::Project {
            root: PathBuf::from("/p/one")
        }
    );
    assert!(report.records[2].id.key.contains("/p/two"));
}

#[test]
fn mdc_frontmatter_strict_and_lenient() {
    let dir = tempfile::tempdir().unwrap();
    let rules = dir.path().join(".cursor/rules");
    write(
        &rules.join("strict.mdc"),
        "---\ndescription: Strict rule\nglobs: [\"src/**\", \"*.toml\"]\nalwaysApply: true\n---\nBody\n",
    );
    write(
        &rules.join("nested/loose.mdc"),
        "---\ndescription: Loose\nglobs: *.rs, *.md\n---\nBody\n",
    );
    write(&rules.join("plain.md"), "# Plain\n\nApplies manually.\n");
    let scope = Scope::Project {
        root: dir.path().to_path_buf(),
    };
    let report = run(vec![root(
        &rules,
        RootLayout::RuleDirectory {
            host: Some("cursor".into()),
            scope: scope.clone(),
        },
        "cursor",
    )]);
    let rule = |name: &str| {
        let record = report.records.iter().find(|r| r.name == name).unwrap();
        let ResourceDetails::Rule(details) = &record.details else {
            panic!()
        };
        (record, details.clone())
    };
    let (strict, details) = rule("strict");
    assert_eq!(details.globs, vec!["src/**", "*.toml"]);
    assert_eq!(details.always_apply, Some(true));
    assert_eq!(details.format, RuleFormat::CursorMdc);
    assert_eq!(strict.summary, "Strict rule");
    let (loose, details) = rule("nested/loose");
    assert_eq!(details.globs, vec!["*.rs", "*.md"]);
    assert!(!loose.has_errors());
    let (plain, details) = rule("plain");
    assert_eq!(details.format, RuleFormat::Generic);
    assert_eq!(plain.summary, "Applies manually.");
    assert_eq!(details.scope, scope);
}

#[test]
fn plugin_hash_tracks_executables_only() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path().join("plugins");
    let plugin = plugins.join("p");
    write(
        &plugin.join(".claude-plugin/plugin.json"),
        r#"{"name": "p", "hooks": {"PreToolUse": []}}"#,
    );
    write(&plugin.join("hooks/run.sh"), "echo one\n");
    write(&plugin.join("README.md"), "# P\n");
    let roots = || {
        vec![host_root(
            &plugins,
            RootLayout::Plugins {
                host: PluginHost::External {
                    name: "claude-code".into(),
                },
            },
            "claude-code",
        )]
    };
    let hash = |report: &ScanReport| {
        let ResourceDetails::Plugin(details) = &report.records[0].details else {
            panic!()
        };
        details.content_hash.clone().unwrap()
    };
    let first = run(roots());
    let ResourceDetails::Plugin(details) = &first.records[0].details else {
        panic!()
    };
    assert_eq!(details.components[0].path, "(inline in manifest)");
    write(&plugin.join("README.md"), "# P changed\n");
    assert_eq!(
        hash(&first),
        hash(&run(roots())),
        "docs do not affect the hash"
    );
    write(&plugin.join("hooks/run.sh"), "echo two\n");
    assert_ne!(
        hash(&first),
        hash(&run(roots())),
        "executable change invalidates"
    );
}

#[test]
fn perkolator_manifest_and_nested_search() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path().join("cache");
    write(
        &plugins.join("market/tool/1.0.0/perkolator-plugin.json"),
        r#"{"name": "tool"}"#,
    );
    let report = run(vec![host_root(
        &plugins,
        RootLayout::Plugins {
            host: PluginHost::External {
                name: "cursor".into(),
            },
        },
        "cursor",
    )]);
    let ResourceDetails::Plugin(details) = &report.records[0].details else {
        panic!()
    };
    assert_eq!(details.host, PluginHost::Perkolator);
    assert!(details.executable_files.is_empty());
}

#[test]
fn cancellation_stops_a_scan_early() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    for index in 0..20 {
        write(
            &lib.join(format!("s{index}/SKILL.md")),
            &skill_md(&format!("s{index}"), "x"),
        );
    }
    let config = ScanConfig {
        roots: vec![root(&lib, RootLayout::SkillLibrary, "library")],
        limits: ScanLimits::default(),
    };
    let cancel = CancelToken::new();
    cancel.cancel();
    let report = scan(&config, &cancel);
    assert!(report.cancelled);
    assert!(report.records.is_empty());
    let full = scan(&config, &CancelToken::new());
    assert!(!full.cancelled);
    assert_eq!(full.records.len(), 20);
}

#[cfg(unix)]
#[test]
fn symlinked_agent_file_is_one_record_with_linked_location() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Agents/helper.md");
    write(
        &source,
        "---\ndescription: Helps\ntools: [Read]\n---\nBody\n",
    );
    let claude = dir.path().join(".claude/agents");
    let cursor = dir.path().join(".cursor/agents");
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&cursor).unwrap();
    symlink(&source, claude.join("helper.md")).unwrap();
    symlink(&source, cursor.join("helper.md")).unwrap();
    symlink(dir.path().join("missing.md"), cursor.join("gone.md")).unwrap();
    let report = run(vec![
        host_root(
            &claude,
            RootLayout::AgentProfiles {
                host: "claude-code".into(),
            },
            "claude-code",
        ),
        host_root(
            &cursor,
            RootLayout::AgentProfiles {
                host: "cursor".into(),
            },
            "cursor",
        ),
    ]);
    assert_eq!(report.records.len(), 1);
    let record = &report.records[0];
    assert_eq!(record.name, "helper");
    assert_eq!(record.deployments().count(), 2);
    assert_eq!(report.broken_links.len(), 1);
    assert_eq!(report.broken_links[0].host.as_deref(), Some("cursor"));
    assert!(matches!(record.locations[0].role, LocationRole::Canonical));
}

#[test]
fn rule_file_root_and_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("GEMINI.md");
    write(&path, "# Gemini\n\nBe brief.\n");
    let layout = RootLayout::RuleFile {
        host: Some("gemini-cli".into()),
        format: RuleFormat::GeminiMd,
        scope: Scope::Global,
    };
    let report = run(vec![
        host_root(&path, layout.clone(), "gemini-cli"),
        host_root(&dir.path().join("absent.md"), layout, "gemini-cli"),
    ]);
    assert_eq!(report.records.len(), 1);
    assert_eq!(report.records[0].name, "GEMINI.md (global)");
    assert_eq!(report.records[0].summary, "Be brief.");
    assert_eq!(report.problems.len(), 1);
}
