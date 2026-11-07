# Current development acceleration references (checked 2026-11-07)

Use these to **reduce redundant work**, not to add frameworks without reason. Consult the linked original docs for version changes.

| Tool / feature | Official resource | Why it matters | Integration rule |
|---|---|---|---|
| Codex subagents / worktrees | https://learn.chatgpt.com/docs/agent-configuration/subagents and https://learn.chatgpt.com/docs/environments/git-worktrees | parallel independent tasks with isolated changes | ask lead to delegate; never invent flag/config names |
| Claude Code parallel worktrees | https://support.claude.com/en/articles/14554000-claude-code-power-user-tips | `claude --worktree` and optional tmux, isolated sessions | use only when installed; separate write ownership |
| Git worktree | https://git-scm.com/docs/git-worktree | isolated branch directories sharing Git objects | use `scripts/setup-worktrees.sh` after initial commit |
| Rust cargo-nextest | https://nexte.st/ | faster isolated parallel test execution and filtering | use recent patched release >= 0.9.150; run doctests separately |
| Mozilla sccache | https://github.com/mozilla/sccache | cache repeated Rust compilation | optional `RUSTC_WRAPPER=sccache`; avoid incremental cache mismatches |
| cargo-deny | https://github.com/EmbarkStudios/cargo-deny | audit dependencies, advisories, provenance and licences | run at integration/release stage, not per file |
| Nucleo | https://github.com/helix-editor/nucleo | fast fuzzy search, matching and incremental updates | search service, not necessarily UI toolkit |
| Rox | https://github.com/zealsprince/rox | KDE Wayland, GPUI windows, tray, docking, design tokens | inspect specific files; don't copy AGPL app code blindly |
| Sailry Harness | https://github.com/sailry/sailry-harness | Mac AppKit, global shortcuts, agent context, previews | version-check GPUI Kit fork |
| GPUI Kit | https://github.com/dux-web/gpui-component | editor, trees, popovers, dialogs and components | use pinned compatible revision only |
| Vercel Skills | https://github.com/vercel-labs/skills | deployment/copy/symlink destination examples | adapt paths/semantics, add journal & safety |

## Repo setup commands (illustrative, not auto-executed)

```sh
# After extracting and reading AGENTS.md:
cd perkolator
# Only if this isn't already a git repo
[ -d .git ] || git init
git add . && git commit -m 'docs: seed Perkolator project scaffold'
# Then optionally create 4 isolated branch/worktree directories
./scripts/setup-worktrees.sh

# Once the Cargo workspace exists
./scripts/check.sh
# Optional, if installed: RUSTC_WRAPPER=sccache ./scripts/check.sh
```

## Rust validation

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo nextest run --workspace` when a supported version is installed, otherwise `cargo test --workspace`
- `cargo test --workspace --doc` when documentation tests exist
- `cargo clippy --workspace --all-targets -- -D warnings` after the integration interface stabilises
- `cargo deny check` after defining an acceptable licence/advisory policy

## Platform acceptance

`cargo check --target ...` alone does not test macOS AppKit or KDE Wayland global shortcut, focus, clipboard or compositor behaviour. Build platform-local smoke tests and record evidence in `docs/progress/STATUS.md`.

## External code / licence boundaries

Do not vendor entire repositories. Use source files as design references first, verify provenance, preserve all attribution when legally reusing specific files and check platform API compatibility. Never execute copy/pasted remote scripts automatically.
