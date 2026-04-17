---
name: swift-testing
description: Swift testing conventions using XCTest, Swift Testing, and UI testing
language: swift
paths: ["**/*.swift"]
category: testing
---

# Swift Testing Rules

## Framework Choice

- Use the new Swift Testing framework (`import Testing`, `@Test`) for new test suites (Xcode 16+ / Swift 6).
- Use XCTest (`import XCTest`) for existing test suites and when Swift Testing does not yet support a needed feature (e.g., performance testing).
- Do not mix frameworks within a single test file. A test target can use both, but each file should be consistent.

## Test Organization

- Test files mirror the source structure: `Sources/Auth/LoginViewModel.swift` -> `Tests/AuthTests/LoginViewModelTests.swift`.
- Group tests by feature, not by test type. Integration and unit tests for the same feature live near each other.
- Name test functions descriptively: `testCreateUserWithDuplicateEmailThrowsConflictError`.

## Swift Testing Framework

- Use `@Test` attribute for test functions:
  ```swift
  import Testing

  @Test("Create user with valid email succeeds")
  func createUserWithValidEmail() async throws {
      let service = UserService(repository: MockUserRepo())
      let user = try await service.create(email: "test@example.com", name: "Test")
      #expect(user.email == "test@example.com")
  }
  ```
- Use `#expect` for assertions. It provides rich diagnostics on failure.
- Use `#require` for preconditions that must hold for the test to be meaningful (like optional unwrapping).
- Use `@Test` with `arguments:` for parameterized tests:
  ```swift
  @Test("Email validation", arguments: [
      ("valid@email.com", true),
      ("invalid", false),
      ("", false),
  ])
  func validateEmail(email: String, expected: Bool) {
      #expect(isValidEmail(email) == expected)
  }
  ```
- Use `@Suite` for grouping related tests with shared setup.
- Use traits like `.enabled(if:)`, `.disabled("reason")`, `.tags(.networking)` for test configuration.

## XCTest Conventions

- Test classes inherit from `XCTestCase`. Use `setUp()` and `tearDown()` for common setup.
- Use `XCTAssertEqual`, `XCTAssertTrue`, `XCTAssertNil`, `XCTAssertThrowsError` for assertions.
- Use `expectation(description:)` and `wait(for:timeout:)` for async code in XCTest:
  ```swift
  func testAsyncFetch() {
      let expectation = expectation(description: "Fetch completes")
      sut.fetch { result in
          XCTAssertNotNil(result)
          expectation.fulfill()
      }
      wait(for: [expectation], timeout: 5.0)
  }
  ```
- For async/await in XCTest, use `async` test methods directly (Xcode 14.3+):
  ```swift
  func testAsyncFetch() async throws {
      let result = try await sut.fetch()
      XCTAssertNotNil(result)
  }
  ```

## Mocking and Dependency Injection

- Use protocol-based dependency injection for testability. Every external dependency should be behind a protocol:
  ```swift
  protocol UserRepository {
      func find(byId id: UUID) async throws -> User?
  }

  // Production
  struct APIUserRepository: UserRepository { ... }

  // Test
  struct MockUserRepository: UserRepository {
      var findResult: User?
      func find(byId id: UUID) async throws -> User? { findResult }
  }
  ```
- Use manual mocks (protocol conformances) over mocking frameworks. Swift's type system makes manual mocks safe and fast.
- For complex mocking needs, use `swift-dependencies` or a similar DI framework.
- Inject dependencies through initializers, not through singletons or service locators.

## UI Testing

- Use `XCUITest` for end-to-end UI tests that verify critical user flows.
- Set accessibility identifiers on interactive elements for reliable test targeting:
  ```swift
  Button("Login") { ... }
      .accessibilityIdentifier("loginButton")
  ```
- Keep UI tests focused on critical paths: login, core feature flows, error states. Do not UI-test every screen.
- Use `XCUIApplication().launchArguments` and `launchEnvironment` to configure test state:
  ```swift
  let app = XCUIApplication()
  app.launchArguments = ["--uitesting"]
  app.launchEnvironment = ["API_URL": "http://localhost:8080"]
  app.launch()
  ```
- UI tests must be deterministic. Use mock API responses (local server or bundled JSON) to avoid flakiness.

## Snapshot Testing

- Use `swift-snapshot-testing` for visual regression tests on views.
- Record snapshots on multiple device sizes and appearance modes (light/dark).
- Store snapshot references in the test target, not in the main target.
- Re-record snapshots intentionally when UI changes. Never blindly update failing snapshots.

## Async Testing

- Prefer `async` test functions over callback-based expectations.
- Use `Task.sleep` with short durations only when testing time-dependent behavior. Prefer `Clock` abstraction for testable time control.
- Use `AsyncStream` or `AsyncSequence` testing utilities for testing reactive flows.

## Coverage

- Target 80% code coverage for ViewModels and services.
- Views are not measured by line coverage -- use snapshot tests and UI tests instead.
- Exclude generated code, previews, and app delegates from coverage metrics.
- Configure coverage in the Xcode scheme: select only the modules you want measured.

## Test Data

- Use factory functions for creating test data:
  ```swift
  extension User {
      static func fixture(
          id: UUID = UUID(),
          name: String = "Test User",
          email: String = "test@example.com"
      ) -> User {
          User(id: id, name: name, email: email)
      }
  }
  ```
- Keep test data minimal and relevant to the specific test case.
- Use `Decodable` to load complex test fixtures from JSON files in the test bundle.

## CI Requirements

- All tests (unit + integration) run on every pull request.
- UI tests run on the CI machine's simulator. Pin a specific simulator version.
- Test results are reported as JUnit XML for CI dashboard integration.
- Flaky tests are quarantined immediately with a ticket to investigate.
