---
name: common-testing
description: Language-agnostic testing conventions and requirements
paths: []
category: testing
---

# Common Testing Rules

## Test Philosophy

- Tests are first-class code. They deserve the same quality standards as production code: clear naming, no duplication, proper structure.
- Test behavior, not implementation. If you refactor the internals and the tests break, the tests were testing the wrong thing.
- Every bug fix starts with a failing test that reproduces the bug. Then fix the code. Then verify the test passes. This prevents regressions permanently.
- Tests must be deterministic. No flaky tests. If a test passes 99% of the time, it is broken -- find and fix the source of non-determinism.

## Test Naming

- Test names describe the scenario and expected outcome: `test_create_user_with_duplicate_email_returns_conflict_error`.
- Avoid generic names like `test_function1` or `test_happy_path`. Be specific enough that a failure message tells you what broke without reading the test body.
- Group related tests in a describe/context block named after the unit under test.

## Test Structure (Arrange-Act-Assert)

- Every test has three phases, clearly separated:
  1. **Arrange**: Set up test data, mocks, and preconditions.
  2. **Act**: Execute the code under test. One action per test.
  3. **Assert**: Verify the outcome. One logical assertion per test (multiple `assert` calls are fine if they verify one concept).
- Tests must be independent. No test should depend on the outcome or side effects of another test. Shared mutable state between tests is a defect.

## Unit Tests

- Unit tests cover a single function or method in isolation.
- External dependencies (database, network, file system, clock) are mocked or stubbed. Unit tests must run without infrastructure.
- Target: 80% line coverage minimum for general code, 100% for critical business logic, financial calculations, and security-sensitive code.
- Unit tests must run fast: the entire unit test suite should complete in under 30 seconds for a typical project.

## Integration Tests

- Integration tests verify that components work together correctly: service + database, API + auth middleware, etc.
- Use real dependencies when feasible (testcontainers, in-memory databases). Mock only what is impractical to run locally.
- Integration tests run in CI on every push but may be skipped locally with a flag for speed.
- Test the critical paths: the happy path through the full stack, error propagation across boundaries, and authentication/authorization flows.

## Test Data

- Use factories or builders to create test data. Never rely on a shared seed database that can drift.
- Test data should be minimal: only the fields relevant to the test scenario. Use sensible defaults for everything else.
- Avoid hardcoded IDs, timestamps, or values that couple tests to a specific environment or order.

## Mocking Guidelines

- Mock at the boundary, not in the middle. Mock the database client, not the internal service that calls the database client.
- Verify that mocks are called with expected arguments when the interaction itself is the behavior under test.
- Do not over-mock. If you mock everything, you are testing your mocks, not your code. Prefer integration tests for complex interactions.
- Reset mocks between tests. Leaked mock state is a common source of flaky tests.

## Coverage Requirements

| Code Type | Minimum Coverage |
|-----------|-----------------|
| Critical business logic | 100% |
| Public API surface | 90% |
| General application code | 80% |
| Generated code / FFI | Exclude from coverage |
| Test utilities | Not measured |

- Coverage is a floor, not a ceiling. 80% coverage with meaningful tests beats 95% coverage with trivial assertions.
- Measure branch coverage in addition to line coverage. A function with an if/else that only tests the true branch is 50% tested regardless of line coverage.

## CI Requirements

- All tests run on every pull request. No merging with failing tests.
- Test runs must be reproducible: same commit, same result, every time.
- Flaky tests are quarantined immediately (marked as skip/pending with a ticket to fix) and never ignored.
- Test timeouts are set explicitly. A test that hangs indefinitely blocks the entire pipeline.

## What NOT to Test

- Framework internals (the ORM already tests its own query builder).
- Trivial getters/setters with no logic.
- Third-party library behavior (test your integration with the library, not the library itself).
- Implementation details that may change during refactoring.

## Test Maintenance

- Delete obsolete tests when the feature they cover is removed. Dead tests add noise and slow the suite.
- When a test starts failing after a refactor, ask: is the test wrong, or is the new code wrong? Do not blindly update assertions to make the test pass.
- Review test code in PRs with the same rigor as production code.
