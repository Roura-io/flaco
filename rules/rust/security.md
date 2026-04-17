---
name: rust-security
description: Rust-specific security rules for unsafe code, panic safety, and injection prevention
language: rust
paths: ["**/*.rs"]
category: security
---

# Rust Security Rules

## Unsafe Code

- Every `unsafe` block requires a `// SAFETY:` comment immediately above it that explains exactly which invariants are being upheld and why they hold.
- Minimize the scope of `unsafe`. Wrap unsafe operations in safe abstractions with well-defined invariants. The unsafe block should be as small as possible.
- Never use `unsafe` to bypass the borrow checker out of convenience. If the borrow checker rejects your code, restructure the design.
- Audit all raw pointer dereferences. Document the lifetime guarantees that make the dereference valid.
- Use `std::ptr::NonNull` instead of raw `*mut T` when the pointer is guaranteed non-null.
- `unsafe impl Send` and `unsafe impl Sync` require ironclad reasoning. Document the thread-safety invariants.
- Prefer `MaybeUninit<T>` over `std::mem::uninitialized()` or `std::mem::zeroed()` for uninitialized memory.
- Run Miri (`cargo +nightly miri test`) in CI to detect undefined behavior in unsafe code.

## Panic Prevention

- Library code must not panic on valid (even unusual) input. Return `Result` or `Option` instead.
- Replace `.unwrap()` in production paths with `.expect("reason")` at minimum, but prefer `?` with context.
- Use `#[cfg(debug_assertions)]` for debug-only assertions. Release builds should not panic on recoverable conditions.
- Avoid `panic!` in Drop implementations. A panic during unwinding causes an abort.
- Index arrays and slices with `.get()` instead of `[]` when the index might be out of bounds.
- Use `checked_add`, `checked_mul`, and similar methods for arithmetic that might overflow in release mode (where overflow wraps silently).

## Command Injection

- Never use `std::process::Command::new("sh").arg("-c").arg(user_input)`. Build the command with explicit arguments: `Command::new("git").arg("clone").arg(&sanitized_url)`.
- Validate and sanitize all external input before passing it to system commands. Reject input containing shell metacharacters if passing to a shell is unavoidable.
- Prefer `Command::args(&["arg1", "arg2"])` over string interpolation.

## SQL and Data Injection

- Use parameterized queries with `sqlx::query!` or `diesel` query builder. Never format user input into SQL strings with `format!`.
- For dynamic queries, use query builders that handle escaping, not string concatenation.
- Validate input types and ranges before they reach the query layer.

## Cryptography

- Use `ring`, `rustls`, or `rcgen` for cryptographic operations. Do not implement custom crypto.
- Never use `rand::thread_rng()` for security-sensitive randomness. Use `rand::rngs::OsRng` or `getrandom`.
- Use constant-time comparison (`ring::constant_time::verify_slices_are_equal`) for secret comparison.
- Store password hashes with `argon2` or `bcrypt` crate, never with SHA-256 or MD5.

## Dependency Security

- Run `cargo audit` in CI on every build. Block merges on advisories with severity >= medium.
- Pin dependencies to exact versions in `Cargo.lock` and commit the lockfile for binaries (not for libraries).
- Audit new dependencies before adding them. Check for `unsafe` usage, maintenance status, and known issues.
- Use `cargo deny` for license compliance and duplicate dependency detection.

## Memory Safety

- Use `Vec`, `Box`, `Arc`, and `Rc` instead of raw allocations. Manual memory management via raw pointers is a last resort.
- Avoid `std::mem::transmute` unless absolutely necessary. Prefer `From`/`Into` conversions or `as` casts.
- Use `Pin` for self-referential types. Document why pinning is necessary.
- Validate all FFI boundaries. Extern functions cannot rely on Rust's safety guarantees.

## Serialization Security

- Never deserialize untrusted data without size limits and validation. Use `serde` with `#[serde(deny_unknown_fields)]` where appropriate.
- Set maximum payload sizes on HTTP request bodies and WebSocket messages.
- Validate deserialized data against business rules even after successful parsing.

## Timing Attacks

- Use constant-time operations for comparing secrets (tokens, hashes, MACs).
- Do not short-circuit on the first mismatched byte when comparing authentication tokens.

## Logging

- Never log secrets, tokens, passwords, or API keys. Implement `Debug` and `Display` traits that redact sensitive fields.
- Use structured logging (`tracing` crate) with appropriate levels. Security events get `warn` or `error` level.
- Include request/correlation IDs in logs for traceability without exposing user data.
