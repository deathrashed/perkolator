# Marketplace Integration Acceptance Tests

## Source selection
- [ ] Every listed source appears in Source Manager with icon/name/resource types/transport.
- [ ] Enable, disable, enable all and disable all persist across restarts.
- [ ] Disabled remote sources generate zero network/CLI requests (verified with mocks).
- [ ] Source selection independently filters global Discover and quick panel.
- [ ] Filter by one or more resource types, including skills, MCP, agents, prompts, rules and plugins.
- [ ] Missing credentials show `Needs setup`, not an endless loading state.

## Search
- [ ] Local search appears before remote completion; UI remains responsive with all providers offline.
- [ ] Different provider results merge as they arrive; per-source errors do not erase results.
- [ ] Cancelled older request never replaces newer query result.
- [ ] Pagination supports page and cursor backends; query cache and TTL.
- [ ] Dedup preserves all original source identities, source URLs and version metadata.
- [ ] Categories unsupported by a backend degrade explicitly, not silently.

## Auth / connectivity
- [ ] Public MCP Registry and GitHub-index sources work anonymously when available.
- [ ] LobeHub user OAuth and M2M status are distinguished, CLI detected, registration not looped.
- [ ] OAuth/CLI/API-key `Connect` opens documented provider flow; `Test` verifies actual search access.
- [ ] Secrets never appear in log output, TOML settings, crash diagnostic, copied URLs or subprocess argument text.
- [ ] Missing CLI returns actionable setup UI and does not crash.
- [ ] Rate limits, 401/403, revoked credentials and access-blocked sources fail gracefully.

## Resource actions
- [ ] Skill install/import uses existing staging/deployment engine; no unsafe writes.
- [ ] MCP config is target-aware; enabled/connected/healthy states differ.
- [ ] Plugin bundles list components and request approval for executable hooks.
- [ ] Agent profiles, prompts and rules preview and offer only valid target-specific actions.
- [ ] Curated-only sources never advertise nonexistent package downloads.
- [ ] Quick panel supports source filter, type filter, copy/reference and resource-specific actions.

## UX/quality
- [ ] New UI matches current neutral Rox/FluxDown styling; no unnecessary green accents or decorative cards.
- [ ] Native keyboard navigation and focus management on macOS and KDE Wayland.
- [ ] Automated provider fixtures; platform builds/test where environment available; no unverified compatibility claims.
- [ ] Source-status matrix lists actual confirmed working endpoints and external blockers.
