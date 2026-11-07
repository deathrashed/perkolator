# KDE Plasma Wayland acceptance checklist (mandatory)

- [ ] Native GPUI Wayland renderer (no XWayland dependency for core UI); test actual KWin compositor
- [ ] Stable desktop app ID and `.desktop` file; xdg-activation behavior verified
- [ ] Register global hotkey through `org.freedesktop.portal.GlobalShortcuts` if available; user-configured KDE shortcut fallback
- [ ] Warm launcher window show/hide repeatedly, input focus and Escape/back behavior
- [ ] Correct placement on multiple displays/scaling and Wayland compositor placement restrictions
- [ ] Return to previous application without arbitrary global key injection assumption
- [ ] Clipboard copy, user-authorized paste and explicit unsupported-app fallback
- [ ] Context detection: app/session via supported adapter only; no window enumeration guarantees
- [ ] Local skill inventory and nested reference preview work offline
- [ ] Symlink/copy operations tested against Linux native filesystems and permission errors
- [ ] KDE tray/autostart/session service and last-window lifecycle
- [ ] Window decorations, resizing, popup anchoring, pointer grab/drag, theme switches
- [ ] Tested with native Ghostty/Konsole, OpenCode and supported editors where available

Report observed compositor version, portal backend, GPU renderer, logs, known caveats and screenshots. Compilation alone does not pass.
