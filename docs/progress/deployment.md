# Deployment lane — handoff (2026-11-07)

**Status:** headless engine is implemented and tested on Linux (CachyOS, ext/btrfs tempdirs). macOS is **UNVERIFIED** because no runs have happened there. The code uses std unix symlinks and has `/var`→`/private/var` handling, but needs a `cargo test` run on a Mac.

**Gates:** `CARGO_TARGET_DIR=target/headless cargo test -p perkolator-targets -p perkolator-deployment -j 4` → 16 + 5 + 24 = 45 passed. clippy `--all-targets -D warnings` is clean, and fmt was applied.

## perkolator-targets
- `PathTemplate` (`~`, `$HOME`, `$XDG_CONFIG_HOME`, `${VAR}`, `${VAR:-fallback}`, absolute; the tail is a `RelPath`) resolves against an explicit `HomeEnv`. `HomeEnv::from_process_env()` is the only place that reads env vars.
- `TargetRegistry::builtin()` contains claude-code, codex, opencode, cursor, gemini-cli, github-copilot, universal and generic. `from_json`/`apply_json` takes a versioned file (`version: 1`), rejects unknown fields, and is all-or-nothing.
- Skill dirs and detect markers were verified against vercel-labs/skills `src/agents.ts` @ `d3701c11b110` (2026-11-06). MCP, rule, profile and prompt paths are data marked *unverified* in `notes`.
- `universal` uses `~/.agents/skills` (what Codex/Cline read). Vercel's own `universal` uses `$XDG_CONFIG_HOME/agents/skills`, which can be overridden via JSON.

## perkolator-deployment
- `plan_deploy` / `plan_remove` / `plan_repair(dest, manifest, new_target)` are read-only, and each returns a `Plan` with ops, conflicts and manifest upserts/removals.
- `Executor::new(journal_dir, manifest_path)`:
  - `apply` re-runs preflight checks and writes an fsynced JSON-lines journal record before and after every step. It verifies each step, rolls back on failure, and commits the manifest last.
  - `recover` finalizes committed runs and rolls back all others. It is idempotent.
- Modes: Symlink, ManagedCopy, LinkSource, IndependentCopy. There is no symlink→copy fallback.
- Staging and trash are hidden siblings (`.perkolator-{staging,trash,link}-<run>-<i>`) on the same filesystem. Trash is deleted only after the manifest commits and its fingerprint is re-checked.
- Deletion is allowed only when the path is in the manifest *and* the link target or fingerprint is re-verified, or when it is a generated name that was journalled before creation.
- Policy: if `approved_roots` is empty, a skills dir that is itself a symlink is refused (`SymlinkedParent`). With approved roots, a symlinked parent is allowed only when its real location stays inside a root.

## Known limits
- `rename` has no `RENAME_NOREPLACE`, so a destination created between the final check and the rename could be replaced if it is an empty dir. This is documented in `executor.rs`.
- Multi-destination apply is recoverable, not ACID.
- Repair only re-points or recreates links. Copies get `RepairUnsupported`.
- Duplicate physical destinations (e.g. codex + universal) are a conflict; the UI should dedupe them.

## Next
- UI deployment matrix via `deployment_matrix`.
- Call `recover()` at app start before any apply.
- Persist `TargetRegistry` overrides.
- MCP/rules writers (separate lane).
