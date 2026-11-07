# Add Perkolator Federated Marketplace Integration — Coding Agent Prompt

Read the repository's existing `AGENTS.md`, current architecture and these supplemental docs (do not replace existing project files):

1. `docs/MARKETPLACE_INTEGRATION.md`
2. `docs/MARKETPLACE_AUTH.md`
3. `docs/MARKETPLACE_UI.md`
4. `docs/MARKETPLACE_PARALLEL_WORK.md`
5. `tests/MARKETPLACE_ACCEPTANCE.md`
6. `config/marketplace-providers.example.toml`

**Goal:** Add all listed marketplace sources for skills, MCPs, agents, plugins, prompts and rules. Wire enabled/disabled source selection and resource-type filters into both the full application and global quick panel, plus proper auth/setup/test actions for sources needing login, CLI registration, OAuth or API keys. Public sources work anonymously. Metadata-only sources remain inspectable; unsupported install options are not offered.

The app already exists and is close to finished. **Do not scaffold/rewrite the app or substitute framework versions.** Extend existing provider/search/cache/auth/deployment interfaces and styling. The existing specification has priority in a conflict; report only concrete contradictions.

**Execution:** Use multiple parallel workers on disjoint modules according to `docs/MARKETPLACE_PARALLEL_WORK.md`; lead owns shared interfaces and integration. No review of every tiny step, repeated planning or permission requests for routine changes. Implement in meaningful batches, test automated behaviours, fix failures, and report at milestones. Ask for credentials or approval only when actually needed. Avoid many simultaneous full Rust builds.

**Required behaviour:** enabled sources are queried concurrently with timeouts and partial result streaming; disabled sources receive zero remote requests; local indexing always responds; type/source filters are orthogonal; credentials are secure and source scoped; CLI adapters are optional/detected; results preserve publisher/source provenance and deduplicate safely; resource-specific actions reuse existing deployment engine; source status and auth UI remain functional offline.

**Source list:** LobeHub CLI; skills.sh CLI/conditional API; SkillsMP; official MCP Registry; Smithery; Glama; Claude plugin marketplace GitHub; Cursor plugin marketplace GitHub; wshobson agents; VoltAgent awesome agent skills and awesome Claude subagents; OfficialSkills directory; prompts.chat MCP/GitHub; awesome-claude-code; PulseMCP partner API; generic GitHub direct source; existing local inventory.

**Validation:** run the acceptance matrix, include unit tests with fixture responses and a small set of legitimate live smoke tests. Never claim auth or an external API works until demonstrated. Do not bypass access controls. Produce an integration summary with each source status `working`, `needs-auth`, `metadata-only`, `unsupported`, or `blocked-by-provider`, and any exact setup step for the user.
