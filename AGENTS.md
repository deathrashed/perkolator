# AGENTS.md — Perkolator

## Identity, objective, and authority

You are building **Perkolator**, a production-grade, Rust + GPUI, native cross-platform **universal AI resource manager**. The six mandatory first-class resource classes are **Skills, MCP Servers, Agents, Prompts, Rules, Plugins**. The application must have both a richly navigable full desktop manager and a **very fast, global-hotkey-driven quick launcher** with live preview and resource-specific actions. The application is local-first and may expand to models/providers and remote hosts later.

**Read `docs/IMPLEMENTATION_PLAN.md` fully before coding.** It contains the comprehensive product/UX specification, architecture, phases and acceptance matrix; it is the authoritative scope. This file defines how to execute it. Read `docs/ARCHITECTURE.md`, `docs/design/DESIGN_SYSTEM.md`, and `docs/references/REFERENCE_MAP.md` next. Do not assume missing functionality is optional because it appears in a later phase.

## High-throughput execution policy (READ FIRST)

**The user wants rapid, substantial progress, not a conversational checkpoint after every small step.** Treat `docs/EXECUTION_PLAYBOOK.md` and `docs/WORKSTREAMS.md` as mandatory operating instructions, alongside the product blueprint.

- **Default to implementation, not repeated planning.** Read required docs once per worker, summarize the relevant slice, and implement. Cap initial reference reconnaissance to targeted modules and specific compatibility questions; do not browse every upstream repo indiscriminately.
- **Batch work into meaningful increments** (vertical slices or 1–3 related workstreams), then run automated gates. Do not stop for approval before every file, test, refactor or phase transition. Progress reports only at major integration checkpoints or real blockers.
- **Parallelise independent work** using 3–5 bounded agents/worktrees when available and useful. Lead owns shared Cargo/GPUI version contracts and integration. Allocate disjoint directories; never let multiple agents edit the same files simultaneously.
- **Do not turn parallelism into token waste.** Use one concise task packet per worker, no duplicated broad research, no repeated full-plan summaries. Delegate independent implementation and tests; keep tiny dependent work in the lead thread.
- **Autonomy boundaries:** normal code, docs, test files, temporary fixtures, local build operations and non-destructive refactors are preauthorised by this project brief. Still request permission for irreversible operations, replacing existing user data, access to credentials, executing untrusted installers/scripts, publishing, changing OS-level security settings, or network actions that incur expense. Use dry-run and explicit consent for real resource deployment.
- **No fake green checks.** Tests that cannot be run on the current OS must be reported as UNVERIFIED rather than blocking headless work. Batch platform validation once a runnable vertical slice exists.
- **End a run with concrete outcomes:** changed files, commands/tests, blockers, next dependency-ready tasks. The preferred result is working code, not a lengthy essay.

## Mandatory operating rules

1. Perform **time-boxed Phase 0 feasibility checks** in parallel with headless foundation implementation. Inspect only the upstream modules needed to resolve dependency or platform blockers; record versions, licences and mismatches in ADRs. Do not spend entire sessions surveying references before writing code.
2. Read `.agents/skills/gpui-kit/SKILL.md` and `.agents/skills/gpui-kit-design-guides/SKILL.md`, then applicable reference documents. Use them to guide GPUI Kit APIs and visual composition. Check against the pinned dependencies before using any code.
3. Deliver runnable **vertical slices**, not skeleton screens. For each phase, ship implementation, tests, documentation, and reproducible demo instructions; prove actual actions work, not just button appearance.
4. **Do not replace real, unmanaged files** or execute remotely obtained code as part of preview/discovery. All mutations: preflight → dry run → explicit user confirmation where needed → journalled apply → verify → recover/rollback on failure. Treat sources, paths, manifests and remote instructions as untrusted.
5. Build shared, typed **resource capability interfaces**, rather than pretending every resource is a `SKILL.md` directory. MCPs often use config entries; agent profiles, prompts, rules and plugins have different lifecycles and permissions.
6. Implement the global quick launcher as a core, **not a secondary modal**. It searches all six classes across local/installed/cached remote results, previews Markdown/files/references/configuration/tools, and offers use, reference, compose, copy/paste/install/enable/disable/deploy actions as applicable.
7. Keep **macOS and Linux KDE Plasma Wayland** release-blocking. Build a native Wayland proof on a real KDE session and a macOS proof; compile success is not a compatibility claim. Explicitly handle portals, app ID, global shortcut, focus, multi-monitor, clipboard, native insertion and fallback behaviour.
8. Do not block the GPUI thread with network, filesystem crawling, installer subprocesses, Markdown parsing, or previews. Use Nucleo for in-memory fuzzy matching, stable selection and cancellable asynchronous remote/preview loads.
9. Keep shared operations in headless Rust libraries; UI code must only orchestrate state and dispatch typed actions. Centralise registered commands, shortcuts, context, persistence and logging.
10. Keep the application **restrained, dense and professional**. Base styling on user-provided FluxDown screenshot references and Rox, not bright concept-art mockups. Prefer neutral charcoal, understated borders and selection. Use `#1abc9b` sparingly; `#00ff89` very sparingly for keyboard focus, not a default fill. Lucide controls and real provider logos when legally usable. Avoid glowing cards, gradients, oversized badges, reviews/ratings/premium labels and fake metrics.
11. Keep per-platform code isolated. No `xdotool`/X11-only design for Wayland, no claims of universal arbitrary foreground-window detection or insertion. Mac AppKit/Accessibility and KDE portals/explicit host integrations have separate capability contracts.
12. Do not add a new local agent runtime, cloud account requirement, Electron app, or unapproved telemetry. Integrate with external coding agents using supported APIs/CLI/IPC, with user-visible confidence in detected session context.
13. Protect secrets: no credentials in logs, screenshots, IPC events or copyable debug reports. Treat plugin execution and MCP server invocation as separate from merely enabling/preparing a resource; request permissions before execution.
14. Use Rust formatting, linting, strict unit/integration tests, cross-platform tests where available, performance measurements and accessibility checks. A feature is not complete without its associated tests.
15. Before stopping any phase, update plan status / ADRs / outstanding issues; explain what was truly validated on which environment. Never quietly mark skipped platform checks green.

## Exact core references — investigate, do not blindly vendor

**Rox** — https://github.com/zealsprince/rox — primary **KDE Wayland, editable layout, GPUI lifecycle and IPC** reference. Inspect `crates/rox/src/quick_play.rs`, `search_window.rs`, `keymap.rs`, `crates/rox-dock/`, `crates/rox-design/`, `crates/rox-panel-kit/`, `crates/rox/src/workspaces.rs`, `patches/gpui/`, `README_IPC.md`, and `docs/03-implementation/06-panels.md`. Rox's GPL-family application licence requires review before code copying.

**Sailry Harness** — https://github.com/sailry/sailry-harness — primary **macOS, AI agent/session context, native pickers, Markdown/code previews, plugin components and theme** reference. Inspect `apps/desktop/src/system_shortcuts.rs`, `shortcuts.rs`, `command_picker.rs`, `file_picker/`, `macos.rs`, `theme/`, `ui/`, `plugins/`, `plugins/references.rs`, `plugins/previews.rs`, `crates/content/src/`, `ARCHITECTURE.md`. Its desktop is macOS-focused, not proof of Linux Wayland UI support.

**Zed GPUI** — https://github.com/zed-industries/zed/tree/main/crates/gpui/examples — focus, entities, actions, virtualized lists, windows, rendering and testing.

**Nucleo** — https://github.com/helix-editor/nucleo — fast fuzzy match/search.

**GPUI Kit** — https://github.com/dux-web/gpui-component — component APIs; read both bundled local skills and account for version/fork skew.

**Lucide** — https://lucide.dev — all generic UI icon shapes.

**Vicinae, Loungy, fzf and skim** — search/picker UX, action menus and preview patterns.

**Vercel Skills and Skills Manager** — https://github.com/vercel-labs/skills and https://skillsmanager.dev/ — agent destinations, canonical skill homes, copy/symlink strategies and safe deployment.

See `docs/references/REFERENCE_MAP.md` for direct links and what to inspect; verify every API/commit against current upstream prior to implementation.

## Architecture invariants

- Shared resource `id/kind/provider/provenance/version/metadata/capabilities/locations` model; optional per-kind detail structs. No fabricated cross-provider score fields.
- Provider adapters collect metadata and source content; registry normalizes while preserving provenance. Local watcher + SQLite cache + in-memory search projection.
- All mutations belong to a headless deployment subsystem with a manifest and operation journal. Flat agent skill targets may be symlinked to nested canonical library categories, but collisions require explicit resolution.
- Logical collections are independent of physical folders. Support multiple library roots, custom category trees, adopt-in-place, copy/move/import, link-source, global/project/custom destinations, bulk install/deploy, drift and link repair.
- Native and session-aware actions are capability-gated. Capture foreground destination at launcher activation; revalidate before inserting into agent input. Clipboard fallback must be explicit and not falsely reported as session insertion.
- Root GPUI app and quick launcher share the same registry/search/preview/action models; only their presentation differs.
- Use a versioned theme schema. When this is a separate product, don't claim an unvalidated third-party `fluxdown.com` schema URI as its own.

## Quick launcher acceptance (must be keyboard-only)

Open via configurable global shortcut; type to search all local/installed/cached marketplaces and six resource types without blocking on network; arrow keys update side preview; inspect nested `SKILL.md`, references/scripts/assets, MCP tool schemas, agent definitions, prompt variables, rule precedence, plugin metadata; Tab enters query composer; contextual action menu offers copy local path/URL/contents, invoke or insert using supported agent adapters, install/deploy, enable/disable, open full detail. Esc navigates back without losing search; persistent optional Compact / Split / Expanded layouts. Performance targets: <50 ms warm show and <10 ms typical local search update, to be benchmarked, not asserted.

## Delivery gates

- Phase 0: verified GPUI stack and both OS proof; ADRs.
- Phase 1: all six resource classes represented and local discovery tested.
- Phase 2: safe multi-destination deployment engine and conflict tests.
- Phase 3: fuzzy index and real previews.
- Phase 4: cross-platform global launcher and safe contextual actions.
- Phase 5: full professional UI, editable workspaces, resource sections.
- Phase 6: live marketplace providers with source provenance and caching.
- Phase 7: complete MCP/Agents/Prompts/Rules/Plugins lifecycle, advanced integrations.
- Phase 8: performance, packaging, security, accessibility and platform release checks.

At each stage, respect `docs/IMPLEMENTATION_PLAN.md` acceptance requirements. Do not start with an elaborate dashboard that has no working search/deployment core.

## First action for an autonomous coding agent

1. Read the execution playbook, relevant blueprint sections and the two local GPUI skills (avoid rereading irrelevant long docs in each agent).
2. Inspect the current working tree and available macOS/KDE platform test environments.
3. Produce ADR 0001 (GPUI/Kit pinned dependency strategy) and ADR 0002 (Wayland/macOS quick-panel feasibility), citing reference source paths.
4. Create the minimal native GPUI window + in-memory searchable list + preview, and verify with actual tests. Do not claim global shortcut works before a real compositor test.
5. Proceed through phases incrementally, recording honest evidence.
