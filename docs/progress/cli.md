# cli lane — handoff (2026-11-07)

Status: headless `perk` CLI (`apps/cli`) implemented and tested on Linux (CachyOS). It proves the Phase 1 gate (all six kinds discovered, searched, previewed and validated) and the Phase 2 gate (A12 multi-target symlink deploy, A14 conflict refusal).

## Surface
- Global options: `--home DIR` (canonicalized; an explicit home ignores `CLAUDE_CONFIG_DIR`/`XDG_CONFIG_HOME` so nothing resolves outside it), `--project DIR` (repeatable), `--state-dir DIR` (default `<home>/.local/state/perkolator`, holding `manifest.json` and `journal/`).
- Commands: `scan|list|search|targets [--json]`, `show`, `validate [--strict]`, `actions`, `copy ID ACTION`, `deploy ID --target T… [--scope global|project] [--mode symlink|managed-copy|link-source|independent-copy] [--managed-root DIR] [--name N]`, `undeploy DEST`, `repair DEST [--to DIR]`, `recover`, `status`, `matrix`.
- Library API: `perkolator_cli::{Cli, run(&Cli) -> Result<CommandOutput, CliError>}`. `CommandOutput { text, success }`, where `success == false` means exit 1 (conflicting plan, validation errors, refusal).
- Lookup accepts a full id, a record key (path), `kind:name`, or a name (exact, then case-insensitive). A shared name returns `CliError::Ambiguous` listing the ids; no match returns `NotFound` with search suggestions.

## Safety semantics
- Mutations are dry runs by default. `--apply` without `--yes` prints the plan and refuses. Only `--apply --yes` calls `recover()`, re-plans on the recovered manifest, refuses when `!is_applicable()`, skips no-op plans, then runs `Executor::apply`.
- `deploy` refuses non-skills, skills with validation errors, unknown targets, and targets without a skills dir for the requested scope. `approved_roots` is empty, so the engine refuses symlinked skills dirs.
- `copy` prints `resolve()` clipboard text to stdout (no clipboard access) and never sets `include_secrets`. `ActionContext::new(Surface::Cli)` leaves platform features unprobed: copy actions resolve to text, while insert and compose show as disabled.
- `validate` counts scan problems and broken links as warnings. Errors come only from record `ValidationIssue`s.

## Verified (Linux, CachyOS, debug)
- `CARGO_TARGET_DIR=target/headless cargo test -p perkolator-cli -j 4` → 21 passed (4 unit + 17 integration in `apps/cli/tests/cli.rs`, all on tempdir fixture copies with symlinks preserved).
  - A12: three links read through, re-run is AlreadySatisfied, undeploying one keeps the source.
  - A14: an unmanaged dir blocks the plan and leaves the disk byte-identical; `--apply` without `--yes` changes nothing.
  - Also covered: status/matrix, idempotent `recover`, managed-copy at project scope with full removal, secret redaction (`FAKE_TOKEN`/`FAKE_BEARER` never printed), ambiguous names, `mcp: git` returning only MCP results.
- `cargo clippy -p perkolator-cli --all-targets -- -D warnings` is clean, and `cargo fmt --check` is clean.
- The search time printed by `perk search` is a real `Instant` measurement (about 0.04 ms over 15 fixture docs in debug), not a benchmark.
- UNVERIFIED: macOS (no runs there), and Windows (not considered).

## Issues found in other crates
- registry: a symlink deployed into `~/.agents/skills` (the codex/universal target) is not recorded as a `Deployed`/`Linked` location. That directory is scanned as a `SkillLibrary` root (`crates/registry/src/config.rs:181-191`), and library symlinks merge silently (`crates/registry/src/kinds/skills.rs:53-59`). `perk status`/`matrix` (manifest-based) do show it.
- deployment matrix: `codex` and `universal` share a skills dir, so both columns report `linked` for one link. The UI should dedupe destinations by physical path.
- actions: `ActionId` has no `FromStr`; the CLI parses kebab-case ids via serde.

## Next
- Optional `--approved-root`; JSON output for `status`/`matrix`/`show`; a desktop "Deploy…" planner can reuse the same plan → recover → apply flow.
