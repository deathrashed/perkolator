# actions-theme lane — handoff (2026-11-07)

## Status
- `perkolator-actions`: registry, resolver, template engine, composer, MCP snippet redaction, truthful delivery. 29 unit tests.
- `perkolator-theme`: schema v1, `Rgba`, WCAG contrast, built-in dark/light JSON (`crates/theme/themes/`). 14 unit tests.
- Headless only; no GPUI. Validated on Linux (CachyOS) with `cargo test`/`clippy -D warnings`/`fmt --check`.

## Key decisions
- Default (Enter) action: Skill/Agent `CopyInvocation`, MCP `CopyConfig`, Prompt `ExpandTemplate` when it has
  something to expand (Arguments syntax, or DoubleBrace with declared variables) else `CopyContent`, Rule `CopyContent`,
  Plugin `ShowDetails`. Context-independent so launcher and manager agree; Compose is Tab in the launcher.
- Path actions (CopyPath/Reveal/CopyContent/CopyDeployedPath/Compose/Insert/CopyReference/CopyInvocation/Expand)
  need `Capability::LocalPath` **and** a location (U10). MCP never gets `CopyContent` (config files hold other entries'
  secrets); its CopyPath is labelled "Copy config file path".
- `resolve` re-checks offered + available; mutating ids always → `OpenDeployPlanner`. Insert resolves to the text
  payload; `deliver()` decides Insert vs `ClipboardFallback{reason}` after `validate_session(captured, current)`.
- Session age = `current.captured_at − captured.captured_at` (no wall clock in validation); `ActionContext.current_target`
  must be re-probed by the platform layer immediately before delivery.
- MCP snippet redaction also covers URL userinfo/query strings and args after/inside `--*token|key|secret|password|auth*`.
- Theme validation: sizes in range, fg/bg ≥ 4.5 (fg on background/surface/chrome, muted, selection fg), tertiary ≥ 3.0,
  `selection` ≠ `primary`/`focus`. Own schema; no FluxDown compatibility claim.

## Contract-change requests (core is frozen)
1. `PlatformFeature`: derive `PartialOrd, Ord` so `ActionContext.platform` can be a `BTreeMap` (now `HashMap`).
2. Optional: `ActionEffect::InsertText { payload, target }` if the UI wants insertion as a first-class effect
   (today: `WriteClipboard` + `deliver()`).

## Next dependency-ready tasks
- Desktop/launcher: render `actions_for` entries; wire Tab → composer using `compose_effect`/`compose_payload`.
- platform-linux: fill `ActionContext.platform` from probes (AgentSessionInsert = Unavailable on KDE Wayland with
  explanation "No verified session adapter on KDE Wayland").
- Map `ThemeFile` tokens to gpui-kit theme (`Rgba::to_u32` = 0xRRGGBBAA). Theme file loading from user dir is not done.
- Unverified: any real clipboard/insert behaviour (needs platform layer + compositor test).
