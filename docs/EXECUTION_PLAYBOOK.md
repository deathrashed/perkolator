# Perkolator — fast autonomous execution playbook

**Required by AGENTS.md.** Goal: deliver working slices rapidly, using concurrent workers when independent, while never gambling with real user data or credentials.

## Operating mode

- Lead reads this document, the architecture contract, the workstream ownership table, and relevant blueprint sections. Worker reads only its assigned scope and applicable skills/references.
- No micro-approvals or status chatter. Work in batches of 2–4 hours of coding-agent scope (not a promise of wall-clock completion). Execute edits and tests without stopping for routine confirmation.
- Questions only for truly blocked architecture, irreversible user-data changes, credentials, publishing or costly actions. If a choice is reversible, choose the simpler conservative option and record it in the ADR / task log.
- Produce useful code before beautifying a UI. Always keep the main branch buildable at integration checkpoints.
- Max 1 primary architectural decision record per foundational boundary; don't generate documentation for every minor decision.

## Orchestration loop

1. **Lead:** establish crate ownership/contracts (resource IDs, action/result types, filesystem safety, event types, platform capabilities), pinned GPUI stack and scoped initial milestone.
2. **Spawn 3–5 independent workers:** assign precise paths, expected API, runnable tests and exit artifact. Prefer native subagents where the CLI supports them; otherwise separate worktrees. Do not invent unsupported CLI flags.
3. **Workers:** implement on isolated branch/worktree; test their touched crate; record concise handoff in `docs/progress/<lane>.md`; report commit hash and contract changes. Do not edit shared root `Cargo.toml`, global lockfile, AGENTS.md or another lane's files.
4. **Lead:** integrate in dependency order; rebase/cherry-pick once per coherent slice, resolve interface mismatches, run integration gate and update `docs/progress/STATUS.md`.
5. **Iterate without user prompts** until a real blocker or completion. Present short milestone summary and remaining validation gaps only.

## Integration order (dependency-aware, not serial research)

- Batch A: common resource model + provider abstraction + safe filesystem deployment probes + UI theme/components proof, in parallel. Lead owns top-level Cargo choices.
- Batch B: Nucleo search + content preview + manager browser shell + agent installation target registry, after contracts freeze.
- Batch C: macOS launcher adapter + KDE Wayland launcher adapter + action engine + composer, in parallel using platform-specific stubs on non-target OS.
- Batch D: skills/MCPs/prompts/rules/agents/plugins providers; live marketplace connectors may proceed in separate source-specific lanes.
- Batch E: integration, performance measurements, security and both OS acceptance; run automated tests once per merged slice, not after each line edit.

## Risk / stop thresholds

Continue without asking for ordinary implementation decisions, dependency installs inside the project, compilation, formatting and isolated tests. Pause for destructive changes outside project/test fixtures; permission elevation, Keychain/secret extraction, production credentials, arbitrary terminal or desktop automation against user apps, installing third-party skills/plugins on the host, remote scripts, paid APIs, or publication.

## Validation ladder

1. Fast lane-level `cargo check -p <crate>` and scoped unit tests.
2. At merge checkpoint: `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo nextest run --workspace` when available, fallback `cargo test --workspace`; doctests separately when relevant.
3. Before release gate: clippy, cargo-deny and target-platform integration / perf / accessibility suites.
4. For a platform unavailable to the worker, add an integration test and label `NOT RUN (environment unavailable)`. Never claim Wayland success from macOS compilation.

## Message / context budget

Each worker task packet <= roughly one screen: objective, owned paths, shared API, references, tests, definition of done. Do not paste the entire 70k-character blueprint into every subagent; tell them exactly which sections to consult. Prefer 1–2 direct upstream source links to general web searching. Lead should request concise output: file list, tests, blockers, commit hash.

## Team sizing

Use 3–5 workers for independent foundational lanes. More concurrent GPUI builders against the same workspace often increases merge cost; scale only with mutually exclusive ownership. A single worker is preferable for small dependent changes or when compute/memory is constrained. On a 16 GiB Linux machine limit simultaneous heavy Rust compilations; separate `CARGO_TARGET_DIR` per worktree or serialize link steps, and enable `sccache` if installed. Worker count is a configurable maximum, not a target to fill.
