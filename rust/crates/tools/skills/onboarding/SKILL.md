---
name: onboarding
description: "Generate onboarding guide for new contributors"
---

# /onboarding — New Contributor Onboarding Guide

You are generating a comprehensive onboarding guide for someone new to this project.

## Step 1: Analyze the Project

Use `glob_search`, `read_file`, and `bash` to discover:

1. **Language and framework**: Check Cargo.toml, package.json, pyproject.toml, Package.swift, go.mod
2. **Project structure**: `ls` the top-level directory, identify key directories
3. **Build system**: How to build, run, and test
4. **Documentation**: Existing README, CONTRIBUTING, docs/ directory
5. **CI/CD**: .github/workflows, .gitlab-ci.yml, etc.
6. **Instruction files**: FLACOAI.md, CLAUDE.md, .editorconfig, etc.
7. **Dependencies**: List key dependencies and their purpose

## Step 2: Trace the Entry Points

1. Find the main entry point(s) — `main.rs`, `main.py`, `index.ts`, etc.
2. Trace one request/command through the system to understand the flow
3. Identify the key modules and their responsibilities

## Step 3: Generate the Guide

Use `write_file` to create or update a `ONBOARDING.md`:

```markdown
# Onboarding Guide — [Project Name]

Welcome! This guide will get you productive in [project name] as quickly as possible.

## Prerequisites

- [Language] version X.Y+
- [Tool] for building/running
- [Other requirements]

## Quick Start

```bash
# Clone and setup
git clone [repo]
cd [project]

# Install dependencies
[command]

# Build
[command]

# Run tests
[command]

# Run the project
[command]
```

## Project Structure

```
project/
├── src/          — [what this contains]
├── tests/        — [what this contains]
├── docs/         — [what this contains]
└── ...
```

## Architecture Overview

[Brief description of how the system works, key abstractions, data flow]

## Key Concepts

- **[Concept 1]** — What it is and why it matters
- **[Concept 2]** — What it is and why it matters

## Development Workflow

1. Create a branch: `git checkout -b feature/my-feature`
2. Make changes
3. Run tests: `[command]`
4. Run linter: `[command]`
5. Commit and push
6. Open a PR

## Common Tasks

### Adding a new [feature/module/endpoint]
Step-by-step instructions...

### Running specific tests
How to run a single test or test file...

### Debugging
How to debug, common issues and solutions...

## Code Conventions

- [Convention 1 from FLACOAI.md or detected patterns]
- [Convention 2]

## Useful Commands

| Command | Description |
|---------|-------------|
| `...` | ... |

## Who to Ask

- [Area] — [person/team, if known]

## Resources

- [Link to docs, wiki, etc.]
```

Tailor the guide to what actually exists in the project. Don't include sections for things that don't apply.
