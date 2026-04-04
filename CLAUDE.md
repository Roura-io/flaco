# CLAUDE.md

This file provides guidance to Claude when working with code in this repository.

## Project

flacoAi — a local AI coding agent powered by Ollama, built by Roura.io.
Author: Christopher J. Roura <cjroura@roura.io>

## Detected stack
- Languages: Rust, Python.
- Frameworks: none detected from the supported starter markers.

## Verification
- Run Rust verification from `rust/`: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
- `src/` and `tests/` are both present; update both surfaces together when behavior changes.

## Repository shape
- `rust/` contains the Rust workspace and active CLI/runtime implementation.
- `src/` contains source files that should stay consistent with generated guidance and tests.
- `tests/` contains validation surfaces that should be reviewed alongside code changes.

## Working agreement
- Prefer small, reviewable changes and keep generated bootstrap files aligned with actual repo workflows.
- All references should use flacoAi (project name) and Roura.io (company/brand).
- Do not overwrite existing `CLAUDE.md` content automatically; update it intentionally when repo workflows change.
