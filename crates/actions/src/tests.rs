use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use perkolator_core::{
    ActionAvailability, ActionEffect, ActionId, ActionOutcome, AgentProfileDetails, Capability,
    CapabilitySet, ConfigFormat, EnvBinding, FeatureSupport, InstallState, LocationRole,
    McpServerDetails, McpTransport, PlatformFeature, PluginDetails, PluginHost,
    PromptTemplateDetails, PromptVariable, Provenance, ProviderId, ResourceDetails, ResourceId,
    ResourceLocation, ResourceRecord, RuleDetails, RuleFormat, Scope, SecretString, SkillDetails,
    SourceKind, TemplateSyntax,
};
use tempfile::TempDir;

use crate::*;

const KDE_INSERT_REASON: &str = "No verified session adapter on KDE Wayland";
const LITERAL_SECRET: &str = "ghp_do_not_leak_123";

fn provider() -> ProviderId {
    ProviderId::new("library").unwrap()
}

fn record(name: &str, path: Option<&Path>, details: ResourceDetails) -> ResourceRecord {
    let kind = details.kind();
    let mut capabilities = CapabilitySet::for_details(&details);
    if path.is_none() {
        capabilities = capabilities
            .iter()
            .filter(|cap| *cap != Capability::LocalPath)
            .collect();
    }
    ResourceRecord {
        id: ResourceId::new(
            kind,
            provider(),
            path.map_or_else(|| format!("remote/{name}"), |p| p.display().to_string()),
        ),
        name: name.into(),
        summary: format!("{name} summary"),
        publisher: None,
        tags: vec![],
        version: None,
        provenance: Provenance {
            source: match path {
                Some(path) => SourceKind::Library {
                    root: path.to_path_buf(),
                },
                None => SourceKind::Remote {
                    provider: "catalogue".into(),
                    url: format!("https://example.invalid/{name}"),
                },
            },
            origin_url: None,
            fingerprint: None,
        },
        locations: path
            .map(|path| ResourceLocation {
                path: path.to_path_buf(),
                role: LocationRole::Canonical,
                state: InstallState::Installed,
            })
            .into_iter()
            .collect(),
        capabilities,
        details,
        issues: vec![],
    }
}

fn skill(dir: &Path) -> ResourceRecord {
    let root = dir.join("pdf");
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("SKILL.md");
    fs::write(
        &entry,
        "---\nname: pdf\ndescription: PDFs\n---\n# PDF\nBody\n",
    )
    .unwrap();
    let mut record = record(
        "pdf",
        Some(&root),
        ResourceDetails::Skill(SkillDetails {
            root: root.clone(),
            entry,
            category_path: vec![],
            frontmatter: BTreeMap::new(),
            files: vec![],
        }),
    );
    record.provenance.origin_url = Some("https://github.com/example/pdf".into());
    record.capabilities.insert(Capability::SourceUrl);
    record
}

fn mcp_details(config_file: PathBuf, enabled: Option<bool>) -> McpServerDetails {
    McpServerDetails {
        client: "cursor".into(),
        config_file,
        config_format: ConfigFormat::Json,
        entry: "github".into(),
        transport: McpTransport::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "server-github".into()],
            cwd: None,
            env: vec![EnvBinding {
                name: "GITHUB_TOKEN".into(),
                value: SecretString::new(LITERAL_SECRET),
            }],
        },
        enabled,
        declared_tools: vec![],
    }
}

fn local_mcp(dir: &Path, enabled: Option<bool>) -> ResourceRecord {
    let config = dir.join("mcp.json");
    fs::write(
        &config,
        format!("{{\"mcpServers\":{{\"other\":{{\"env\":{{\"K\":\"{LITERAL_SECRET}\"}}}}}}}}"),
    )
    .unwrap();
    let mut record = record(
        "github",
        Some(&config),
        ResourceDetails::McpServer(mcp_details(config.clone(), enabled)),
    );
    record.locations[0].role = LocationRole::ConfigEntry {
        file: config,
        entry: "github".into(),
    };
    record
}

fn pathless_mcp() -> ResourceRecord {
    record(
        "github",
        None,
        ResourceDetails::McpServer(mcp_details(PathBuf::from("mcp.json"), None)),
    )
}

fn var(name: &str, default: Option<&str>, required: bool) -> PromptVariable {
    PromptVariable {
        name: name.into(),
        description: None,
        default: default.map(Into::into),
        required,
    }
}

fn prompt(
    dir: &Path,
    text: &str,
    syntax: TemplateSyntax,
    variables: Vec<PromptVariable>,
) -> ResourceRecord {
    let file = dir.join("review.md");
    fs::write(&file, text).unwrap();
    record(
        "review",
        Some(&file),
        ResourceDetails::Prompt(PromptTemplateDetails {
            file: file.clone(),
            host: None,
            syntax,
            variables,
        }),
    )
}

fn pathless_prompt() -> ResourceRecord {
    record(
        "review",
        None,
        ResourceDetails::Prompt(PromptTemplateDetails {
            file: PathBuf::from("review.md"),
            host: None,
            syntax: TemplateSyntax::Plain,
            variables: vec![],
        }),
    )
}

fn agent(dir: &Path) -> ResourceRecord {
    let file = dir.join("reviewer.md");
    fs::write(&file, "---\nname: reviewer\n---\nReview code.\n").unwrap();
    record(
        "reviewer",
        Some(&file),
        ResourceDetails::Agent(AgentProfileDetails {
            host: Some("claude-code".into()),
            file: file.clone(),
            model: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
        }),
    )
}

fn rule(dir: &Path, always_apply: Option<bool>) -> ResourceRecord {
    let file = dir.join("AGENTS.md");
    fs::write(&file, "# Rules\nBe concise.\n").unwrap();
    record(
        "agents-md",
        Some(&file),
        ResourceDetails::Rule(RuleDetails {
            file: file.clone(),
            format: RuleFormat::AgentsMd,
            host: None,
            scope: Scope::Global,
            description: None,
            always_apply,
            globs: vec![],
            precedence: None,
        }),
    )
}

fn plugin(dir: &Path) -> ResourceRecord {
    let root = dir.join("plugin");
    fs::create_dir_all(&root).unwrap();
    let manifest = root.join("plugin.json");
    fs::write(&manifest, "{\"name\":\"p\"}").unwrap();
    record(
        "p",
        Some(&root),
        ResourceDetails::Plugin(PluginDetails {
            manifest,
            root: root.clone(),
            host: PluginHost::External {
                name: "cursor".into(),
            },
            components: vec![],
            permissions: vec![],
            executable_files: vec![],
            content_hash: None,
            files: vec![],
        }),
    )
}

fn kde_ctx() -> ActionContext {
    ActionContext::new(Surface::Launcher)
        .with_feature(PlatformFeature::ClipboardWrite, FeatureSupport::Supported)
        .with_feature(
            PlatformFeature::AgentSessionInsert,
            FeatureSupport::Unavailable {
                explanation: KDE_INSERT_REASON.into(),
            },
        )
}

fn session(app: &str, at: SystemTime) -> SessionTarget {
    SessionTarget {
        app_id: app.into(),
        label: "Claude Code — perkolator".into(),
        agent: Some("claude-code".into()),
        project_root: Some(PathBuf::from("/work/perkolator")),
        captured_at: at,
    }
}

fn insert_ctx(captured: SessionTarget, current: SessionTarget) -> ActionContext {
    ActionContext::new(Surface::Launcher)
        .with_feature(
            PlatformFeature::AgentSessionInsert,
            FeatureSupport::Supported,
        )
        .with_session(captured)
        .with_current_target(current)
}

fn ids(entries: &[ActionEntry]) -> Vec<ActionId> {
    entries.iter().map(|entry| entry.id).collect()
}

fn entry(entries: &[ActionEntry], id: ActionId) -> &ActionEntry {
    entries.iter().find(|entry| entry.id == id).unwrap()
}

fn clipboard_text(effect: ActionEffect) -> String {
    match effect {
        ActionEffect::WriteClipboard(payload) => payload.text,
        other => panic!("expected clipboard effect, got {other:?}"),
    }
}

const PATH_ACTIONS: [ActionId; 4] = [
    ActionId::CopyPath,
    ActionId::RevealInFileManager,
    ActionId::CopyDeployedPath,
    ActionId::CopyContent,
];

#[test]
fn u10_pathless_resources_never_get_path_actions() {
    let ctx = kde_ctx();
    for record in [pathless_mcp(), pathless_prompt()] {
        let listed = ids(&actions_for(&record, &ctx));
        for id in PATH_ACTIONS {
            assert!(!listed.contains(&id), "{id:?} offered for {}", record.id);
        }
        assert!(matches!(
            resolve(ActionId::CopyPath, &record, &ctx, &ActionInputs::default()),
            Err(ActionError::NotOffered { .. })
        ));
    }
    let dir = TempDir::new().unwrap();
    let skill = skill(dir.path());
    let listed = ids(&actions_for(&skill, &ctx));
    assert!(listed.contains(&ActionId::CopyPath));
    assert!(listed.contains(&ActionId::RevealInFileManager));
    let path = clipboard_text(
        resolve(ActionId::CopyPath, &skill, &ctx, &ActionInputs::default()).unwrap(),
    );
    assert_eq!(path, dir.path().join("pdf").display().to_string());
}

#[test]
fn mcp_path_is_labelled_as_config_file_and_content_is_never_copied() {
    let dir = TempDir::new().unwrap();
    let mcp = local_mcp(dir.path(), None);
    let entries = actions_for(&mcp, &kde_ctx());
    assert_eq!(
        entry(&entries, ActionId::CopyPath).label,
        "Copy config file path"
    );
    assert!(!ids(&entries).contains(&ActionId::CopyContent));
    assert!(!ids(&entries).contains(&ActionId::PlanDeploy));
}

#[test]
fn insert_is_disabled_with_actionable_reason_when_unsupported() {
    let dir = TempDir::new().unwrap();
    let skill = skill(dir.path());
    let entries = actions_for(&skill, &kde_ctx());
    assert_eq!(
        entry(&entries, ActionId::InsertIntoSession).availability,
        ActionAvailability::Disabled {
            reason: format!("{KDE_INSERT_REASON} — use Copy invocation"),
        }
    );
    assert!(matches!(
        resolve(
            ActionId::InsertIntoSession,
            &skill,
            &kde_ctx(),
            &ActionInputs::default()
        ),
        Err(ActionError::Unavailable { .. })
    ));

    let unknown = ActionContext::new(Surface::Launcher);
    let reason =
        match &entry(&actions_for(&skill, &unknown), ActionId::InsertIntoSession).availability {
            ActionAvailability::Disabled { reason } => reason.clone(),
            other => panic!("expected disabled, got {other:?}"),
        };
    assert!(reason.contains("not been verified"), "{reason}");

    let no_session = ActionContext::new(Surface::Launcher).with_feature(
        PlatformFeature::AgentSessionInsert,
        FeatureSupport::Supported,
    );
    assert_eq!(
        entry(
            &actions_for(&skill, &no_session),
            ActionId::InsertIntoSession
        )
        .availability,
        ActionAvailability::Disabled {
            reason: "No agent session was captured when Perkolator opened — use Copy invocation"
                .into(),
        }
    );

    let now = SystemTime::now();
    let ready = insert_ctx(session("claude", now), session("claude", now));
    assert!(entry(&actions_for(&skill, &ready), ActionId::InsertIntoSession).is_available());
}

#[test]
fn copy_config_redacts_unless_secrets_explicitly_included() {
    let dir = TempDir::new().unwrap();
    let mcp = local_mcp(dir.path(), None);
    let ctx = kde_ctx();
    let effect = resolve(ActionId::CopyConfig, &mcp, &ctx, &ActionInputs::default()).unwrap();
    let ActionEffect::WriteClipboard(payload) = effect else {
        panic!("expected clipboard");
    };
    assert!(!payload.text.contains(LITERAL_SECRET));
    assert!(payload.text.contains("\"mcpServers\""));
    assert_eq!(payload.label, "MCP configuration (secrets redacted)");
    assert!(!format!("{:?}", actions_for(&mcp, &ctx)).contains(LITERAL_SECRET));

    let with_secrets = ActionInputs {
        include_secrets: true,
        ..ActionInputs::default()
    };
    let text = clipboard_text(resolve(ActionId::CopyConfig, &mcp, &ctx, &with_secrets).unwrap());
    assert!(text.contains(LITERAL_SECRET));
}

#[test]
fn expand_template_through_resolve() {
    let dir = TempDir::new().unwrap();
    let record = prompt(
        dir.path(),
        "---\ndescription: review\n---\nReview {{ file }} in {{lang}}.",
        TemplateSyntax::DoubleBrace,
        vec![var("file", None, true), var("lang", Some("Rust"), false)],
    );
    let ctx = kde_ctx();
    let error = resolve(
        ActionId::ExpandTemplate,
        &record,
        &ctx,
        &ActionInputs::default(),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "missing required template values: file");

    let mut inputs = ActionInputs::default();
    inputs
        .template_values
        .insert("file".into(), "{{lang}}.rs".into());
    let text = clipboard_text(resolve(ActionId::ExpandTemplate, &record, &ctx, &inputs).unwrap());
    assert_eq!(text, "Review {{lang}}.rs in Rust.");
}

#[test]
fn arguments_prompt_expands_through_resolve() {
    let dir = TempDir::new().unwrap();
    let record = prompt(
        dir.path(),
        "Fix $ARGUMENTS",
        TemplateSyntax::Arguments,
        vec![],
    );
    assert_eq!(default_action(&record), ActionId::ExpandTemplate);
    let mut inputs = ActionInputs::default();
    inputs
        .template_values
        .insert("ARGUMENTS".into(), "issue 7".into());
    let text =
        clipboard_text(resolve(ActionId::ExpandTemplate, &record, &kde_ctx(), &inputs).unwrap());
    assert_eq!(text, "Fix issue 7");
}

#[test]
fn compose_payload_exact_text() {
    let dir = TempDir::new().unwrap();
    let skill = skill(dir.path());
    let rule = rule(dir.path(), None);
    let skill_md = dir.path().join("pdf").join("SKILL.md");
    let agents_md = dir.path().join("AGENTS.md");

    assert_eq!(
        compose_payload(
            &[&skill],
            "  Summarise report.pdf  ",
            ComposeFormat::Generic
        ),
        format!(
            "Use the `pdf` skill: {}\n\nSummarise report.pdf",
            skill_md.display()
        )
    );
    assert_eq!(
        compose_payload(&[&skill, &rule, &skill], "Do it", ComposeFormat::Generic),
        format!(
            "Use the `pdf` skill: {}\nApply the `agents-md` rule: {}\n\nDo it",
            skill_md.display(),
            agents_md.display()
        )
    );
    assert_eq!(
        compose_payload(&[&skill, &rule], "Do it", ComposeFormat::PathReferences),
        format!("{}\n{}\n\nDo it", skill_md.display(), agents_md.display())
    );
    assert_eq!(
        compose_payload(&[&skill], "   ", ComposeFormat::Generic),
        format!("Use the `pdf` skill: {}", skill_md.display())
    );
    assert_eq!(
        compose_payload(&[&pathless_mcp()], "q", ComposeFormat::PathReferences),
        "MCP Server `github`\n\nq"
    );
}

#[test]
fn compose_action_enters_composer_or_builds_payload() {
    let dir = TempDir::new().unwrap();
    let skill = skill(dir.path());
    let ctx = kde_ctx();
    assert_eq!(
        resolve(ActionId::Compose, &skill, &ctx, &ActionInputs::default()).unwrap(),
        ActionEffect::EnterCompose {
            id: skill.id.clone()
        }
    );
    let inputs = ActionInputs {
        compose_text: Some("hello".into()),
        ..ActionInputs::default()
    };
    let text = clipboard_text(resolve(ActionId::Compose, &skill, &ctx, &inputs).unwrap());
    assert!(text.ends_with("\n\nhello"));
    let cli = ActionContext::new(Surface::Cli);
    assert!(!entry(&actions_for(&skill, &cli), ActionId::Compose).is_available());
    assert_eq!(
        entry(&actions_for(&skill, &ctx), ActionId::Compose).shortcut_hint,
        Some("Tab")
    );
}

#[test]
fn deliver_never_claims_insert_when_unsupported_or_stale() {
    let effect = compose_effect(&[&pathless_mcp()], "q", ComposeFormat::Generic);
    let now = SystemTime::now();

    // A10: unsupported platform → truthful clipboard fallback.
    let delivery = deliver(&effect, &kde_ctx(), true).unwrap();
    assert!(matches!(
        &delivery,
        Delivery::ClipboardFallback { reason, .. } if reason == KDE_INSERT_REASON
    ));
    assert!(matches!(
        delivery.outcome(),
        ActionOutcome::CopiedAsFallback { .. }
    ));

    // A09: stale captured target is not silently used.
    let stale = insert_ctx(
        session("claude", now - Duration::from_secs(3600)),
        session("claude", now),
    );
    let delivery = deliver(&effect, &stale, true).unwrap();
    assert!(
        matches!(&delivery, Delivery::ClipboardFallback { reason, .. } if reason.contains("captured 3600s ago")),
        "{delivery:?}"
    );

    let switched = insert_ctx(session("claude", now), session("org.kde.konsole", now));
    assert!(matches!(
        deliver(&effect, &switched, true).unwrap(),
        Delivery::ClipboardFallback { .. }
    ));

    let mut unchecked = insert_ctx(session("claude", now), session("claude", now));
    unchecked.current_target = None;
    assert!(matches!(
        deliver(&effect, &unchecked, true).unwrap(),
        Delivery::ClipboardFallback { .. }
    ));

    let valid = insert_ctx(session("claude", now), session("claude", now));
    let delivery = deliver(&effect, &valid, true).unwrap();
    assert!(matches!(delivery, Delivery::Insert { .. }));
    assert_eq!(
        delivery.outcome(),
        ActionOutcome::Inserted {
            target: "Claude Code — perkolator".into()
        }
    );

    assert!(matches!(
        deliver(&effect, &valid, false).unwrap(),
        Delivery::Clipboard { .. }
    ));
    let details = ActionEffect::OpenDetails {
        id: pathless_mcp().id,
    };
    assert!(deliver(&details, &valid, true).is_none());
}

#[test]
fn validate_session_detects_changes() {
    let now = SystemTime::now();
    let base = session("claude", now);
    let max = DEFAULT_SESSION_MAX_AGE;
    assert!(validate_session(&base, &base, max).is_ok());

    let mut moved = base.clone();
    moved.project_root = Some(PathBuf::from("/work/other"));
    assert!(matches!(
        validate_session(&base, &moved, max),
        Err(StaleTarget::ProjectChanged { .. })
    ));

    let mut other_agent = base.clone();
    other_agent.agent = Some("codex".into());
    assert!(matches!(
        validate_session(&base, &other_agent, max),
        Err(StaleTarget::AgentChanged { .. })
    ));

    let mut earlier = base.clone();
    earlier.captured_at = now - Duration::from_secs(5);
    assert_eq!(
        validate_session(&base, &earlier, max),
        Err(StaleTarget::ClockSkew)
    );
}

#[test]
fn default_action_per_kind_is_first_entry() {
    let dir = TempDir::new().unwrap();
    let plain_dir = dir.path().join("plain");
    fs::create_dir_all(&plain_dir).unwrap();
    let cases = [
        (skill(dir.path()), ActionId::CopyInvocation),
        (local_mcp(dir.path(), None), ActionId::CopyConfig),
        (agent(dir.path()), ActionId::CopyInvocation),
        (
            prompt(
                dir.path(),
                "Hi {{who}}",
                TemplateSyntax::DoubleBrace,
                vec![var("who", Some("you"), false)],
            ),
            ActionId::ExpandTemplate,
        ),
        (
            prompt(&plain_dir, "Hi", TemplateSyntax::Plain, vec![]),
            ActionId::CopyContent,
        ),
        (rule(dir.path(), None), ActionId::CopyContent),
        (plugin(dir.path()), ActionId::ShowDetails),
        (pathless_prompt(), ActionId::ShowDetails),
    ];
    let ctx = kde_ctx();
    for (record, expected) in cases {
        assert_eq!(default_action(&record), expected, "{}", record.id);
        let entries = actions_for(&record, &ctx);
        assert_eq!(entries[0].id, expected);
        assert_eq!(entries[0].shortcut_hint, Some("Enter"));
    }
}

#[test]
fn mutating_actions_only_open_the_planner() {
    let dir = TempDir::new().unwrap();
    let mut skill = skill(dir.path());
    skill.locations.push(ResourceLocation {
        path: dir.path().join("agent-root").join("pdf"),
        role: LocationRole::Deployed {
            target: "claude-code".into(),
            scope: Scope::Global,
        },
        state: InstallState::Broken,
    });
    let records = [
        skill,
        local_mcp(dir.path(), Some(true)),
        rule(dir.path(), Some(false)),
    ];
    let ctx = kde_ctx();
    let mut seen = Vec::new();
    for record in &records {
        for entry in actions_for(record, &ctx) {
            if !entry.id.is_mutating() {
                continue;
            }
            seen.push(entry.id);
            assert_eq!(
                resolve(entry.id, record, &ctx, &ActionInputs::default()).unwrap(),
                ActionEffect::OpenDeployPlanner {
                    id: record.id.clone()
                },
                "{:?}",
                entry.id
            );
        }
    }
    for id in [
        ActionId::PlanDeploy,
        ActionId::RepairLink,
        ActionId::RemoveDeployment,
        ActionId::Disable,
        ActionId::Enable,
    ] {
        assert!(seen.contains(&id), "{id:?} never offered");
    }
    let mcp_ids = ids(&actions_for(&records[1], &ctx));
    assert!(mcp_ids.contains(&ActionId::Disable) && !mcp_ids.contains(&ActionId::Enable));
}

#[test]
fn copy_content_reads_whole_file_with_limit() {
    let dir = TempDir::new().unwrap();
    let skill = skill(dir.path());
    let ctx = kde_ctx();
    let effect = resolve(
        ActionId::CopyContent,
        &skill,
        &ctx,
        &ActionInputs::default(),
    )
    .unwrap();
    let ActionEffect::WriteClipboard(payload) = effect else {
        panic!("expected clipboard");
    };
    assert_eq!(payload.label, "SKILL.md contents");
    assert!(payload.text.starts_with("---\nname: pdf"));

    let big = dir.path().join("big.md");
    fs::write(&big, vec![b'a'; 2048]).unwrap();
    assert!(matches!(
        read_text_bounded(&big, 1024),
        Err(ActionError::TooLarge { size: 2048, .. })
    ));
    let binary = dir.path().join("bin.md");
    fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
    assert!(matches!(
        read_text_bounded(&binary, 1024),
        Err(ActionError::NotText { .. })
    ));
    assert!(read_text_bounded(dir.path(), 1024).is_err());
}

#[test]
fn source_url_requires_known_origin() {
    let dir = TempDir::new().unwrap();
    let mut skill = skill(dir.path());
    let ctx = kde_ctx();
    assert_eq!(
        clipboard_text(
            resolve(
                ActionId::CopySourceUrl,
                &skill,
                &ctx,
                &ActionInputs::default()
            )
            .unwrap()
        ),
        "https://github.com/example/pdf"
    );
    skill.provenance.origin_url = None;
    assert!(!ids(&actions_for(&skill, &ctx)).contains(&ActionId::CopySourceUrl));
}

#[test]
fn clipboard_unavailable_disables_copy_actions_with_reason() {
    let dir = TempDir::new().unwrap();
    let skill = skill(dir.path());
    let ctx = ActionContext::new(Surface::Manager).with_feature(
        PlatformFeature::ClipboardWrite,
        FeatureSupport::Unavailable {
            explanation: "No clipboard portal".into(),
        },
    );
    let entries = actions_for(&skill, &ctx);
    assert_eq!(
        entry(&entries, ActionId::CopyPath).availability,
        ActionAvailability::Disabled {
            reason: "No clipboard portal".into()
        }
    );
    assert!(entry(&entries, ActionId::ShowDetails).is_available());
}

#[test]
fn invocation_and_reference_are_adapter_neutral() {
    let dir = TempDir::new().unwrap();
    let agent = agent(dir.path());
    let ctx = kde_ctx();
    let invocation = clipboard_text(
        resolve(
            ActionId::CopyInvocation,
            &agent,
            &ctx,
            &ActionInputs::default(),
        )
        .unwrap(),
    );
    assert_eq!(
        invocation,
        format!(
            "Use the `reviewer` agent profile: {}",
            dir.path().join("reviewer.md").display()
        )
    );
    assert!(!invocation.starts_with('/') && !invocation.starts_with('@'));
    let reference = clipboard_text(
        resolve(
            ActionId::CopyReference,
            &agent,
            &ctx,
            &ActionInputs::default(),
        )
        .unwrap(),
    );
    assert!(reference.starts_with("Agent `reviewer`: "));
}
