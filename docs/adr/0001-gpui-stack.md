# ADR 0001 — GPUI / GPUI Kit stack

**Status:** Accepted
**Date:** 2026-11-07

## Context

Perkolator needs one native Rust UI stack that renders on macOS and on KDE Plasma Wayland, provides text input, virtual lists, Markdown and theming, and can host a launcher overlay. Rox pins an old patched GPUI, and Sailry pins a different GPUI Kit Git revision. Mixing them unreviewed is forbidden by the plan.

## Decision

- Depend on **`gpui-kit = "=0.9.0"`** (crates.io, Apache-2.0, repository `longbridge/gpui-kit`) with `default-features = false, features = ["component", "assets"]`. It pins the published GPUI fork **`gpui-pre =0.3.11`** (Wayland + X11 enabled by default) together with `gpui-component 0.9.0` and `gpui-base 0.9.0`.
- Applications import GPUI only through `gpui_kit::*`, as the bundled skill requires.
- Toolchain: `rust-toolchain.toml` uses `stable` (rustc 1.99.0 on the build machine).
- Icons come from `gpui_kit::assets::AllAssets`, the full Lucide catalog (1830 SVGs). The default `Assets` only embeds GPUI Kit's own subset.
- The GPUI target directory lives outside `/home` on the build machine (`CARGO_TARGET_DIR=/mnt/bulk/projects/perkolator-target`) because the root filesystem has under 7 GB free. Headless crates use `target/headless`.

## Alternatives considered

- **Upstream Zed `gpui` via Git:** unpublished, moves daily, and needs a separate component library.
- **Rox's patched GPUI:** old, AGPL-family application context, and Wayland patches would need re-verification.
- **Sailry's GPUI Kit Git revision:** macOS-focused and differs from Rox.

## Source references and licensing

`gpui-kit`, `gpui-component`, `gpui-base` and `gpui-pre` are Apache-2.0; Lucide is ISC (`LICENSE-LUCIDE` in `gpui-kit-assets`). No Rox (AGPL) or Sailry source has been copied.

## Cross-platform behavior (macOS / KDE Wayland)

- **KDE Wayland:** verified on this machine (see ADR 0002).
- **macOS:** UNVERIFIED. No Mac build or run has happened yet.

## Security and backward compatibility

The pin is exact. Upgrade only through a new ADR plus re-running the platform checks.

## Testing and measured evidence (CachyOS, KDE Plasma 6.8.2 Wayland, x86_64)

- **Build times:** a clean debug build of the GPUI stack took 5 min 07 s (`-j 4`); a release build (`lto = "thin"`) took about 8 min 20 s.
- **API skew against the bundled skill docs (written for 0.7):**
  - `Tab::new()` takes no id.
  - `Kbd` has no size modifiers.
  - Keystroke strings must be `ctrl-`/`cmd-`; `secondary-` panics at `KeyBinding::new`.
  - Async `Entity::update` returns the value directly, while `WeakEntity::update` returns `Result`.
- **Input key routing:** binding `"PerkLauncher > Input"` after `gpui_kit::init` overrides the text field's own up/down/enter/escape/tab. The keymap ranks bindings by depth, then later-added first; this was verified in `gpui-pre-0.3.11/src/keymap.rs`.

## Consequences and follow-ups

- **Release cold-start stall:** `cosmic_text::FontSystem::new` loads every system font before the Wayland connection opens. More than 4 s was observed with a release binary on this machine. Investigate restricting font loading to the bundled and default UI fonts.
- **GPUI UI integration tests** (`#[gpui_kit::test]`) are not written yet.
