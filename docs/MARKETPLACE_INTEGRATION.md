# Federated Marketplace Integration — Implementation Specification

## Goal

Extend the **existing** Perkolator application (do not rebuild/refactor it) with independently configurable marketplace sources for **skills, MCP servers, plugins, agents, prompts and rules**. Search local inventory immediately, then query multiple enabled remote registries concurrently, merging results incrementally. Sources must also be addressable individually. Registry/CLI discovery is distinct from deployment; use Perkolator's existing safe installation/deployment engine.

## Initial provider inventory

Implement every source below behind the existing provider abstraction. If a provider is not currently usable or auth cannot be verified, present it as `Needs setup` or `Unsupported`, do not remove it or pretend results work.

| ID | Source | Resources | Integration and canonical reference | Authentication / caveats |
| --- | --- | --- | --- | --- |
| `lobehub` | LobeHub Market | Skills, MCP, agents, plugins | `lhm` (`@lobehub/market-cli`), https://market.lobehub.com ; app source https://github.com/lobehub/lobehub | User OAuth or device M2M, capabilities differ by endpoint; `lhm register`, `lhm login`, `lhm auth status`; check `lhm mcp --help`, `lhm skills --help` |
| `skillssh` | skills.sh | Skills | `npx skills find`, `npx skills add`, https://skills.sh/ ; API docs https://skills.sh/docs/api | Direct API may require Vercel OIDC; use supported CLI where direct access not authorised |
| `skillsmp` | SkillsMP | Skills | https://skillsmp.com/docs/api | Anonymous/API-key limits; verify endpoint and terms against current documentation |
| `mcp-official` | Official MCP Registry | MCP | https://registry.modelcontextprotocol.io/docs , https://github.com/modelcontextprotocol/registry | Public read/search API; paginated, versioned |
| `smithery` | Smithery | MCP, tools | https://smithery.ai/ , https://www.npmjs.com/package/@smithery/cli | Use documented CLI or API; verify JSON output/options/version, authenticate where required |
| `glama` | Glama | MCP | https://glama.ai/mcp/reference | API key, quota, attribution/licensing conditions; do not use beyond terms |
| `claude-plugins` | Anthropic Claude plugins | Plugins, bundled skills, agents, commands, MCP | https://github.com/anthropics/claude-plugins-official | Parse GitHub `.claude-plugin/marketplace.json` + plugin manifests; follow authenticated GitHub API quotas if used |
| `cursor-plugins` | Cursor plugins | Plugins, rules, skills, agents, MCP, commands, hooks | https://github.com/cursor/plugins | Parse repository manifests, inspect actual schema version; do not auto-enable hooks |
| `wshobson-agents` | wshobson/agents | Agents, skills, plugin bundles | https://github.com/wshobson/agents | GitHub index parser + canonical asset paths; do not assume every entry is installable |
| `voltagent-skills` | VoltAgent Awesome Agent Skills | Skills | https://github.com/VoltAgent/awesome-agent-skills | Curated index; resolve publisher GitHub sources to `SKILL.md` and preserve provenance |
| `voltagent-subagents` | VoltAgent subagents | Agents | https://github.com/VoltAgent/awesome-claude-code-subagents | Curated index, GitHub markdown parsing; verify each resource file |
| `officialskills` | OfficialSkills | Skills | https://officialskills.sh/ | Curated directory / outbound canonical links, not a confirmed public structured API; use supported data only |
| `prompts-chat` | prompts.chat | Prompts, bots | https://prompts.chat/docs/api , https://github.com/f/prompts.chat | MCP discovery at https://prompts.chat/api/mcp where supported; public repo fallback |
| `awesome-claude` | Awesome Claude Code | Agents, plugins, skills, commands, hooks | https://github.com/ianwieds/awesome-claude-code | Curated index with source links, not direct installer |
| `pulsemcp` | PulseMCP | MCP | https://www.pulsemcp.com/api | Partner/approved-access path; show `Needs access` if unavailable |
| `github-direct` | GitHub source resolver | All six where metadata format recognised | https://docs.github.com/en/rest/search/search , https://docs.github.com/en/rest/repos/contents | GitHub API token optional for higher limits; user-provided repos and manifests; respect GitHub API rate limits |
| `local-inventory` | Existing user filesystem and agent configs | All six | Local Perkolator inventory | Never requires marketplace login; always searchable; do not conflate with remote provider count |

Sources may return metadata-only listings; use `view source` when download or deployment cannot be established. Real remote actions are opt-in and must be allowed by the source terms. Model/provider catalogue can be added in a separate feature; it must not silently absorb this six-type deliverable.

## Provider capabilities

A provider must declare `resource_types`, `search`, `detail`, `download_or_source`, `install`, `auth`, `pagination`, `filters`, `can_fetch_files`, `rate_limit_policy`, `requires_attribution`, and `health_probe`, with unsupported functions absent. Keep `ProviderHealth` distinct from `AuthStatus`. All registry metadata is untrusted.

Recommended typed operations (reuse existing interfaces):

```rust
// Conceptual contract only: map to the existing Rust codebase.
struct ProviderDescriptor {
    id: String,
    display_name: String,
    resource_types: Vec<ResourceType>,
    transport: TransportKind,        // Cli | Rest | Mcp | GithubIndex | Filesystem
    supports_search: bool,
    supports_detail: bool,
    supports_install: bool,
    auth: AuthStrategy,              // None | OAuthPkce | DeviceRegistration | ApiKey | ExternalCli
}

struct SourceSearchRequest {
    query: String,
    kinds: Vec<ResourceType>,
    selected_source_ids: Vec<String>,
    category: Option<String>,
    page_size: usize,
    // each provider handles its own pagination cursor
}
```

Use normalised results with `source_id`, `upstream_id`, `resource_type`, `title`, `summary`, `canonical_url`, `repo_url`, `package_ref`, `version`, `author`, `source_capabilities`, `last_seen_at`, `stale`, and provenance. Model IDs should be stable and scoped by provider; never deduplicate solely by title. Retain original provider identity and all alternate sources after deduplication.

## Search and pagination

1. Trigger local Nucleo search synchronously against the existing index; do not await CLI/network.
2. Filter requested resource kinds and enabled provider IDs **before scheduling** remote work.
3. Run independent remote provider requests concurrently with per-provider timeouts, concurrency budgets and backoff. Cancellation/generation token for query changes; stale responses must never replace newer results.
4. Merge responses as each settles; show source labels and per-source counts/errors; one backend failure does not erase other results.
5. Cache pages and detail metadata per provider (query/filter/locale and schema version); use stale-while-revalidate where appropriate. Respect explicit refresh.
6. Pagination is per provider; avoid assuming page number for cursor-based registries.
7. Merge duplicate resources by verified canonical repository URL + relative resource path, or upstream canonical URI. Expose all discovered source records.
8. Do not multiply results/duplicate actions for marketplace mirrors. Provide filter `Installed`, `Available`, `Updates`, `Offline`, `Needs Auth` as status dimensions.
9. Prevent any remote code execution, scripts, hooks or MCP startup during passive discovery/preview.
10. CLI invocations must use structured args, never shell-interpolated user queries. Validate JSON and command version; bound subprocess time/output; no credentials in logs.

## Installation and enablement

- Skill packages: stage into temp directory, validate archive/paths, inspect `SKILL.md`, then import to the existing canonical library/deployment engine. Multi-agent symlink/copy is managed by Perkolator.
- MCP: target-agent config adapter; distinguish `configured`, `enabled`, `connected`, `healthy` (not interchangeable). Surface tools from approved schema discovery and redact environment secrets.
- Plugins: enumerate contained components, permission requirements and hooks. Explicit review before enabling hooks/executable code; support uninstall/disable in target-agent-specific manner.
- Agents: install/export agent profiles in target-supported format; do not promise execution in every coding agent.
- Prompts: local templates, metadata, variables, copy/insert/deploy where target supports it.
- Rules: parse e.g. AGENTS.md and `.mdc`; deploy/symlink/copy by target; respect precedence and user-owned files.
- Use the same actions registry in main app and launcher. Distinguish global/project/custom scope; report unavailable capabilities rather than guessing target paths.

## Source selection rules

- Per-source persisted `enabled_for_search` boolean, independently togglable (defaults sensible, avoid automatic surprise network calls).
- `enabled_for_search = false` means no remote query; installed local resources from that source remain searchable unless user separately disables local indexing.
- Provide `Search all enabled`, `Current source only`, and explicit multi-select groups.
- Category/type filters and source filters are orthogonal. Category availability is provider-specific; incompatible filters must be ignored for that provider with clear UI labels, never silently suppress everything.
- Expose errors clearly (`Needs login`, `Rate limited`, `Unavailable`, `Unsupported API`, `CLI missing`, `Permission denied`).

## Security and compliance

Do not scrape hidden endpoints, bypass auth, embed user tokens in config, run remote scripts as part of discovery, or assume unlicensed marketplace feeds permit commercial redistribution. Capture source attribution where required. Verify external URLs before opening; allowlisted auth callback origins, state + PKCE validation. Keep credentials OS-keychain/secret-service-backed, not plaintext in settings.
