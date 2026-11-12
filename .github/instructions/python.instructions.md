---
applyTo: "**/*.py"
---

# Python Coding Standards

## Docstrings
Add Google-style docstrings to all classes and public methods:

```python
"""
Purpose of this function/method.

Args:
    param_name (type): Description of the parameter.

Returns:
    type: Description of the return value.
"""
```

## Logging
- Use the `logging` module via module-level loggers.
- Guard expensive debug log construction:

```python
if logger.isEnabledFor(logging.DEBUG):
    logger.debug("Expensive message: %s", expensive_computation())
```

## Comments
- Write all comments and documentation in English.

## Testing
- Use `pytest` with `pytest-asyncio` for async tests.
- Tests are located in `python/tests/`.
