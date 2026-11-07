# Providers — prompts & generic GitHub (Worker D)

Code: `crates/providers/src/prompts/{prompts_chat,github_direct}.rs`, registered in `prompts::providers()`.
Tests: `crates/providers/tests/prompts_{chat,github_direct}.rs`; fixtures under `tests/fixtures/{prompts-chat,github-direct}/` (trimmed real responses, captured 2026-11-07).

## prompts-chat — working (anonymous)
- Documented REST `GET https://prompts.chat/api/prompts?q=&type=&page=&perPage=` (≤100/page), detail `GET /api/prompts/{id}`.
- `type=BOT` → **agent** (bots carry instructions, skills, routines, connectors and per-platform install entries); TEXT/STRUCTURED/IMAGE/VIDEO/AUDIO/TASTE → **prompt**; SKILL/CLASSIFIER skipped (not declared kinds).
- Up to 2 requests per page (prompts + bots); page cursor; `total` only for bot-only queries (untyped total includes skipped skills).
- Category filter ignored (API takes internal category ids). Content CC0 per upstream LICENSE. MCP (`POST /api/mcp`) not used: seam is GET-only.

## github-direct — working (anonymous; token optional, slot `github`)
- Repo location query (`owner/repo[/path]`, `github.com/...`, `/tree|blob/<ref>/<path>`) → 1 tree request + 1 raw read per item on the page (≤20). Detects SKILL.md (skill), `.claude-plugin|.cursor-plugin/plugin.json` (plugin), `server.json` registry manifests (mcp), AGENTS.md / `.mdc` (rule), frontmatter markdown under `agents/` (agent), markdown under `prompts/|commands/` (prompt). Template versions like `${VERSION}` dropped.
- Keyword query → repository search per kind topic (`agent-skills`, `mcp-server`, `claude-code-plugin`, `claude-code-subagents`, `cursor-rules`, `prompts`); topics can't be OR-ed, so ≤2 searches/page anonymously, ≤6 with a token; cursor `t<n>` walks remaining kinds. Kind is topic-derived until resolved.
- Bare `a/b` that 404s falls back to keyword search; URL form surfaces the 404.
- Ids: `<kind>:<owner>/<repo>:<ref>:<file>` (file empty for keyword hits).

## Verified
- Offline: 12 prompts-chat + 20 github-direct tests (parse, Malformed, 401/403/429, kind filter w/o requests, no-request auth_status, cancel, detail, token sent as `Authorization` but absent from items/errors/Debug/descriptor).
- Live (`-- --ignored`, anonymous, 2026-11-07): prompts-chat 7 items + bot detail; github-direct resolved cursor/plugins/create-plugin (2 skills, plugin, agent, rule) and `pdf` skill search (25 items, total 618).

## Contract requests (lead-owned, not implemented)
1. `HttpClient` POST (JSON body) to use prompts.chat MCP (`initialize` + `tools/call search_prompts/search_bots`).
2. `github::search_repositories` page parameter (today keyword search cannot page within a kind).
3. Expose response headers (X-RateLimit-Remaining/Reset) so 403 rate-limit can map to `RateLimited`.
4. `SearchPage` warnings/partial flag (e.g. truncated tree, kinds deferred to next cursor).

## Next
- Optional: map prompts.chat SKILL entries if the lead adds `skill` to this source's kinds.
