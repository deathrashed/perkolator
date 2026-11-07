# Continual Learning

Automatically and incrementally keeps `AGENTS.md` up to date from transcript changes.

The plugin combines:

- A `stop` hook that decides when to trigger learning.
- A `continual-learning` skill that orchestrates the learning flow.
- An `agents-memory-updater` subagent that mines new or changed transcripts and updates `AGENTS.md`.

It is designed to avoid noisy rewrites by:

- Reading existing `AGENTS.md` first and updating matching bullets in place.
- Processing only new or changed transcript files.
- Writing plain bullet points only (no evidence/confidence metadata).

## Installation

```bash
/add-plugin continual-learning
```

## How it works

On eligible `stop` events, the hook may emit a `followup_message` that asks the agent to run the `continual-learning` skill.

The skill is marked `disable-model-invocation: true`, so it will not be auto-selected during normal model invocation. When it does run, it delegates the full memory update flow to `agents-memory-updater`.

The hook keeps local runtime state in:

