use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use perkolator_core::{
    AgentProfileDetails, CancelToken, CapabilitySet, ConfigFormat, EnvBinding, FileKind,
    InstallState, LocationRole, McpServerDetails, McpToolDecl, McpTransport, PluginDetails,
    PluginHost, PromptTemplateDetails, PromptVariable, Provenance, ProviderId, RelPath,
    ResourceDetails, ResourceId, ResourceLocation, ResourceRecord, RuleDetails, RuleFormat, Scope,
    SecretString, SkillDetails, SourceKind, TemplateSyntax,
};
use perkolator_preview::{
    Document, PreviewError, PreviewLimits, PreviewScheduler, PreviewTab, build_preview, file_tree,
    preview_file, read_confined,
};
use serde_json::json;
use tempfile::TempDir;

fn write(root: &Path, rel: &str, contents: impl AsRef<[u8]>) -> PathBuf {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

fn rel(value: &str) -> RelPath {
    RelPath::new(value).unwrap()
}

fn record(name: &str, path: &Path, details: ResourceDetails) -> ResourceRecord {
    ResourceRecord {
        id: ResourceId::new(
            details.kind(),
            ProviderId::new("library").unwrap(),
            path.display().to_string(),
        ),
        name: name.to_owned(),
        summary: String::new(),
        publisher: None,
        tags: vec!["test".into()],
        version: None,
        provenance: Provenance {
            source: SourceKind::Library {
                root: path.to_path_buf(),
            },
            origin_url: None,
            fingerprint: None,
        },
        locations: vec![ResourceLocation {
            path: path.to_path_buf(),
            role: LocationRole::Canonical,
            state: InstallState::Installed,
        }],
        capabilities: CapabilitySet::for_details(&details),
        details,
        issues: Vec::new(),
    }
}

fn skill_fixture() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("pdf-tools");
    write(
        &root,
        "SKILL.md",
        "---\nname: pdf-tools\ndescription: Work with PDFs\n---\n# PDF tools\n\nSee [forms](references/forms.md), \
         [deep](references/deep/c.md#usage), [escape](../outside.md), [abs](/etc/passwd) and \
         [site](https://example.invalid).\n\n## Usage\n",
    );
    write(
        &root,
        "references/forms.md",
        "# Forms\n[back](../SKILL.md)\n",
    );
    write(&root, "references/b.md", "b");
    write(&root, "references/deep/c.md", "c");
    write(&root, "scripts/run.sh", "#!/bin/sh\necho never-executed\n");
    write(&root, "assets/logo.png", [0x89, b'P', b'N', b'G']);
    write(&root, "Alpha.md", "alpha");
    write(&root, "z.txt", "z");
    (dir, root)
}

fn skill_record(root: &Path) -> ResourceRecord {
    let mut frontmatter = BTreeMap::new();
    frontmatter.insert("name".to_owned(), json!("pdf-tools"));
    frontmatter.insert("allowed-tools".to_owned(), json!(["Read", "Bash"]));
    record(
        "pdf-tools",
        root,
        ResourceDetails::Skill(SkillDetails {
            root: root.to_path_buf(),
            entry: root.join("SKILL.md"),
            category_path: vec!["documents".into()],
            frontmatter,
            files: Vec::new(),
        }),
    )
}

fn tab_names(tabs: &[PreviewTab]) -> Vec<&'static str> {
    tabs.iter()
        .map(|tab| match tab {
            PreviewTab::Overview(_) => "overview",
            PreviewTab::Files(_) => "files",
            PreviewTab::Config { .. } => "config",
            PreviewTab::Tools(_) => "tools",
            PreviewTab::Template { .. } => "template",
            PreviewTab::Precedence(_) => "precedence",
            PreviewTab::Permissions { .. } => "permissions",
            PreviewTab::Metadata(_) => "metadata",
        })
        .collect()
}

#[test]
fn skill_tree_is_in_display_order() {
    let (_dir, root) = skill_fixture();
    let tree = file_tree(&root, &PreviewLimits::default());
    let listed: Vec<(String, usize)> = tree
        .nodes
        .iter()
        .map(|node| (node.path.to_string(), node.depth))
        .collect();
    let expected = [
        ("SKILL.md", 0),
        ("assets", 0),
        ("assets/logo.png", 1),
        ("references", 0),
        ("references/deep", 1),
        ("references/deep/c.md", 2),
        ("references/b.md", 1),
        ("references/forms.md", 1),
        ("scripts", 0),
        ("scripts/run.sh", 1),
        ("Alpha.md", 0),
        ("z.txt", 0),
    ];
    let expected: Vec<(String, usize)> = expected
        .iter()
        .map(|(path, depth)| ((*path).to_owned(), *depth))
        .collect();
    assert_eq!(listed, expected);
    assert!(!tree.truncated);
    assert!(tree.nodes[1].is_dir);
    assert_eq!(tree.nodes[1].kind, FileKind::Directory);
    assert_eq!(tree.nodes[2].kind, FileKind::Image);
}

#[test]
fn tree_truncates_at_entry_limit() {
    let (_dir, root) = skill_fixture();
    let limits = PreviewLimits {
        max_tree_entries: 3,
        ..PreviewLimits::default()
    };
    let tree = file_tree(&root, &limits);
    assert_eq!(tree.nodes.len(), 3);
    assert!(tree.truncated);
}

#[test]
fn skill_preview_has_overview_files_metadata_and_resolves_links() {
    let (_dir, root) = skill_fixture();
    let preview = build_preview(
        &skill_record(&root),
        7,
        &PreviewLimits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(preview.generation, 7);
    assert_eq!(preview.title, "pdf-tools");
    assert_eq!(tab_names(&preview.tabs), ["overview", "files", "metadata"]);
    let PreviewTab::Overview(Document::Markdown {
        text,
        frontmatter,
        headings,
        links,
    }) = &preview.tabs[0]
    else {
        panic!("expected markdown overview, got {:?}", preview.tabs[0]);
    };
    assert!(
        text.starts_with("# PDF tools"),
        "frontmatter must be split out"
    );
    assert_eq!(
        frontmatter,
        &[
            ("allowed-tools".to_owned(), "Read, Bash".to_owned()),
            ("name".to_owned(), "pdf-tools".to_owned()),
        ]
    );
    let anchors: Vec<&str> = headings.iter().map(|h| h.anchor.as_str()).collect();
    assert_eq!(anchors, ["pdf-tools", "usage"]);
    let resolved: Vec<(Option<String>, bool)> = links
        .iter()
        .map(|link| {
            (
                link.resolved.as_ref().map(ToString::to_string),
                link.external,
            )
        })
        .collect();
    assert_eq!(
        resolved,
        [
            (Some("references/forms.md".to_owned()), false),
            (Some("references/deep/c.md".to_owned()), false),
            (None, false),
            (None, false),
            (None, true),
        ]
    );

    // A link from a nested reference back to SKILL.md resolves and opens.
    let nested = preview_file(
        &root,
        &rel("references/forms.md"),
        &PreviewLimits::default(),
    );
    let Document::Markdown { links, .. } = nested else {
        panic!("expected markdown");
    };
    assert_eq!(links[0].resolved.as_ref().unwrap().as_str(), "SKILL.md");
    let script = preview_file(&root, &rel("scripts/run.sh"), &PreviewLimits::default());
    assert_eq!(
        script,
        Document::Code {
            language: "shell".into(),
            text: "#!/bin/sh\necho never-executed\n".into()
        }
    );
}

#[cfg(unix)]
#[test]
fn symlinks_escaping_root_are_refused_and_not_followed() {
    use std::os::unix::fs::symlink;

    let (dir, root) = skill_fixture();
    let outside = dir.path().join("outside");
    write(&outside, "secret.txt", "outside-content");
    symlink(outside.join("secret.txt"), root.join("leak.txt")).unwrap();
    symlink(&outside, root.join("leak-dir")).unwrap();
    symlink(root.join("z.txt"), root.join("inside-link.txt")).unwrap();

    let limits = PreviewLimits::default();
    let refused = read_confined(&root, &rel("leak.txt"), &limits);
    assert!(
        matches!(&refused, Document::Unavailable { reason } if reason.contains("outside")),
        "{refused:?}"
    );
    let through_dir = read_confined(&root, &rel("leak-dir/secret.txt"), &limits);
    assert!(matches!(through_dir, Document::Unavailable { .. }));
    assert_eq!(
        read_confined(&root, &rel("inside-link.txt"), &limits),
        Document::PlainText("z".into())
    );

    let tree = file_tree(&root, &limits);
    let leak_dir = tree
        .nodes
        .iter()
        .find(|node| node.name == "leak-dir")
        .unwrap();
    assert!(matches!(
        leak_dir.kind,
        FileKind::Symlink { broken: false, .. }
    ));
    assert!(!leak_dir.is_dir);
    assert!(
        tree.nodes
            .iter()
            .all(|node| !node.path.as_str().starts_with("leak-dir/")),
        "symlinked directory must not be descended"
    );
}

#[test]
fn oversize_binary_and_missing_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(root, "big.md", "x".repeat(2048));
    write(root, "nul.txt", b"abc\0def");
    write(root, "latin1.txt", [0xE9, 0x74, 0xE9]);
    write(root, "bundle.zip", "PK");
    write(root, ".env", "API_KEY=literal-secret");
    let limits = PreviewLimits {
        max_file_bytes: 1024,
        ..PreviewLimits::default()
    };
    assert_eq!(
        read_confined(root, &rel("big.md"), &limits),
        Document::TooLarge {
            size: 2048,
            limit: 1024
        }
    );
    assert_eq!(
        read_confined(root, &rel("nul.txt"), &limits),
        Document::Binary { size: 7 }
    );
    assert_eq!(
        read_confined(root, &rel("latin1.txt"), &limits),
        Document::Binary { size: 3 }
    );
    assert_eq!(
        read_confined(root, &rel("bundle.zip"), &limits),
        Document::Binary { size: 2 }
    );
    assert!(matches!(
        read_confined(root, &rel("missing.md"), &limits),
        Document::Unavailable { .. }
    ));
    let env = read_confined(root, &rel(".env"), &limits);
    assert!(matches!(&env, Document::Unavailable { reason } if reason.contains("credentials")));
    assert!(!format!("{env:?}").contains("literal-secret"));
}

fn mcp_record(
    transport: McpTransport,
    format: ConfigFormat,
    tools: Vec<McpToolDecl>,
) -> ResourceRecord {
    let config = PathBuf::from("/home/user/.cursor/mcp.json");
    record(
        "github",
        &config,
        ResourceDetails::McpServer(McpServerDetails {
            client: "cursor".into(),
            config_file: config.clone(),
            config_format: format,
            entry: "github".into(),
            transport,
            enabled: Some(true),
            declared_tools: tools,
        }),
    )
}

const LITERAL_SECRETS: &[&str] = &[
    "ghp_literalsecret000000",
    "flagsecret-123",
    "assignsecret",
    "sk-headersecret-0000",
    "query-secret",
];

#[test]
fn mcp_config_snippet_never_contains_literal_secrets() {
    let stdio = McpTransport::Stdio {
        command: "npx".into(),
        args: vec![
            "-y".into(),
            "@modelcontextprotocol/server-github".into(),
            "--token".into(),
            "flagsecret-123".into(),
            "GITHUB_HOST_TOKEN=assignsecret".into(),
        ],
        cwd: None,
        env: vec![
            EnvBinding {
                name: "GITHUB_PERSONAL_ACCESS_TOKEN".into(),
                value: SecretString::new("ghp_literalsecret000000"),
            },
            EnvBinding {
                name: "GITHUB_REF".into(),
                value: SecretString::new("${GITHUB_TOKEN}"),
            },
        ],
    };
    let http = McpTransport::StreamableHttp {
        url: "https://mcp.example.invalid/mcp?token=query-secret".into(),
        headers: vec![EnvBinding {
            name: "Authorization".into(),
            value: SecretString::new("Bearer sk-headersecret-0000"),
        }],
    };
    for (transport, format) in [
        (stdio.clone(), ConfigFormat::Json),
        (stdio, ConfigFormat::Toml),
        (http, ConfigFormat::Json),
    ] {
        let preview = build_preview(
            &mcp_record(transport, format, Vec::new()),
            1,
            &PreviewLimits::default(),
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(tab_names(&preview.tabs), ["config", "metadata"]);
        let PreviewTab::Config {
            format: shown,
            text,
        } = &preview.tabs[0]
        else {
            panic!("expected config tab");
        };
        let everything = format!("{preview:?}\n{}", serde_json::to_string(&preview).unwrap());
        for secret in LITERAL_SECRETS {
            assert!(
                !everything.contains(secret),
                "{secret} leaked in {shown}:\n{text}"
            );
        }
        assert!(text.contains("github"), "{text}");
        assert!(text.contains(SecretString::REDACTED), "{text}");
        if format == ConfigFormat::Toml {
            assert_eq!(shown, "toml");
            assert!(text.contains("mcp_servers"), "{text}");
        }
        let PreviewTab::Metadata(metadata) = &preview.tabs[1] else {
            panic!("expected metadata");
        };
        assert!(
            metadata
                .iter()
                .any(|(key, value)| key == "Tools" && value.contains("requires permission"))
        );
    }
}

#[test]
fn mcp_declared_tools_get_a_tools_tab() {
    let tool = McpToolDecl {
        name: "create_issue".into(),
        description: Some("Create an issue".into()),
        input_schema: Some(json!({
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "Issue title"},
                "labels": {"type": ["array", "null"]}
            },
            "required": ["title"]
        })),
    };
    let transport = McpTransport::Sse {
        url: "https://mcp.example.invalid/sse".into(),
        headers: Vec::new(),
    };
    let preview = build_preview(
        &mcp_record(transport, ConfigFormat::Json, vec![tool]),
        1,
        &PreviewLimits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(tab_names(&preview.tabs), ["config", "tools", "metadata"]);
    let PreviewTab::Tools(tools) = &preview.tabs[1] else {
        panic!("expected tools");
    };
    let title = tools[0]
        .parameters
        .iter()
        .find(|p| p.name == "title")
        .unwrap();
    assert!(title.required);
    assert_eq!(title.type_name.as_deref(), Some("string"));
    let labels = tools[0]
        .parameters
        .iter()
        .find(|p| p.name == "labels")
        .unwrap();
    assert!(!labels.required);
    assert_eq!(labels.type_name.as_deref(), Some("array | null"));
}

#[test]
fn agent_prompt_and_rule_tabs() {
    let dir = TempDir::new().unwrap();
    let agent_file = write(
        dir.path(),
        "agents/reviewer.md",
        "---\nmodel: opus\n---\nReview code.\n",
    );
    let agent = build_preview(
        &record(
            "reviewer",
            &agent_file,
            ResourceDetails::Agent(AgentProfileDetails {
                host: Some("claude-code".into()),
                file: agent_file.clone(),
                model: Some("opus".into()),
                tools: vec!["Read".into()],
                skills: Vec::new(),
                mcp_servers: vec!["github".into()],
            }),
        ),
        1,
        &PreviewLimits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(tab_names(&agent.tabs), ["overview", "metadata"]);
    let PreviewTab::Metadata(metadata) = &agent.tabs[1] else {
        panic!()
    };
    assert!(metadata.contains(&("Model".to_owned(), "opus".to_owned())));
    assert!(metadata.contains(&("MCP servers".to_owned(), "github".to_owned())));

    let prompt_file = write(
        dir.path(),
        "prompts/commit.md",
        "Write a commit for {{diff}}.\n",
    );
    let variables = vec![PromptVariable {
        name: "diff".into(),
        description: None,
        default: None,
        required: true,
    }];
    let prompt = build_preview(
        &record(
            "commit",
            &prompt_file,
            ResourceDetails::Prompt(PromptTemplateDetails {
                file: prompt_file.clone(),
                host: None,
                syntax: TemplateSyntax::DoubleBrace,
                variables: variables.clone(),
            }),
        ),
        1,
        &PreviewLimits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(
        tab_names(&prompt.tabs),
        ["overview", "template", "metadata"]
    );
    assert_eq!(
        prompt.tabs[1],
        PreviewTab::Template {
            variables,
            raw: "Write a commit for {{diff}}.\n".into()
        }
    );

    let rule_file = write(
        dir.path(),
        ".cursor/rules/style.mdc",
        "---\nglobs: *.rs\n---\nUse rustfmt.\n",
    );
    let rule = build_preview(
        &record(
            "style",
            &rule_file,
            ResourceDetails::Rule(RuleDetails {
                file: rule_file.clone(),
                format: RuleFormat::CursorMdc,
                host: Some("cursor".into()),
                scope: Scope::Project {
                    root: dir.path().to_path_buf(),
                },
                description: None,
                always_apply: Some(false),
                globs: vec!["*.rs".into()],
                precedence: None,
            }),
        ),
        1,
        &PreviewLimits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(
        tab_names(&rule.tabs),
        ["overview", "precedence", "metadata"]
    );
    let PreviewTab::Precedence(precedence) = &rule.tabs[1] else {
        panic!()
    };
    assert!(precedence.contains(&("Globs".to_owned(), "*.rs".to_owned())));
    assert!(precedence.contains(&("Always apply".to_owned(), "no".to_owned())));
    assert!(
        !precedence.iter().any(|(key, _)| key == "Precedence"),
        "unknown order is not invented"
    );
    let PreviewTab::Overview(Document::Markdown { text, .. }) = &rule.tabs[0] else {
        panic!()
    };
    assert_eq!(text, "Use rustfmt.\n");
}

fn plugin_record(root: &Path) -> ResourceRecord {
    record(
        "lint-hooks",
        root,
        ResourceDetails::Plugin(PluginDetails {
            manifest: root.join(".cursor-plugin/plugin.json"),
            root: root.to_path_buf(),
            host: PluginHost::External {
                name: "cursor".into(),
            },
            components: Vec::new(),
            permissions: vec!["filesystem:read".into()],
            executable_files: vec![rel("hooks/pre.sh")],
            content_hash: Some("abc123".into()),
            files: Vec::new(),
        }),
    )
}

#[test]
fn plugin_overview_prefers_readme_then_manifest_description() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("lint-hooks");
    write(
        &root,
        ".cursor-plugin/plugin.json",
        r#"{"name":"lint-hooks","description":"Runs linters"}"#,
    );
    write(&root, "hooks/pre.sh", "exit 0");

    let preview = build_preview(
        &plugin_record(&root),
        1,
        &PreviewLimits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(
        tab_names(&preview.tabs),
        ["overview", "files", "permissions", "metadata"]
    );
    let PreviewTab::Overview(overview) = &preview.tabs[0] else {
        panic!()
    };
    assert_eq!(overview.text(), Some("Runs linters"));
    assert_eq!(
        preview.tabs[2],
        PreviewTab::Permissions {
            permissions: vec!["filesystem:read".into()],
            executable_files: vec![rel("hooks/pre.sh")],
            content_hash: Some("abc123".into()),
        }
    );

    write(&root, "README.md", "# Lint hooks\n");
    let preview = build_preview(
        &plugin_record(&root),
        2,
        &PreviewLimits::default(),
        &CancelToken::new(),
    )
    .unwrap();
    let PreviewTab::Overview(overview) = &preview.tabs[0] else {
        panic!()
    };
    assert_eq!(overview.text(), Some("# Lint hooks\n"));
}

#[test]
fn cancelled_token_aborts_build_preview() {
    let (_dir, root) = skill_fixture();
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert_eq!(
        build_preview(
            &skill_record(&root),
            1,
            &PreviewLimits::default(),
            &cancelled
        ),
        Err(PreviewError::Cancelled)
    );
    assert_eq!(
        build_preview(
            &plugin_record(&root),
            1,
            &PreviewLimits::default(),
            &cancelled
        ),
        Err(PreviewError::Cancelled)
    );

    let scheduler = PreviewScheduler::new();
    let (first, first_token) = scheduler.begin();
    let (second, second_token) = scheduler.begin();
    assert!(first_token.is_cancelled());
    assert!(!scheduler.is_current(first));
    assert_eq!(
        build_preview(
            &skill_record(&root),
            first,
            &PreviewLimits::default(),
            &first_token
        ),
        Err(PreviewError::Cancelled)
    );
    let preview = build_preview(
        &skill_record(&root),
        second,
        &PreviewLimits::default(),
        &second_token,
    )
    .unwrap();
    assert!(scheduler.is_current(preview.generation));
}

#[test]
fn scheduler_drops_stale_generations_under_rapid_begin() {
    const THREADS: usize = 8;
    const BEGINS_PER_THREAD: usize = 200;
    let scheduler = Arc::new(PreviewScheduler::new());
    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let scheduler = Arc::clone(&scheduler);
            thread::spawn(move || {
                (0..BEGINS_PER_THREAD)
                    .map(|_| scheduler.begin())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut all: Vec<_> = handles
        .into_iter()
        .flat_map(|handle| handle.join().unwrap())
        .collect();
    all.sort_by_key(|(generation, _)| *generation);

    let total = (THREADS * BEGINS_PER_THREAD) as u64;
    let generations: Vec<u64> = all.iter().map(|(generation, _)| *generation).collect();
    assert_eq!(
        generations,
        (1..=total).collect::<Vec<_>>(),
        "generations are unique and gap-free"
    );
    assert_eq!(scheduler.current_generation(), total);
    for (generation, token) in &all {
        let latest = *generation == total;
        assert_eq!(scheduler.is_current(*generation), latest);
        assert_eq!(token.is_cancelled(), !latest, "generation {generation}");
    }

    scheduler.cancel();
    assert!(all.last().unwrap().1.is_cancelled());
    assert!(!scheduler.is_current(total));
}
