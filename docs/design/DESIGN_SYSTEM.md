# Perkolator Design System — restrained native desktop

## Visual authority and priorities

1. **Primary:** screenshots under `assets/references/fluxdown/`: dense desktop list/settings, subtle visual hierarchy and realistic controls.
2. **Secondary:** Rox actual UI/theme architecture; replicate semantic-role token approach, not any copyright-protected branding or unrelated features.
3. **Information architecture only:** `assets/references/lobehub/` for rich discover grids, filtered catalogs and detailed Docs/MCP/Agents/Models pages; **do not copy** social/review/score/premium badges or busy color treatments.
4. **Illustrative only:** `assets/references/concepts/`; prior mockups were progressively refined. Later `restrained-*` concepts outrank `mint-*`/`neon-*` imagery; nothing generated is a functional spec.

## Interaction aesthetic

Mac-native + KDE-native, compact and professional. Very dark neutral surfaces; low-chroma selection; clear typographic hierarchy; soft 1px dividers; tight row heights; icon-only actions where recognizable; text labels for destructive/uncommon actions; sensible keyboard hints. Icons from Lucide (uniform 1.5–2px stroke); source/agent branding from trustworthy provider artwork where usable.

**Key caveat:** source reference FluxDown uses `#1abc9b` heavily for progress/focus; this product is deliberately even more restrained following user feedback. **Do not paint entire cards, active tabs, buttons and borders bright mint simultaneously.** No vivid all-over neon highlights, glowing edges, decorative gradients or card hover animations.

## Tokens

- `background #1e1e1e`, `chrome #222224`, `surface #2c2c2e`, `secondary #31363b`
- `text #eff0f1`, `text-muted #a1a1a6`, `text-tertiary #757578`
- `border #48484a`, hairlines nearer `#3e3e40` where practical
- `primary #1abc9b` as semantic accent for sparse primary operations and select states
- `focus #00ff89` **optional limited keyboard focus only**, not the default green line everywhere
- `info #1d99f3` or Breeze blue as controlled second accent; `warning #fdbc4b`; `error #ed1515`
- border radius 3/6/8/12px, navigation rows 28px, titlebar 40px, body text 13px, caption 11px, control height 28px
- categories mostly grayscale; logos may retain authentic brand colours; metadata no invented numbers.

See `config/theme.dark.json` as a local example. Define our **own** versioned JSON schema when implementing; do not declare compatibility with FluxDown's schema unless formally implemented and tested.

## Screen inventory

Full manager: Home/Discover, Skills, MCP, Agents, Prompts, Rules, Plugins, Installed, Libraries, Collections, Deployments, Updates, Activity, Workspaces, Settings. Treat models/provider browse as future optional. Support dense list/grid toggle and inspector, tabs, collapsible details, native previews and per-type configuration panels.

Quick panel: compact list; split result+live preview default; expanded documents layout. Search first; result highlights preview instantly; keyboard actions and Tab→compose. No need to reproduce the entire app navigation inside launcher. Preview nested references, scripts, assets and metadata in context; retain selection/query on back.

Install/deploy: target matrix + scope + link/copy mode + dry-run plan + safe apply + state/result log, all in neutral native controls.

Settings: FluxDown-style 2-column navigation, shortcuts rows with keycaps, Appearance/theme/density, Agent Targets/paths, Library Roots, Provider Sources, Launcher actions/layout, Platform Permissions/Integrations and Security.

## UX accessibility

Contrast at least readable against dark backgrounds; keyboard-only result navigation and file tree; focus visibility without garish fill; disable controls when unsupported with explanation; live loading/error states; no accidental destructive Enter. Persist user-adjusted panel size, window location if compositor permits, selected theme, density and shortcuts.
