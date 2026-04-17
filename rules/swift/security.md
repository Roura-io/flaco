---
name: swift-security
description: Swift and iOS security rules for keychain, transport security, and secure coding
language: swift
paths: ["**/*.swift"]
category: security
---

# Swift / iOS Security Rules

## Keychain Usage

- Store all sensitive data (tokens, passwords, API keys, certificates) in the Keychain. Never in `UserDefaults`, property lists, or plain files.
- Use a Keychain wrapper library (`KeychainAccess`, `KeychainSwift`) for ergonomic access. Raw Security framework APIs are error-prone.
- Set appropriate Keychain accessibility levels:
  - `.whenUnlockedThisDeviceOnly` for most secrets (not backed up, not synced).
  - `.afterFirstUnlockThisDeviceOnly` for background-accessible tokens.
  - Never use `.always` -- it provides no protection.
- Enable biometric protection (`SecAccessControlCreateFlags.biometryCurrentSet`) for high-value secrets.
- Clear Keychain entries on account logout or app uninstall detection (check a UserDefaults flag on first launch).

## App Transport Security (ATS)

- Never disable ATS globally (`NSAllowsArbitraryLoads = YES`). Apple will reject this during App Review unless you have a valid exception.
- If an exception is needed for a specific domain, use `NSExceptionDomains` with the narrowest scope possible.
- All API communication must use TLS 1.2 or later. Enforce this at the network layer.
- Use certificate pinning for critical API endpoints (banking, auth, health data).

## Certificate Pinning

- Implement certificate pinning for your primary API endpoints using `URLSessionDelegate`:
  ```swift
  func urlSession(
      _ session: URLSession,
      didReceive challenge: URLAuthenticationChallenge,
      completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
  ) {
      guard let serverTrust = challenge.protectionSpace.serverTrust,
            let certificate = SecTrustCopyCertificateChain(serverTrust) as? [SecCertificate],
            let serverCert = certificate.first else {
          completionHandler(.cancelAuthenticationChallenge, nil)
          return
      }

      let serverCertData = SecCertificateCopyData(serverCert) as Data
      if pinnedCertificates.contains(serverCertData) {
          completionHandler(.useCredential, URLCredential(trust: serverTrust))
      } else {
          completionHandler(.cancelAuthenticationChallenge, nil)
      }
  }
  ```
- Pin the leaf certificate or public key, not the root CA. Rotate pins before certificate expiration.
- Include backup pins for certificate rotation without requiring an app update.
- Use `TrustKit` or `Alamofire`'s built-in pinning for production-grade implementations.

## Data Protection

- Enable file protection on sensitive files: `FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUnlessOpen], ofItemAtPath: path)`.
- Use `Data.WritingOptions.completeFileProtection` when writing sensitive data to disk.
- Encrypt local databases (Core Data + SQLCipher, or Realm encryption) for sensitive data.
- Clear sensitive data from memory when no longer needed. Use `Data` over `String` for secrets (Data can be zeroed; String cannot due to copy-on-write).

## Authentication

- Use Sign in with Apple, OAuth 2.0 with PKCE, or platform biometrics (Face ID / Touch ID) for user authentication.
- Never store raw passwords locally. Store only tokens, and refresh them before expiration.
- Implement token refresh logic that handles 401 responses transparently.
- Use `ASWebAuthenticationSession` for OAuth flows. Do not use embedded `WKWebView` for login (it cannot share cookies with Safari and is a phishing vector).

## Jailbreak and Tamper Detection

- For high-security apps (banking, health), implement runtime integrity checks: check for common jailbreak artifacts (`/Applications/Cydia.app`, `/usr/sbin/sshd`, writable system directories).
- Use App Attest (`DeviceCheck` framework) for server-side device integrity verification.
- Do not rely solely on client-side checks -- a jailbroken device can bypass them. Use server-side validation as the primary control.

## Secure Coding Practices

- Avoid `String(describing:)` for sensitive types in logs. Implement `CustomStringConvertible` that redacts sensitive fields.
- Use `SecRandomCopyBytes` or `SystemRandomNumberGenerator` for security-sensitive randomness.
- Validate all deeplink and universal link parameters. Treat URL scheme input as untrusted:
  ```swift
  func handle(url: URL) -> Bool {
      guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
            let action = components.queryItems?.first(where: { $0.name == "action" })?.value,
            allowedActions.contains(action) else {
          return false
      }
      // Process validated action
  }
  ```
- Do not log sensitive user data (PII, health records, financial data). Use `os_log` with `.private` sensitivity for development debugging.
- Sanitize data before displaying in `WKWebView` to prevent XSS.

## Network Security

- Use `URLSession` with ephemeral configuration for requests that should not cache responses or cookies.
- Set request timeouts explicitly: `URLSessionConfiguration.default.timeoutIntervalForRequest = 30`.
- Validate server responses: check status codes, content types, and response sizes before processing.
- Implement retry logic with exponential backoff and jitter for resilience.

## Third-Party SDK Security

- Audit third-party SDKs for data collection practices. Many analytics and advertising SDKs collect excessive data.
- Use Privacy Manifests (iOS 17+) to declare data collection practices.
- Prefer open-source SDKs where you can audit the source code over closed-source binary frameworks.
- Keep SDKs updated. Security vulnerabilities in SDKs affect your app.
