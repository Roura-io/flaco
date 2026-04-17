---
name: swift-coding-style
description: Swift and SwiftUI coding conventions with protocol-oriented design and MVVM patterns
language: swift
paths: ["**/*.swift"]
category: coding-style
---

# Swift / SwiftUI Coding Style

## Architecture

- Use MVVM (Model-View-ViewModel) for SwiftUI apps. Views are declarative and stateless. ViewModels own the logic and state.
- ViewModels are `@Observable` classes (Swift 5.9+) or `ObservableObject` with `@Published` properties for older targets.
- Views observe state through `@State`, `@Binding`, `@Environment`, and `@Bindable`. Never put business logic in a View body.
- Models are plain structs that conform to `Codable`, `Identifiable`, `Hashable`, and `Sendable` as needed.
- Use a service/repository layer for data access. ViewModels call services; services call network/persistence layers. Views never call services directly.

## SwiftUI Best Practices

- Keep View bodies short. If a body exceeds 30 lines, extract subviews.
- Use `@ViewBuilder` for conditional view composition. Avoid complex ternary operators in view bodies.
- Prefer computed properties for derived view state instead of storing redundant state.
- Use `.task { }` for async work initiated by view appearance. Do not use `onAppear` with `Task { }` -- `.task` handles cancellation automatically.
- Use `@State` for view-local state, `@Binding` to pass writeable state to children, `@Environment` for app-wide dependencies.
- Avoid `@EnvironmentObject` in new code. Use `@Environment` with custom `EnvironmentKey` or the new `@Observable` + `@Environment` pattern.
- Use `PreviewProvider` / `#Preview` macros for every view. Provide multiple preview configurations (light/dark, different data states, accessibility sizes).

## Naming Conventions

- Follow the Swift API Design Guidelines (swift.org/documentation/api-design-guidelines).
- Types and protocols: `PascalCase` -- `UserProfile`, `Authenticatable`.
- Functions, methods, properties, variables: `camelCase` -- `fetchUser()`, `isAuthenticated`.
- Omit needless words: `removeElement(at: index)` is redundant; prefer `remove(at: index)`.
- Boolean properties read as assertions: `isEmpty`, `isValid`, `canSubmit`.
- Protocol names describe capability: `-able` (`Decodable`, `Sendable`), `-ible` (`Convertible`), or `-ing` (`Loading`).
- Enum cases are `camelCase`: `.loading`, `.loaded(data)`, `.error(message)`.
- Factory methods start with `make`: `func makeRequest() -> URLRequest`.

## Protocol-Oriented Design

- Prefer protocols over class inheritance. Define behavior contracts with protocols; provide default implementations via extensions.
- Use protocol extensions for shared default behavior instead of base classes.
- Use `some Protocol` (opaque types) for return types when the concrete type is an implementation detail.
- Use `any Protocol` (existential types) sparingly -- prefer generics with `some` or concrete types for performance.
- Constrain generics with `where` clauses: `func process<T: Codable & Sendable>(_ item: T)`.

## Concurrency (Swift Concurrency)

- Use `async`/`await` for all asynchronous work. Do not use completion handlers in new code.
- Mark types that are safe to share across concurrency boundaries as `Sendable`. Use `@Sendable` on closures that cross isolation boundaries.
- Use `actor` for mutable state that needs synchronization. Prefer actors over manual locks.
- Use `MainActor` for UI updates: annotate ViewModels with `@MainActor`.
- Use structured concurrency (`async let`, `TaskGroup`) over unstructured `Task { }` where possible.
- Never call `Task.detached` unless you specifically need to escape the current actor's context. Document why.

## Error Handling

- Use `throws` for functions that can fail. Callers use `do/catch` or `try?`/`try!`.
- Never use `try!` in production code. Use `try?` only when the failure case genuinely should be ignored.
- Define custom error types as enums conforming to `Error` and `LocalizedError`:
  ```swift
  enum AuthError: LocalizedError {
      case invalidCredentials
      case sessionExpired
      case networkFailure(underlying: Error)

      var errorDescription: String? {
          switch self {
          case .invalidCredentials: "Invalid email or password"
          case .sessionExpired: "Your session has expired"
          case .networkFailure(let error): "Network error: \(error.localizedDescription)"
          }
      }
  }
  ```
- Use `Result<T, Error>` sparingly -- `async throws` is usually cleaner.

## Code Organization

- One primary type per file. File name matches the type name: `UserProfileView.swift`.
- Use `// MARK: -` comments to organize sections within a file: `// MARK: - Properties`, `// MARK: - Lifecycle`, `// MARK: - Private Methods`.
- Extensions for protocol conformances go in the same file or a dedicated `+ProtocolName.swift` file for large conformances.
- Group files by feature, not by type: `Features/Auth/LoginView.swift`, `Features/Auth/AuthViewModel.swift`, not `Views/LoginView.swift`, `ViewModels/AuthViewModel.swift`.

## Formatting

- Use SwiftFormat or swift-format for consistent formatting.
- Maximum line length: 120 characters.
- Use trailing closures for the last closure parameter. Use labeled closures when there are multiple closure parameters.
- Prefer multi-line formatting for function declarations with 3+ parameters.
- Use guard for early exits: `guard let user = user else { return }`.

## Dependencies

- Use Swift Package Manager for dependency management. Avoid CocoaPods in new projects.
- Pin dependencies to exact versions or minor version ranges in `Package.swift`.
- Audit dependencies for Swift 6 concurrency compatibility (`Sendable` conformance).
