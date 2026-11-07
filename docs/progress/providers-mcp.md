# providers-mcp lane (Worker B) — handoff (2026-11-07)

## Source status (verified against live endpoints 2026-11-07, Linux/CachyOS)
| id | status | evidence |
| --- | --- | --- |
| `mcp-official` | **working** | `GET /v0.1/servers?version=latest&limit&search&cursor` + `/servers/{name}/versions/{v}`; anonymous; live search → page 2 → detail passes. No tool schemas in registry (tools empty). |
| `smithery` | **blocked-by-provider** (browse + detail only) | Smithery shut down API keys/CLI/run.tools on 2026-10-31 (smithery.ai/future). `registry.smithery.ai/servers?page&pageSize` and `/servers/{name}` (with tools) still answer anonymously; any `q=` search returns HTTP 500 → surfaced as `Http{500}`, never filtered locally. |
| `glama` | **needs-auth** | Every read endpoint returns 401 without `Authorization: Bearer` (live). Key slot `glama`; API Data License → `requires_attribution` set, `canonical_url` = Glama listing URL. Success parsing is only fixture-tested (synthetic, from OpenAPI). |
| `pulsemcp` | **needs-auth** (partner) | `api.pulsemcp.com/v0.1` = generic registry API + `X-API-Key` + `X-Tenant-ID`; anonymous → 401 (live); v0beta sunset (410). `PartnerAccess` → `NeedsAccess` unless slots `pulsemcp` + `pulsemcp-tenant` exist; no request otherwise. Premium `_meta` tools → `RemoteDetail.tools`. |

## Design
- `src/mcp/registry.rs`: shared generic MCP Registry v0.1 client (official + PulseMCP). Drops `status: deleted`; `author` = reverse-DNS namespace; `resource_path` = `repository.subfolder`.
- Redaction (`registry::redact_server`, `common::redact_schema_defaults`): literal env/header values and any `isSecret` value/default → `<redacted>`; `{placeholder}` templates kept. Config-schema defaults (Glama env schema, Smithery `configSchema`) always redacted.
- Official search needs ≥3 chars (server 422); shorter non-empty queries return an empty page without a request.
- Glama uses `sort=added-at:desc` (documented Boost-free) so no "advertising" label is needed; trade-off: newest-first, not relevance.
- Popularity/visitor/use counts, quality scores, Boost and verified flags are deliberately not mapped.
- No CLI is used by any MCP source (Smithery CLI is discontinued), so `CliMissing` does not apply.

## Tests
- `cargo test -p perkolator-providers --lib --test mcp_official --test mcp_smithery --test mcp_glama --test mcp_pulsemcp`: 9 + 12 + 11 + 7 + 6 passed, 4 ignored (live).
- `-- --ignored`: 4/4 live passed (official search/paginate/detail; Smithery browse/detail, search=HTTP 500; Glama 401; PulseMCP 401).
- Fixtures: real trimmed responses for official/Smithery/401s; Glama/PulseMCP success bodies and literal-secret entry are SYNTHETIC (`_fixture_note`).

## For the lead
- Config example: `smithery` says `transport = "cli"`; adapter is REST (CLI discontinued). Consider `enabled = false` by default.
- Contract requests: `AuthStrategy::PartnerAccess` has no credential slot(s) (PulseMCP needs two); no `RemoteItem` field for deprecation/status or for "promoted" flags; `RemoteActions.install` stays false although registry `packages`/`remotes` are concrete config recipes for the MCP deploy adapter (in `manifest_json`).
- UNVERIFIED: Glama and PulseMCP authenticated success paths (no credentials available).
