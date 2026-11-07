# Providers — bundles & agent indexes (Worker C)

Code: `crates/providers/src/bundles/` (`claude_plugins`, `cursor_plugins`, `wshobson_agents` share `plugin_marketplace.rs`; `voltagent_subagents`, `awesome_claude`; helpers in `common.rs`). Tests: `crates/providers/tests/bundles_*.rs`, fixtures (real, trimmed upstream files) under `tests/fixtures/{claude-plugins,cursor-plugins,wshobson-agents,voltagent-subagents,awesome-claude}/`.

## Source status (verified 2026-11-07, anonymous, Linux)

| Source | Status | Evidence |
| --- | --- | --- |
| `claude-plugins` | working | `.claude-plugin/marketplace.json` (317 plugins; string, git-subdir, url, command sources). Live: 100-item page, hookify detail lists 25 files and flags hooks. |
| `cursor-plugins` | working | `.cursor-plugin/marketplace.json` has only name, source, description and `minClientVersions`; components come from the folder layout. Live: plugins, rules, skills and agents; rule detail parsed. |
| `wshobson-agents` | working | Claude-format marketplace `claude-code-workflows` (94 entries, 2 external git-subdir). Live: 46 agents and 35 skills on the first page. |
| `voltagent-subagents` | working | README category index has 170 entries; detail parses frontmatter (name, description, tools, model). Live search and detail pass. |
| `awesome-claude` | metadata-only | `ianwieds/awesome-claude-code` exists (HTTP 200, CC0). README sections are mapped to kinds; results offer view-source only, with no detail, fetch or install. |

## Behaviour
- Search makes one raw request for the index or README. Plugin marketplaces add one GitHub API `tree()` request, only when component kinds (skill, agent, rule, MCP) are requested; it is memoised per adapter for 10 minutes. Kinds the source does not support cause no request.
- If the tree is rate-limited, plugin results are still returned from the index; component-only searches return the error.
- Hooks, LSP servers and `source: command` entries are flagged in the summary and never run. Hook JSON appears verbatim in detail `manifest_json` for review. MCP `env`/`headers` literal values are redacted; `${VAR}` placeholders are kept.
- Ids: `plugin:<name>` and `<plugin>/<kind>:<repo-path>` (detail re-resolves them through the index), `VoltAgent/...:<path>` and `<kind>:<url>`. `repo_url` and `resource_path` are always set for GitHub resources. `total` is always `None`; the cursor is an offset.
- `package_ref` is `name@marketplace` (Claude Code) for the claude-format sources. `version` is set only when the entry declares one.

## Contract-change requests (not applied)
1. `SearchPage.warnings` so partial degradation (tree rate-limited) is visible to users rather than silent.
2. A refresh/no-cache flag on `SearchRequest` (adapter tree memo).
3. Structured `RemoteDetail`/`RemoteItem` flags (`executable_hooks`, `components`) instead of summary text, so the UI can gate approval.
4. A pinned git ref on `RemoteItem` (it is currently only embedded in `canonical_url`).

## Gates run
`cargo test -p perkolator-providers` (bundles: 36 integration + 10 unit tests pass, 5 live tests ignored); `-- --ignored` on `bundles_*`: 5/5 pass live; clippy `-D warnings` clean; fmt applied.
