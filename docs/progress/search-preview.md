# search-preview lane — handoff

Status: headless implementation complete; GPUI binding not started (owned by UI lane).

## perkolator-search (`crates/search`)
- `SearchDoc::from_record` (adds `remote: bool`; `extra` = categories, file names, hosts, tool/variable names, capped at 48).
- `Query::parse` — leading `skill:`/`mcp:`/`agent:`/`prompt:`/`rule:`/`plugin:` (plurals, glued `mcp:git`) and `@installed`/`@local`/`@remote`/`@all`; remaining text supports nucleo/fzf atoms (`^`, `$`, `'`, `!`).
- `SearchIndex` (`new`, `from_records`, `from_docs`, `upsert`, `remove`, `get`, `len`, `is_empty`, `search`) and `reselect`.
- Ranking: strict tiers `NameExact > NamePrefix > NameFuzzy > Secondary(tags/publisher) > Summary > Extra` (`SearchHit::tier`), each atom scored on its best field; boosts (installed 120, favorite 400, recent up to 300) reorder only within a tier and never drop. Tie-break: score desc, name (case-insensitive), id string. Empty text: installed, kind order, name.
- One `nucleo_matcher::Matcher` behind a `Mutex` (index is `Send + Sync`); contention falls back to a temporary matcher.

## perkolator-preview (`crates/preview`)
- `build_preview(record, generation, limits, cancel)`; `read_confined` / `preview_file`; `file_tree` (returns `FileTree { nodes, truncated }`); `parse_markdown`; `mcp_config_snippet`, `redact_args`, `redact_url`; `PreviewScheduler` (`begin`, `is_current`, `current_generation`, `cancel`).
- Tabs: Skill = Overview/Files/Metadata; MCP = Config/[Tools]/Metadata (no-tools note); Agent = Overview/Metadata; Prompt = Overview/Template/Metadata; Rule = Overview/Precedence/Metadata; Plugin = [Overview]/Files/Permissions/Metadata.
- Safety: canonical containment, escaping symlinks refused, trees never follow symlinks, regular files only, size cap, NUL/invalid UTF-8 = Binary, likely-credential files (`.env`, `*.pem`, `id_rsa`, ...) refused, `.git`/`node_modules`/`target` not descended. MCP snippet is rebuilt from typed details (file not re-read) with env/header via `display_value()`, secret flags, `KEY=value`, auth headers, token-shaped args and URL userinfo/query/path tokens redacted.

## Evidence (Linux, CachyOS, headless)
- `CARGO_TARGET_DIR=target/headless cargo test -p perkolator-search -p perkolator-preview -j 4` — 39 passed.
- `cargo clippy ... --all-targets -- -D warnings` clean; `cargo fmt` applied.
- `cargo run --release -p perkolator-search --example bench_search` (20 queries x 25 runs, limit 50):
  10k docs: build 14 ms, p50 2.0 ms, p95 6.0 ms. 100k docs: build 151 ms, p50 22 ms, p95 55 ms.

## Open issues / next
- 100k docs exceeds the 10 ms typing target; options: shard over threads, or the threaded `nucleo` crate. 10k is within target.
- Only subsequence matching: dropped-letter typos match, substitution/transposition typos do not.
- `name_match` indices are nucleo grapheme indices (equal to char indices for ASCII).
- UI must run `search`/`build_preview` off the GPUI thread and drop results with `!scheduler.is_current(gen)`.
- Tree walk cancellation is checked per directory; a single huge directory is listed before the next check.
