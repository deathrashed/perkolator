# Perkolator Architecture (planned, not yet implemented)

**Read `IMPLEMENTATION_PLAN.md` for full domain models, detailed workflows and phases.** These are invariants for the implementation, not existing code claims.

## Fundamental separation

- **Presentation:** full desktop GPUI manager, quick panel, settings and workspace designer. Share state/controllers and components.
- **Domain:** typed resource identities, six resource kinds, capabilities, provenance and relationships, source normalization.
- **Read side:** local discovery and filesystem watching, provider adapters, SQLite index, Nucleo in-memory ranked projection, preview cache.
- **Write side:** installation/deployment plans, manifest and journal, target-agent compatibility registry, rollback/recovery and health.
- **Actions:** context-dependent copy, reference, compose, invoke, install, enable/disable, toggle and remove operations.
- **Platform adapters:** macOS AppKit/global hotkeys/Accessibility, Linux GPUI Wayland/KDE portal/KWin integration and clipboard; capability-detection contract.
- **Integrations:** agent/session adapters, optional CLI/IPC/MCP API, optional SSH destinations.

## Suggested crate boundaries (to review in ADR)

```text
apps/desktop               Main GPUI UI + quick panel + settings
apps/cli                   Optional headless CLI
crates/core                ResourceKind, ResourceId, metadata, commands, errors
crates/catalog             Source registries, provenance, normalization, de-duplication
crates/providers           Local, GitHub, skills.sh, LobeHub, MCP registries etc.
crates/search              Nucleo index, ranking, query syntax
crates/preview             Markdown, source trees, references, schemas, assets
crates/library             Master folders, logical collections, import/adoption
crates/deploy              Operation planner, journal, atomic changes, recovery
crates/targets             Agent path/format capability descriptors
crates/actions             Typed, context-sensitive action catalog
crates/sessions            Capture + validate target agent/session, safe insertion
crates/platform            Platform capabilities and traits
crates/platform-macos      AppKit/macOS backend
crates/platform-linux      KDE Wayland/portals backend
crates/theme               Semantic tokens, theme IO, density
crates/storage             SQLite, migrations, caches
crates/ipc                 Authenticated local IPC + CLI/MCP optional
```

Start with a small Cargo workspace if appropriate; only split crates when dependency isolation or test boundaries demand it. Avoid a microservice design.

## Domain rules

Every resource records type, provider source, canonical identity, provenance, inspection capability, install locations, and target-specific configuration. Distinguish **available**, **installed**, **enabled**, **running**, **linked**, **copy**, **outdated**, **unmanaged**, **broken** and **unknown** where appropriate. These statuses are not interchangeable.

All six resource types share browse/inspect/action abstractions but **must not** share a fake universal installer: skills commonly deploy directories, MCPs use config + transport + credentials, prompts may insert templates, agents use profile formats, rules have scope/precedence, plugins may load executable code and require trust. Providers implement capabilities rather than forced unsupported operations.

## Reference and deployment semantics

Master skill tree may be nested, e.g. `~/Skills/Development/frontend-design/`, while agent destinations may require flat `skills/frontend-design/` entries. Symlink to canonical skill root; preserve full `SKILL.md`, scripts, assets and references. Track source-to-target manifest entries, ownership, declared version and link/copy mode. Never replace existing files or rename unmanaged directories without explicit review. Scan before apply; recheck open/write boundaries, reject traversal/cycles and detect name collisions, symlink loops, dangling links and differing edits. Avoid claiming multi-file distributed filesystem transactions are atomic; journal partial work and make recovery idempotent.

## Concurrency

UI thread: input, paint and light state updates only. Async workers: provider/network, disk/index building, parsing and previews. Preview results keyed by selection generation, cancelled on navigation. Display cached/local matches before network is available. UI never blocks on provider refresh. Explicit timeout and error surfaces.

## Platform capability contract

`GlobalShortcut`, `QuickPanelShow`, `FocusRestore`, `ForegroundContext`, `ClipboardWrite`, `PasteToTarget`, `AgentSessionInsert`, `FileReveal`, `Tray`, `Autostart`. Each returns `supported / permission-required / fallback / unavailable` with actionable explanation. Wayland must be exercised on actual KWin. No unapproved synthetic input or X11-only hacks.

## Security

Preview is inert; do not execute scripts as part of inspection. Scope remote provider downloads, pin validated identities, sanitize Markdown/HTML rendering, gate plugins/MCP execution, protect credentials, and require consent for external application insertion. Local IPC must validate peer/permissions and restrict dangerous methods.
