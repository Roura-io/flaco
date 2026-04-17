---
name: python-security
description: Python-specific security rules for eval, pickle, subprocess, and injection prevention
language: python
paths: ["**/*.py"]
category: security
---

# Python Security Rules

## Dangerous Functions

- **Never** use `eval()`, `exec()`, or `compile()` with user input. There is no safe way to sandbox these functions in CPython.
- **Never** use `pickle.loads()` or `pickle.load()` on untrusted data. Pickle can execute arbitrary code during deserialization. Use JSON, MessagePack, or Protocol Buffers for external data.
- **Never** use `yaml.load()` without `Loader=yaml.SafeLoader`. The default loader executes arbitrary Python code. Always: `yaml.safe_load(data)`.
- **Never** use `os.system()`, `os.popen()`, or `subprocess.call(shell=True)` with user input. These invoke a shell and are vulnerable to command injection.
- Avoid `__import__()` and `importlib.import_module()` with user-controlled module names.

## Subprocess Safety

- Use `subprocess.run()` with a list of arguments, never a shell string:
  ```python
  # DANGEROUS - command injection
  subprocess.run(f"git clone {url}", shell=True)

  # SAFE - no shell interpretation
  subprocess.run(["git", "clone", url], check=True)
  ```
- Always set `check=True` to raise on non-zero exit codes, or explicitly handle the return code.
- Set `timeout` on subprocess calls to prevent hanging processes.
- Capture and validate output with `capture_output=True` when processing subprocess results.

## SQL Injection Prevention

- Use parameterized queries with your ORM or database driver. Never use f-strings or `%` formatting for SQL:
  ```python
  # DANGEROUS
  cursor.execute(f"SELECT * FROM users WHERE id = {user_id}")

  # SAFE - parameterized
  cursor.execute("SELECT * FROM users WHERE id = %s", (user_id,))

  # SAFE - SQLAlchemy
  session.query(User).filter(User.id == user_id).first()
  ```
- For dynamic query construction (variable columns, table names), use an allowlist of valid identifiers. Never interpolate user input into SQL identifiers.

## Path Traversal

- Validate and resolve file paths before accessing them:
  ```python
  from pathlib import Path

  base = Path("/allowed/directory").resolve()
  target = (base / user_input).resolve()
  if not target.is_relative_to(base):
      raise SecurityError("Path traversal attempt")
  ```
- Never use `os.path.join()` with unsanitized user input. An absolute path in the second argument replaces the first.
- Reject file names containing `..`, null bytes, or OS-specific separators from user input.

## Template Injection

- Use Jinja2 with `autoescape=True` (the default in modern Flask/FastAPI). Never use `|safe` on user-supplied content.
- When rendering user content in non-HTML contexts (JSON, email, markdown), use context-appropriate escaping.
- Never pass user input as a template string: `Template(user_input).render()` is a server-side template injection vulnerability.

## Secrets and Credentials

- Store secrets in environment variables or a secrets manager. Never in source code, config files committed to git, or default arguments.
- Use `secrets` module for generating tokens: `secrets.token_urlsafe(32)`. Never `random` module for security-sensitive randomness.
- Use `hmac.compare_digest()` for constant-time comparison of secrets and tokens. Never `==`.

## Dependency Security

- Run `pip-audit` or `safety check` in CI. Block merges on known critical vulnerabilities.
- Pin all dependencies in `requirements.txt` or `pyproject.toml` with exact versions for deployable applications.
- Use a lockfile (`pip-compile`, `poetry.lock`, `pdm.lock`) to ensure reproducible builds.
- Audit new dependencies before adding. Check PyPI download stats, GitHub activity, and known CVEs.

## HTTP Security

- Validate Content-Type headers on incoming requests. Do not blindly parse request bodies.
- Set appropriate timeouts on HTTP clients (`httpx.Client(timeout=30.0)`).
- Verify TLS certificates. Never set `verify=False` in production.
- Use CSRF protection on state-changing endpoints in web frameworks.
- Set security headers: `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Content-Security-Policy`.

## Serialization

- For external data, prefer JSON (`json.loads`) or Protocol Buffers. Both are safe to deserialize.
- If you must use YAML, always `yaml.safe_load()`. Never `yaml.load()` without a safe loader.
- Validate deserialized data with Pydantic models or `marshmallow` schemas before using it in business logic.
- Set maximum payload sizes on API endpoints to prevent memory exhaustion.

## Logging Security

- Never log passwords, tokens, API keys, or PII. Redact sensitive fields before logging.
- Use structured logging (`structlog`, `python-json-logger`) with appropriate levels.
- Log authentication events (login, logout, failed attempts) at `WARNING` or `INFO` level for audit trails.
- Sanitize user input in log messages to prevent log injection (newlines, control characters).

## Code Review Security Checklist

- Is user input validated and sanitized at every entry point?
- Are all database queries parameterized?
- Are subprocess calls using argument lists, not shell strings?
- Are file path operations validated against traversal attacks?
- Are secrets stored externally, not in code?
- Are dangerous functions (`eval`, `pickle`, `yaml.load`) absent or mitigated?
- Are new HTTP endpoints authenticated and rate-limited?
