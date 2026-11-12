---
applyTo: "**"
---

# Test Generation Standards

## Rust
- Use `#[tokio::test]` for async tests, `#[test]` for sync tests.
- Use `InternalResult` as the return type for tests that can fail.
- Place unit tests in a `#[cfg(test)] mod tests` block within the same file.
- Place integration tests in `tests/` directory of the respective crate.

## Python
- Use `pytest` with `pytest-asyncio` for async tests.
- Place tests in `python/tests/` mirroring the source module structure.
- Use fixtures for shared setup; avoid global mutable state.

## TypeScript / React
- Test components and hooks in isolation.
- Use mock data for API responses (do not call real endpoints).
