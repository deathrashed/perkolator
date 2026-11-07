---
name: frontend-design
description: Build restrained, dense desktop and web interfaces using design tokens, consistent spacing and accessible contrast.
version: 1.2.0
author: Perkolator Fixtures
tags: [design, ui, css]
license: Apache-2.0
metadata:
  audience: frontend
  reviewed: true
---

# Frontend Design

Use this skill when building or reviewing user interface code.

## Tokens

| Token | Value | Use |
| --- | --- | --- |
| `space-2` | 8px | Dense row padding |
| `radius-sm` | 4px | Inputs and list rows |
| `accent` | `#1abc9b` | Sparing emphasis only |

## Checklist

1. Read `references/design-tokens.md` before choosing colours.
2. Prefer neutral surfaces and understated borders.
3. Run the contrast check:

```sh
scripts/check.sh src/
```

See `references/deep/layout.md` for split-pane layout guidance.
