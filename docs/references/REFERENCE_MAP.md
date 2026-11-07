# Upstream Reference Map

**Mandatory:** study exact files, verify licence, current APIs, package versions and platform behavior before using code. Source links are to upstream projects; never imply they ship as vendored dependencies.

## Rox — primary KDE Wayland / configurable UI

Repository: https://github.com/zealsprince/rox

- `crates/rox/src/quick_play.rs`: reusable quick search, keyboard navigation, `uniform_list`, selected rows
- `crates/rox/src/search_window.rs`: reuse quick view inside full power-search window, singleton, focus
- `crates/rox/src/keymap.rs`: central command registry, rebinding, focus-context rules
- `crates/rox-dock/`: editable split/tab/panel tree, serialization, design mode
- `crates/rox-design/src/palette.rs` and `tokens.rs`: semantic styling, color vs geometry roles
- `crates/rox-panel-kit/src/window_chrome.rs`: Wayland fallback chrome/resizing
- `crates/rox/src/workspaces.rs`: shareable workspace presets
- `patches/gpui/`: geometry, drag-grab, decorations bug reports; compare to selected GPUI revision before porting
- `docs/03-implementation/06-panels.md`: panel lifecycle and serialization
- `docs/0R-research/03-quit-to-tray.md`: KDE tray and windowless lifecycle
- `README_IPC.md`, `crates/rox-ipc/`: local JSON-RPC, test/debug commands
- `README_PLUGINS.md`: consent and file-hash-based plugin change detection

**Licence caution:** Rox application source is AGPL-3.0-only; some vendored components have separate Apache-2.0 origins. Copy only after careful licence/provenance review.

## Sailry Harness — primary macOS / agent contexts

Repository: https://github.com/sailry/sailry-harness

- `apps/desktop/src/system_shortcuts.rs`: global hotkeys, context capture, verify session before insertion
- `apps/desktop/src/shortcuts.rs`: scoped command registry
- `apps/desktop/src/command_picker.rs`: searchable UI, keyboard selection, list delegates
- `apps/desktop/src/file_picker/navigation.rs`: async cancellation, generation checks, history
- `apps/desktop/src/macos.rs`: AppKit window size, native backdrop materials
- `apps/desktop/src/theme/`, `theme/floating.rs`: consistent native popup and theme treatment
- `apps/desktop/src/plugins/`, `plugins/references.rs`, `plugins/previews.rs`: resource panels, native preview and reference insertion
- `crates/content/src/markdown/`, `editor/`, `syntax/`, `diff/`: content inspection and editing
- `ARCHITECTURE.md`: presentation / protocol / host ownership boundaries

Sailry desktop currently targets macOS; Linux **host** support is distinct from native Linux desktop proof. Apache-2.0 project; verify third-party assets and modules.

## GPUI and components

- Zed GPUI examples: https://github.com/zed-industries/zed/tree/main/crates/gpui/examples
- GPUI Kit: https://github.com/dux-web/gpui-component
- Local skills: `.agents/skills/gpui-kit/SKILL.md` and `.agents/skills/gpui-kit-design-guides/SKILL.md` plus references
- Nucleo: https://github.com/helix-editor/nucleo
- Lucide: https://lucide.dev

## Launchers and deployment managers

- Vicinae: https://github.com/vicinaehq/vicinae — action palettes, keyboard UX, scripts
- Loungy: https://github.com/MatthiasGrandl/Loungy — GPUI launcher architecture, archived project
- fzf: https://github.com/junegunn/fzf — live preview, stable keyboard navigation
- skim: https://github.com/lotabout/skim — Rust fuzzy finder and preview patterns
- Vercel Skills CLI: https://github.com/vercel-labs/skills; inspect `src/installer.ts` and `src/agents.ts` for installation/canonical/link/copy modes
- Skills Manager: https://skillsmanager.dev/ — central library, multi-agent deployment, presets
- Agent Skills spec: https://agentskills.io/specification

## User-supplied visual references

- `assets/references/fluxdown/`: preferred real desktop UI, design density/settings
- `assets/references/lobehub/`: how to organise resource kinds and detail pages, **not** a desired visual style
- `assets/references/concepts/`: rough iterations, later neutral variants preferred
