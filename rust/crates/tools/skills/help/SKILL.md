---
name: help
description: Guide on using oh-my-codex plugin and the bundled flaco skills.
---

# help

Guide on using oh-my-codex plugin and the bundled flaco skills.

## Overview

This skill is the landing page a user hits when they invoke `/help` or
`$help` inside the REPL. It points at the rest of the bundled skills
and explains how to run them.

## Usage

Invoke with no args for the overview:

```
$help
```

Or pass a topic for details:

```
$help overview
$help bundled-skills
```

## Bundled skills

- `architecture` — guided architecture review
- `code-review` — PR-style code review of staged changes
- `debug` — structured debugging walkthrough
- `deploy-checklist` — pre-ship sanity checklist
- `documentation` — generate or update inline docs
- `incident-response` — incident triage runbook
- `onboarding` — onboarding questions for a new repo
- `retro` — sprint retro template
- `standup` — daily standup template
- `tech-debt` — tech-debt inventory pass

Each lives at `skills/<name>/SKILL.md` inside the `tools` crate and
ships alongside the installed binary.
