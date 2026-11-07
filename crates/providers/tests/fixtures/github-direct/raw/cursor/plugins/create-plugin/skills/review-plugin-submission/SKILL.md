---
name: review-plugin-submission
description: Audit a Cursor plugin for marketplace readiness. Use when validating manifests, component metadata, discovery paths, and submission quality before publishing.
---

# Review plugin submission

## Trigger

A plugin is implemented and needs a final quality check before submission or release.

## Workflow

1. Verify manifest validity:
   - `.cursor-plugin/plugin.json` exists
   - `name` is valid lowercase kebab-case
