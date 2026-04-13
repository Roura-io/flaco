# flacoAi Training Rubric — Chris's Swift 6 / SwiftUI / POP Architecture

> This is the **ground truth** the LoRA is being trained against. Every
> synthetic training pair is generated to conform to this rubric, and
> every eval case is scored against it.
>
> **Chris: this is gate 1. Read it, mark anything I got wrong, and
> reply with corrections before I burn ~5 hours of local Ollama time
> synthesizing 25k training pairs off of it.**

---

## Source of truth

Extracted from:
- Your RIOExperimentationKit package (`~/Documents/dev/packages/RIOExperimentationKit`)
- Your description of the MVVM + DI + Environment pattern (hackathon 3, q4)
- Your working preferences captured in flaco memory

If the rubric disagrees with your actual taste, the rubric is wrong — tell me.

---

## R1 — Protocol-oriented by default

Every capability starts as a small, single-purpose protocol. Concrete
types conform. Composition happens via protocol intersection exposed as
a typealias. Example from RIOKit:

```swift
public protocol FlagEvaluating: Sendable { ... }
public protocol FlagObserving: Sendable { ... }
public protocol ContextConfiguring: Sendable { ... }

/// Composite protocol combining all experimentation client capabilities.
public typealias ExperimentationClientProviding =
    FlagEvaluating & FlagObserving & ContextConfiguring
```

**Rules:**
- One responsibility per protocol. `FlagEvaluating` doesn't observe; `FlagObserving` doesn't evaluate.
- Every protocol is `Sendable` unless there is a concrete reason not to be.
- When multiple protocols naturally compose into one "client" or "provider"
  concept, that composite lives as a `typealias` — **not** a new super-protocol.
- Callers depend on the typealias (or the narrowest protocol they need).
  Never depend on a concrete type across a module boundary.

---

## R2 — Manager protocol + concrete + factory

A "manager" is the high-level facade consumed by views and view models.
Pattern:

```swift
// 1. Facade protocol — refines Observable so SwiftUI re-renders on changes
public protocol ExperimentationManaging: Observable {
    func initialize(flags: [any ExperimentFlag]) async throws
    // ...
}

// 2. Concrete — @Observable final class, takes composite client via init
@Observable
public final class ExperimentationManager: @unchecked Sendable {

    // MARK: - Private Properties

    private let client: ExperimentationClientProviding
    private var activeValues: [String: FlagValue] = [:]
    public private(set) var isInitialized = false

    // MARK: - Initialization

    public init(client: ExperimentationClientProviding) {
        self.client = client
    }
}

// 3. Conformance via extension — main class body stays focused
extension ExperimentationManager: ExperimentationManaging {
    public func initialize(flags: [any ExperimentFlag]) async throws { ... }
}

// 4. Live and mock factories
extension ExperimentationManager {
    public static func live(mobileKey: String) -> ExperimentationManager {
        ExperimentationManager(client: LaunchDarklyClient(mobileKey: mobileKey))
    }

    public static var mock: ExperimentationManager {
        ExperimentationManager(client: MockExperimentationClient())
    }
}

// 5. Private helpers in a private extension
private extension ExperimentationManager {
    func resolveValue(forKey key: String, policy: UpdatePolicy) -> FlagValue? { ... }
}
```

**Rules:**
- Facade protocol refines `Observable` if it is SwiftUI-consumed.
- Concrete is `@Observable final class`, marked `@unchecked Sendable`
  when it needs mutable state; protect invariants with care.
- Dependencies come through `init`. No property injection. No singletons.
- Protocol conformance lives in an `extension`, not the main declaration.
- `.live(...)` and `.mock` static factories go on the concrete type.
  Never expose `init(client:)` as the preferred construction path for
  callers — they call `.live` / `.mock`.
- Private helpers live in a `private extension` block at the bottom of
  the file, not interleaved with public API.

---

## R3 — Actors for real I/O, `@unchecked Sendable` for managed state

The boundary with real system SDKs is an `actor`. Example:

```swift
public actor LaunchDarklyClient {
    private let mobileKey: String
    private var isInitialized = false
    nonisolated public let flagUpdateStream: AsyncStream<FlagUpdate>

    public init(mobileKey: String) { ... }
}

extension LaunchDarklyClient: FlagEvaluating { ... }
extension LaunchDarklyClient: ContextConfiguring { ... }
```

**Rules:**
- I/O-heavy clients that wrap third-party SDKs are actors.
- Streams that must be observable from outside actor isolation are
  declared `nonisolated public let ... : AsyncStream<...>`.
- Facade managers (used by views) are not actors — they are
  `@Observable` classes so SwiftUI observation works.
- `@unchecked Sendable` is allowed on manager classes when their own
  concurrency discipline guarantees safety; comment why if non-obvious.

---

## R4 — `@Entry` macro + DI hybrid for environment wiring

This is the pattern Chris is most opinionated about. Every
environment-exposed dependency looks like this:

### Step 1 — EnvironmentValues extension with `@Entry`

```swift
// Environment/EnvironmentValues+ExperimentationManager.swift
import SwiftUI

public extension EnvironmentValues {
    @Entry var experimentationManager: ExperimentationManager = {
        ExperimentationManager.mock
    }()
}
```

- File lives under `Environment/` subdirectory.
- One file per environment key, named `EnvironmentValues+TypeName.swift`.
- Default value is the `.mock` factory — so previews work for free.

### Step 2 — App.swift injects BOTH ways

```swift
@main
struct LuminaeApp: App {
    let experimentationManager = ExperimentationManager.live(mobileKey: "...")

    var body: some Scene {
        WindowGroup {
            RootView(experimentationManager: experimentationManager)
                .environment(\.experimentationManager, experimentationManager)
        }
    }
}
```

- Live instance is constructed once, at app launch.
- It is passed to the root view's `init` via normal DI.
- It is **also** set into the environment via `.environment(\.keypath, instance)`.
- The double wiring is intentional and non-negotiable.

### Step 3 — Views with a ViewModel take the dep through `init`

```swift
struct RootView: View {
    let experimentationManager: ExperimentationManager
    @State private var viewModel: RootViewModel

    init(experimentationManager: ExperimentationManager) {
        self.experimentationManager = experimentationManager
        _viewModel = State(
            wrappedValue: RootViewModel(
                experimentationManager: experimentationManager
            )
        )
    }

    var body: some View { ... }
}
```

- The ViewModel is constructed **inside the view's `init`** using the
  injected dependency, via `_viewModel = State(wrappedValue: ...)`.
- **Reason:** environment values are unavailable until after `init`, so
  a VM that needs the dep cannot be initialized from the environment —
  it must come through `init`.
- The view also holds a stored `let experimentationManager` so subviews
  that need it can receive it through their own `init`.

### Step 4 — Views WITHOUT a ViewModel read straight from environment

```swift
struct StatusBadgeView: View {
    @Environment(\.experimentationManager) private var experimentationManager

    var body: some View {
        if experimentationManager.value(for: newBadgeFlag) {
            Text("New")
        }
    }
}
```

- Pure leaf views with no need for a VM skip the DI dance and use the
  environment directly.
- This is the **one** place environment reading is the default.

**Summary of R4:**
| View has a ViewModel? | How to get the dep |
|---|---|
| Yes | Through `init`, then used to build the VM inside `init` via `_vm = State(wrappedValue:)` |
| No | `@Environment(\.keypath)` directly |

The environment value acts as a **fallback** for VM-less views and a
**free preview mock**, not as the primary DI channel for VM-backed views.

---

## R5 — Mock types mirror the protocol exactly

```swift
public final class MockExperimentationClient: @unchecked Sendable {

    // MARK: - Configurable Values
    public var boolValues: [String: Bool] = [:]
    public var stringValues: [String: String] = [:]

    // MARK: - State
    public private(set) var initializeCalled = false
    public private(set) var lastUserState: ExperimentationUserState?

    // MARK: - Flag Update Stream
    private var flagUpdateContinuation: AsyncStream<FlagUpdate>.Continuation?
    public let flagUpdateStream: AsyncStream<FlagUpdate>

    public init() {
        var continuation: AsyncStream<FlagUpdate>.Continuation?
        self.flagUpdateStream = AsyncStream { continuation = $0 }
        self.flagUpdateContinuation = continuation
    }

    // MARK: - Test Helpers
    public func simulateFlagUpdate(key: String) {
        flagUpdateContinuation?.yield(FlagUpdate(key: key))
    }
}

extension MockExperimentationClient: FlagEvaluating { ... }
extension MockExperimentationClient: FlagObserving {}
extension MockExperimentationClient: ContextConfiguring { ... }
```

**Rules:**
- Mock is a `final class`, `@unchecked Sendable`.
- Configurable inputs are public mutable dictionaries keyed by the same
  key the real client uses.
- Tracked state (`initializeCalled`, `lastUserState`, etc.) is
  `public private(set)` — tests read it, tests can't write it directly.
- Test helpers (`simulateFlagUpdate`) are separate methods, not part of
  the protocol conformance.
- Protocol conformances split into `extension` blocks with MARK comments.

---

## R6 — File organization

```
Sources/
  PackageName/
    PackageNameMain.swift          # @Observable final class (the manager)
    PackageNameManaging.swift      # the facade protocol
    Providing.swift                # composite typealias
    FeatureEvaluating.swift        # single-responsibility protocol 1
    FeatureObserving.swift         # single-responsibility protocol 2
    MockPackageNameClient.swift    # mock implementation
    LivePackageNameClient.swift    # actor-backed real implementation
    Environment/
      EnvironmentValues+PackageName.swift
    Models/
      ModelType1.swift
      ModelType2.swift
      PackageNameError.swift
```

**Rules:**
- One type per file.
- `Models/` for plain data, errors, enums.
- `Environment/` for `EnvironmentValues` extensions.
- No catch-all `Utils.swift` or `Helpers.swift`.

---

## R7 — File header, MARKs, and code style

Every file opens with:

```swift
//
//  FileName.swift
//  PackageName
//
//  Created by Christopher J Roura on MM/DD/YY.
//
```

Every type uses MARK sectioning in this order:

```swift
// MARK: - Private Properties
// MARK: - Public Properties
// MARK: - Initialization
// MARK: - ProtocolName    (one block per conformance, in an extension)
// MARK: - Factory          (if the type has static factories)
```

Private helpers live at the bottom in:

```swift
// MARK: - Private Methods

private extension TypeName {
    func helper() { ... }
}
```

---

## R8 — Strict concurrency + Swift 6 defaults

- `Package.swift` has `swiftLanguageModes: [.v6]`.
- Every protocol is `Sendable` unless there's a real reason.
- Actor types for I/O boundaries.
- `@Observable final class` for SwiftUI-consumed managers, marked
  `@unchecked Sendable` when state is managed.
- Async APIs throughout; no completion handlers in new code.
- Use `async throws` where failure is a real outcome, not `Result` types.
- `AsyncStream<T>` for event streams that cross isolation boundaries.
- `nonisolated` on actor members that are safe for cross-context access
  (like a preconfigured `AsyncStream`).

---

## R9 — Error types are dedicated enums

```swift
public enum ExperimentationError: Error {
    case clientNotInitialized
    case initializationFailed(reason: String)
    case timeout
}
```

- One error enum per module, lives in `Models/`.
- Cases carry context (`reason: String`) when the underlying cause varies.
- Thrown via `throw ExperimentationError.case`, never as raw strings.

---

## R10 — What NOT to do (anti-patterns the model must actively avoid)

The LoRA needs "wrong way → right way" training pairs to unlearn the
defaults. Here are the anti-patterns a generic LLM produces by default
that Chris does **not** want:

| Anti-pattern | What Chris wants instead |
|---|---|
| `@EnvironmentObject` | `@Environment(\.keypath)` with an `@Entry` extension |
| `@StateObject private var vm = ViewModel()` with no DI | VM constructed inside `view.init` with `_vm = State(wrappedValue: ViewModel(dep: dep))` |
| `ObservableObject` + `@Published` | `@Observable final class` |
| Class-based singleton (`static let shared`) | `.live` / `.mock` static factory on the concrete type |
| Dependency looked up in `.onAppear` or `.task` | Dependency passed through `init` |
| Concrete type as an init parameter | A `Providing` protocol / composite typealias as the init parameter |
| Helpers in the main class body | Helpers in a `private extension` at the bottom of the file |
| Protocol conformance in main declaration line | Protocol conformance in an `extension` |
| `Result<T, Error>` for async APIs | `async throws` |
| Completion-handler APIs | `async` / `AsyncStream` |
| No `Sendable` annotation on protocols | Every protocol is `Sendable` unless justified otherwise |
| `@MainActor class Manager` when the class is used from multiple actors | `@Observable final class ... @unchecked Sendable` with disciplined state |
| Stored property initializer for a VM that depends on env | VM built inside `view.init`, taking env value that was passed through init |

---

## R11 — VIPER variant (Chris is migrating toward this)

When the module is VIPER instead of MVVM:

- `View` is a SwiftUI view that takes its `Presenter` through `init`.
- `Presenter` is an `@Observable final class`, takes `Interactor` and
  `Router` via `init`.
- `Interactor` is the service-layer boundary, depends on composite
  `...Providing` protocols.
- `Router` is an `@Observable final class` stored in the environment
  so subviews that need to push can read it via `@Environment(\.router)`.
- Dependencies still follow R4: init-injection for the Presenter's
  deps, environment for the Router (because subviews without a
  Presenter still need to navigate).

The rest of the rubric (R1-R10) applies identically in VIPER modules.

---

## Review checklist for Chris

- [ ] R1 — POP + composite typealias is accurate
- [ ] R2 — manager / facade / factory pattern is accurate
- [ ] R3 — actors-for-I/O rule is correct
- [ ] R4 — MVVM environment + DI hybrid is captured exactly as I use it
- [ ] R5 — mock shape is accurate
- [ ] R6 — file organization matches my preference
- [ ] R7 — file header and MARK order is accurate
- [ ] R8 — strict concurrency defaults are right
- [ ] R9 — error-enum pattern is accurate
- [ ] R10 — anti-pattern list is the right things to train away from
- [ ] R11 — VIPER variant captures what I'm moving toward

Reply with any corrections or "approved" and I will freeze this rubric
and use it to synthesize the training dataset.
