# One-shot prompt: start Perkolator at high throughput

Implement Perkolator from this repository. Read root `AGENTS.md` and `docs/EXECUTION_PLAYBOOK.md` first; consult blueprint sections, architecture and GPUI Kit skills relevant to each task. **Do not spend the run writing another plan or pausing at tiny checkpoints.** Create working Rust code and tests.

Use parallel agents/worktrees for independent tasks (default 4 lanes) with disjoint file ownership per `docs/WORKSTREAMS.md`. You are lead integrator: freeze minimal public contracts and chosen GPUI dependencies; delegate resource core, deployment, search/preview and native UI proof, then integrate. Start platform-specific macOS and KDE Wayland probes once core contracts exist. Time-box source research; use the direct source pointers in `docs/references/REFERENCE_MAP.md` and `docs/references/DEVELOPMENT_ACCELERATORS.md`.

Carry out safe routine changes, tool setup, tests and merges autonomously. Ask only for genuinely blocking permissions, credentials, irreversible user-data operations, paid actions or unsafe changes. Never overwrite existing external skill/agent directories during development; use isolated temporary fixtures.

Prioritise a runnable vertical slice over superficial dashboards: six typed resource kinds → local registry → safe deployment dry run → Nucleo search → Markdown/files/references preview → native quick launcher and contextual actions. Keep Mac and KDE Wayland release blockers and report physical platform checks truthfully.

At the end, report only: working features, changed modules, tests (passed/failed/not run), merge status and next dependency-ready tasks. Continue implementing while independent work remains, rather than asking permission between phases.
