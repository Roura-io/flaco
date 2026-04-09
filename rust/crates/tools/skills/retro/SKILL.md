---
name: retro
description: "Sprint or project retrospective generator"
---

# /retro — Retrospective

You are facilitating a sprint or project retrospective.

## Step 1: Gather Data

Use `bash` to collect objective data about the period:

```bash
# Commits in the sprint period (adjust dates)
git log --since="2 weeks ago" --oneline --all --no-merges

# Files most frequently changed (hotspots)
git log --since="2 weeks ago" --name-only --format="" | sort | uniq -c | sort -rn | head -20

# Contributors
git shortlog --since="2 weeks ago" -sn --all --no-merges
```

Check for any TODO items, open issues, or known tech debt.

## Step 2: Analyze Patterns

From the git data, identify:
- **Velocity** — How many commits/features shipped?
- **Hotspots** — Files changed most often (potential complexity/debt)
- **Churn** — Files that were changed then changed again (rework)
- **Test coverage** — Were tests added alongside features?

## Step 3: Generate Retrospective

```markdown
## Retrospective — [Sprint/Period Name]

**Period:** YYYY-MM-DD to YYYY-MM-DD
**Team:** [names or solo]

### By the Numbers
- Commits: N
- Files changed: N
- Features shipped: N
- Bugs fixed: N

### What Went Well
- Specific accomplishment with evidence from git history
- Process improvement that worked
- Tool or technique that helped

### What Didn't Go Well
- Specific challenge or friction point
- Rework or wasted effort (cite file churn data)
- Missing tests, docs, or process gaps

### What Was Surprising
- Unexpected findings from the data
- Patterns that weren't obvious during the sprint

### Action Items
| Action | Owner | Priority |
|--------|-------|----------|
| ... | ... | High/Medium/Low |

### Shoutouts
- Recognition for notable contributions
```

Encourage honest, blameless reflection. Focus on systems and processes, not individuals.
