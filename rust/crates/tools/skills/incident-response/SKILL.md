---
name: incident-response
description: "Structured incident triage and post-incident analysis"
---

# /incident-response — Incident Response & RCA

You are conducting a structured incident response or post-incident analysis.

## Mode: Active Incident (Triage)

If the incident is ongoing:

### Step 1: Assess Impact
- What is broken? Which users/services are affected?
- When did it start? (check recent deploys, commits)
- What is the severity? (P0 critical / P1 high / P2 medium / P3 low)

### Step 2: Identify Cause
Use `bash` and `grep_search` to:
1. Check recent deployments: `git log --oneline -10`
2. Check for config changes: `git diff HEAD~5 -- '*.json' '*.toml' '*.yaml' '*.env'`
3. Check error logs if accessible
4. Identify the most likely root cause

### Step 3: Mitigate
Suggest immediate actions:
- Rollback to last known good state
- Feature flag disable
- Config revert
- Scaling adjustment

### Step 4: Communicate
Draft a status update:
```
**Incident Update — [Title]**
Status: Investigating / Identified / Mitigating / Resolved
Impact: [description]
Current action: [what's being done]
ETA: [if known]
```

## Mode: Post-Incident (RCA)

If the incident is resolved:

### Step 1: Build Timeline

Use `bash` to gather:
```bash
git log --since="INCIDENT_START" --until="INCIDENT_END" --format="%ai %h %s"
```

### Step 2: Write RCA Document

Use `write_file` to create the RCA:

```markdown
# Post-Incident Review: [Title]

**Date:** YYYY-MM-DD
**Duration:** X hours Y minutes
**Severity:** P0/P1/P2/P3
**Author:** [name]

## Summary
One paragraph describing what happened and its impact.

## Timeline
| Time | Event |
|------|-------|
| HH:MM | First alert / user report |
| HH:MM | Investigation started |
| HH:MM | Root cause identified |
| HH:MM | Fix deployed |
| HH:MM | Incident resolved |

## Root Cause
Technical explanation of what went wrong and why.

## Impact
- Users affected: N
- Duration: X hours
- Revenue impact: if applicable
- Data impact: if applicable

## What Went Well
- ...

## What Went Poorly
- ...

## Action Items
| Action | Owner | Priority | Due Date |
|--------|-------|----------|----------|
| ... | ... | ... | ... |

## Lessons Learned
- ...
```
