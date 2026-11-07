# Workstream map / parallel agent contracts

Lead owns **Cargo.toml/Cargo.lock, pinned GPUI and GPUI Kit versions, shared public APIs, repository docs, conflict resolution, release integration**. Freeze interfaces early; publish signatures in docs/ARCHITECTURE.md. Workers own only their lanes.

| Lane | Disjoint ownership | Output/exit gate | Target references |
|---|---|---|---|
| core | `crates/resource-core/`, `crates/registry/` | six typed resource kinds, canonical identities/capabilities, normalization tests | Agent Skills specification, project blueprint |
| deployment | `crates/deployment/`, `crates/targets/` | dry-run/journal/rollback, symlink-copy-adopt, conflicts and temporary-directory tests | vercel-labs/skills, Skills Manager |
| search-preview | `crates/search/`, `crates/preview/` | Nucleo index, stable/cancellable search and file/Markdown preview model with tests | Nucleo, Sailry content, Zed |
| ui | `apps/desktop/`, `crates/theme/`, `crates/widgets/` | dense GPUI native UI, manager and reusable split-pane; no fake action wiring | Rox dock/design, Sailry UI, GPUI skills |
| platform-macos | `crates/platform-macos/` | macOS hotkey/window/clipboard adapter with platform test instructions | Sailry system_shortcuts, macos.rs |
| platform-wayland | `crates/platform-linux/` | KDE Wayland portal/window/clipboard integration; test on actual compositor | Rox patches, xdg-desktop-portal |
| provider-* | `crates/providers/<provider>/` when created | independently testable cached source adapter | upstream official APIs for that provider |

**Suggested first parallel wave:** core, deployment, search-preview, UI proof. Lead owns interfaces and Cargo bootstrap. Mac/Wayland adapters can start as concurrent research+proof after platform traits are frozen. Avoid having five agents modify `apps/desktop/src/main.rs` at once.

## Shared contracts agreed before work

- Stable `ResourceId` = kind + provider + upstream canonical key; separate local installations/deployments.
- Every resource has typed capabilities and kind-specific optional details; no universal fake install or toggle operations.
- `SourceAdapter`: discover/search/fetch metadata + content; no filesystem mutation.
- `DeploymentPlanner`: immutable dry-run plan (operations, conflicts, expected state); `DeploymentExecutor`: preflight/recheck, journal, apply, verify, recovery.
- `PreviewProvider`: content handles, preview request generation/cancellation; never execute.
- `ActionRegistry`: capability predicate + target context + typed action result; UI dispatch only.
- `PlatformIntegration`: global hotkey, show/hide, context detection, clipboard, insertion; feature flags expose unsupported operations.
- `Theme`: semantic tokens not inline green hex values.

## Merge discipline

Each worker commits its branch, records API or schema changes in its lane handoff, and does not merge into main. Lead cherry-picks or merges in integration order and runs workspace checks. If the repository is not yet Git-initialised, lead initialises it, makes one baseline commit, then creates worktrees; workers do not create separate unrelated repositories.

## First wave task packets

### Worker A — resource core

Implement the six resource types and typed capability registry. Own only `crates/resource-core` and `crates/registry`. Provide unit tests for duplicate providers, IDs and resource-kind-specific operations. No GPUI dependencies. Read resource model from `docs/ARCHITECTURE.md` and `docs/IMPLEMENTATION_PLAN.md`.

### Worker B — safe deployment

Implement a headless, dry-run-first filesystem deployment engine. Own only `crates/deployment` and `crates/targets`. Test symlink, copy, nested canonical folder, collision, unmanaged destination, broken symlink, rollback and refusal to delete real folders. No actual user's agent directories touched. Reference `vercel-labs/skills` installer/agents implementations.

### Worker C — search and preview

Implement Nucleo-backed indexed fuzzy search with stable IDs, rank by installed/project relevance, cache-ready remote result merging. Build cancellable preview model for Markdown, file trees and references. Own only `crates/search` and `crates/preview`. Headless tests; GPUI host binds later. Reference Nucleo, Sailry `crates/content`, Zed picker patterns.

### Worker D — native visual proof

Build initial window, theme tokens, compact list + split preview and one keyboard action, with mock typed data until shared APIs land. Own only `apps/desktop`, `crates/theme`, `crates/widgets`. Keep surfaces neutral and accents rare. Reference Rox `quick_play`/design, Sailry `command_picker`, both local GPUI Kit skills. Do not create fake marketplace integrations.

## Definition of done for worker handoff

`Implemented` / `Tests executed with exact commands` / `Files changed` / `Public API changes` / `Blockers and unverified platform features` / `Commit hash`. Five short fields; no lengthy diary.
