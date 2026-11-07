# ADR 0002 — Quick launcher feasibility on KDE Plasma Wayland and macOS

**Status:** Accepted for KDE Wayland; macOS Proposed (unverified)
**Date:** 2026-11-07

## Context

The quick launcher must appear on top, take keyboard focus and return focus when dismissed, all from a global shortcut. Wayland forbids arbitrary window placement, foreground-app inspection and synthetic input into other apps.

## Decision

- **KDE Wayland window:** the launcher is a `zwlr_layer_shell_v1` overlay surface (`WindowKind::LayerShell`, layer `Overlay`, anchored top, centred horizontally, with exclusive keyboard interactivity by default). Setting `PERKOLATOR_LAYER_KEYBOARD=on-demand` gives on-demand keyboard focus instead. Setting `PERKOLATOR_LAYER_SHELL=0`, or a layer-shell failure, falls back to a `PopUp` window.
- **Activation:** a single resident process, controlled through a per-user socket at `$XDG_RUNTIME_DIR/perkolator.sock`:
  - The socket is mode 0600, inside a directory that must be user-owned and closed to group/other.
  - It accepts only two inert verbs, `show` and `toggle`.
  - `perkolator --toggle` is the global-shortcut target.
- **Global shortcut:** the GlobalShortcuts portal is preferred when the portal exports it. Otherwise the fallback is a KDE custom shortcut running `perkolator --toggle`; `perkolator --print-desktop-entry` emits a `.desktop` entry with a `Toggle` action.
- **App id:** `dev.perkolator.Perkolator` (provisional).
- **Session insertion and paste-to-target:** reported Unavailable on Wayland. Composer output is copied, and the UI says so ("Copied … paste it yourself").

## Alternatives considered

- **xdotool / X11 key injection:** rejected (X11-only).
- **A normal toplevel window:** cannot be positioned or kept on top under Wayland, so it is used only as the fallback.

## Cross-platform behavior

- **KDE Wayland:** see the evidence below.
- **macOS:** planned as an AppKit floating panel with a native global hotkey (Sailry `system_shortcuts.rs` / `macos.rs` as references). UNVERIFIED: no Mac available in this run.

## Security

The socket accepts only `show`/`toggle` and never executes input. No input injection and no window enumeration.

## Testing and measured evidence (KDE Plasma / KWin 6.8.2, xdg-desktop-portal 1.22.1, xdg-desktop-portal-kde 6.8.2, single 1920×1080 output at scale 1)

- **Compositor protocols:** KWin advertises `zwlr_layer_shell_v1` v5 and `xdg_activation_v1`.
- **Layer surface mapped:**
  - A `WAYLAND_DEBUG` trace shows `get_layer_surface(..., 3 /*overlay*/, "perkolator-launcher")`, `set_size(1040, 620)`, `set_keyboard_interactivity(1)`, then buffer attach and commit.
  - Locating the launcher's exact theme colours in a screenshot places the surface at x≈462–1498, y≈250–866: below the top panel's exclusive zone, horizontally centred.
  - Evidence image: `docs/evidence/kde-launcher-split-fixture.png`.
- **Portal state:** the running portal frontend does **not** export `org.freedesktop.portal.GlobalShortcuts` (or Clipboard). It is using the GTK backend; `xdg-desktop-portal-kde` is installed and D-Bus-activatable but was not active. Restarting the portal is a user-session change and was not done.
- **Timings (fixture home, debug build, `PERKOLATOR_DEBUG=1`):**
  - Index of 13 fixture resources: 5.2 ms.
  - Local search: 0.02 ms.
  - Launcher window open: 2475 ms cold, about 215–223 ms warm via `--toggle`.
  - These are measurements, not the < 50 ms target; release timings are blocked by the font-loading stall in ADR 0001.
- **Not yet exercised:**
  - Multi-monitor and scaling other than 1.
  - Focus return.
  - Tray and autostart.
  - 100× show/hide.

## Consequences and follow-ups

- Implement portal registration via `ashpd` once the portal exports GlobalShortcuts; ask the user before restarting `xdg-desktop-portal`.
- Keep the launcher window alive between toggles (hide instead of destroy) to approach the warm-show target; GPUI exposes no hide for layer surfaces, so this needs investigation.
- macOS proof on real hardware.
