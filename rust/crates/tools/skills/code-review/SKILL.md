---
name: code-review
description: "Review code changes for security, performance, and correctness"
---

# /code-review — Code Review

You are performing a structured code review. Follow this workflow precisely.

## Step 1: Identify Changes

Run `bash` to execute:
```
git diff --name-only HEAD~1
```
If no commits exist, fall back to `git diff --name-only` for unstaged changes, or `git diff --cached --name-only` for staged changes.

## Step 2: Read Each Changed File

Use `read_file` to load the full content of every changed file. For large files, focus on the changed regions using `git diff` output.

## Step 3: Review Checklist

For each file, evaluate against these categories:

### Security
- Command injection, SQL injection, XSS, path traversal
- Hardcoded secrets, API keys, credentials
- Unsafe deserialization, improper input validation
- Missing authentication or authorization checks

### Performance
- N+1 queries, unbounded loops, missing pagination
- Unnecessary allocations, copies, or clones
- Missing caching for expensive operations
- Blocking calls in async contexts

### Correctness
- Off-by-one errors, nil/null dereference risks
- Race conditions, missing locks in concurrent code
- Unhandled error cases, swallowed errors
- Logic bugs in conditionals or state transitions

### Code Quality
- Naming clarity, dead code, unused imports
- Missing or misleading documentation
- Violation of project conventions (check FLACOAI.md)
- Test coverage gaps for new behavior

## Step 4: Output

Present findings as a structured report:

```
## Code Review Summary

**Files reviewed:** N
**Issues found:** N (X critical, Y warning, Z info)

### Critical
- [file:line] Description of issue and suggested fix

### Warnings
- [file:line] Description and recommendation

### Info
- [file:line] Observation or suggestion

### Verdict
APPROVE / REQUEST_CHANGES / NEEDS_DISCUSSION
```

If no issues are found, say so explicitly and approve.
