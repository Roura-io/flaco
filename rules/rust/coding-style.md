---
name: rust-coding-style
description: Rust coding conventions for ownership, error handling, and idiomatic patterns
language: rust
paths: ["**/*.rs"]
category: coding-style
---

# Rust Coding Style

## Ownership and Borrowing

- Prefer borrowing (`&T`, `&mut T`) over ownership transfer unless the function genuinely needs to own the data. If in doubt, start with a reference and upgrade to owned only when the compiler demands it.
- Never `.clone()` just to satisfy the borrow checker. If you need to clone, document why in a comment. Most clones indicate a design issue that can be solved by restructuring lifetimes or splitting borrows.
- Use `Cow<'_, str>` and `Cow<'_, [T]>` at API boundaries where the caller may or may not own the data. This avoids forcing allocations on callers who already have a reference.
- Prefer `&str` over `&String` and `&[T]` over `&Vec<T>` in function parameters. Accept the most general form.
- Use `impl AsRef<str>` or `impl Into<String>` for public API parameters that should accept both owned and borrowed strings.

## Error Handling

- Use `Result<T, E>` for recoverable errors. Use `panic!` only for unrecoverable programmer errors (invariant violations, impossible states).
- Never use `.unwrap()` or `.expect()` in library code or production paths. Use `?` with proper error context.
- Use `thiserror` for library error types and `anyhow`/`eyre` for application-level error handling.
- Error messages are lowercase, no trailing punctuation: `"failed to connect to database"` not `"Failed to connect to database."`.
- Wrap errors with context using `.context()` or `.with_context(|| ...)` from anyhow/eyre: `file.read_to_string(&mut buf).context("reading config file")?;`.
- Every `// SAFETY:` comment on an `unsafe` block must document the specific invariants being upheld. `// SAFETY: safe because we checked` is not acceptable.

## Formatting and Linting

- Run `cargo fmt` before every commit. No exceptions. Use `rustfmt.toml` for project-specific overrides.
- Run `cargo clippy -- -D warnings` in CI. Treat clippy warnings as errors. If a lint is genuinely wrong, suppress it with `#[allow(clippy::lint_name)]` and a comment explaining why.
- Maximum line length: 100 characters (rustfmt default). Let rustfmt handle wrapping.

## Naming Conventions

- Follow Rust API Guidelines (RFC 430): `snake_case` for functions and variables, `PascalCase` for types and traits, `SCREAMING_SNAKE_CASE` for constants and statics.
- Conversion functions: `as_*` for cheap reference-to-reference, `to_*` for expensive conversions that allocate, `into_*` for ownership-consuming conversions.
- Builder methods return `&mut Self` (mutable builder) or `Self` (consuming builder). Be consistent within a type.
- Getter methods omit the `get_` prefix: `fn name(&self) -> &str`, not `fn get_name(&self) -> &str`.

## Type Design

- Use newtypes to enforce domain semantics: `struct UserId(u64)` prevents accidentally passing a `PostId` where a `UserId` is expected.
- Prefer enums over boolean flags: `enum Visibility { Public, Private }` is clearer than `is_public: bool`.
- Make illegal states unrepresentable. If a struct can only be valid when certain fields are set together, use a builder or a more constrained type.
- Derive traits liberally: `#[derive(Debug, Clone, PartialEq, Eq, Hash)]` on most data types. Add `Serialize, Deserialize` when the type crosses a serialization boundary.
- Use `#[must_use]` on functions whose return value should not be ignored (especially Result-returning functions and builder methods).
- Use `#[non_exhaustive]` on public enums and structs in libraries to preserve future extensibility.

## Async Code

- Never block in async context. No `std::thread::sleep`, no `std::fs`, no synchronous I/O. Use `tokio::time::sleep`, `tokio::fs`, etc.
- Use `tokio::spawn` for concurrent tasks, not `std::thread::spawn`, unless you need OS-level parallelism for CPU-bound work.
- Annotate async functions with `Send` bounds on their futures when they will be spawned across threads.
- Prefer structured concurrency: `tokio::join!` or `JoinSet` over fire-and-forget spawns.

## Performance Defaults

- Use `Vec::with_capacity(n)` when the size is known or estimable.
- Prefer iterators over indexed loops. The compiler optimizes iterator chains aggressively.
- Use `&str` and slices for read-only data. Allocate only when you need to store or mutate.
- Profile before optimizing. Use `cargo bench` with criterion for benchmarks. Do not optimize based on intuition.

## Documentation

- Every public item gets a `///` doc comment. The first line is a summary sentence. Additional paragraphs follow after a blank line.
- Include `# Examples` sections in doc comments for non-obvious APIs. These double as doc-tests and run in CI.
- Use `# Panics`, `# Errors`, and `# Safety` sections in doc comments where applicable.
- Module-level docs (`//!`) describe the purpose of the module and how its types relate to each other.
