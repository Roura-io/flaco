---
name: tech-debt
description: "Identify and prioritize technical debt"
---

# /tech-debt — Technical Debt Audit

You are conducting a technical debt audit of this project.

## Step 1: Scan for Debt Indicators

Use `grep_search` to find common debt markers:

```
# Explicit markers
TODO, FIXME, HACK, XXX, WORKAROUND, TEMPORARY, TECH_DEBT

# Code smells
unwrap() (in Rust — unchecked panics)
# type: ignore (Python — suppressed type checks)
@ts-ignore, @ts-expect-error (TypeScript)
// eslint-disable (JavaScript — suppressed lint)
#allow (Rust — suppressed clippy)
```

Use `bash` to find structural indicators:
```bash
# Large files (complexity risk)
find . -name '*.rs' -o -name '*.py' -o -name '*.ts' | xargs wc -l | sort -rn | head -20

# Files with most git churn (stability risk)
git log --since="3 months ago" --name-only --format="" | sort | uniq -c | sort -rn | head -20

# Old uncommitted TODOs
git log --all --oneline --grep="TODO" | head -10
```

## Step 2: Categorize Debt

Group findings into categories:

| Category | Description | Risk |
|----------|-------------|------|
| **Code Quality** | Dead code, complex functions, missing abstractions | Medium |
| **Testing** | Missing tests, flaky tests, untested paths | High |
| **Dependencies** | Outdated deps, security advisories, deprecated APIs | High |
| **Architecture** | Tight coupling, missing boundaries, god objects | High |
| **Documentation** | Missing docs, outdated docs, undocumented APIs | Low |
| **Performance** | Known bottlenecks, unoptimized paths | Medium |
| **Security** | Hardcoded credentials, missing validation | Critical |

## Step 3: Prioritize

Score each item on:
- **Impact** — How much does this hurt? (1-5)
- **Effort** — How hard is the fix? (1-5, where 1 = easy)
- **Risk** — What happens if we don't fix it? (1-5)
- **Priority** = (Impact + Risk) / Effort

## Step 4: Output Report

```markdown
## Tech Debt Audit — [Project Name]

**Date:** YYYY-MM-DD
**Files scanned:** N
**Debt items found:** N

### Critical (fix now)
| Item | File | Category | Impact | Effort | Score |
|------|------|----------|--------|--------|-------|
| ... | ... | ... | ... | ... | ... |

### High Priority (next sprint)
| ... | ... | ... | ... | ... | ... |

### Medium Priority (backlog)
| ... | ... | ... | ... | ... | ... |

### Low Priority (when convenient)
| ... | ... | ... | ... | ... | ... |

### Summary
- Total debt items: N
- Estimated effort to clear critical+high: ...
- Top 3 recommendations: ...
```
