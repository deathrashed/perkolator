# protect-mcp

Cedar policy enforcement + Ed25519 signed receipts for every Claude Code tool call.

[![npm version](https://img.shields.io/npm/v/protect-mcp)](https://www.npmjs.com/package/protect-mcp)
[![Downloads](https://img.shields.io/npm/dm/protect-mcp)](https://www.npmjs.com/package/protect-mcp)
[![License](https://img.shields.io/badge/license-MIT-blue)](../../LICENSE)

The first Claude Code plugin that enforces declarative authorization policies
and produces cryptographically verifiable audit trails. Every tool call is
evaluated against a Cedar policy, every tool call that runs gets an
Ed25519-signed receipt, and every receipt is verifiable offline by anyone
with the public key.

## What You Get

- **Cedar policy enforcement** — Block tool calls that violate your rules before they execute. Cedar is AWS's open authorization engine, formally verified.
- **Ed25519 signed receipts**: every tool call that runs produces a tamper-evident receipt, signed with RFC 8032 Ed25519 over RFC 8785 JCS canonical bytes.
- **Receipts file**: receipts are appended to `./receipts/receipts.jsonl`. A modified receipt fails verification, but receipts carry no link to the previous receipt, so a deleted line goes undetected.
- **Offline verification**: `npx @veritasacta/verify@0.9.2 --replay-chain` with the public key needs no vendor lookup or account. `npx` downloads the verifier on first use, so run `npm install --no-save @veritasacta/verify@0.9.2` in the project before you go offline.

## Quick Start

```bash
# 1. Install this plugin
claude plugin install wshobson/agents/protect-mcp

# 2. Create a Cedar policy file at ./protect.cedar
#    (see skills/protect-mcp-setup/SKILL.md for examples)

