//! End-to-end deployment safety tests. Everything happens inside tempdirs
//! with a fake home; real agent directories are never touched.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use perkolator_core::{CancelToken, Scope};
use perkolator_deployment::{
    ApplyStatus, Conflict, DeployError, DeployRequest, DeploymentMode, DeploymentStatus,
    Destination, Executor, FaultInjection, MANAGED_TARGET_ID, Manifest, ManifestEntry, PlannedOp,
    deployment_matrix, fingerprint_tree, plan_deploy, plan_remove, plan_repair, status_of,
};
use perkolator_targets::{HomeEnv, TargetRegistry};

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: HomeEnv,
    source: PathBuf,
    manifest_path: PathBuf,
    journal_dir: PathBuf,
    registry: TargetRegistry,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let home_dir = root.join("home");
        fs::create_dir_all(&home_dir).unwrap();
        let source = root.join("library/engineering/rust/my-skill");
        fs::create_dir_all(source.join("references")).unwrap();
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: my-skill\n---\n# My skill\n",
        )
        .unwrap();
        fs::write(source.join("references/guide.md"), "guide").unwrap();
        fs::write(source.join("scripts/run.sh"), "#!/bin/sh\necho hi\n").unwrap();
        symlink("references/guide.md", source.join("guide-link.md")).unwrap();
        Self {
            home: HomeEnv::new(&home_dir),
            manifest_path: root.join("state/manifest.json"),
            journal_dir: root.join("state/journal"),
            registry: TargetRegistry::builtin(),
            _tmp: tmp,
            root,
            source,
        }
    }

    fn home_dir(&self) -> &Path {
        &self.home.home
    }

    fn dest(&self, target: &str) -> Destination {
        Destination::for_target(
            self.registry.get(target).unwrap(),
            Scope::Global,
            &self.home,
        )
        .unwrap()
    }

    fn three(&self) -> Vec<Destination> {
        vec![
            self.dest("claude-code"),
            self.dest("cursor"),
            self.dest("gemini-cli"),
        ]
    }

    fn request(&self, mode: DeploymentMode, destinations: Vec<Destination>) -> DeployRequest {
        DeployRequest {
            source: self.source.clone(),
            name: "my-skill".to_owned(),
            mode,
            destinations,
            managed_root: Some(self.root.join("managed")),
            approved_roots: vec![self.root.clone()],
        }
    }

    fn executor(&self) -> Executor {
        Executor::new(&self.journal_dir, &self.manifest_path)
    }

    fn deploy(&self, mode: DeploymentMode, manifest: &mut Manifest) {
        let plan = plan_deploy(&self.request(mode, self.three()), manifest);
        assert!(plan.is_applicable(), "{}", plan.summary());
        self.executor().apply(&plan, manifest).unwrap();
    }

    fn skill_path(&self, target: &str) -> PathBuf {
        self.dest(target).skills_dir.join("my-skill")
    }
}

fn has_conflict(conflicts: &[Conflict], predicate: impl Fn(&Conflict) -> bool) -> bool {
    conflicts.iter().any(predicate)
}

fn listing(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .map(|e| {
            e.unwrap()
                .path()
                .strip_prefix(dir)
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a12_symlink_nested_skill_into_three_targets() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let plan = plan_deploy(&fx.request(DeploymentMode::Symlink, fx.three()), &manifest);
    assert!(plan.is_applicable(), "{}", plan.summary());
    let links = plan
        .operations
        .iter()
        .filter(|op| matches!(op, PlannedOp::CreateSymlink { .. }))
        .count();
    assert_eq!(links, 3);
    assert!(
        !fx.home_dir().join(".claude").exists(),
        "planning must not write"
    );

    let report = fx.executor().apply(&plan, &mut manifest).unwrap();
    assert_eq!(report.status, ApplyStatus::Applied);
    for target in ["claude-code", "cursor", "gemini-cli"] {
        let path = fx.skill_path(target);
        assert_eq!(fs::read_link(&path).unwrap(), fx.source);
        assert!(
            fs::read_to_string(path.join("SKILL.md"))
                .unwrap()
                .contains("# My skill")
        );
        assert_eq!(
            status_of(manifest.entry(&path).unwrap()),
            DeploymentStatus::Linked
        );
    }
    assert_eq!(Manifest::load(&fx.manifest_path).unwrap(), manifest);
    assert_eq!(manifest.entries.len(), 3);
}

#[test]
fn replan_is_idempotent_and_already_satisfied() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::Symlink, &mut manifest);
    let plan = plan_deploy(&fx.request(DeploymentMode::Symlink, fx.three()), &manifest);
    assert!(plan.is_applicable());
    assert!(plan.is_noop(), "{}", plan.summary());
    assert!(
        plan.operations
            .iter()
            .all(|op| matches!(op, PlannedOp::AlreadySatisfied { owned: true, .. }))
    );
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert_eq!(manifest.entries.len(), 3);
}

#[test]
fn a13_independent_copies_detect_divergence() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::IndependentCopy, &mut manifest);
    let source_fp = fingerprint_tree(&fx.source).unwrap();
    for target in ["claude-code", "cursor", "gemini-cli"] {
        let path = fx.skill_path(target);
        assert!(fs::symlink_metadata(&path).unwrap().is_dir());
        assert_eq!(fingerprint_tree(&path).unwrap(), source_fp);
        assert_eq!(
            fs::read_link(path.join("guide-link.md")).unwrap(),
            Path::new("references/guide.md")
        );
        assert_eq!(
            status_of(manifest.entry(&path).unwrap()),
            DeploymentStatus::Copy
        );
        assert!(
            !listing(path.parent().unwrap())
                .iter()
                .any(|p| p.contains(".perkolator-"))
        );
    }

    let edited = fx.skill_path("cursor");
    fs::write(edited.join("SKILL.md"), "locally edited").unwrap();
    assert_eq!(
        status_of(manifest.entry(&edited).unwrap()),
        DeploymentStatus::Modified
    );
    assert_eq!(
        status_of(manifest.entry(&fx.skill_path("claude-code")).unwrap()),
        DeploymentStatus::Copy
    );

    let plan = plan_deploy(
        &fx.request(DeploymentMode::IndependentCopy, fx.three()),
        &manifest,
    );
    assert_eq!(
        plan.conflicts,
        vec![Conflict::DivergedCopy {
            path: edited.clone()
        }]
    );
    assert!(matches!(
        fx.executor().apply(&plan, &mut manifest),
        Err(DeployError::NotApplicable(_))
    ));
    let remove = plan_remove(&edited, &manifest);
    assert_eq!(
        remove.conflicts,
        vec![Conflict::DivergedCopy {
            path: edited.clone()
        }]
    );
    assert_eq!(
        fs::read_to_string(edited.join("SKILL.md")).unwrap(),
        "locally edited"
    );
}

#[test]
fn updated_source_refreshes_unmodified_owned_copies() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::IndependentCopy, &mut manifest);
    fs::write(fx.source.join("references/new.md"), "new").unwrap();
    let plan = plan_deploy(
        &fx.request(DeploymentMode::IndependentCopy, fx.three()),
        &manifest,
    );
    assert!(plan.is_applicable(), "{}", plan.summary());
    assert_eq!(
        plan.operations
            .iter()
            .filter(|op| matches!(op, PlannedOp::RemoveOwnedCopy { .. }))
            .count(),
        3
    );
    fx.executor().apply(&plan, &mut manifest).unwrap();
    for target in ["claude-code", "cursor", "gemini-cli"] {
        let path = fx.skill_path(target);
        assert_eq!(
            fs::read_to_string(path.join("references/new.md")).unwrap(),
            "new"
        );
        assert_eq!(
            status_of(manifest.entry(&path).unwrap()),
            DeploymentStatus::Copy
        );
        assert!(
            !listing(path.parent().unwrap())
                .iter()
                .any(|p| p.contains(".perkolator-"))
        );
    }
}

#[test]
fn a14_unmanaged_directory_is_never_overwritten_or_removed() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let existing = fx.skill_path("claude-code");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("SKILL.md"), "user's own").unwrap();

    for mode in [DeploymentMode::Symlink, DeploymentMode::IndependentCopy] {
        let plan = plan_deploy(&fx.request(mode, fx.three()), &manifest);
        assert!(has_conflict(&plan.conflicts, |c| matches!(
            c,
            Conflict::UnmanagedExists { path, .. } if *path == existing
        )));
        assert!(matches!(
            fx.executor().apply(&plan, &mut manifest),
            Err(DeployError::NotApplicable(_))
        ));
    }
    assert!(
        plan_remove(&existing, &manifest)
            .conflicts
            .contains(&Conflict::NotManaged {
                path: existing.clone()
            })
    );
    assert_eq!(
        fs::read_to_string(existing.join("SKILL.md")).unwrap(),
        "user's own"
    );
}

#[test]
fn a14_dir_created_after_planning_aborts_and_rolls_back() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let plan = plan_deploy(&fx.request(DeploymentMode::Symlink, fx.three()), &manifest);
    assert!(plan.is_applicable());
    // Race: the user creates a real dir in a later destination after planning.
    let raced = fx.skill_path("gemini-cli");
    fs::create_dir_all(&raced).unwrap();
    fs::write(raced.join("notes.txt"), "keep").unwrap();

    let error = fx.executor().apply(&plan, &mut manifest).unwrap_err();
    let DeployError::OperationFailed(report) = error else {
        panic!("expected OperationFailed, got {error:?}");
    };
    assert!(
        matches!(report.status, ApplyStatus::RolledBack { .. }),
        "{report:?}"
    );
    assert_eq!(fs::read_to_string(raced.join("notes.txt")).unwrap(), "keep");
    assert!(!fx.home_dir().join(".claude").exists());
    assert!(!fx.home_dir().join(".cursor").exists());
    assert!(manifest.entries.is_empty());
    assert!(!fx.manifest_path.exists());
}

#[test]
fn a15_owned_broken_link_repaired_foreign_broken_link_refused() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::Symlink, &mut manifest);
    let claude = fx.skill_path("claude-code");

    // Library reorganised: canonical source moved.
    let moved = fx.root.join("library/engineering/my-skill");
    fs::rename(&fx.source, &moved).unwrap();
    let moved_fp = fingerprint_tree(&moved).unwrap();
    assert_eq!(
        status_of(manifest.entry(&claude).unwrap()),
        DeploymentStatus::Broken
    );

    let plan = plan_repair(&claude, &manifest, Some(&moved));
    assert!(plan.is_applicable(), "{}", plan.summary());
    assert!(matches!(
        plan.operations[..],
        [PlannedOp::ReplaceOwnedSymlink { .. }]
    ));
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert_eq!(fs::read_link(&claude).unwrap(), moved);
    assert!(
        fs::read_to_string(claude.join("SKILL.md"))
            .unwrap()
            .contains("# My skill")
    );
    assert_eq!(
        status_of(manifest.entry(&claude).unwrap()),
        DeploymentStatus::Linked
    );
    assert_eq!(manifest.entry(&claude).unwrap().source, moved);
    assert_eq!(
        fingerprint_tree(&moved).unwrap(),
        moved_fp,
        "target data untouched"
    );

    // Owned link deleted: repair recreates it with the recorded target.
    fs::remove_file(&claude).unwrap();
    let plan = plan_repair(&claude, &manifest, None);
    assert!(matches!(
        plan.operations[..],
        [PlannedOp::CreateSymlink { .. }]
    ));
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert_eq!(
        status_of(manifest.entry(&claude).unwrap()),
        DeploymentStatus::Linked
    );

    // Foreign broken link: refused by repair and deploy, left untouched.
    let foreign_dir = fx.home_dir().join(".copilot/skills");
    fs::create_dir_all(&foreign_dir).unwrap();
    let foreign = foreign_dir.join("my-skill");
    symlink("/nonexistent/elsewhere", &foreign).unwrap();
    let plan = plan_repair(&foreign, &manifest, Some(&moved));
    assert!(matches!(
        plan.conflicts[..],
        [Conflict::BrokenForeignSymlink { .. }]
    ));
    assert!(fx.executor().apply(&plan, &mut manifest).is_err());
    let mut request = fx.request(DeploymentMode::Symlink, vec![fx.dest("github-copilot")]);
    request.source = moved.clone();
    let plan = plan_deploy(&request, &manifest);
    assert!(matches!(
        plan.conflicts[..],
        [Conflict::BrokenForeignSymlink { .. }]
    ));
    assert_eq!(
        fs::read_link(&foreign).unwrap(),
        Path::new("/nonexistent/elsewhere")
    );
}

#[test]
fn a16_crash_mid_run_then_recover_restores_consistency() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let plan = plan_deploy(&fx.request(DeploymentMode::Symlink, fx.three()), &manifest);
    let total = plan.operations.len();
    assert!(total >= 6);
    let before = listing(fx.home_dir());

    let crash = fx
        .executor()
        .with_fault(FaultInjection::CrashAfterSteps(total - 2));
    assert!(matches!(
        crash.apply(&plan, &mut manifest),
        Err(DeployError::SimulatedCrash(_))
    ));
    assert_ne!(listing(fx.home_dir()), before, "crash left partial state");
    assert!(
        !fx.manifest_path.exists(),
        "manifest not committed before crash"
    );

    let mut loaded = Manifest::load(&fx.manifest_path).unwrap();
    let report = fx.executor().recover(&mut loaded).unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(report.runs.len(), 1);
    assert_eq!(listing(fx.home_dir()), before);
    assert!(loaded.entries.is_empty());

    // Idempotent: nothing left to recover.
    assert!(fx.executor().recover(&mut loaded).unwrap().runs.is_empty());
    // And a fresh apply works afterwards.
    let plan = plan_deploy(&fx.request(DeploymentMode::Symlink, fx.three()), &loaded);
    fx.executor().apply(&plan, &mut loaded).unwrap();
    assert_eq!(loaded.entries.len(), 3);
}

#[test]
fn a16_crash_mid_copy_cleans_staging() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let plan = plan_deploy(
        &fx.request(DeploymentMode::IndependentCopy, fx.three()),
        &manifest,
    );
    let crash = fx.executor().with_fault(FaultInjection::CrashMidCopy);
    assert!(matches!(
        crash.apply(&plan, &mut manifest),
        Err(DeployError::SimulatedCrash(_))
    ));
    let skills = fx.dest("claude-code").skills_dir;
    assert!(
        listing(&skills)
            .iter()
            .any(|p| p.starts_with(".perkolator-staging-"))
    );

    let report = fx.executor().recover(&mut manifest).unwrap();
    assert!(report.is_clean(), "{report:?}");
    assert!(!fx.home_dir().join(".claude").exists());
    assert_eq!(listing(fx.home_dir()), vec![String::new()]);
}

#[test]
fn a16_crash_after_manifest_commit_finalizes_instead_of_rolling_back() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::Symlink, &mut manifest);
    // Emulate a crash after the manifest commit: drop the journal's End line.
    let journal = fs::read_dir(&fx.journal_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = fs::read_to_string(&journal).unwrap();
    let truncated: Vec<&str> = text
        .lines()
        .filter(|line| !line.contains("\"end\""))
        .collect();
    fs::write(&journal, truncated.join("\n") + "\n").unwrap();

    let mut loaded = Manifest::load(&fx.manifest_path).unwrap();
    let report = fx.executor().recover(&mut loaded).unwrap();
    assert!(report.is_clean(), "{report:?}");
    for target in ["claude-code", "cursor", "gemini-cli"] {
        assert_eq!(
            status_of(loaded.entry(&fx.skill_path(target)).unwrap()),
            DeploymentStatus::Linked
        );
    }
}

#[test]
fn injected_failure_and_cancellation_roll_back() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let plan = plan_deploy(
        &fx.request(DeploymentMode::IndependentCopy, fx.three()),
        &manifest,
    );
    let last = plan.operations.len() - 1;
    let failing = fx.executor().with_fault(FaultInjection::FailStep(last));
    let DeployError::OperationFailed(report) = failing.apply(&plan, &mut manifest).unwrap_err()
    else {
        panic!("expected OperationFailed");
    };
    assert!(matches!(report.status, ApplyStatus::RolledBack { .. }));
    assert_eq!(listing(fx.home_dir()), vec![String::new()]);

    let token = CancelToken::new();
    token.cancel();
    let cancelled = fx.executor().with_cancel(token);
    assert!(matches!(
        cancelled.apply(&plan, &mut manifest),
        Err(DeployError::OperationFailed(_))
    ));
    assert_eq!(listing(fx.home_dir()), vec![String::new()]);
    assert!(
        fx.executor()
            .recover(&mut manifest)
            .unwrap()
            .runs
            .is_empty()
    );
}

#[test]
fn remove_deletes_only_the_deployment_never_the_source() {
    let fx = Fixture::new();
    let source_fp = fingerprint_tree(&fx.source).unwrap();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::Symlink, &mut manifest);
    let claude = fx.skill_path("claude-code");
    let plan = plan_remove(&claude, &manifest);
    assert!(matches!(
        plan.operations[..],
        [PlannedOp::RemoveOwnedSymlink { .. }]
    ));
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert!(fs::symlink_metadata(&claude).is_err());
    assert!(manifest.entry(&claude).is_none());
    assert_eq!(fingerprint_tree(&fx.source).unwrap(), source_fp);

    // Removing a copy moves it aside and deletes it only after commit.
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::IndependentCopy, &mut manifest);
    let cursor = fx.skill_path("cursor");
    let plan = plan_remove(&cursor, &manifest);
    assert!(matches!(
        plan.operations[..],
        [PlannedOp::RemoveOwnedCopy { .. }]
    ));
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert!(!cursor.exists());
    assert_eq!(listing(cursor.parent().unwrap()), vec![String::new()]);
    assert_eq!(fingerprint_tree(&fx.source).unwrap(), source_fp);

    // Already-missing owned entry: manifest-only forget.
    let gemini = fx.skill_path("gemini-cli");
    let trash = fx.root.join("user-trash");
    fs::rename(&gemini, &trash).unwrap();
    let plan = plan_remove(&gemini, &manifest);
    assert!(matches!(
        plan.operations[..],
        [PlannedOp::ForgetEntry { .. }]
    ));
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert!(trash.join("SKILL.md").exists());
}

#[test]
fn self_reference_is_rejected() {
    let fx = Fixture::new();
    let manifest = Manifest::default();
    let inside = Destination {
        target_id: "generic".into(),
        scope: Scope::Custom {
            path: fx.source.join("nested"),
        },
        skills_dir: fx.source.join("nested"),
        supports_symlinks: true,
    };
    let onto_source = Destination {
        target_id: "generic".into(),
        scope: Scope::Custom {
            path: fx.source.parent().unwrap().to_path_buf(),
        },
        skills_dir: fx.source.parent().unwrap().to_path_buf(),
        supports_symlinks: true,
    };
    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![inside, onto_source]),
        &manifest,
    );
    assert_eq!(
        plan.conflicts
            .iter()
            .filter(|c| matches!(c, Conflict::SelfReference { .. }))
            .count(),
        2,
        "{}",
        plan.summary()
    );
    let mut request = fx.request(DeploymentMode::ManagedCopy, fx.three());
    request.managed_root = Some(fx.source.clone());
    assert!(has_conflict(
        &plan_deploy(&request, &manifest).conflicts,
        |c| matches!(c, Conflict::SelfReference { .. })
    ));
}

#[test]
fn outside_approved_roots_and_symlinked_parents_are_rejected() {
    let fx = Fixture::new();
    let manifest = Manifest::default();
    let outside = tempfile::tempdir().unwrap();
    let custom = Destination::for_target(
        fx.registry.get("generic").unwrap(),
        Scope::Custom {
            path: outside.path().join("skills"),
        },
        &fx.home,
    )
    .unwrap();
    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![custom]),
        &manifest,
    );
    assert!(matches!(
        plan.conflicts[..],
        [Conflict::OutsideApprovedRoots { .. }]
    ));

    // ~/.claude/skills is a symlink leading outside the approved root.
    fs::create_dir_all(fx.home_dir().join(".claude")).unwrap();
    symlink(outside.path(), fx.home_dir().join(".claude/skills")).unwrap();
    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![fx.dest("claude-code")]),
        &manifest,
    );
    assert!(
        matches!(plan.conflicts[..], [Conflict::SymlinkedParent { .. }]),
        "{}",
        plan.summary()
    );
    let mut unrestricted = fx.request(DeploymentMode::Symlink, vec![fx.dest("claude-code")]);
    unrestricted.approved_roots.clear();
    assert!(matches!(
        plan_deploy(&unrestricted, &manifest).conflicts[..],
        [Conflict::SymlinkedParent { .. }]
    ));
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[test]
fn symlinked_skills_dir_inside_approved_roots_is_allowed() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fs::create_dir_all(fx.home_dir().join(".agents/skills")).unwrap();
    fs::create_dir_all(fx.home_dir().join(".claude")).unwrap();
    symlink(
        fx.home_dir().join(".agents/skills"),
        fx.home_dir().join(".claude/skills"),
    )
    .unwrap();
    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![fx.dest("claude-code")]),
        &manifest,
    );
    assert!(plan.is_applicable(), "{}", plan.summary());
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert!(
        fx.home_dir()
            .join(".agents/skills/my-skill/SKILL.md")
            .exists()
    );

    // The same physical dir via two targets is a duplicate, not two deploys.
    let plan = plan_deploy(
        &fx.request(
            DeploymentMode::Symlink,
            vec![fx.dest("codex"), fx.dest("universal")],
        ),
        &Manifest::default(),
    );
    assert!(has_conflict(&plan.conflicts, |c| matches!(
        c,
        Conflict::DuplicateDestination { .. }
    )));
}

#[test]
fn foreign_symlink_is_refused_and_untouched() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let other = fx.root.join("library/other-skill");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("SKILL.md"), "other").unwrap();
    let path = fx.skill_path("claude-code");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    symlink(&other, &path).unwrap();

    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![fx.dest("claude-code")]),
        &manifest,
    );
    assert!(matches!(
        plan.conflicts[..],
        [Conflict::ForeignSymlink { .. }]
    ));
    assert!(fx.executor().apply(&plan, &mut manifest).is_err());
    assert!(matches!(
        plan_remove(&path, &manifest).conflicts[..],
        [Conflict::NotManaged { .. }]
    ));
    assert!(matches!(
        plan_repair(&path, &manifest, None).conflicts[..],
        [Conflict::ForeignSymlink { .. }]
    ));
    assert_eq!(fs::read_link(&path).unwrap(), other);

    // An unmanaged link that already points at the source is left unmanaged.
    fs::remove_file(&path).unwrap();
    symlink(&fx.source, &path).unwrap();
    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![fx.dest("claude-code")]),
        &manifest,
    );
    assert!(matches!(
        plan.operations[..],
        [PlannedOp::AlreadySatisfied { owned: false, .. }]
    ));
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert!(manifest.entries.is_empty());
}

#[test]
fn managed_copy_mode() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::ManagedCopy, &mut manifest);
    let managed = fx.root.join("managed/my-skill");
    assert!(fs::symlink_metadata(&managed).unwrap().is_dir());
    assert_eq!(
        fingerprint_tree(&managed).unwrap(),
        fingerprint_tree(&fx.source).unwrap()
    );
    let entry = manifest.entry(&managed).unwrap();
    assert_eq!(entry.target_id, MANAGED_TARGET_ID);
    assert_eq!(status_of(entry), DeploymentStatus::Copy);
    for target in ["claude-code", "cursor", "gemini-cli"] {
        let path = fx.skill_path(target);
        assert_eq!(fs::read_link(&path).unwrap(), managed);
        assert!(path.join("SKILL.md").exists());
        assert_eq!(
            status_of(manifest.entry(&path).unwrap()),
            DeploymentStatus::Linked
        );
    }
    assert!(matches!(
        plan_remove(&managed, &manifest).conflicts[..],
        [Conflict::ManagedPathInUse { dependents: 3, .. }]
    ));
    let replan = plan_deploy(
        &fx.request(DeploymentMode::ManagedCopy, fx.three()),
        &manifest,
    );
    assert!(replan.is_noop(), "{}", replan.summary());
}

#[test]
fn link_source_mode() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    fx.deploy(DeploymentMode::LinkSource, &mut manifest);
    let managed = fx.root.join("managed/my-skill");
    assert_eq!(fs::read_link(&managed).unwrap(), fx.source);
    for target in ["claude-code", "cursor", "gemini-cli"] {
        let path = fx.skill_path(target);
        assert_eq!(fs::read_link(&path).unwrap(), managed);
        assert!(
            fs::read_to_string(path.join("SKILL.md"))
                .unwrap()
                .contains("# My skill")
        );
    }
    assert_eq!(
        status_of(manifest.entry(&managed).unwrap()),
        DeploymentStatus::Linked
    );
}

#[test]
fn manifest_atomic_round_trip() {
    let fx = Fixture::new();
    assert_eq!(
        Manifest::load(&fx.manifest_path).unwrap(),
        Manifest::default()
    );
    let mut manifest = Manifest::default();
    manifest.entries.push(ManifestEntry {
        destination: fx.root.join("d/my-skill"),
        source: fx.source.clone(),
        mode: DeploymentMode::Symlink,
        target_id: "claude-code".into(),
        scope: Scope::Project {
            root: fx.root.join("proj"),
        },
        link_target: Some(fx.source.clone()),
        fingerprint: None,
        applied_at_unix: 1_762_500_000,
    });
    manifest.last_run_id = Some("run-1".into());
    manifest.save(&fx.manifest_path).unwrap();
    assert_eq!(Manifest::load(&fx.manifest_path).unwrap(), manifest);
    manifest.entries.clear();
    manifest.save(&fx.manifest_path).unwrap();
    assert_eq!(Manifest::load(&fx.manifest_path).unwrap(), manifest);
    assert_eq!(
        listing(fx.manifest_path.parent().unwrap()),
        vec!["".to_owned(), "manifest.json".to_owned()]
    );
    fs::write(&fx.manifest_path, "{not json").unwrap();
    assert!(
        Manifest::load(&fx.manifest_path).is_err(),
        "corrupt manifest must not read as empty"
    );
}

#[test]
fn source_and_name_validation() {
    let fx = Fixture::new();
    let manifest = Manifest::default();
    for name in ["../escape", "Upper", ".hidden", "a/b", ""] {
        let mut request = fx.request(DeploymentMode::Symlink, fx.three());
        request.name = name.into();
        assert!(has_conflict(
            &plan_deploy(&request, &manifest).conflicts,
            |c| matches!(c, Conflict::InvalidName { .. })
        ));
    }
    let mut request = fx.request(DeploymentMode::Symlink, fx.three());
    request.source = fx.root.join("missing");
    assert!(matches!(
        plan_deploy(&request, &manifest).conflicts[..],
        [Conflict::SourceMissing { .. }]
    ));
    request.source = fx.source.join("references");
    assert!(matches!(
        plan_deploy(&request, &manifest).conflicts[..],
        [Conflict::SourceNotSkill { .. }]
    ));
    request.source = fx.root.join("library/../library/engineering/rust/my-skill");
    assert!(matches!(
        plan_deploy(&request, &manifest).conflicts[..],
        [Conflict::InvalidPath { .. }]
    ));
    let mut request = fx.request(DeploymentMode::ManagedCopy, fx.three());
    request.managed_root = None;
    assert!(
        plan_deploy(&request, &manifest)
            .conflicts
            .contains(&Conflict::ManagedRootRequired)
    );
}

#[test]
fn source_symlink_loops_and_escapes() {
    let fx = Fixture::new();
    let manifest = Manifest::default();
    symlink("loop", fx.source.join("loop")).unwrap();
    let plan = plan_deploy(&fx.request(DeploymentMode::Symlink, fx.three()), &manifest);
    assert!(
        matches!(plan.conflicts[..], [Conflict::SourceSymlinkLoop { .. }]),
        "{}",
        plan.summary()
    );
    fs::remove_file(fx.source.join("loop")).unwrap();

    symlink("/etc/hostname", fx.source.join("escape")).unwrap();
    symlink("../../outside.md", fx.source.join("references/up")).unwrap();
    assert!(
        plan_deploy(&fx.request(DeploymentMode::Symlink, fx.three()), &manifest).is_applicable()
    );
    let plan = plan_deploy(
        &fx.request(DeploymentMode::IndependentCopy, fx.three()),
        &manifest,
    );
    assert_eq!(
        plan.conflicts
            .iter()
            .filter(|c| matches!(c, Conflict::UnsafeSourceSymlink { .. }))
            .count(),
        2,
        "{}",
        plan.summary()
    );
}

#[test]
fn mode_unsupported_by_target() {
    let fx = Fixture::new();
    let manifest = Manifest::default();
    let mut no_links = fx.dest("cursor");
    no_links.supports_symlinks = false;
    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![no_links.clone()]),
        &manifest,
    );
    assert!(matches!(
        plan.conflicts[..],
        [Conflict::ModeUnsupportedByTarget { .. }]
    ));
    assert!(
        plan_deploy(
            &fx.request(DeploymentMode::IndependentCopy, vec![no_links]),
            &manifest
        )
        .is_applicable()
    );
}

#[test]
fn matrix_reports_per_agent_status() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let plan = plan_deploy(
        &fx.request(
            DeploymentMode::Symlink,
            vec![fx.dest("claude-code"), fx.dest("cursor")],
        ),
        &manifest,
    );
    fx.executor().apply(&plan, &mut manifest).unwrap();
    let other = fx.root.join("library/other");
    fs::create_dir_all(&other).unwrap();
    let gemini_other = fx.dest("gemini-cli").skills_dir.join("other");
    fs::create_dir_all(&gemini_other).unwrap();

    let matrix = deployment_matrix(&manifest, &[fx.source.clone(), other], &fx.three());
    let statuses: Vec<Vec<DeploymentStatus>> = matrix
        .iter()
        .map(|row| row.iter().map(|cell| cell.status).collect())
        .collect();
    use DeploymentStatus::*;
    assert_eq!(
        statuses,
        vec![
            vec![Linked, Missing],
            vec![Linked, Missing],
            vec![Missing, Unmanaged]
        ]
    );
    assert_eq!(matrix[0][0].target_id, "claude-code");
}

#[test]
fn project_scope_destinations_resolve_under_project_root() {
    let fx = Fixture::new();
    let mut manifest = Manifest::default();
    let project = fx.root.join("work/repo");
    fs::create_dir_all(&project).unwrap();
    let scope = Scope::Project {
        root: project.clone(),
    };
    let destination =
        Destination::for_target(fx.registry.get("claude-code").unwrap(), scope, &fx.home).unwrap();
    assert_eq!(destination.skills_dir, project.join(".claude/skills"));
    let plan = plan_deploy(
        &fx.request(DeploymentMode::Symlink, vec![destination]),
        &manifest,
    );
    fx.executor().apply(&plan, &mut manifest).unwrap();
    assert!(project.join(".claude/skills/my-skill/SKILL.md").exists());
}
