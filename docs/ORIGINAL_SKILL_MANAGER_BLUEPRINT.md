# GPUI Skill Manager — Production Implementation Blueprint

**Status:** Engineering handoff / greenfield implementation plan  
**Date:** 2026-11-07  
**Primary environments:** macOS (Apple Silicon + Intel, if practical); Arch/CachyOS KDE Plasma Wayland x86_64  
**Product:** Native Rust + GPUI Skill Manager, universal launcher, previewer, organiser, and multi-agent deployment manager.

> **Mission:** Build a professional, restrained, keyboard-first native desktop manager for Agent Skills. It must combine a rich full-window browser, a very fast global quick panel, editable skill libraries, secure multi-agent install/symlink/copy deployment, and context-aware copy/insert/compose actions. Linux KDE Wayland is a release-blocking platform, not a best-effort port.

## 0. Operating contract for the coding agent

1. **Read the entire plan before coding.** Begin with the research and verification checklist, then create ADRs documenting dependency choices, platform capabilities, and licensing.
2. **Inspect the exact upstream source modules linked in section 3.** Use reference code as guidance, not wholesale copies. Verify their present APIs and licensing before borrowing.
3. **Do not claim working OS functionality from source inspection alone.** Real macOS and KDE Wayland runtime tests are mandatory for platform features.
4. **Develop in vertical slices** with runnable demonstrators and automated tests. The launcher, preview and deployment engine must be usable before secondary features are polished.
5. **Never silently overwrite existing agent skills or execute downloaded code.** Every filesystem mutation must be planned, previewed, validated and recorded.
6. **Keep the product native, offline-capable, compact and sober.** No Electron/Web frontend for the core UI; a webview is optional for exceptional documents only.
7. **Do not build a second agent runtime.** Integrate with external agents via adapters, documented APIs/CLI, protocols, or explicit clipboard fallback.
8. **Document assumptions and unsupported cases.** Do not invent provider APIs, download counts, review data, agent session support, executable paths or permissions.
9. **Completion means meeting acceptance gates**, not producing screens that appear to work.
10. Every milestone must include: implementation, tests, screenshots on both platforms where UI is involved, known limits, and an ADR if architecture changes.

---

## 1. Complete product requirements

### 1.1 Core user journeys

**J1 — Discover.** Open main application → search/browse skills from local inventory, cached sources, skills.sh, GitHub, LobeHub, or configured repositories → filter by source, category, installed state, compatibility → view detailed skill and actual contents → install to chosen agents/project.

**J2 — Quick use.** From Ghostty, editor, browser, or coding agent press global hotkey → floating quick panel appears focused → type a partial skill name → results immediately include local and cached remote sources → arrows change preview → Enter uses configurable action; Tab composes `skill + query`; action menu copies path, URL, content, or installs. Escape returns to prior state/dismisses.

**J3 — Inspect without leaving task.** In quick panel select skill → split preview renders `SKILL.md` → navigate nested `references/`, `scripts/`, `assets/` → read syntax-highlighted code, Markdown tables, images or raw text → go back to result, preserving query and scroll.

**J4 — Make a master library.** Choose `~/Skills` or existing roots → keep real nested categories → import/move/adopt/link skills → manage logical collections independently of folders → edit metadata/content in place with safe saves.

**J5 — Deploy everywhere.** Select skills or a collection → choose Claude Code, Codex, OpenCode, Cursor, Gemini CLI, other registered agents and target scope (global/project/custom) → choose symlink, managed copy, link-source or independent copy → see complete file operation plan → safely apply → inspect resulting agent × skill matrix.

**J6 — Session-aware composition.** Open launcher while editing in a supported agent → snapshot active app/project/session where possible → choose skill → optionally type a natural language query → preview final payload and target → send via native integration, or copy/paste fallback. Explicitly identify when direct insertion is unsupported.

**J7 — Repair/sync.** Changes to master skill or agent folder → filesystem watcher/reconciliation detects drift → badges show linked/duplicate/broken/outdated/unmanaged/conflict → action menu offers repair, resync, convert to copy/link, adopt or unlink without damaging original files.

**J8 — Edit workspace.** Switch main manager into design mode → rearrange independent docked panes (Explorer, Preview, Files, Deployments, Activity etc.) → save named layouts/workspace bundles; choose between dense table, grid, and inspector views. Quick panel retains focused presets instead of arbitrary dock editing.

**J9 — Cross-platform.** Same capabilities and appearance on macOS and KDE Wayland, with platform-dependent insertion/window features represented clearly as supported, permission-required, fallback-only or unavailable.

### 1.2 Explicit non-goals

- No social/reviews/ratings marketplace clone, popularity graphs or unnecessary LobeHub-specific UI.
- No requirement for user sign-in to search installed or cached skills.
- No mandatory remote account, cloud backend, Electron or always-on internet.
- No blind execution of code from skill scripts during browsing or preview.
- No unrestricted synthetic input, focus capture or window inspection promises on Wayland.
- No automatic filesystem deduplication that destroys user-owned files.
- No promise to support every marketplace with one generic undocumented API.

---

## 2. Visual and interaction design specification

### 2.1 Approved direction

References: provided **FluxDown downloads list**, **FluxDown settings/shortcuts**, restrained recent skill-manager renders, and Rox's panel-based theming. Style is **dense, neutral, subtle, technically mature**. No ornamental glow, coloured card outlines everywhere, neon fill-buttons, excessive gradients, giant dashboard metrics or status chips.

**Theme roles (dark base):**

| Token | Colour | Usage |
| --- | --- | --- |
| background | `#1e1e1e` | Main content background |
| chrome | `#222224` | Title bar, status bar |
| surface | `#2c2c2e` | Dialogs, panels, selected elevation |
| raised | `#31363b` | Inputs and subtle section surfaces |
| border | `#48484a` | Delicate separators, only where needed |
| foreground | `#eff0f1` / `#f5f5f7` | High-emphasis text |
| mutedForeground | `#a1a1a6` | Metadata and help text |
| tertiary | `#757578` | Low priority labels |
| primary | `#1abc9b` | **Rare** key interaction/action accent |
| focus | `#00ff89` | Focus indicator only; no extensive neon surfaces |
| information | `#1d99f3` | Information, when semantically needed |
| warning | `#fdbc4b` | Actual warning/changed state |
| destructive | `#ed1515` or `#ef4444` | Errors / destructive operations |
| selectedRow | `#343436` with understated tint | Single selected result, not vivid green |

Dark mode first; implement FluxDown-inspired light mode (`#f8f9fa` bg, white surface, restrained green selection), system mode, and user editable theme packages. Use semantic tokens in every component; never scatter literal hex values.

**Density:** titlebar 40px; statusbar 24–28px; sidebar nav 28px; compact list rows 30–36px, comfortable 40–44px; sidebar 190–240px; captions 11–12px; dense body 13px; primary text 13–15px; 12/14/16px Lucide controls; 6px controls, 8px cards, 12px large dialog. Radius, spacing and fonts editable through versioned theme JSON matching the *concept* of FluxDown GPUI Theme v2, not falsely claiming schema compatibility.

**Icons:** Lucide for universal UI icons; actual provider/agent logos only where appropriate and licensed. Display logos at restrained sizes with cache; generic filetype icons remain neutral. Do not encode status exclusively through colour.

**Rox design lessons:** independent theming roles; per-panel overrides; minimal shared spacing ladder; layouts are separate from themes; configurable panels in design mode and fixed UX in normal mode.

### 2.2 Screen inventory — full app

1. **Explore/Marketplace**: fixed slim icon rail OR left nav; search field; source+category filters; grid/table toggle; category sidebar optional; 2–3 column understated skill cards or dense row table. Cards show logo, name, publisher, short description, source, files count, installed/update icon. No rating/review metrics.
2. **Skill detail**: header icon/name/publisher/installed state; tabs Overview, Files, References, Install, Info (Activity/Changelog when available); rendered actual `SKILL.md`; optional full-screen source viewer; source and local paths; version from real metadata only; compatibility; links; context action menu.
3. **Installed & Deployments**: unified inventory; agent × skill checkbox matrix; scope and method; linked source path, resolved target, ownership and status; bulk operations with dry-run and conflict review.
4. **Skill Library**: nested real folders, additional roots, logical collections, favourites, saved sets, drag/drop within managed bounds, rename/move/adopt; file tree and preview or editor.
5. **Agents**: agent discovery, install root settings, supported skill format, global/project capability, health and links; disabled/experimental integrations clearly identified.
6. **MCP / Integrations**: registry and installed endpoints, per-agent config where supported, inspect and manage with separate permission handling. MCP integration can be phased after core skills and must not masquerade as skills.
7. **Workspaces**: saved panel layouts, current project, optional remote host, collection profiles; import/export.
8. **Activity**: install/update/remove tasks, errors, retry, undo/recovery, log with provenance; compact, not social.
9. **Settings**: General, Appearance, Quick Launcher, Shortcuts, Sources, Libraries, Agent Paths, Deployment Defaults, Integrations, Privacy/Permissions, Advanced/Diagnostics, About.
10. **Design Mode**: dock, split, tab, duplicate and pop-out supported panes, save named layouts, lock layout in normal use.

### 2.3 Quick Launcher (the critical UI)

- **Compact preset** approximately 650×430: search + result list + shortcut footer; keyboard opens preview in place.
- **Split Preview preset** approximately 950–1100×550–700: results left 40–45%, preview right 55–60%. Default.
- **Expanded preset** approximately 1100×750: larger preview/docs + file tree/navigation, still one floating window.
- Preserve search string, selection, preview active file, scroll offsets on transition between layouts and navigation stack screens.
- Search first line always focused; result list uses virtualisation. An item shows icon/logo, skill name, publisher, source, installed state; result density approx 36–40px. Avoid extended descriptions unless selected.
- Preview tabs: Overview (`SKILL.md` rendered), Files, References, Scripts/Assets, Metadata. Optional Raw toggle, code highlighting, links, line numbers, image preview, copy content, open externally.
- Always preview selection asynchronously, cancel stale requests; do not lose typing focus when preview changes.
- Action menu: Use, Compose query, Copy path, Copy remote URL, Copy `SKILL.md`, Copy selected reference path/content, Paste into supported session, Open folder/repository, Install, Update, Remove deployment, Deploy targets, Repair links, Open in full manager.
- Footer shows factual key hints only for active state. Scope/source filters optional; avoid button overload.
- Compose mode replaces or adds an input for natural-language query, attaches selected skill as a chip, previews target agent/session and actual output. Enter acts according to configured action; optional `Shift+Enter` newline.
- Capture intended target context at launcher invocation; validate on commit. Use agent-native invocation syntax only if adapter declares it; otherwise insert a correct path/reference/message template, or copy to clipboard.

### 2.4 Keyboard contract

Global default **configurable**: macOS `Cmd+Shift+Space` or user's choice; KDE `Meta+Shift+Space` or portal-assigned preferred combination. Avoid assuming `Cmd+Space`/`Meta+Space` are free.

| Context | Shortcut | Action |
| --- | --- | --- |
| Panel | Up/Down | Move result selection and refresh preview |
| Panel | Enter | Default action for item/context |
| Panel | Tab | Enter compose mode (not steal focus invisibly) |
| Panel | Space when results focused | Toggle preview / open full preview (configurable) |
| Panel | Cmd/Ctrl+Enter | Searchable action menu |
| File tree | Right/Left | Expand/open or collapse/go back |
| Preview | Cmd/Ctrl+C | Copy selected text; item command when no selection |
| Compose | Enter | Apply/send/copy according to target adapter |
| Compose | Shift+Enter | Newline |
| Anywhere | Escape | Close popover → back one screen → dismiss launcher |
| Anywhere | Configurable | Copy local path / URL / open details / install |

All shortcuts independently rebindable; support keyboard context predicates, conflict detection, reset and live update without restart.

---

## 3. Mandatory reference investigation (exact paths and takeaways)

**Agent instruction:** for each project below, clone or inspect the cited sources at a pinned revision. Write `docs/reference-audit.md` recording version, licence, observed APIs, relevant code links, reusable patterns and incompatibilities. Build an isolated PoC where the feature is OS-dependent.

### 3.1 Rox — primary KDE Wayland + layout/persistence reference

Repo: https://github.com/zealsprince/rox  
Licence: AGPL-3.0-only for Rox application; its `rox-dock` was vendored from upstream Apache-2.0 `gpui-component`; verify exact file history before copying.

- `crates/rox/src/quick_play.rs` — GPUI searchable overlay, `uniform_list`, focus, selection, row density, key routing, reusable embedded search.
- `crates/rox/src/search_window.rs` — host same view in its own window; singleton, restoring query and focus, window bounds.
- `crates/rox/src/keymap.rs` — central rebindable command registry, contexts, input focus safeguards, chords.
- `crates/rox-dock/src/{lib.rs,state.rs,stack_panel.rs,tab_panel.rs,tiles.rs}` — editable recursive dock tree, nested splits/tabs/tiles, layout serialization, Design Mode.
- `crates/rox-panel-kit/src/{window_chrome.rs,search_picker.rs,window_buttons.rs,gesture.rs}` — Wayland decorations/fallback chrome and reusable controls.
- `crates/rox-design/src/{palette.rs,tokens.rs,assets.rs}` — semantic palette, spacing, scope overrides, asset/icon setup.
- `crates/rox/src/workspaces.rs` + `docs/03-implementation/06-panels.md` — workspace presets, layouts, independent configs and import/export.
- `crates/rox-core/src/lib.rs` — stable Wayland/X11 app id for activation.
- `docs/0R-research/03-quit-to-tray.md` — real Plasma Wayland process/windowless/tray prototype, `ksni`, D-Bus, GPUI quit mode.
- `patches/gpui/` — check Wayland resize geometry, pointer grab, decoration fallback and lifecycle patches **against selected upstream GPUI**; don't transplant blindly.
- `README_IPC.md` + `crates/rox-ipc/src/` — local JSON-RPC socket protocol, authenticated-by-local-user access, debug and action invocation.
- `README_PLUGINS.md` — plugin subprocess RPC, manifest, per-host approval hash and change detection; study security limits.
- `docs/02-architecture/03-non-functional.md` — metrics, virtual lists, startup snapshots, Wayland risk, version pinning.
- `scripts/vendor-gpui.sh` + workspace `Cargo.toml` — patch isolation and dependency pinning.

**Do NOT** bring over Rox's music-specific code, audio engine, FFTs or AGPL source wholesale. Record licence decisions.

### 3.2 Sailry Harness — primary macOS + agent context + preview reference

Repo: https://github.com/sailry/sailry-harness  
Licence: Apache-2.0 main repository, with separate notices/third-party licenses. **Desktop currently macOS-focused; host supports macOS+Linux.**

- `apps/desktop/src/command_picker.rs` — GPUI Kit list delegate, custom focused-key intercept, selected item and confirm behavior.
- `apps/desktop/src/system_shortcuts.rs` and `system_shortcuts/selection.rs` — OS global shortcut service, event dispatch, insertion targeting; captured Node/worktree/session identity before insert.
- `apps/desktop/src/shortcuts.rs` — dynamic app-wide actions and shortcut contexts.
- `apps/desktop/src/file_picker/{navigation.rs,selection.rs,render.rs,breadcrumbs.rs}` — async navigation, cancellation tokens, generation checks, history, preserving focus/selection.
- `crates/content/src/{markdown,editor,syntax,diff,media.rs,selection.rs}` — native Markdown preview, code rendering, syntax, diffs and selections.
- `apps/desktop/src/macos.rs` — AppKit `NSWindow` / `NSVisualEffectView` native integration, safe main-thread handling.
- `apps/desktop/src/theme/{floating.rs,cards.rs,dialog.rs,package,market}` — styled popup, modal, semantic theme token and theme package patterns.
- `apps/desktop/src/ui/{modal_list.rs,card_list.rs,overlays.rs,status_list,form.rs,details.rs}` — reusable application UI.
- `apps/desktop/src/plugins/{references.rs,previews.rs,tree.rs,picker.rs,controls.rs,forms.rs,layout.rs}` — secure reference insertion, file previews, native plugin UI and content injection.
- `ARCHITECTURE.md` — separation of desktop view, protocol, client, host and remote transport; no duplicate agent runtime.
- `crates/ssh/`, `crates/vt/` — optional later reference for SSH host and terminal/PTY support.
- Workspace `Cargo.toml` — pinned `gpui-kit`, `gpui-shell`, component revisions and third-party dependences; **versions differ from Rox**.

**Do NOT** reuse Sailry branding, assume its macOS shortcuts work on Wayland, or import its entire AI execution engine.

### 3.3 Search, GPUI and components

- https://github.com/helix-editor/nucleo (`nucleo`, `nucleo-matcher`) — fuzzy matching and ranked incremental updates; adapt bonus weighting for skill names and recent usage.
- https://github.com/zed-industries/zed/tree/main/crates/gpui/examples — view/entity lifecycle, focus, actions, keyed rendering, virtual lists, windows; repository example path may move, verify.
- https://github.com/zed-industries/zed — actual modal pickers and editor file preview patterns.
- https://github.com/MatthiasGrandl/Loungy — compact Rust/GPUI launcher using Nucleo and Lucide; reference only, not the product base.
- https://github.com/vicinaehq/vicinae — command/action workflow, extensions and launcher UX; different UI stack, use interaction references.
- https://github.com/junegunn/fzf — preview window and keyboard-first picker UX; no need to ship the fzf binary.
- https://github.com/lotabout/skim — picker/preview alternatives.
- https://github.com/dux-web/gpui-component and https://gpui-kit.com/docs/ — component library options (lists, trees, forms, Markdown, editor, dialogs). Must choose versions compatible with selected GPUI baseline.
- https://lucide.dev/ — universal icon language, supplied locally as approved assets/crate.
- https://github.com/lobehub/lobe-icons — actual agent/provider logos, check each asset licence.

### 3.4 Skill format and deployment engines

- https://agentskills.io/specification ; https://github.com/agentskills/agentskills — official skill file/folder format, metadata, references, validation.
- https://github.com/vercel-labs/skills/blob/main/src/installer.ts — symlink/copy/link modes, canonical dir, conflicts, agent directories, overlap handling. Reference behavior; implement safe Rust equivalent, do not shell out by default.
- https://github.com/vercel-labs/skills/blob/main/src/agents.ts — agent-specific global/project installation roots and detection; registry must be versioned and overrideable.
- https://github.com/vercel-labs/skills/blob/main/src/skills.ts — discovery/parser patterns.
- https://github.com/vercel-labs/skills/blob/main/src/skill-lock.ts — origin and version recording.
- https://github.com/vercel-labs/skills/blob/main/docs/commands.md — `add`, `list`, `update`, `sync`, global/project, copy methods.
- https://skillsmanager.dev/ — agent × skill deployment matrix, managed library, symlinks/copies, presets, unmanaged-path refusal, portable CLI.
- https://skills.sh/ — discovery provider; validate supported public ingestion sources/terms.
- https://lobehub.com/community/skill — visual inspiration only: description + files + install workflow, **not** reviews/ratings/social data and not an assumed accessible API.

### 3.5 Visual references to preserve

1. Supplied screenshots of **FluxDown download list**: understated UI density, 3-level navigation, table rows and context menus.
2. Supplied screenshots of **FluxDown Shortcuts settings**: controls arranged in clean rows, grouped sections, no giant callouts.
3. Supplied screenshot of **LobeHub skill detail**: Overview/Install/Files/Info *information architecture only*, remove reviews/ratings/stars.
4. Rox screenshots (`docs/0S-screenshots/`, `crates/rox/assets/workspaces/`): panel composition and theme restraints.
5. Sailry screenshot (`assets/readme/workspace-dark.png`, `assets/readme/files.png`): macOS native AI workspace details and editor/content panes.
6. Approved **muted blue-grey** recent mockups, NOT earlier vivid saturated-green mockups.

Create image-backed design references under `docs/design/references/` only when image licences allow; otherwise link to upstream screenshots. Build screenshot tests for both themes and all three quick-panel layouts.

---

## 4. Proposed Rust workspace and dependency decisions

**Architecture:** one cross-platform native GUI binary with a persistent launcher controller, optionally a small CLI client. Background indexing, providers and install jobs use worker tasks; UI thread never blocks on I/O. CLI/library code must be testable headlessly.

```text
skill-manager/
  Cargo.toml
  Cargo.lock
  apps/
    desktop/                   # GPUI main window, settings, quick-panel shell
    cli/                       # headless CLI commands / optional IPC client
  crates/
    core/                      # IDs, typed domain models, permissions
    skill-format/              # SKILL.md parser, validator, package inventory
    storage/                   # SQLite schema, migrations, cached snapshots
    registry/                  # canonical identity, local inventory, source variants
    search/                    # Nucleo index, query parsing, ranking
    providers/                 # Local, GitHub, skills.sh, LobeHub adapters
    preview/                   # Markdown/raw/code/image/reference cache
    deployment/                # path planning, copy/symlink/rollback/reconcile
    agents/                    # agent registry, roots, compatibility
    sessions/                  # foreground context, native agent connectors
    actions/                   # typed action registry, invocation and outcomes
    themes/                    # semantic JSON tokens, per-mode bindings
    workspace/                 # panel layout state and presets
    platform/                  # shared capability traits
    platform-macos/            # AppKit/Hotkeys/Accessibility/clipboard
    platform-linux/            # Wayland/KDE portals/D-Bus/clipboard/tray
    ipc/                       # local socket protocol + authorization
  assets/
    icons/lucide/
    providers/
    themes/
  docs/
    adr/
    design/
    reference-audit.md
    implementation-status.md
    compatibility-matrix.md
    platform-acceptance.md
  tests/
    fixtures/skills/
    integration/
    e2e/
  benches/
```

**Dependency ADR before coding:**

- Select **one pinned GPUI revision and compatible GPUI Kit/component baseline**. Prototype window, text input, virtual list, Markdown view, Wayland window creation and macOS AppKit integration in a small demo first. Rox pins old patched GPUI; Sailry pins a different GPUI Kit Git revision. Never mix them unreviewed.
- `nucleo` / `nucleo-matcher` for filtering; `rusqlite` (or another well-supported SQLite library) for local metadata; `tokio` or structured GPUI background executors; `serde`, `serde_json`, `toml`, YAML frontmatter parser, Markdown parser, syntax highlighter; `notify` for file watching; hashing (`blake3` or SHA-256) for managed provenance; `tracing` for diagnostics.
- Platform integration: current `global-hotkey` or macOS native equivalent; D-Bus + `xdg-desktop-portal` GlobalShortcuts API on Wayland; `ksni` for KDE tray as suitable; clipboard through GPUI/Wayland-safe paths. Verify upstream compatibility and feature flags.
- Avoid unnecessary dependency churn and heavy JS runtimes. Optional provider/plugin extension subprocesses may be isolated via a documented RPC protocol later.

### 4.1 Model structs (conceptual; implement exact Rust types)

```rust
struct SkillId { source_namespace: String, canonical_locator: String, subpath: String }
struct SkillRecord {
    id: SkillId,
    name: String,
    description: String,
    publisher: Option<String>,
    categories: Vec<String>,
    source_variants: Vec<SourceRef>,
    skill_md: ContentRef,
    files: Vec<SkillFile>,
    version: Option<String>,
    compatibility: Compatibility,
    installations: Vec<DeploymentId>,
    trust: TrustState,
}
struct Deployment {
    id: DeploymentId,
    skill: SkillId,
    agent: AgentId,
    scope: Scope,             // Global / Project / Custom
    host: HostId,
    canonical_source: PathBuf,
    destination: PathBuf,
    mode: DeploymentMode,     // Symlink / ManagedCopy / LinkSource / IndependentCopy
    ownership: Ownership,     // CreatedByUs / Adopted / ExternalUnmanaged
    expected_fingerprint: String,
    status: DeploymentStatus,
}
struct ContextSnapshot {
    platform: Platform,
    app_id: Option<String>,
    window_identity: Option<String>,
    project_root: Option<PathBuf>,
    agent_id: Option<AgentId>,
    session_id: Option<String>,
    captured_at: Instant,
    adapter_capabilities: Vec<Capability>,
}
```

Canonical IDs must not collapse distinct skills with the same name. Dedup uses canonical repo/source identity and hashes when appropriate. Show duplicate variants rather than hiding meaningful versions. Distinguish category folder and logical collection.

### 4.2 Provider contract

```rust
trait SkillProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn search(&self, query: &SearchQuery, cancel: CancellationToken) -> Result<Vec<SkillSummary>>;
    async fn details(&self, id: &SkillId) -> Result<SkillDetail>;
    async fn file_tree(&self, id: &SkillId) -> Result<Vec<FileEntry>>;
    async fn read_file(&self, id: &SkillId, path: &RelativePath) -> Result<Vec<u8>>;
    async fn fetch_package(&self, id: &SkillId) -> Result<StagedPackage>;
}
```

Implement as capability-based adapters, not require all endpoints to work for every provider. If LobeHub API isn't publicly usable, support verified GitHub/source URLs or explicitly mark LobeHub as read-only/browser integration until an official method is discovered. Provider network requests are background, rate-limited, cached, bounded, cancellable; respect credentials/terms. Source labels always truthful.

---

## 5. Functional subsystems and precise requirements

### 5.1 Skill format, content and inspection

- Validate required frontmatter name/description and standard directory naming; preserve unknown metadata; report strict validation errors separately from lenient browsing warnings.
- Enumerate all paths in a skill (files/dirs) without following unsafe external symlinks by default. Expose relative paths, size, file type, executable indicator and hash when available.
- Render full Markdown, code blocks, tables, relative links and anchored headings. Local relative links open inside preview navigation. Show raw text and search within file.
- Syntax highlighting for Rust, Python, TypeScript, JavaScript, Shell, JSON, YAML, TOML, XML and common file types. Images get bounded previews; binary files get info/download/open external, not gibberish.
- Files pane supports nested tree, breadcrumbs, Back/Forward, scroll restoration, link click and shortcut navigation. Never execute scripts from preview.
- Remote path navigation prevents `../`, absolute path injection and symlink traversal outside allowed root. Content read has file size/time limits and stale request cancellation.
- Files and references available in **both** quick launcher and full app through shared renderer.

### 5.2 Local inventory and libraries

- Multiple library roots, user home and project roots, default managed root configurable (e.g. `~/Skills`). Nested physical categories do not imply nesting in agent's flat skills directory.
- Scan known agent skill roots read-only initially; record original owner, root and symlink target; never automatically import/alter.
- `adopt`, `import copy`, `move`, `link source`, `detach/unmanage`, `rename` and bulk category movement with preview/undo.
- Logical collections/presets can contain items from any physical category or root; no duplicate files needed.
- Watch for rename/change and run periodic full reconciliation because file watchers can miss events.
- Library is real human-readable files; SQLite is a rebuildable index/manifest, not sole source of skill content.
- User edits optionally via embedded text editor with atomic write and conflict/hash checks; default safety for external sources is read-only until explicitly enabled.

### 5.3 Agent target compatibility

- Data-driven agent adapter registry: detection methods, configured install dirs, project/global support, path format, symlink tolerance, native invocation support, session insertion support, version and warnings.
- Built-ins at minimum: Claude Code, Codex, OpenCode, Cursor, Gemini CLI, VS Code/Copilot (where applicable), configurable Generic Agent.
- Paths are *configurable* and discovered/verified; don't hardcode illustrative home paths as universally valid.
- Detect shared/universal install locations to prevent redundant links; handle agents with different conventions or flat file formats via explicit adapter transforms.
- Agent status: detected, configured, missing, incompatible, disabled. Provide test path and dry-run.

### 5.4 Deployment engine (highest safety priority)

**Modes:**
1. `Symlink`: canonical managed directory remains source; destination links directly to it.
2. `ManagedCopy`: copy external source to canonical managed directory; destination links to managed copy.
3. `LinkSource`: canonical managed path is a link to an existing external source; agent destinations link to canonical, retaining provenance.
4. `IndependentCopy`: files copied to each target; tracked for drift.

**Planning:** read-only `plan()` returns ordered operations and conflict diagnostics for each destination. Dry-run is default for bulk actions. UI shows source, target, scope, mode, existing path ownership, method, effect on files and rollback option.

**Safety rules:**
- Use `symlink_metadata` / `lstat` semantics; distinguish broken link, real directory, managed link, external link, symlinked parent and mount point.
- Never call recursive delete on an unknown source or unmanaged target. Delete only specifically owned symlink/file/directory after verifying inode/type/path identity and ownership.
- Prevent overlap/self-reference, symlink loops, traversal and escape outside approved roots. Stage downloads in quarantine, inspect archive traversal, size and file-count limits, then atomically promote after verification.
- Apply filesystem changes with backups/staging and per-operation journal. Multi-destination deployment is **recoverable**, not falsely ACID across filesystems; compensate on partial failure.
- Record expected file fingerprint and last applied operation; refuse replacing locally modified independent copies without compare/backup/approval.
- Use temporary dirs + rename for safe copy when possible. Expose durability/atomicity limits by filesystem.
- Only symlink on same host where target path exists and can be resolved. Remote host uses explicit copy/package and remote manifest; local symlink cannot target arbitrary SSH filesystem.
- No silent copy fallback if chosen method fails: prompt to retry as copy and correctly record actual mode.
- Support remove *deployment* separately from delete *canonical source*. Distinct confirmations.
- Provide repair broken link, replace link, convert mode, reinstall missing package and scan integrity.

**Deployment matrix:** row skill, columns target agents/scopes; cells `linked`, `copy`, `missing`, `modified`, `broken`, `conflict`, `unsupported`. Multi-select rows, deploy a collection to several agents, filter drift, inspect per-cell real path and method.

### 5.5 Search and performance

- Nucleo in-memory index loaded at app start from persisted snapshot; initial local search does not block on SQLite, network or filesystem scan.
- Weighted searchable fields: name exact/prefix > name fuzzy > aliases > publisher > categories > description > reference filenames/headings > content (optional extended search).
- Ranking incorporates usage recency, installed state, compatible current agent and favourites as **bounded boosts**; never suppress valid matches by hard priority.
- Source filter, Local/Installed/Remote, categories, compatibility and scope; preserve query/selection during stream updates.
- 10k / 100k synthetic fixtures. Virtualised rows, immutable snapshots, bounded worker concurrency, predecoded icon cache; async provider refresh.
- Preliminary SLO targets on warm desktop: global activation to interactive first frame p95 <100ms, target <50ms; local query update p95 <20ms, target <10ms; preview metadata visible p95 <50ms if cached. Measure hardware and include idle/warm/cold. Never present targets as guarantees.
- No slow network call or directory traversal on GPUI main thread. Search query debounce only for remote, not local.

### 5.6 Actions and composition

**Typed action interface** with ID, label, icon, required capabilities, target validation, result and optional undo. Actions used by UI, CLI and IPC, with permissions per surface.

`CopyCanonicalPath`, `CopyDeployedPath(agent/scope)`, `CopySourceUrl`, `CopySkillMarkdown`, `CopyReference`, `RevealFolder`, `OpenRepository`, `UseSkill`, `ComposeWithSkill`, `InsertAtTarget`, `Install`, `Deploy`, `Update`, `RemoveDeployment`, `RepairLink`, `Edit`, `ShowDetails`.

- Copy != paste; expose distinct user intentions. Copy uses clipboard across macOS and Linux. Paste/insertion is capability-checked, permission-aware, and reports actual result.
- Compose output is adapter-defined; never assume slash commands work. Supports skill invocation token, path reference, SKILL.md text, repository URL and composed prompt; user can edit template.
- Snapshot target before launcher takes focus, validate immediately before insertion, do not send to stale or changed session.
- For unsupported external agent, copy composed payload and show precise fallback message (e.g. “Copied — paste into active agent”).
- Always permit using a remote skill as a URL/reference without installing when a compatible format exists; don't claim remote files are local.
- Optional multi-skill composer: attach several ordered skill chips and one prompt; show token/length estimation if accurately calculable.

### 5.7 Platform adapters — macOS

- Native AppKit GPUI integration, stable bundle identifier, proper app activation and floating panel. Optional transparency off by default. Support multi-monitor and HiDPI.
- Configurable global keyboard shortcut (test conflicts and resume after sleep/login). Capture foreground app/project/session via public APIs and explicit user permission; Accessibility permission only where needed.
- Clipboard use GPUI/macOS APIs. Native agent integrations first; accessibility/automation insertion only on explicit user initiation, with target verification and user visibility.
- Menu bar/tray optional; single-instance launcher, login startup, background persistence with window hidden.
- Test native Fullscreen Spaces behavior, window ordering, dock vs menu bar activation, focus restoration and keyboard-only usage.

### 5.8 Platform adapters — KDE Plasma Wayland (mandatory)

- **Native Wayland GPUI backend; no XWayland requirement.** Real KDE Plasma testing.
- Global shortcut via XDG Desktop Portal GlobalShortcuts where implemented by current KDE; desktop shortcut invoking CLI/IPC as fallback. Detect portal availability and show setup guidance.
- Use stable application id and matching `.desktop` entry; correct xdg-activation handling and compositor focus constraints. Do not assume client can force arbitrary topmost/position; test quick-panel placement. If needed implement KDE-specific KWin/portal adapter with documented permissions, not brittle unsafe hacks.
- KDE StatusNotifierItem via D-Bus (`ksni` where compatible); process persists when all windows hidden; systemd --user service optional and correct Wayland session environment.
- Wayland clipboard via supported APIs; arbitrary active-app window interrogation and key injection must not be assumed. Where native agent integration absent, safe copy fallback is the supported UX.
- Test scale=1 / 1.25 / 2, two monitors, launcher on active output, workspace switching, open-close 100x, focus handling, stale clipboard, shortcuts after session restart, deny-portal case, resizing/moving, borderless controls, tab/arrow navigation, tray and power resume.
- Investigate Rox's patched GPUI source for missing upstream fixes; only port patches after reproduction and upstream comparison.

### 5.9 Optional remote host management

Phase after core: register remote Linux/macOS host via SSH with verified host key; browse remote skills and deploy **copies**, not local symlinks; separate host and path identity; record provenance. Do not require Tailscale if LAN SSH works; remote network features remain opt-in. Later may use a small remote helper daemon, but first prefer direct SSH/SFTP with explicit scopes.

### 5.10 Themes, layout editor and settings

- Use FluxDown-style versioned token schema for colours, spacing, fonts, radii, shadow, focus, density and component variants; separate `dark` and `light` tokens.
- Theme editor can preview the manager and quick panel, apply/import/export/share/revert, per-panel override, density compact/balanced/comfortable.
- Avoid garish category palettes: neutral icons and grey labels; only source logo retains brand colour.
- Rox-style design mode in main workspace: rearrange panels, resize, tabs, popout, duplicate with independent configs, named layout presets; a stable locked normal mode.
- Quick panel uses small preset configuration only: Compact / Split / Expanded with individually editable dimensions and preview side.
- Settings screen modeled on FluxDown Shortcuts: searchable sidebar, flat rows, no giant feature cards.

### 5.11 IPC + CLI + optional MCP

- CLI commands: `skillmgr search`, `show`, `inspect`, `paths`, `install`, `deploy`, `sync --dry-run`, `repair`, `remove-deployment`, `validate`, `launcher show`, `settings export`, `diagnostics`.
- Typed local JSON-RPC over Unix socket on macOS/Linux, mode 0600 and owner verification; commands separated into read-only versus mutating; optional request confirmation for deployment/session-insertion; no unauthenticated TCP control.
- Event subscriptions for index refreshed, deployments changed, previews loaded, job status, launcher shown; errors are typed.
- Provide a CLI operation even if GUI not running where safe, or clear automatic background-start semantics; don't spawn several conflicting managers.
- MCP server later exposing safe read-only skills inventory first; mutating tools require explicit control and permissions.

### 5.12 Security and privacy

- Treat remote `SKILL.md`, metadata and scripts as untrusted data, never instruction to the installer itself. No automatic execution.
- Download via HTTPS with limits; verify Git refs/versions/hashes, record source URL and last known digest. Path sanitisation, archive zip-slip and zip-bomb prevention, preview escaping, HTML sanitisation, link safety.
- Permissions per provider and local root. User controls network discovery, telemetry off by default, no secret/token in logs; credentials in OS keychain/secure store.
- Provenance display with source type, signed/unsigned status if verifiable, local modifications, file list and permissions/capabilities.
- Trust prompts for executable integration plugins; hash packages and revoke approval on change. Sandboxing when feasible, otherwise explicit warning and restricted operations.
- No privilege escalation required for ordinary skills; no `sudo` scripts automatically.

---

## 6. Phased build order and gates

### Phase 0 — Repository audit, compatibility & prototype (gate: proof of feasibility)

1. Clone/pin Rox and Sailry source revisions; inspect modules in §3 and create `reference-audit.md`.
2. Determine licensing policy; document restrictions on GPL/AGPL borrowing, icons/assets and SDK dependencies.
3. Choose GPUI/GPUI Kit revision based on **compiling macOS and KDE Wayland prototypes**, not popularity.
4. Prototype: show GPUI window and text input; virtualised 10k-row list; Markdown + code preview; windowless process reopens window; macOS hotkey and KDE portal shortcut proof.
5. Capture both OS screenshots, benchmark event latency, record unsupported APIs; ADRs for platform and renderer.

**Exit:** working minimal executable launched on both platforms; no major dependency/API mismatch hidden.

### Phase 1 — Domain + persistence + local scanner (gate: headless skills inventory)

1. Create core structs, database/migrations, typed errors, filesystem root configuration.
2. Implement `SKILL.md` parser/validator and full file inventory.
3. Read-only scanning of configured library roots and installed agent directories, including broken links and symlink metadata.
4. Implement rebuildable SQLite catalogue and file watchers + periodic reconciliation.
5. Fixture-based tests for nested categories, unknown metadata, malformed skills, unreadable files, symlink loops, duplicate names.

**Exit:** `skillmgr search/list/inspect/validate` works offline, with meaningful errors.

### Phase 2 — Deployment engine (gate: safe multilateral installs)

1. Agent compatibility registry (verified target paths for installed versions).
2. Operation planner with simulation and ownership model.
3. Copy, symlink, managed-copy, link-source, import/adopt/unlink/move; transaction journal and recovery.
4. Bulk collections and agent × skill matrix model; drift detection.
5. End-to-end tests with temporary roots, link loops, file changes, collisions, permissions and interrupted operations.

**Exit:** CLI can safely deploy multiple skills to at least three agent root types, dry-run + rollback/recovery; unmanaged target never overwritten.

### Phase 3 — Nucleo index + native preview (gate: fast core UX)

1. Build Nucleo incremental ranking and source filtering.
2. Implement virtualised results, highlighted matches, keyboard navigation.
3. Implement shared `SkillPreview` with Markdown/raw/code/file tree/reference navigation, caching, cancellation and safe file access.
4. Implement action registry and clipboard copy variants; test selection stability.
5. Benchmarks 10k/100k entries and rapid preview switching.

**Exit:** local search, preview and actions entirely keyboard controlled and responsive offline.

### Phase 4 — Quick Launcher + OS integration (gate: mandatory core launch)

1. Create hidden/reusable GPUI floating window and three layout presets.
2. Bind global hotkey on macOS and KDE portal on Wayland; desktop shortcut fallback.
3. Implement result/preview navigation stack, action palette, compose query, target context snapshot.
4. Agent session adapters, with at least a safe clipboard fallback everywhere and one validated native adapter where possible.
5. Test always-on availability, focus, text input, multi-monitor, 100 show/hide cycles, start on login.

**Exit:** from another application, global key opens immediate search + preview; compose/copy works on both OSs; unsupported insertion is clearly explained.

### Phase 5 — Full GPUI app + editable workspace (gate: complete manager)

1. Implement left rail, Explore, Installed, Skill Detail, Library, Deployments, Agents and Settings.
2. Add grid/list toggle and subtle 2-level cards, dense table, inspector, context menus.
3. Integrate install preview and deployment matrix with actual engine, long-running task progress.
4. Add Rox-inspired dock layout and Design Mode; persist named workspaces separately from theme packages.
5. Apply light/dark FluxDown-inspired theme tokens; verify scaling and accessibility.

**Exit:** all J1, J4, J5, J7, J8 function, not only mocked.

### Phase 6 — Marketplace sources + updates (gate: discover & install remote skills)

1. Local provider first; GitHub via verified repo API and repository paths; skills.sh via documented supported interfaces; LobeHub via only authorized/public integration.
2. Cache summaries/file trees/files with TTL and fingerprints; rate limits, cancellation and offline search.
3. Resolve canonical source identities, dedup without losing variants, download and validate packages in staging.
4. Show source-specific metadata, no invented ratings/download counts; source errors isolated by provider.
5. Install/update/rollback provenance, version compare only where meaningful.

**Exit:** can discover and install from at least two verified remote methods, preserve correct source/file metadata and work offline with cached summaries.

### Phase 7 — Advanced actions, remote & extensions (gate: integrations)

1. Built-in agent connectors for Claude Code, Codex, OpenCode and other supported adapters.
2. Project/session target snapshot and capability-specific insert/compose behavior.
3. Optional remote SSH skill roots and copying into target machine; no fake cross-host symlinks.
4. Read-only local IPC/CLI/MCP and gated mutable operations.
5. Optional plugin marketplace adapters with manifests/permissions/hashes and audit UI.

**Exit:** session-aware actions target correct app/agent where supported, and fallback consistently elsewhere; remote sync handles actual host boundaries.

### Phase 8 — Hardening, packaging, performance and release (gate: production)

1. Profiling and memory ceilings, fuzz property testing parser/path operations, cancellation races.
2. Security review of path handling, permission prompts and provider networking.
3. Screenshot regression tests and keyboard-only E2E on both OSs.
4. macOS signed/notarized package when credentials exist; Linux Arch PKGBUILD/AUR-friendly tarball and optionally AppImage/Flatpak. Do not claim signed deliverable without signing keys.
5. Documentation: install, migration, settings, user guide, troubleshooting, keyboard cheat sheet, developer API, release notes, compatibility matrix.

**Exit:** no P0/P1 bugs, full acceptance checklist passed on macOS and native KDE Wayland session.

---

## 7. Acceptance test matrix

| ID | Test | Pass criterion |
| --- | --- | --- |
| A01 | macOS install/start | Native main app + hidden launcher controller runs, no runtime panic |
| A02 | KDE Wayland install/start | Native Wayland app, no required XWayland session |
| A03 | Global hotkey | Opens responsive launcher from unrelated foreground app, user configurable |
| A04 | Search offline | Local and cached results appear without network; fuzzy matching correct |
| A05 | Search online | Provider results load asynchronously, do not steal selection, cancel stale searches |
| A06 | Preview | Complete SKILL.md + nested references/scripts/assets, keyboard access, real files |
| A07 | Preview concurrency | Rapid selection never shows content for the wrong skill |
| A08 | Compose | Select skill, type query, preview exact payload, copy/action works |
| A09 | Session target | Captured agent/session validated; stale target not silently used |
| A10 | Paste fallback | Clipboard copy truthful and working on KDE Wayland when injection unavailable |
| A11 | Library | Nested physical categories preserved, collections independent |
| A12 | Deploy link | 1 master skill linked into 3 agent roots and readable from all |
| A13 | Deploy copy | 3 independent copies tracked; local edit divergence detected |
| A14 | Conflict | Existing unmanaged directory never overwritten or removed |
| A15 | Broken symlink | Repair operates on owned link only; does not remove target data |
| A16 | Bulk rollback | Interrupted partial deploy recovers or offers precise compensation |
| A17 | Agent path | Global and project paths resolved via correct selected agent adapter |
| A18 | Marketplace | Downloaded folder validated, source verified, safe cache and update behavior |
| A19 | Theme | Restrained dark/light styles, edit tokens, reset/reload, no bright-green overload |
| A20 | Workspace | Custom layout saved/reloaded and lockable in normal mode |
| A21 | Keyboard | All primary workflows complete without mouse |
| A22 | Multi-monitor | KDE and macOS scaling/position/focus validated |
| A23 | Performance | Measured p50/p95 results documented with 10k and 100k fixtures |
| A24 | Privacy/security | Untrusted skill preview doesn't execute; no secrets logged; explicit mutation confirmations |
| A25 | Persistence | UI layout, indexing, deployment manifest and shortcuts survive restart |
| A26 | Packaging | Repeatable macOS & Arch/CachyOS release builds with recorded dependencies |

Include focused unit, integration and end-to-end tests, not just manually checked UI.

---

## 8. Engineering workflow and PR/commit sequence

1. `build: bootstrap workspace and platform probes`
2. `add(core): model skills, sources, agents and deployments`
3. `add(format): validate SKILL.md and enumerate resources`
4. `add(storage): catalog, migration and filesystem scanning`
5. `add(deploy): plan symlink and copy operations with recovery`
6. `test(deploy): cover ownership and path safety scenarios`
7. `add(search): integrate nucleo with indexed skill snapshots`
8. `add(preview): render markdown, files, references and code`
9. `add(actions): typed copy, open, install and compose operations`
10. `add(platform): macOS global hotkey and quick window`
11. `add(platform): KDE Wayland shortcut, tray and activation`
12. `add(launcher): split preview, keyboard navigation and compose`
13. `add(ui): native marketplace, library and agent matrix`
14. `add(workspace): dock design mode and saveable layouts`
15. `add(theme): restrained dark/light theme and settings`
16. `add(providers): verified remote discovery adapters`
17. `add(integration): native agent session adapters and safe fallbacks`
18. `add(ipc): local CLI, event stream and opt-in MCP`
19. `test: cross-platform regression and performance suite`
20. `build: signed/reproducible packaging and release documentation`

Every PR should include tests and a brief section: impact on macOS, KDE Wayland, storage security and startup latency. Keep changes separated by subsystem for review.

---

## 9. Deliverables required from the coding agent

- A complete Rust workspace, pinned lockfile, a clean two-platform build command and packaged binaries.
- Full GPUI app and a fast global quick panel with preview/navigation/actions, real data not fake UI.
- Configurable local master libraries, categories, logical collections and agent path registry.
- Transactionally planned copy/symlink/link deployments, source provenance and repair UI.
- Nucleo fuzzy search and cached remote provider adapters.
- macOS and KDE Wayland platform adapters with honest fallbacks.
- Theme schema + dark/light presets and user-controlled layout/density/shortcuts.
- Test fixtures, automated unit/integration/E2E suites, benchmark harness and platform testing log.
- User docs, developer architecture, security notes, provider integration guide and reference audit.
- Evidence screenshots of all screens and a short screen recording of actual quick-launch workflow on both systems.

### Final instruction

**Do not stop at prototypes or mockups.** Deliver progressively runnable, tested vertical slices, with Phase 4 launcher and Phase 2 safe deployment treated as highest-priority product fundamentals. Refuse unsafe filesystem operations, do not fabricate support for unknown agent sessions or private provider APIs, and document every platform limitation. The final product should feel visually as restrained as FluxDown/Rox, be as navigable as Zed/Sailry, search as quickly as Nucleo, and manage skill libraries more flexibly than a simple marketplace installer.

---

## 10. Direct reference URLs

- Rox: https://github.com/zealsprince/rox
- Rox quick play: https://github.com/zealsprince/rox/blob/main/crates/rox/src/quick_play.rs
- Rox search window: https://github.com/zealsprince/rox/blob/main/crates/rox/src/search_window.rs
- Rox keymap: https://github.com/zealsprince/rox/blob/main/crates/rox/src/keymap.rs
- Rox dock: https://github.com/zealsprince/rox/tree/main/crates/rox-dock
- Rox panels doc: https://github.com/zealsprince/rox/blob/main/docs/03-implementation/06-panels.md
- Rox Wayland research: https://github.com/zealsprince/rox/blob/main/docs/0R-research/03-quit-to-tray.md
- Sailry: https://github.com/sailry/sailry-harness
- Sailry command picker: https://github.com/sailry/sailry-harness/blob/main/apps/desktop/src/command_picker.rs
- Sailry system shortcuts: https://github.com/sailry/sailry-harness/blob/main/apps/desktop/src/system_shortcuts.rs
- Sailry file picker: https://github.com/sailry/sailry-harness/tree/main/apps/desktop/src/file_picker
- Sailry content: https://github.com/sailry/sailry-harness/tree/main/crates/content/src
- Sailry AppKit: https://github.com/sailry/sailry-harness/blob/main/apps/desktop/src/macos.rs
- Sailry architecture: https://github.com/sailry/sailry-harness/blob/main/ARCHITECTURE.md
- Nucleo: https://github.com/helix-editor/nucleo
- GPUI examples: https://github.com/zed-industries/zed/tree/main/crates/gpui/examples
- Loungy: https://github.com/MatthiasGrandl/Loungy
- Vicinae: https://github.com/vicinaehq/vicinae
- fzf: https://github.com/junegunn/fzf
- GPUI Kit: https://github.com/dux-web/gpui-component
- Lucide: https://lucide.dev/
- Lobe Icons: https://github.com/lobehub/lobe-icons
- Agent Skills format: https://agentskills.io/specification
- Vercel skills CLI: https://github.com/vercel-labs/skills
- Vercel installer: https://github.com/vercel-labs/skills/blob/main/src/installer.ts
- Vercel agents registry: https://github.com/vercel-labs/skills/blob/main/src/agents.ts
- Skills Manager: https://skillsmanager.dev/
- LobeHub skill UI reference: https://lobehub.com/community/skill
