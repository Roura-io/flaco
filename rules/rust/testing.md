---
name: rust-testing
description: Rust testing conventions using cargo test, property-based testing, and mocking
language: rust
paths: ["**/*.rs"]
category: testing
---

# Rust Testing Rules

## Test Organization

- Unit tests live in a `#[cfg(test)] mod tests` block at the bottom of the source file they test. This gives them access to private items.
- Integration tests live in `tests/` at the crate root. Each file in `tests/` is compiled as a separate crate and can only access the public API.
- Test helpers shared across integration tests go in `tests/common/mod.rs` (not `tests/common.rs`, which would be treated as a test itself).
- Benchmark tests use `criterion` in `benches/` with `[[bench]]` entries in `Cargo.toml`.

## Test Naming

- Test function names describe the scenario: `fn rejects_empty_email()` not `fn test1()`.
- Use `#[test]` for synchronous tests, `#[tokio::test]` for async tests.
- Group related tests with descriptive module names: `mod parse_config { ... }`.

## Writing Tests

- Follow Arrange-Act-Assert:
  ```rust
  #[test]
  fn parses_valid_config() {
      // Arrange
      let input = r#"port = 8080"#;

      // Act
      let config = parse_config(input).unwrap();

      // Assert
      assert_eq!(config.port, 8080);
  }
  ```
- Prefer `assert_eq!` and `assert_ne!` over `assert!` for better error messages on failure.
- Use `assert!(matches!(result, Pattern))` for enum matching. On nightly or with a helper, consider `assert_matches!`.
- Return `Result<(), E>` from tests that use `?` instead of `.unwrap()` everywhere. This produces cleaner test code.
- Never use `#[should_panic]` when `Result::is_err()` works. `#[should_panic]` cannot verify the error type.

## Property-Based Testing

- Use `proptest` for functions that should hold invariants across a range of inputs: roundtrip encode/decode, sorting invariants, mathematical properties.
- Define custom strategies for domain types: `prop_compose!` or `Arbitrary` implementations.
- Set `PROPTEST_CASES=1000` in CI for thorough coverage. Use fewer cases locally for speed.
  ```rust
  use proptest::prelude::*;

  proptest! {
      #[test]
      fn roundtrip(input in ".*") {
          let encoded = encode(&input);
          let decoded = decode(&encoded).unwrap();
          prop_assert_eq!(input, decoded);
      }
  }
  ```

## Parameterized Tests with rstest

- Use `rstest` for table-driven tests with multiple input/output combinations:
  ```rust
  use rstest::rstest;

  #[rstest]
  #[case("hello", 5)]
  #[case("", 0)]
  #[case("rust", 4)]
  fn test_len(#[case] input: &str, #[case] expected: usize) {
      assert_eq!(input.len(), expected);
  }
  ```
- Use `#[fixture]` for shared test setup that needs to be reusable across test functions.

## Mocking

- Use `mockall` for trait-based mocking. Define traits for external boundaries (database, HTTP client, clock).
- Mock at the boundary: mock the `trait DatabaseClient`, not the internal service that calls it.
- Prefer real implementations in integration tests. Use mocks only in unit tests for isolation.
- Reset expectations between tests. Each test should set up its own mock expectations.
  ```rust
  #[automock]
  trait UserRepo {
      fn find_by_id(&self, id: u64) -> Option<User>;
  }

  #[test]
  fn returns_none_for_missing_user() {
      let mut mock = MockUserRepo::new();
      mock.expect_find_by_id()
          .with(eq(42))
          .returning(|_| None);

      let service = UserService::new(mock);
      assert!(service.get_user(42).is_none());
  }
  ```

## Async Testing

- Use `#[tokio::test]` with `tokio::test` macro for async tests.
- Use `tokio::time::pause()` to control time in tests instead of real sleeps.
- Use `tokio::sync::Notify` or `tokio::sync::oneshot` for synchronization between tasks in tests, never `std::thread::sleep`.

## Coverage

- Run `cargo llvm-cov` for coverage reports. Target 80% minimum for general code.
- Use `cargo llvm-cov --html` for visual coverage reports during development.
- Exclude generated code, FFI bindings, and unreachable `panic!` branches from coverage with `#[cfg(not(tarpaulin_include))]` or equivalent.

## Test Commands

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output (see println! in tests)
cargo test -- --nocapture

# Run without stopping on first failure
cargo test --no-fail-fast

# Run only unit tests (skip integration)
cargo test --lib

# Run only integration tests
cargo test --test integration_test_name

# Coverage
cargo llvm-cov --fail-under-lines 80
```

## What to Test

- All public API functions and methods.
- Error paths and edge cases: empty inputs, maximum sizes, invalid formats.
- Conversion functions: verify roundtrip fidelity.
- State machines: test every transition and invalid transition.
- Concurrent code: test with multiple threads/tasks and verify no data races (use `--release` and `loom` for exhaustive testing).

## What NOT to Test

- Derived trait implementations (`Debug`, `Clone`, `Serialize`).
- Private helper functions directly (test them through the public API).
- The standard library or third-party crate internals.
- Simple struct construction with no logic.
