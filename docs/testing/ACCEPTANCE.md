# Test policy and gates

Use `docs/IMPLEMENTATION_PLAN.md` §7 as definitive acceptance matrix; extend tests rather than replacing it. Every tested capability needs a traceable automated case or clearly documented manual OS test.

## Core invariants

- Search: all six resource kinds; installed+local+cached remote; stable selection under async updates; no network required to open quick panel; 10k+ index benchmark.
- Preview: `SKILL.md` including nested references, `scripts/`, `assets/`; Markdown tables/code; source view; MCP schemas and credentials redaction; rule precedence; plugin permissions; cancelled stale loads.
- Deployment: temp roots + nested categories; symlink/copy/adopt modes; global/project/custom target; collisions, loops, dangling links, unmanaged pre-existing directories, divergent copies, interrupted operations, journal replay/rollback.
- Actions: capability gating and disabled reasons; copy URL/path; compose with prompt; target identity revalidation; no false success on clipboard fallback.
- Platform: real macOS and real KDE Wayland, multi-screen/focus/hotkey/paste/autostart checks.
- Security: no remote execution on preview, secrets redacted, allowed roots, path traversal rejection, mutation dry run.
- UI: compact dense neutral theme; desktop keyboard access; multiple themes; full manager ↔ quick panel consistent state.

## Performance benchmarks (targets, not claims)

- Warm quick panel visible and interactive <50 ms on reference hardware.
- Typical in-memory search update <10 ms.
- No process spawn or network request needed for ordinary local search.
- Scroll 10k+ rows smoothly using virtualised lists.

Treat failure to meet thresholds as measurable issues with environment + trace; do not fake benchmark results.
