---
name: python-coding-style
description: Python coding conventions following PEP 8 with type hints and modern idioms
language: python
paths: ["**/*.py"]
category: coding-style
---

# Python Coding Style

## PEP 8 Compliance

- Follow PEP 8 as the baseline. Use `ruff` or `black` for automatic formatting. No manual formatting debates.
- Maximum line length: 88 characters (black default). Use implicit line continuation inside parentheses, brackets, and braces rather than backslash continuation.
- Imports are grouped in this order, separated by blank lines: (1) standard library, (2) third-party, (3) local. Use `isort` to enforce this.
- Use absolute imports: `from mypackage.module import function`, not relative imports unless inside a package where relative is clearer.
- Two blank lines before and after top-level definitions (functions, classes). One blank line between methods in a class.

## Naming Conventions

- `snake_case` for functions, methods, variables, and modules.
- `PascalCase` for classes and type aliases.
- `SCREAMING_SNAKE_CASE` for module-level constants.
- Private names start with a single underscore: `_internal_helper`. Double underscore (`__name_mangled`) is almost never what you want.
- Avoid single-character variable names except for loop counters (`i`, `j`), coordinates (`x`, `y`), and lambda parameters where context is obvious.

## Type Hints

- All public functions and methods must have complete type annotations: parameters and return type.
- Use modern type syntax (Python 3.10+): `str | None` instead of `Optional[str]`, `list[int]` instead of `List[int]`.
- For older Python support, import from `__future__` annotations: `from __future__ import annotations`.
- Use `TypeAlias` for complex type definitions: `UserId: TypeAlias = int`.
- Use `Protocol` for structural subtyping instead of abstract base classes where possible.
- Run `mypy --strict` or `pyright` in CI. Type errors are build failures.

## Docstrings

- Every public module, class, function, and method gets a docstring.
- Use Google-style docstrings:
  ```python
  def fetch_user(user_id: int) -> User | None:
      """Fetch a user by ID from the database.

      Args:
          user_id: The unique identifier of the user.

      Returns:
          The User object if found, None otherwise.

      Raises:
          DatabaseError: If the database connection fails.
      """
  ```
- The first line is a single-sentence summary in imperative mood. Additional detail follows after a blank line.
- Do not docstring trivial or self-evident functions (`__init__` with obvious parameters, simple property accessors).

## Pythonic Idioms

- Use list/dict/set comprehensions instead of `map`/`filter` with lambdas: `[x.name for x in users if x.active]`.
- Use f-strings for string formatting: `f"Hello, {name}"`. Never `"Hello, " + name` or `"Hello, %s" % name`.
- Use context managers (`with`) for all resource management: files, database connections, locks.
- Use `pathlib.Path` instead of `os.path` for file system operations.
- Use `enumerate()` instead of `range(len(...))`.
- Use `collections.defaultdict`, `collections.Counter`, and `itertools` instead of reinventing them.
- Use `dataclasses` or `pydantic.BaseModel` for data containers instead of plain dicts or tuples.

## Error Handling

- Never use bare `except:`. Always catch specific exceptions: `except ValueError:` or `except (IOError, OSError):`.
- Do not catch `Exception` unless you are at a top-level error boundary (e.g., a request handler). Even then, log the exception.
- Use custom exception classes for domain errors. Inherit from a project-wide base exception.
- Prefer EAFP (Easier to Ask Forgiveness than Permission) over LBYL (Look Before You Leap) when the check and the operation would race:
  ```python
  # Good (EAFP)
  try:
      value = cache[key]
  except KeyError:
      value = compute(key)
  ```

## Function Design

- Functions should do one thing. If the name has "and" in it, split it.
- Maximum function length: 30 lines of logic. Extract helpers for longer functions.
- Use keyword-only arguments (after `*`) for functions with more than 3 parameters to prevent positional argument confusion.
- Default arguments must be immutable. Never use `def f(items=[])`. Use `def f(items=None)` and assign inside.
- Return early to avoid deep nesting:
  ```python
  def process(data):
      if not data:
          return None
      if not data.is_valid():
          raise InvalidDataError(data.errors)
      return transform(data)
  ```

## Class Design

- Prefer composition over inheritance. Use mixins sparingly.
- Use `@dataclass` for classes that are primarily data containers.
- Use `@property` for computed attributes. Avoid explicit getter/setter methods.
- Use `__slots__` on classes that will be instantiated many times.
- Abstract base classes (`abc.ABC`) define interfaces. Use `@abstractmethod` on methods that subclasses must implement.

## Async Code

- Use `async`/`await` consistently. Never mix synchronous blocking calls in async functions.
- Use `asyncio.gather()` for concurrent I/O operations.
- Use `aiohttp` or `httpx` for async HTTP, `asyncpg` or `databases` for async database access.
- Never call `time.sleep()` in async code. Use `asyncio.sleep()`.
