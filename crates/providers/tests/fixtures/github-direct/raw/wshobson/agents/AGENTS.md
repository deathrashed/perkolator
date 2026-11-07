# Agents, skills, and commands for seven coding tools

<a id="claude-agents--multi-harness-agentic-plugin-marketplace"></a>

The repository provides 94 plugins (92 local + 2 external), 202 agents, 185 skills, and 105 commands for seven coding tools: Claude Code, OpenAI Codex CLI, Cursor, OpenCode, Google Antigravity CLI (`agy`), GitHub Copilot, and Pi. Shared source files use the Claude Code format, and adapters provide the formats used by other tools. External entries have their own tool support.

AGENTS.md is the canonical context file. Codex, Cursor, OpenCode, Antigravity CLI, GitHub Copilot, and Pi read it directly. Claude Code reads it via `CLAUDE.md`, a symlink to this file.

> **Read this file like a table of contents.** Detail lives in `docs/`. Authoring conventions live in `docs/authoring.md`. Per-harness setup and capability deltas live in [`docs/harnesses.md`](docs/harnesses.md). This file should never grow beyond ~150 lines (per OpenAI's [harness-engineering](https://openai.com/index/harness-engineering/) practice).

## Map

- **[ARCHITECTURE.md](ARCHITECTURE.md)** — top-level architectural overview (adapter framework, source-of-truth invariant, capability matrix summary)
- **[docs/architecture.md](docs/architecture.md)** — detailed design principles
- **[docs/plugins.md](docs/plugins.md)** — full plugin catalog (94 plugins by category)
- **[docs/agents.md](docs/agents.md)** — agent reference (202 agents, model tiers)
- **[docs/agent-skills.md](docs/agent-skills.md)** — skill reference (progressive disclosure model)
- **[docs/usage.md](docs/usage.md)** — commands, workflows, examples
- **[docs/authoring.md](docs/authoring.md)** — portable-content style guide (read before adding plugins)
- **[docs/harnesses.md](docs/harnesses.md)** — per-harness capability matrix
