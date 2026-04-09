---
name: deploy-checklist
description: "Pre-deployment verification checklist"
---

# /deploy-checklist — Pre-Deployment Verification

You are generating and executing a pre-deployment checklist. Adapt checks to the detected project stack.

## Step 1: Detect Project Stack

Use `glob_search` and `read_file` to identify:
- Language/framework (Cargo.toml, package.json, pyproject.toml, Package.swift, etc.)
- CI configuration (.github/workflows, .gitlab-ci.yml, etc.)
- Docker/container files
- Database migration files
- Environment configuration (.env.example, etc.)

## Step 2: Execute Checklist

Run each applicable check via `bash` and report pass/fail:

### Build
- [ ] Project compiles without errors
- [ ] No compiler warnings (or only known/accepted ones)

### Tests
- [ ] Unit tests pass
- [ ] Integration tests pass (if present)
- [ ] Test coverage has not decreased (if measurable)

### Code Quality
- [ ] Linter passes (clippy, eslint, pylint, swiftlint, etc.)
- [ ] Formatter passes (rustfmt, prettier, black, etc.)
- [ ] No TODO/FIXME/HACK comments in changed files

### Security
- [ ] No hardcoded secrets in codebase (`grep_search` for API keys, passwords, tokens)
- [ ] Dependencies are up to date (`cargo audit`, `npm audit`, etc.)
- [ ] No known vulnerability advisories

### Configuration
- [ ] Environment variables documented and set
- [ ] Feature flags configured correctly
- [ ] Config files are production-ready (no debug settings)

### Database
- [ ] Migrations are reversible
- [ ] No destructive schema changes without a rollback plan
- [ ] Migration tested against a copy of production data

### Deployment
- [ ] CHANGELOG or release notes updated
- [ ] Version number bumped appropriately
- [ ] Rollback procedure documented

## Step 3: Output

```
## Deploy Checklist — [project name]

Date: YYYY-MM-DD
Branch: current branch
Commit: short SHA

| Check | Status | Notes |
|-------|--------|-------|
| Build | PASS/FAIL | ... |
| Tests | PASS/FAIL | ... |
| ... | ... | ... |

### Blockers
- List any FAIL items that must be resolved before deploy

### Verdict
READY TO DEPLOY / BLOCKED (N items)
```
