# Perkolator

**Native, local-first AI resource manager and universal keyboard launcher.**

Perkolator is the planned cross-platform Rust/GPUI application for **Skills, MCP servers, Agents, Prompts, Rules, and Plugins**. It will provide a fast global launcher, unified previews, contextual actions, native agent/session integrations, and safe deployment to global/project/custom paths via symlinks, managed copies, and independent copies.

**Target platforms:** macOS and **KDE Plasma Wayland on Arch/CachyOS**, both release-blocking. Optional future models/provider catalogue and remote hosts.

> This ZIP is a **project/documentation scaffold**, not a compiled application. No Cargo crates or functioning GPUI app have been implemented yet. The coding agent must create and test those components according to the plan.

## Start here

1. Read [`AGENTS.md`](AGENTS.md) before generating code.
2. Read [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md) in full; it is the definitive requirements and milestone document.
3. Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/design/DESIGN_SYSTEM.md`](docs/design/DESIGN_SYSTEM.md), and [`docs/references/REFERENCE_MAP.md`](docs/references/REFERENCE_MAP.md).
4. Read both local skills under `.agents/skills/` including their `references/` folders. These are **development instructions**, not user-deployed resource examples.
5. Follow [`docs/IMPLEMENTATION_PLAN.md` §6](docs/IMPLEMENTATION_PLAN.md) Phase 0 first: verify pinned GPUI/Kit APIs and macOS + KDE Wayland feasibility before building the app.

## Repo structure

```text
perkolator/
  AGENTS.md                     Coding-agent operational contract
  README.md                     Quick start and current state
  docs/IMPLEMENTATION_PLAN.md   Complete mandatory v2 blueprint
  docs/ARCHITECTURE.md          Boundaries, domain model, ownership
  docs/design/                  Visual system and curated screenshots
  docs/platform/                macOS and Wayland acceptance checklist
  docs/references/              Exact upstream modules to inspect
  docs/testing/                 Test matrix and release gates
  docs/adr/                     Architecture decision records (template)
  config/                       Example theme and product configuration
  .agents/skills/               GPUI Kit development guidance
  assets/references/            Original user-provided reference screenshots and concepts
  apps/                         App crates (to be implemented)
  crates/                       Library crates (to be implemented)
  tests/                        Integration tests (to be implemented)
```

## Product distinction

Perkolator is **not** a skills-only tool or a clone of LobeHub. Keep the six resource types first-class. Skills are the initial implementation vertical slice, and the interfaces and deployment engine must support distinct resource capabilities. No social ratings/reviews/premium marketplace clutter.

## Important

- Never promise unrestricted input injection on Wayland. Use portals and native agent adapters, with explicit fallback.
- All reference images are **design references only**; their text/metadata are not authoritative; verify sources and copyrights before reusing original assets.
- `#1abc9b` is an available accent, **not** a colour to apply on every selected row, border or button. Neutral surfaces dominate.
- Reference repositories and their licences differ. Rox app code is AGPL; Sailry Harness is Apache-2.0. Keep provenance for any ported code.

## Marketplace sources (add-on)

Remote catalogues (LobeHub, skills.sh, SkillsMP, the official MCP Registry, Smithery, Glama, Claude/Cursor plugin marketplaces, wshobson/agents, VoltAgent indexes, OfficialSkills, prompts.chat, Awesome Claude Code, PulseMCP and direct GitHub) are specified in [`docs/MARKETPLACE_INTEGRATION.md`](docs/MARKETPLACE_INTEGRATION.md), [`docs/MARKETPLACE_AUTH.md`](docs/MARKETPLACE_AUTH.md), [`docs/MARKETPLACE_UI.md`](docs/MARKETPLACE_UI.md), [`docs/MARKETPLACE_PARALLEL_WORK.md`](docs/MARKETPLACE_PARALLEL_WORK.md) and [`tests/MARKETPLACE_ACCEPTANCE.md`](tests/MARKETPLACE_ACCEPTANCE.md), with example source settings in `config/marketplace-providers.example.toml`. The provider contract lives in `crates/marketplace`; adapters live in `crates/providers`.

## Fast implementation / parallel agents

**Start with [`prompts/START_HERE.md`](prompts/START_HERE.md).** It tells the lead to implement immediately, delegate independent code to four isolated workers and avoid micro-checkpoints. Read [`docs/EXECUTION_PLAYBOOK.md`](docs/EXECUTION_PLAYBOOK.md), [`docs/WORKSTREAMS.md`](docs/WORKSTREAMS.md), and [`docs/references/DEVELOPMENT_ACCELERATORS.md`](docs/references/DEVELOPMENT_ACCELERATORS.md) for concrete ownership, tooling and performance gates.

The archive is a specification-first scaffold; no functioning GPUI binary is included. An optional `scripts/setup-worktrees.sh` creates separate Git worktrees **after the initial Git commit**, and `scripts/check.sh` runs Rust checks **after Cargo bootstrapping**. The scripts perform no skill deployment and do not touch existing agent directories.

