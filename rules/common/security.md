---
name: common-security
description: Language-agnostic security rules for all code
paths: []
category: security
---

# Common Security Rules

## Secrets Management

- Never hardcode secrets, API keys, tokens, passwords, or connection strings in source code. Not even in test files unless they are explicitly fake/test-only values.
- Use environment variables or a secrets manager (Vault, AWS Secrets Manager, 1Password CLI). Access them at runtime, not at build time.
- Rotate secrets on a schedule. If a secret is committed to version control, consider it compromised and rotate immediately.
- `.env` files are local-only. They must be in `.gitignore`. Provide a `.env.example` with placeholder values.

## Input Validation

- Validate all external input at the system boundary: HTTP request parameters, CLI arguments, file contents, environment variables, message queue payloads.
- Validation is allowlist-based, not denylist-based. Define what IS valid, reject everything else.
- Validate type, length, range, and format. A "username" that is 10MB long is not a username.
- Never trust client-side validation alone. The server must re-validate everything.
- Sanitize output based on context: HTML-encode for web pages, parameterize for SQL, escape for shell commands.

## Injection Prevention

- SQL: Always use parameterized queries or an ORM. Never interpolate user input into SQL strings.
- Command injection: Never pass user input to `system()`, `exec()`, `os.popen()`, or equivalent. Use language-native APIs that accept argument arrays instead of shell strings.
- Path traversal: Validate and canonicalize file paths. Reject paths containing `..`, absolute paths when relative is expected, and null bytes.
- Template injection: Use auto-escaping template engines. Disable raw/unescaped rendering unless explicitly required and reviewed.
- LDAP/XML/SSRF: Apply the same principle -- never trust user input in query construction.

## Authentication and Authorization

- Use established libraries and protocols (OAuth 2.0, OIDC, SAML). Do not roll your own auth.
- Hash passwords with bcrypt, scrypt, or Argon2id. Never MD5 or SHA-256 for passwords.
- Enforce rate limiting on authentication endpoints to prevent brute force.
- Implement the principle of least privilege: every component gets the minimum permissions it needs.
- Check authorization on every request, not just at login. Verify the authenticated user has access to the specific resource they are requesting.

## Transport Security

- All network communication uses TLS. No exceptions for "internal" services -- zero-trust means internal traffic is also encrypted.
- Certificate validation must not be disabled in production code. Self-signed certs are acceptable only in dev/test with explicit configuration.
- HTTP Strict Transport Security (HSTS) headers on all web responses.
- API tokens transmitted only in headers, never in URL query parameters (which get logged).

## Data Protection

- Encrypt sensitive data at rest (PII, financial data, health records).
- Log at the appropriate level. Never log secrets, tokens, passwords, or full credit card numbers. Mask or redact sensitive fields.
- Apply data retention policies. Do not store data longer than necessary.
- Implement proper session management: secure cookies (HttpOnly, Secure, SameSite), session expiration, and session invalidation on logout.

## Dependency Security

- Run `npm audit`, `cargo audit`, `pip-audit`, `govulncheck`, or equivalent on every CI build.
- Do not use dependencies with known critical CVEs. Upgrade or find alternatives.
- Pin dependency versions and verify checksums/integrity hashes.
- Review new dependencies before adding them. Check maintainer reputation, last commit date, and open security issues.

## Error Handling for Security

- Never expose stack traces, internal paths, or system details in error responses to users.
- Use generic error messages for authentication failures: "Invalid credentials" not "User not found" or "Wrong password".
- Log detailed error information server-side for debugging; return sanitized messages client-side.

## Code Review Security Checklist

- Are all inputs validated at the boundary?
- Are database queries parameterized?
- Are secrets externalized (not in code)?
- Is authentication and authorization checked on every relevant endpoint?
- Are error messages safe to show to users?
- Does the change introduce new network endpoints? If so, are they authenticated and rate-limited?
- Does the change handle file uploads? If so, are file types, sizes, and paths validated?
