# macOS acceptance checklist (mandatory)

- [ ] Native GPUI/AppKit startup and GPU rendering on supported Mac hardware
- [ ] Configurable system-wide hotkey, reserved shortcut conflict handling
- [ ] Quick panel summon/hide and restore previous focus across Spaces/multiple displays
- [ ] Accessibility permissions with explicit explanation only when required
- [ ] Clipboard copy and supported insertion into foreground terminal/editor or agent interface
- [ ] Snapshot and revalidate agent/project/session before acting (no stale target paste)
- [ ] Markdown / code / image / nested references previews and command composer
- [ ] Native menu/tray/autostart and close/quit lifecycle
- [ ] Safe symlink/copy paths with permission denied, dangling links and external volume cases
- [ ] Appearance and system theme follows macOS selection with optional neutral translucency

Verify real hardware runtime; log permissions and unsupported integrations separately.
