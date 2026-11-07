# Hookify Plugin

Easily create custom hooks to prevent unwanted behaviors by analyzing conversation patterns or from explicit instructions.

## Overview

The hookify plugin makes it simple to create hooks without editing complex `hooks.json` files. Instead, you create lightweight markdown configuration files that define patterns to watch for and messages to show when those patterns match.

**Key features:**
- 🎯 Analyze conversations to find unwanted behaviors automatically
- 📝 Simple markdown configuration files with YAML frontmatter
- 🔍 Regex pattern matching for powerful rules
- 🚀 No coding required - just describe the behavior
- 🔄 Easy enable/disable without restarting

## Quick Start

### 1. Create Your First Rule

```bash
/hookify Warn me when I use rm -rf commands
```

This analyzes your request and creates `.claude/hookify.warn-rm.local.md`.

### 2. Test It Immediately

**No restart needed!** Rules take effect on the very next tool use.

Ask Claude to run a command that should trigger the rule:
```
Run rm -rf /tmp/test
```

You should see the warning message immediately!

## Usage

### Main Command: /hookify

