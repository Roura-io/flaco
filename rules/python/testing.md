---
name: python-testing
description: Python testing conventions using pytest, fixtures, and coverage requirements
language: python
paths: ["**/*.py"]
category: testing
---

# Python Testing Rules

## Framework and Setup

- Use `pytest` as the test framework. Do not use `unittest.TestCase` unless migrating legacy code.
- Configure pytest in `pyproject.toml`:
  ```toml
  [tool.pytest.ini_options]
  testpaths = ["tests"]
  addopts = "-v --tb=short --strict-markers"
  markers = ["slow: marks tests as slow", "integration: integration tests"]
  ```
- Test files follow the pattern `test_<module>.py` or `<module>_test.py` in a `tests/` directory that mirrors the source structure.
- Test functions start with `test_`: `def test_create_user_with_valid_email():`.

## Test Structure

- Follow Arrange-Act-Assert with clear separation:
  ```python
  def test_calculate_discount_for_premium_user():
      # Arrange
      user = User(tier="premium", total_purchases=5000)
      order = Order(subtotal=100.0)

      # Act
      discount = calculate_discount(user, order)

      # Assert
      assert discount == 15.0
  ```
- One logical assertion per test. Multiple `assert` statements are fine if they verify one concept.
- Tests must be independent. No test should depend on another test's side effects.

## Fixtures

- Use `pytest` fixtures for shared setup instead of `setUp`/`tearDown` methods.
- Fixtures should have the narrowest possible scope: `function` (default) > `class` > `module` > `session`.
- Use `yield` fixtures for setup + teardown:
  ```python
  @pytest.fixture
  def db_session():
      session = create_test_session()
      yield session
      session.rollback()
      session.close()
  ```
- Keep fixtures in `conftest.py` files. Use the closest `conftest.py` to where the fixture is used.
- Name fixtures after what they provide, not what they do: `db_session` not `setup_database`.
- Use `@pytest.fixture(autouse=True)` sparingly. Explicit is better than implicit.

## Parametrize

- Use `@pytest.mark.parametrize` for testing multiple input/output combinations:
  ```python
  @pytest.mark.parametrize("email,expected", [
      ("user@example.com", True),
      ("invalid", False),
      ("", False),
      ("user@.com", False),
      ("user+tag@example.com", True),
  ])
  def test_validate_email(email: str, expected: bool):
      assert validate_email(email) == expected
  ```
- When parameter lists get long (>5 cases), use `pytest.param` with `id` for readable test IDs:
  ```python
  @pytest.mark.parametrize("input,expected", [
      pytest.param("valid@email.com", True, id="simple-valid"),
      pytest.param("no-at-sign", False, id="missing-at"),
  ])
  ```

## Mocking

- Use `unittest.mock.patch` or `pytest-mock` (which provides a `mocker` fixture).
- Mock at the boundary where the dependency is used, not where it is defined:
  ```python
  # Module: myapp/service.py imports requests
  # Mock where it's used, not in the requests library
  @patch("myapp.service.requests.get")
  def test_fetch_data(mock_get):
      mock_get.return_value.json.return_value = {"key": "value"}
      result = fetch_data("http://api.example.com")
      assert result == {"key": "value"}
  ```
- Use `spec=True` on mocks to catch attribute typos: `Mock(spec=DatabaseClient)`.
- Do not over-mock. If the mock setup is longer than the test logic, consider an integration test instead.
- Use `freezegun` or `time-machine` for time-dependent tests. Never mock `datetime` manually.

## Async Testing

- Use `pytest-asyncio` for async test functions:
  ```python
  import pytest

  @pytest.mark.asyncio
  async def test_async_fetch():
      result = await fetch_data()
      assert result is not None
  ```
- Use `aioresponses` for mocking `aiohttp` requests, `respx` for mocking `httpx`.

## Coverage

- Run coverage with `pytest --cov=src --cov-report=term-missing --cov-fail-under=80`.
- Coverage targets:
  | Code Type | Minimum |
  |-----------|---------|
  | Business logic | 100% |
  | API handlers | 90% |
  | General code | 80% |
  | Generated/migrations | Exclude |
- Configure coverage exclusions in `pyproject.toml`:
  ```toml
  [tool.coverage.run]
  omit = ["*/migrations/*", "*/tests/*", "*/__main__.py"]

  [tool.coverage.report]
  exclude_lines = [
      "pragma: no cover",
      "if TYPE_CHECKING:",
      "if __name__ == .__main__.",
  ]
  ```

## Test Categories

- **Unit tests** (`tests/unit/`): Fast, isolated, mock external dependencies. Run on every commit.
- **Integration tests** (`tests/integration/`): Use real databases/services (via testcontainers or docker-compose). Run in CI.
- **End-to-end tests** (`tests/e2e/`): Test the full application stack. Run before releases.
- Mark slow tests: `@pytest.mark.slow`. Run all tests in CI, but developers can skip slow tests locally with `-m "not slow"`.

## Error Testing

- Test that functions raise expected exceptions:
  ```python
  def test_divide_by_zero_raises():
      with pytest.raises(ZeroDivisionError, match="division by zero"):
          divide(10, 0)
  ```
- Test error messages, not just error types. The message is part of the contract.
- Test boundary conditions: empty input, None, maximum sizes, concurrent access.

## Test Data

- Use `factory_boy` or `polyfactory` for complex test data generation.
- Keep test data close to the test. Avoid shared fixtures that are modified across tests.
- Use `faker` for realistic but deterministic test data (seed the generator).

## CI Requirements

- All tests run on every pull request.
- Coverage report is generated and checked against thresholds.
- Type checking (`mypy --strict`) runs alongside tests.
- Linting (`ruff check .`) runs alongside tests.
