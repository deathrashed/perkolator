# Integration status

Updated by the lead at integration milestones. "Verified" means it was run on the stated environment; anything else is labelled.

Environment for every "verified" row: CachyOS x86_64, KDE Plasma / KWin 6.8.2 Wayland, rustc 1.99.0. No macOS machine was available.

| Milestone | State | Evidence / test | Platform |
|---|---|---|---|
| GPUI stack pinned | Done (ADR 0001) | Debug and release builds of `gpui-kit =0.9.0` / `gpui-pre =0.3.11` | KDE: built and ran · macOS: UNVERIFIED |
| Six-type core library | Done | `perkolator-core` (17 tests); `perkolator-registry` discovers all six kinds from `tests/fixtures/sample-home` (54 tests) | Headless, Linux |
| Safe deployment engine | Done (engine) | `perkolator-deployment` + `perkolator-targets` (45 tests): symlink ×3 (A12), copies with divergence (A13), unmanaged never touched (A14), owned-link repair (A15), interrupted-run recovery (A16) | Headless, Linux · macOS UNVERIFIED |
| Nucleo search + preview | Done (headless) | 39 tests. Release bench: 10k docs p50 2.0 ms / p95 6.0 ms; 100k docs p50 22 ms / p95 55 ms (over the 10 ms target) | Headless, Linux |
| Actions / composer / theme | Done (headless) | 43 tests: capability gating (U10), redaction, truthful clipboard fallback (A10) | Headless, Linux |
| Quick launcher | Partial | Layer-shell overlay mapped on KWin, with keyboard nav, async preview, action menu, composer and IPC toggle (`docs/evidence/kde-launcher-split-fixture.png`). The portal global shortcut is not exported on this session, so a KDE custom shortcut is the fallback. Warm open ≈215 ms in debug; release blocked by the font-loading stall | KDE: partial · macOS: UNVERIFIED |
| Marketplace runtime | Done (headless) | `perkolator-marketplace` (17 tests). Remote search is opt-in and off by default; disabled sources make zero requests; per-source timeouts; stale results dropped; provenance-preserving dedup | Headless, Linux |
| Marketplace providers | In progress | 16 adapters across 4 workstreams; live status per source is recorded in the `docs/progress/providers-*.md` files | Linux |
| Full manager window | Not started | — | — |
| Real integration actions (session insert) | Not available | Wayland reports Unavailable; copy fallback only | KDE |

## Known blockers and caveats

- **Global shortcut:** the running `xdg-desktop-portal` serves the GTK backend and does not export GlobalShortcuts. `xdg-desktop-portal-kde` is installed but inactive. Fixing it means restarting the portal user service (user decision).
- **Release startup:** `cosmic_text::FontSystem::new` stalls on system font enumeration (more than 4 s before any window).
- **macOS:** no build or run on Apple hardware yet.
