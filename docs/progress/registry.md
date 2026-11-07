# Registry lane — handoff

Status: read-only local discovery of all six kinds implemented in `crates/registry` (headless, no GPUI).

## API (crate `perkolator-registry`)
- `ScanConfig { roots, limits }`, `ScanConfig::well_known(home, projects)`, `for_fixture_home(home)`, `for_current_user(projects)`.
- `ScanRoot { path, layout: RootLayout, provider, source }`; `RootLayout` = SkillLibrary | SkillDirectory | McpConfig | AgentProfiles | Prompts | RuleFile | RuleDirectory | Plugins.
- `McpConfigFormat` = McpServersJson (incl. `~/.claude.json` `projects.<path>.mcpServers`) | VsCodeServersJson | CodexToml | OpenCodeJson (added).
- `scan(&ScanConfig, &CancelToken) -> ScanReport { records, problems, broken_links, cancelled }` — sync; run off the UI thread.
- `Registry` — `insert` (merge by id) / `get` / `iter` / `by_kind` / `find_by_path` / `count_by_kind` / `from_report`.

## Semantics worth knowing
- IDs: key = canonical path; MCP key = `<canonical config>:<entry>` or `<config>:<project>:<entry>`.
- `ConfigEntry.entry` is a JSON pointer (`/mcpServers/github`, `/projects/~1p/mcpServers/db`, `/mcp_servers/docs`).
- Agent-dir symlinks resolve after all roots: attach `Deployed{host,scope}`/`Linked` to the target record, else create a host-provider record with Canonical + Linked. Real dirs → `Unmanaged`. Broken → `BrokenLink` + problem only.
- Same physical file reached from two roots → one record (merged locations).
- Location states: Canonical files `Installed`; MCP `Enabled`/`Disabled`/`Installed` from the explicit flag.
- `.mdc` frontmatter falls back to `key: value` lines when not strict YAML (Info issue).
- Plugin hash = blake3(domain ‖ manifest ‖ sorted executable files, per-file cap); docs edits don't change it.
- Problem messages never echo file contents (TOML errors reduced to line + message).

## Fixture
`tests/fixtures/sample-home/` (relative symlinks: `.claude/skills/frontend-design`, `.claude/skills/dangling`). Expect 4 skills, 4 MCP, 1 agent, 2 prompts, 3 rules, 1 plugin, 1 broken link, 1 problem.

## Verified (Linux, CachyOS, debug build)
- `CARGO_TARGET_DIR=target/headless cargo test -p perkolator-registry -j 4` → 53 passed (30 unit, 7 fixture, 16 behaviour).
- clippy `--all-targets -D warnings` clean; `cargo fmt` applied.
- Smoke: `cargo run -p perkolator-registry --example scan_home -- ~ <project>` on real home → 237 records, 0 problems, 3.4 s (debug; not benchmarked, plugin caches likely dominate).
- UNVERIFIED: macOS paths/behaviour; Windows not considered.

## Follow-ups
- MCP `url`/`args` are stored unredacted in core types (could carry tokens) — contract question for lead.
- Watcher/incremental rescan, SQLite cache, release-mode scan benchmark.
