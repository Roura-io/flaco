---
name: debug
description: "Structured debugging session — reproduce, isolate, diagnose, and fix"
---

# /debug — Structured Debugging

You are running a structured debugging session. Follow these phases in order. Do not skip phases.

## Phase 1: Reproduce

Ask the user (or infer from context) what the bug is. Then:

1. Use `grep_search` to find the relevant code
2. Use `bash` to run the failing command or test
3. Capture the exact error output
4. Document: **what happens** vs **what should happen**

Output:
```
## Reproduction
- Command: `...`
- Actual: ...
- Expected: ...
- Reproducible: yes/no
```

## Phase 2: Isolate

Narrow down the root cause:

1. Read the stack trace or error message carefully
2. Use `grep_search` to trace the call chain from the error back to the origin
3. Use `read_file` to examine each function in the chain
4. Add diagnostic output if needed (temporary print/log statements via `edit_file`)
5. Re-run to confirm the exact line or condition that triggers the bug

Output:
```
## Isolation
- Root file: path/to/file.rs
- Root function: function_name (line N)
- Trigger condition: ...
```

## Phase 3: Diagnose

Explain the bug:

1. What is the code doing wrong?
2. Why does it produce the incorrect behavior?
3. What assumptions does the code make that are violated?
4. Are there related bugs or edge cases nearby?

Output:
```
## Diagnosis
- Cause: ...
- Why: ...
- Related risks: ...
```

## Phase 4: Fix

Implement the fix:

1. Use `edit_file` to make the minimal change that corrects the behavior
2. Remove any diagnostic output added in Phase 2
3. Use `bash` to re-run the failing command — confirm it passes
4. Use `bash` to run the full test suite — confirm no regressions

Output:
```
## Fix
- Change: description of what was changed
- Verification: test command and result
- Regression check: test suite result
```

If any phase fails, document why and ask the user for guidance before proceeding.
