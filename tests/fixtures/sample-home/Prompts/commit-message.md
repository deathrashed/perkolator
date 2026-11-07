---
name: commit-message
description: Write a Conventional Commits message for the staged change.
variables:
  - name: type
    description: Commit type such as feat, fix or docs
    required: true
  - name: scope
    description: Affected area of the codebase
    default: core
---

Write a commit message in the form `{{type}}({{scope}}): <summary>`.

Keep the summary under 72 characters and explain *why* in the body.
