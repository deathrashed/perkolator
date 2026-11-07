# Fast Parallel Marketplace Integration

This is an **add-on feature**, not a new project scaffold. Follow existing Perkolator AGENTS.md and workspace setup. Work in sizable batches; no user confirmation for ordinary implementation decisions. Ask only for real credentials, destructive changes or irreversible actions. Prefer tests/recorded evidence over repeated human reviews.

## Lead contract (brief, not an approval gate)

Lead should spend at most one initial pass finding the current provider/search/auth interfaces and file ownership; define adapter contracts and assign worktrees. Do not have every worker reinvent core traits. Reuse existing components/models. Ensure single owner of shared schema/provider registry; workers touch disjoint provider modules and tests.

## Independent workstreams

**Worker A: skill catalogues** — `lobehub`, `skillssh`, `skillsmp`, `voltagent-skills`, `officialskills`; CLI detection, JSON search, GitHub curated index parsing. Does not touch central UI.

**Worker B: MCP registries** — `mcp-official`, `smithery`, `glama`, `pulsemcp`; registry pagination, auth capability flags, CLI probes, MCP metadata. Does not touch central UI.

**Worker C: bundles and agents** — `claude-plugins`, `cursor-plugins`, `wshobson-agents`, `voltagent-subagents`, `awesome-claude`; manifests and read-only component previews; no automatic hook execution. Does not touch central UI.

**Worker D: prompts and shared auth** — `prompts-chat`, `github-direct`, source credential interfaces and CLI auth intents; avoid writing UI files. This worker coordinates auth contract with lead before modifying common auth module.

**Worker E: UI (can start after contract)** — existing GPUI source selector, status rows, filters in full manager + launcher. Use mock provider data first; integrate providers through shared registry. No provider-specific HTTP in UI.

**Lead: integration** — source config schema, scheduler, cache, normalisation, dedup, interop with deployment/action engine, integration tests, merges and release notes.

## Fast integration discipline

- Each worker produces working adapter(s) plus focused contract tests in a bounded change set.
- One build owner for heavyweight Rust workspace validation to avoid parallel RAM/IO saturation; others run targeted tests without compiling entire app repeatedly.
- Merge at functional vertical slices, not each small edit; batch progress reporting at milestone boundaries.
- Keep CLI optional. Feature detects binary + versions, defaults to informative `CLI missing` rather than running automatic npm installs.
- No fake API credentials, no scraping of restricted endpoints, no installation of remote execution files as tests.
- If public API access blocked, mark disabled/degraded, provide official auth/setup and continue other providers.

## Done criteria

At least one live authenticated provider test (where credentials available) for each transport category; offline deterministic fixtures for every provider; all source selectors and auth actions wired, no silent unsupported actions, full quick-panel search works with enabled sources only, and acceptance checklist pass. Provider sources blocked by external access are explicitly documented and do not block shipping others.
