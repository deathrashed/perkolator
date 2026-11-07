//! End-to-end tests of the `perk` command surface (real clap parsing, real
//! command code) against a tempdir copy of `tests/fixtures/sample-home`.
//! Nothing touches the real home: every invocation passes `--home`.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use clap::Parser;
use perkolator_cli::{Cli, CliError, CommandOutput, run};
use serde_json::Value;

const SECRET_TOKEN: &str = "FAKE_TOKEN_FOR_TESTS";
const SECRET_BEARER: &str = "FAKE_BEARER_FOR_TESTS";

struct Home {
    _tmp: tempfile::TempDir,
    home: PathBuf,
}

impl Home {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample-home");
        let home = tmp.path().join("home");
        copy_tree(&fixture, &home);
        let home = home.canonicalize().unwrap();
        Self { _tmp: tmp, home }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.join(relative)
    }

    fn run(&self, args: &[&str]) -> Result<CommandOutput, CliError> {
        let home = self.home.to_str().unwrap().to_owned();
        let project = self.path("projects/demo-app").to_str().unwrap().to_owned();
        let mut argv = vec!["perk", "--home", &home, "--project", &project];
        argv.extend_from_slice(args);
        let cli = Cli::try_parse_from(argv).expect("arguments parse");
        run(&cli)
    }

    /// Successful command output.
    fn ok(&self, args: &[&str]) -> String {
        let output = self
            .run(args)
            .unwrap_or_else(|error| panic!("{args:?}: {error}"));
        assert!(output.success, "{args:?} failed:\n{}", output.text);
        output.text
    }

    /// Command that completed but reported failure (exit code 1).
    fn failed(&self, args: &[&str]) -> String {
        let output = self
            .run(args)
            .unwrap_or_else(|error| panic!("{args:?}: {error}"));
        assert!(
            !output.success,
            "{args:?} unexpectedly succeeded:\n{}",
            output.text
        );
        output.text
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.ok(args)).expect("valid JSON")
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Entry> {
        let mut entries = BTreeMap::new();
        snapshot_into(&self.home, &self.home, &mut entries);
        entries
    }
}

/// Copies regular files and directories; symlinks are recreated as symlinks
/// with their original (relative) contents.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let source = entry.path();
        let dest = to.join(entry.file_name());
        let file_type = fs::symlink_metadata(&source).unwrap().file_type();
        if file_type.is_symlink() {
            symlink(fs::read_link(&source).unwrap(), &dest).unwrap();
        } else if file_type.is_dir() {
            copy_tree(&source, &dest);
        } else {
            fs::copy(&source, &dest).unwrap();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Dir,
    File(Vec<u8>),
    Link(PathBuf),
}

fn snapshot_into(root: &Path, dir: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let relative = path.strip_prefix(root).unwrap().to_path_buf();
        let file_type = fs::symlink_metadata(&path).unwrap().file_type();
        if file_type.is_symlink() {
            entries.insert(relative, Entry::Link(fs::read_link(&path).unwrap()));
        } else if file_type.is_dir() {
            entries.insert(relative, Entry::Dir);
            snapshot_into(root, &path, entries);
        } else {
            entries.insert(relative, Entry::File(fs::read(&path).unwrap()));
        }
    }
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

const THREE_TARGETS: [&str; 6] = [
    "--target",
    "codex",
    "--target",
    "cursor",
    "--target",
    "gemini-cli",
];

fn deploy_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["deploy", "frontend-design"];
    args.extend_from_slice(&THREE_TARGETS);
    args.extend_from_slice(extra);
    args
}

#[test]
fn scan_finds_all_six_kinds_and_the_dangling_link() {
    let home = Home::new();
    let report = home.json(&["scan", "--json"]);
    let counts = &report["counts"];
    assert_eq!(counts["skill"], 4);
    assert_eq!(counts["mcp"], 4);
    assert_eq!(counts["agent"], 1);
    assert_eq!(counts["prompt"], 2);
    assert_eq!(counts["rule"], 3);
    assert_eq!(counts["plugin"], 1);
    assert_eq!(report["total"], 15);
    let broken = report["broken_links"].as_array().unwrap();
    assert_eq!(broken.len(), 1);
    assert_eq!(
        broken[0]["path"],
        home.path(".claude/skills/dangling").to_str().unwrap()
    );
    assert_eq!(report["problems"].as_array().unwrap().len(), 1);

    let text = home.ok(&["scan"]);
    assert!(text.contains("MCP Servers  4"), "{text}");
    assert!(text.contains("Broken links: 1"), "{text}");
}

#[test]
fn list_filters_by_kind() {
    let home = Home::new();
    let items = home.json(&["list", "--kind", "mcp", "--json"]);
    let items = items.as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert!(items.iter().all(|item| item["kind"] == "mcp"));
    let text = home.ok(&["list"]);
    assert!(text.contains("15 resource(s)"), "{text}");
    assert!(
        text.contains("legacy-copy") && text.contains("Unmanaged"),
        "{text}"
    );
}

#[test]
fn search_kind_prefix_returns_only_mcp_results() {
    let home = Home::new();
    for query in ["mcp: git", "mcp:git"] {
        let result = home.json(&["search", query, "--json"]);
        let hits = result["results"].as_array().unwrap();
        assert!(!hits.is_empty(), "{query}: no hits");
        assert!(hits.iter().all(|hit| hit["kind"] == "mcp"), "{result}");
        assert_eq!(hits[0]["name"], "github");
        assert!(result["search_ms"].as_f64().unwrap() >= 0.0);
        assert_eq!(result["query"]["kinds"][0], "mcp");
    }
    let text = home.ok(&["search", "frontend"]);
    assert!(text.contains("frontend-design"), "{text}");
    assert!(text.contains(" ms over 15 docs"), "{text}");
}

#[test]
fn show_prints_metadata_locations_and_preview_tabs() {
    let home = Home::new();
    let text = home.ok(&["show", "frontend-design"]);
    assert!(text.contains("version:   1.2.0"), "{text}");
    assert!(text.contains("deployed claude-code (global)"), "{text}");
    assert!(text.contains("deploy-files"), "{text}");
    for tab in ["[Overview]", "[Files (", "[Metadata]"] {
        assert!(text.contains(tab), "missing {tab}:\n{text}");
    }
    assert!(text.contains("references/"), "{text}");
}

#[test]
fn show_by_ambiguous_name_lists_candidates_and_id_resolves() {
    let home = Home::new();
    copy_tree(
        &home.path("Skills/Writing/release-notes"),
        &home.path(".cursor/skills/release-notes"),
    );
    match home.run(&["show", "release-notes"]) {
        Err(CliError::Ambiguous { candidates, .. }) => {
            assert_eq!(candidates.len(), 2, "{candidates:?}");
            assert!(candidates.iter().any(|c| c.starts_with("skill:library:")));
            assert!(candidates.iter().any(|c| c.starts_with("skill:cursor:")));
        }
        other => panic!("expected ambiguity, got {other:?}"),
    }
    let id = format!(
        "skill:library:{}",
        home.path("Skills/Writing/release-notes").display()
    );
    let text = home.ok(&["show", &id]);
    assert!(text.starts_with("release-notes — Skill"), "{text}");
}

#[test]
fn show_unknown_name_suggests_matches() {
    let home = Home::new();
    match home.run(&["show", "frontend"]) {
        Err(CliError::NotFound { suggestions, .. }) => {
            assert!(suggestions.iter().any(|s| s.contains("frontend-design")));
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[test]
fn validate_fails_because_of_broken_skill_and_strict_counts_warnings() {
    let home = Home::new();
    let text = home.failed(&["validate"]);
    assert!(text.contains("error    skill broken-skill"), "{text}");

    fs::remove_dir_all(home.path("Skills/Writing/broken-skill")).unwrap();
    let text = home.ok(&["validate"]);
    assert!(text.contains("0 error(s), 1 warning(s)"), "{text}");
    let text = home.failed(&["validate", "--strict"]);
    assert!(text.contains("dangling"), "{text}");
}

#[test]
fn copy_config_and_show_never_reveal_secrets() {
    let home = Home::new();
    let config = home.ok(&["copy", "github", "copy-config"]);
    assert!(config.contains("\"github\""), "{config}");
    assert!(config.contains("<redacted>"), "{config}");
    assert!(!config.contains(SECRET_TOKEN), "{config}");
    let remote = home.ok(&["copy", "docs-search", "copy-config"]);
    assert!(!remote.contains(SECRET_BEARER), "{remote}");
    for name in ["github", "docs-search"] {
        let shown = home.ok(&["show", name]);
        assert!(!shown.contains(SECRET_TOKEN) && !shown.contains(SECRET_BEARER));
        let actions = home.ok(&["actions", name]);
        assert!(!actions.contains(SECRET_TOKEN) && !actions.contains(SECRET_BEARER));
    }
    // Whole MCP config files hold other entries' secrets: never offered.
    assert!(matches!(
        home.run(&["copy", "github", "copy-content"]),
        Err(CliError::Failed(_))
    ));
}

#[test]
fn actions_and_copy_payloads_for_a_skill() {
    let home = Home::new();
    let text = home.ok(&["actions", "frontend-design"]);
    assert!(
        text.contains("copy-invocation") && text.contains("[Enter]"),
        "{text}"
    );
    assert!(text.contains("(disabled: Composing needs"), "{text}");
    let path = home.ok(&["copy", "frontend-design", "copy-path"]);
    assert_eq!(
        path.trim_end(),
        home.path("Skills/Development/frontend-design")
            .to_str()
            .unwrap()
    );
    let content = home.ok(&["copy", "frontend-design", "copy-content"]);
    assert!(content.contains("name: frontend-design"), "{content}");
    match home.run(&["copy", "frontend-design", "no-such-action"]) {
        Err(CliError::Usage(message)) => assert!(message.contains("copy-invocation")),
        other => panic!("expected usage error, got {other:?}"),
    }
    assert!(matches!(
        home.run(&["copy", "frontend-design", "plan-deploy"]),
        Err(CliError::Usage(_))
    ));
}

#[test]
fn targets_resolve_against_the_given_home() {
    let home = Home::new();
    let targets = home.json(&["targets", "--json"]);
    let find = |id: &str| {
        targets
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["id"] == id)
            .cloned()
            .unwrap()
    };
    let claude = find("claude-code");
    assert_eq!(
        claude["global_skills_dir"],
        home.path(".claude/skills").to_str().unwrap()
    );
    assert_eq!(claude["status"], "detected");
    assert_eq!(find("gemini-cli")["status"], "missing");
    assert_eq!(find("generic")["global_skills_dir"], Value::Null);
}

/// A12: one canonical skill linked into three agents, re-run is a no-op,
/// removing one deployment leaves the source and the other links intact.
#[test]
fn a12_symlink_deploy_to_three_targets_end_to_end() {
    let home = Home::new();
    let source_skill = home.path("Skills/Development/frontend-design/SKILL.md");
    let original = fs::read_to_string(&source_skill).unwrap();
    let links = [
        home.path(".agents/skills/frontend-design"),
        home.path(".cursor/skills/frontend-design"),
        home.path(".gemini/skills/frontend-design"),
    ];

    let before = home.snapshot();
    let dry = home.ok(&deploy_args(&[]));
    assert!(
        dry.contains("3 destination(s)") && dry.contains("Dry run"),
        "{dry}"
    );
    let refused = home.failed(&deploy_args(&["--apply"]));
    assert!(
        refused.contains("Refusing to apply without --yes"),
        "{refused}"
    );
    assert_eq!(
        home.snapshot(),
        before,
        "dry run / refusal changed the disk"
    );

    let applied = home.ok(&deploy_args(&["--apply", "--yes"]));
    assert!(applied.contains(": applied;"), "{applied}");
    for link in &links {
        assert!(is_symlink(link), "{} is not a symlink", link.display());
        assert_eq!(fs::read_to_string(link.join("SKILL.md")).unwrap(), original);
    }

    let again = home.ok(&deploy_args(&["--apply", "--yes"]));
    assert_eq!(again.matches("+ already in place:").count(), 3, "{again}");
    assert!(again.contains("0 change(s)") && again.contains("Nothing to apply"));

    let status = home.ok(&["status"]);
    assert_eq!(status.matches("\nlinked ").count(), 3, "{status}");
    assert!(status.contains("3 managed deployment(s)"), "{status}");
    let matrix = home.ok(&["matrix"]);
    let row = matrix
        .lines()
        .find(|line| line.starts_with("frontend-design"))
        .unwrap();
    assert!(row.matches("linked").count() >= 3, "{matrix}");
    assert!(
        row.contains("unmanaged"),
        "fixture claude link is unmanaged: {matrix}"
    );

    let removed_link = links[1].to_str().unwrap();
    let removed = home.ok(&["undeploy", removed_link, "--apply", "--yes"]);
    assert!(removed.contains("remove managed link"), "{removed}");
    assert!(
        fs::symlink_metadata(&links[1]).is_err(),
        "link still present"
    );
    assert_eq!(fs::read_to_string(&source_skill).unwrap(), original);
    for link in [&links[0], &links[2]] {
        assert_eq!(fs::read_to_string(link.join("SKILL.md")).unwrap(), original);
    }
    assert!(home.ok(&["status"]).contains("2 managed deployment(s)"));

    for _ in 0..2 {
        let recovered = home.ok(&["recover"]);
        assert!(recovered.contains("No interrupted runs"), "{recovered}");
    }
    assert!(home.ok(&["status"]).contains("2 managed deployment(s)"));
}

/// A14: an unmanaged directory at one destination makes the whole plan
/// non-applicable; nothing on disk changes, not even the other destinations.
#[test]
fn a14_unmanaged_destination_blocks_the_plan_and_changes_nothing() {
    let home = Home::new();
    let blocker = home.path(".cursor/skills/frontend-design");
    fs::create_dir_all(&blocker).unwrap();
    fs::write(blocker.join("SKILL.md"), "user's own copy\n").unwrap();
    let before = home.snapshot();

    // The user's copy is itself a skill, so the name is now ambiguous.
    assert!(matches!(
        home.run(&deploy_args(&[])),
        Err(CliError::Ambiguous { .. })
    ));
    let id = format!(
        "skill:library:{}",
        home.path("Skills/Development/frontend-design").display()
    );
    let by_id = |extra: &[&'static str]| {
        let mut args = vec!["deploy", id.as_str()];
        args.extend_from_slice(&THREE_TARGETS);
        args.extend_from_slice(extra);
        home.failed(&args)
    };
    let dry = by_id(&[]);
    assert!(dry.contains("unmanaged directory already exists"), "{dry}");
    let applied = by_id(&["--apply", "--yes"]);
    assert!(
        applied.contains("Refusing to apply: 1 conflict(s)"),
        "{applied}"
    );

    assert_eq!(home.snapshot(), before, "a blocked plan changed the disk");
    assert_eq!(
        fs::read_to_string(blocker.join("SKILL.md")).unwrap(),
        "user's own copy\n"
    );
    assert!(home.ok(&["status"]).contains("No managed deployments"));
}

#[test]
fn matching_unmanaged_link_is_left_unmanaged() {
    let home = Home::new();
    let before = home.snapshot();
    let text = home.ok(&[
        "deploy",
        "frontend-design",
        "--target",
        "claude-code",
        "--apply",
        "--yes",
    ]);
    assert!(text.contains("(unmanaged, left as is)"), "{text}");
    assert_eq!(home.snapshot(), before);
    assert!(home.ok(&["status"]).contains("No managed deployments"));
}

#[test]
fn deploy_refuses_non_skills_invalid_skills_and_unknown_targets() {
    let home = Home::new();
    let before = home.snapshot();
    for args in [
        &["deploy", "github", "--target", "cursor"][..],
        &[
            "deploy",
            "broken-skill",
            "--target",
            "cursor",
            "--apply",
            "--yes",
        ][..],
        &["deploy", "frontend-design", "--target", "nope"][..],
        &["deploy", "frontend-design", "--target", "generic"][..],
    ] {
        assert!(home.run(args).is_err(), "{args:?} was accepted");
    }
    assert_eq!(home.snapshot(), before);
}

#[test]
fn managed_copy_into_project_scope_and_full_removal() {
    let home = Home::new();
    let managed_root = home.path("managed");
    let managed_arg = managed_root.to_str().unwrap();
    let text = home.ok(&[
        "deploy",
        "release-notes",
        "--target",
        "claude-code",
        "--scope",
        "project",
        "--mode",
        "managed-copy",
        "--managed-root",
        managed_arg,
        "--apply",
        "--yes",
    ]);
    assert!(text.contains("(managed copy)"), "{text}");
    let copy = managed_root.join("release-notes");
    let link = home.path("projects/demo-app/.claude/skills/release-notes");
    assert!(copy.join("SKILL.md").is_file() && !is_symlink(&copy));
    assert!(is_symlink(&link));
    assert_eq!(fs::read_link(&link).unwrap(), copy);
    let status = home.ok(&["status"]);
    assert!(
        status.contains("perkolator-managed") && status.contains("project "),
        "{status}"
    );

    // The managed copy is still used by the project link.
    let blocked = home.failed(&["undeploy", copy.to_str().unwrap(), "--apply", "--yes"]);
    assert!(copy.is_dir(), "{blocked}");
    home.ok(&["undeploy", link.to_str().unwrap(), "--apply", "--yes"]);
    home.ok(&["undeploy", copy.to_str().unwrap(), "--apply", "--yes"]);
    assert!(fs::symlink_metadata(&link).is_err() && !copy.exists());
    assert!(home.path("Skills/Writing/release-notes/SKILL.md").is_file());
    assert!(home.ok(&["status"]).contains("No managed deployments"));
}

#[test]
fn project_scope_requires_a_project() {
    let home = Home::new();
    let cli = Cli::try_parse_from([
        "perk",
        "--home",
        home.home.to_str().unwrap(),
        "deploy",
        "release-notes",
        "--target",
        "cursor",
        "--scope",
        "project",
    ])
    .unwrap();
    assert!(matches!(run(&cli), Err(CliError::Usage(_))));
}

#[test]
fn custom_state_dir_and_idempotent_recover_on_fresh_home() {
    let home = Home::new();
    let state = tempfile::tempdir().unwrap();
    let state_dir = state.path().join("state");
    let state_arg = state_dir.to_str().unwrap();
    for _ in 0..2 {
        let text = home.ok(&["--state-dir", state_arg, "recover"]);
        assert!(text.contains("No interrupted runs"), "{text}");
    }
    assert!(!state_dir.exists(), "recover created state without runs");
    assert!(!home.path(".local").exists());

    home.ok(&[
        "--state-dir",
        state_arg,
        "deploy",
        "release-notes",
        "--target",
        "gemini-cli",
        "--apply",
        "--yes",
    ]);
    assert!(state_dir.join("manifest.json").is_file());
    assert!(!home.path(".local").exists(), "default state dir was used");
    let status = home.ok(&["--state-dir", state_arg, "status"]);
    assert!(status.contains("gemini-cli"), "{status}");
}
