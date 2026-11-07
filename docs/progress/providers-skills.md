# Providers — skill catalogues (Worker A) — handoff

Code: `crates/providers/src/skills/{lobehub,skillssh,skillsmp,voltagent,officialskills}.rs`, shared `common.rs` (GitHub link parsing, local pagination, 10-min index cache, SKILL.md preview) and `curated.rs` (VoltAgent README parser). Registered in `skills::providers()`.

| id | status | transport / auth | evidence |
| --- | --- | --- | --- |
| `skillsmp` | working | REST, `OptionalToken{skillsmp}` (Bearer) | live anonymous search 2026-11-07: 25 items for `terraform`; 50/day, 10/min anonymous |
| `voltagent-skills` | working (metadata + GitHub preview) | GitHub index, `None` | live README fetch: 11 matches for `pdf`; ~1250 entries |
| `officialskills` | working (metadata-only) | Directory, `None` | live `sitemap.xml` + VoltAgent README: 4 matches for `pdf`, all described |
| `skillssh` | needs-auth | REST, `ApiKey{skillssh}` = Vercel OIDC token | live anonymous `/api/v1/skills/search` → 401 `authentication_required` |
| `lobehub` | needs-auth / UNVERIFIED (CLI missing) | CLI `lhm`, `ExternalCli` | `lhm` not on PATH; Market API returns 401 without M2M bearer |

## Verified facts behind the design
- SkillsMP: `GET /api/v1/skills/search?q&page&limit(≤50)&category`; `{success,data:{skills,pagination{hasNext,total,totalIsExact}}}`. Total reported only when `totalIsExact`. Category sent only with a key (anonymous = keyword only). Queries without letters/digits skip the call.
- skills.sh: all endpoints require `Authorization: Bearer <VERCEL_OIDC_TOKEN>` (~12 h, project-scoped). Search has no description field and no pagination; queries <2 chars use the paginated leaderboard. `isDuplicate` rows are hidden. Detail returns SKILL.md contents. `npx skills` is never used.
- LobeHub (`@lobehub/market-cli` 0.0.46, read from the npm tarball): `lhm skills search` / `lhm mcp search` with `--q --category --page --page-size --output json` print the SDK list response. Both exit 1 with "No credentials found" unless `lhm register` M2M creds exist; `lhm login` does not enable them. No agent/plugin-catalogue search, so kinds = skill + mcp. `auth_status` only checks PATH (`lhm auth status` hits the network). The fixtures are schema-derived from `@lobehub/market-types` 1.22.0 and labelled `_fixture_note`.
- VoltAgent: entries `- **[pub/skill](url)** - desc` (also `:`). `view_source` is set only for links to a concrete `SKILL.md`. Detail fetches `SKILL.md` (falling back to README.md for repo/dir links) via `github::raw_file`.
- officialskills.sh: no API (robots disallows `/api/`). The site is maintained by VoltAgent and links that repo, so the sitemap provides the list (`/{owner}/skills/{slug}`) and the README supplies descriptions. README failure degrades to sitemap-only.

## Tests (Linux, CachyOS)
- `CARGO_TARGET_DIR=target/headless cargo test -p perkolator-providers -j 4 --test skills_*` → 38 passed, 5 ignored (live); 3 skills unit tests pass.
- Live (`-- --ignored`) run once: 4 pass with network; lobehub prints UNVERIFIED (CLI missing).
- The full crate run and clippy were blocked by Worker C's `bundles/` (1 failing unit test, 4 clippy errors); `skills/` has no clippy findings.

## Open
- Lead decision: allow `SpawnCliFlow lhm register --name Perkolator` (needed for search; brief allowed only `login`). Currently `AuthIntent::Unsupported` with instructions.
- skills.sh in a desktop app requires users to supply a Vercel project OIDC token. Consider marking it opt-in/disabled by default.
- LobeHub agents (Market has an agents API in the SDK, not exposed by the CLI) are not covered.
