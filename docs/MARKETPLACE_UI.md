# Marketplace Sources UI — GPUI Add-on

Build onto the current Perkolator styling/layout and existing UI primitives. Do not introduce a second design system. Preferred look is restrained neutral Rox/FluxDown: dark greys, 1px borders, 28–40px rows, selective accent color, Lucide outline icons, no giant tiles/glow/stars/reviews.

## Main app: Source Manager

- Navigation: `Discover`, `Installed`, `Libraries`, `Deployments`, `Sources`, `Settings` as current app supports. Resource kinds: Skills / MCP / Agents / Prompts / Rules / Plugins.
- In Discover, a `Sources: N enabled` disclosure/popover next to search field; multi-select with `Select all available`, `Select none`, `Restore defaults`, per-source checkbox, type chips, auth/health status, optional `Connect` or `Manage` action.
- Source Manager page: compact table with icon, source, supported resource types, transport (CLI, API, MCP, GitHub), enabled checkbox, auth badge, connection/test status, last sync, latency, counts (if truly measured), actions. `No auth required` is neutral—not a warning.
- Filters: all enabled vs one/multiple sources, resource type(s), category, local/remote, installed/available, supported agent, scope and updates; show applied filters as removable chips.
- Per-provider detail drawer: endpoint/link; documented auth method; auth state; version/CLI availability; last successful query and errors; quota where actually provided; terms/attribution; `Connect`, `Disconnect`, `Test`, `Open docs`, `Enable`, `Refresh`.
- Search results: flat/dense virtual list default with icons, source chips, format, install status, concise description. Optional list/grid layouts. One canonical resource with multiple source labels (deduplicated).
- Clear pending/source-specific feedback; cached results remain visible with stale label if remote network fails.

## Quick panel

- Opening panel never blocks on source auth or remote network. Query cached local index immediately and dispatch remote lookups only to enabled providers supporting selected kinds.
- Filter popover in launcher toolbar, as in full app. Keyboard: open filter popover, toggle sources via Space, Apply via Enter, Escape cancel. Keep current selection when results update.
- Tabs/filters for All / Skills / MCP / Plugins / Agents / Prompts / Rules, with optional installed-only toggle.
- Split preview: SKILL.md/refs for skills; MCP tool schema and config for MCP; agent profile; plugin included resources/permissions; prompt template; rules contents and precedence.
- Context actions adapt to capabilities: copy path/URL/text, install, deploy, enable/disable, test connection, open docs, configure target, reference/copy/insert to active supported agent session. Unsupported actions hidden or disabled with explanation.
- Auth CTA when selecting inaccessible remote result (`Connect to Source`) opens correct auth flow, but the launcher remains usable.

## Exact visual behavior

- The source selector is a small quiet toolbar popover, NOT a large multi-colour marketplace card gallery.
- Neutral colour for selection rows and hover backgrounds; accent color for active selection/focus or primary action only.
- Use skeletons sparingly, no global loading spinners delaying local search; progress displayed beside each source.
- Use existing GPUI Kit component equivalents for popover, checkbox, virtual list, toolbar, scroll, split panel, markdown, tokens, tooltip and dialogs. Verify actual crate version first.
- Preserve density/theme/keybindings and platform constraints already in project.
