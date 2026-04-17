---
name: common-coding-style
description: Language-agnostic coding conventions
paths: []
category: coding-style
---

# Common Coding Style

## Naming

- Use descriptive, intention-revealing names. A variable named `elapsed_time_in_days` beats `d`.
- Boolean variables and functions start with `is_`, `has_`, `can_`, `should_`, or similar predicates: `is_valid`, `has_permission`, not `valid` or `permission_flag`.
- Constants use SCREAMING_SNAKE_CASE regardless of language: `MAX_RETRY_COUNT`, `DEFAULT_TIMEOUT_MS`.
- Abbreviations follow the casing of the surrounding context. In camelCase: `httpClient`, `jsonParser`. In PascalCase: `HttpClient`, `JsonParser`. Never `hTTPClient`.
- Avoid Hungarian notation and type prefixes. The type system already tells you the type.
- Name things after what they represent, not how they are implemented: `user_accounts` not `user_hash_map`.

## Formatting

- One statement per line. Never chain multiple assignments or side effects on a single line.
- Maximum line length: 100 characters. Break long lines at logical boundaries (after an operator, before a parameter).
- Indentation: use the project's established convention (tabs or spaces). If no convention exists, prefer 4 spaces for most languages, 2 spaces for YAML/HTML/JSON.
- Blank lines separate logical sections within a function. Two blank lines separate top-level definitions (functions, classes, modules).
- Opening braces on the same line as the declaration (K&R style) unless the language convention strongly dictates otherwise.
- No trailing whitespace. Configure your editor.

## Comments

- Code should be self-documenting. If a comment restates what the code does, delete it.
- Comments explain WHY, not WHAT. `// Retry because the upstream API rate-limits at 100 req/min` is useful. `// increment counter` is noise.
- TODO comments include a ticket ID or owner: `// TODO(cjroura): migrate to v2 API after 2026-06-01`.
- Remove commented-out code. That is what version control is for.
- Public APIs get doc comments that describe behavior, parameters, return values, and error conditions. Internal helpers only need comments when the logic is non-obvious.

## DRY (Don't Repeat Yourself)

- If you copy-paste a block of code, extract it into a function or module immediately. The third time you reach for the same pattern, it is a mandatory extraction.
- Configuration values, magic numbers, and string literals that appear more than once belong in a named constant or config file.
- DRY applies to logic, not to structure. Two functions that look similar but serve fundamentally different domains are not duplication -- they are coincidence. Do not force them together.

## KISS (Keep It Simple, Stupid)

- Prefer straightforward control flow. Avoid clever tricks that require a comment to explain.
- Favor composition over inheritance. Small, focused functions over monolithic methods.
- If a function needs more than 3 levels of nesting, refactor. Extract the inner logic into a helper.
- Functions should do one thing. If the function name contains "and", split it.
- Maximum function length: 40 lines of logic (excluding blank lines and comments). If you exceed this, the function is doing too much.
- Avoid premature abstraction. Write concrete code first; abstract only when a real second use case appears.

## Error Handling

- Never silently swallow errors. At minimum, log them.
- Fail fast and fail loudly. Validate inputs at the boundary; do not pass bad data deeper into the system hoping something downstream will catch it.
- Use the language's idiomatic error handling mechanism (exceptions, Result types, error returns). Do not invent a custom error protocol unless the language lacks one.
- Error messages must be actionable: include what happened, what was expected, and what the caller can do about it.

## Dependencies

- Every dependency is a liability. Before adding one, verify it is actively maintained, has a reasonable API surface, and is not pulling in a transitive dependency tree larger than your project.
- Pin dependency versions in production. Floating ranges belong only in libraries, never in deployed applications.
- Audit dependencies for known vulnerabilities on a regular cadence.

## Version Control Hygiene

- Commits are atomic: one logical change per commit.
- Commit messages use imperative mood: "Add retry logic" not "Added retry logic".
- Never commit secrets, credentials, or environment-specific configuration.
- Keep `.gitignore` current. Build artifacts, editor files, and OS metadata do not belong in the repo.
