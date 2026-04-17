---
name: documentation
description: "Write and maintain technical documentation"
---

# /documentation — Technical Documentation

You are writing or updating technical documentation for this project.

## Step 1: Assess Current State

1. Use `glob_search` to find existing documentation: `**/*.md`, `**/docs/**`, `**/README*`
2. Use `read_file` to review what documentation exists
3. Identify gaps: undocumented modules, outdated sections, missing setup instructions

## Step 2: Determine Scope

Based on user input or detected gaps, decide what to document:

- **README** — Project overview, quick start, installation
- **API docs** — Public function/module documentation
- **Architecture** — System design, data flow, key decisions
- **Setup guide** — Development environment, dependencies, configuration
- **Contributing** — How to contribute, code style, PR process
- **Changelog** — Recent changes in user-facing terms

## Step 3: Write Documentation

Follow these principles:

1. **Lead with purpose** — First sentence explains what this thing does and why someone cares
2. **Show, don't tell** — Include code examples, command snippets, expected output
3. **Keep it scannable** — Use headers, bullet points, tables. No walls of text
4. **Stay accurate** — Read the actual code before documenting behavior. Use `grep_search` to verify function signatures, config keys, CLI flags
5. **Match the project voice** — Check existing docs for tone and style, then match it

## Step 4: Write or Update Files

Use `write_file` or `edit_file` to create or update documentation files.

For inline code documentation:
- Rust: `///` doc comments on public items
- Python: docstrings
- Swift: `///` or `/** */` DocC-style
- TypeScript: JSDoc `/** */`

## Step 5: Verify

1. Check that all links and references are valid
2. Ensure code examples actually work (run them via `bash` if possible)
3. Confirm no sensitive information (keys, internal URLs) is included

## Output

Summarize what was documented:
```
## Documentation Update

**Files created/updated:**
- path/to/file.md — description of changes

**Coverage:**
- Modules documented: N/M
- Key gaps remaining: ...
```
