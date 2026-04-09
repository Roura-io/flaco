---
name: standup
description: "Generate daily standup summary from recent git activity"
---

# /standup — Daily Standup Report

You are generating a daily standup report from the repository's recent activity.

## Step 1: Gather Git Activity

Run these commands via `bash`:

```bash
# Commits since yesterday (or last working day)
git log --since="yesterday" --oneline --all --no-merges

# Commits with full details
git log --since="yesterday" --format="%h %s (%an, %ar)" --all --no-merges

# Currently staged/unstaged changes
git status --short

# Current branch
git branch --show-current
```

If there are no commits since yesterday, expand to `--since="3 days ago"` to cover weekends.

## Step 2: Categorize Changes

Group commits by type:
- **Features** — new functionality
- **Fixes** — bug fixes
- **Refactors** — code improvements without behavior change
- **Docs** — documentation updates
- **Tests** — test additions or updates
- **Chores** — dependency updates, CI, config changes

## Step 3: Check Work In Progress

1. Check uncommitted changes via `git status`
2. Check for any TODO items in the project (look for `.flacoai-todos.json`)
3. Check current branch name for context on what's being worked on

## Step 4: Generate Report

```
## Standup — YYYY-MM-DD

**Branch:** main (or current branch)
**Period:** since yesterday / since Friday

### Completed
- [hash] Description of completed work
- [hash] Description of completed work

### In Progress
- Current branch work: description
- Uncommitted changes: N files modified

### Blocked / Needs Attention
- Any failing tests or build issues
- Any items that need review

### Plan for Today
- Based on current branch and WIP, suggest next steps
```

If running in a team context, group by author. If solo, keep it personal.
