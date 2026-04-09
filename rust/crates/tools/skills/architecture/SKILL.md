---
name: architecture
description: "Create or evaluate an architecture decision record (ADR)"
---

# /architecture — Architecture Decision Records

You are creating or evaluating an Architecture Decision Record (ADR).

## Mode: Create (default)

### Step 1: Gather Context

1. Ask the user what decision needs to be made (or infer from conversation)
2. Use `glob_search` and `read_file` to understand the current architecture
3. Check for existing ADRs: `glob_search` for `**/adr/**`, `**/docs/decisions/**`, `**/architecture/**`

### Step 2: Identify Options

Research at least 2-3 viable approaches. For each:
- How it works technically
- Pros and cons
- Impact on existing code
- Operational complexity
- Team skill requirements

### Step 3: Write the ADR

Use this template and write via `write_file`:

```markdown
# ADR-NNN: [Title]

**Date:** YYYY-MM-DD
**Status:** Proposed | Accepted | Deprecated | Superseded
**Author:** [name]

## Context

What is the problem or opportunity? Why does this decision need to be made now?

## Decision Drivers

- Driver 1 (e.g., performance requirements)
- Driver 2 (e.g., team familiarity)
- Driver 3 (e.g., operational cost)

## Options Considered

### Option A: [Name]
- Description
- **Pros:** ...
- **Cons:** ...

### Option B: [Name]
- Description
- **Pros:** ...
- **Cons:** ...

### Option C: [Name]
- Description
- **Pros:** ...
- **Cons:** ...

## Decision

We chose **Option X** because [reasoning tied back to decision drivers].

## Consequences

### Positive
- ...

### Negative
- ...

### Risks
- ...

## Follow-up Actions
- [ ] Action item 1
- [ ] Action item 2
```

## Mode: Evaluate

If the user provides an existing ADR or architectural proposal:

1. Read the document
2. Check assumptions against the actual codebase
3. Identify missing considerations
4. Assess feasibility of the proposed approach
5. Provide a structured evaluation with recommendations
