---
applyTo: "**"
description: "Language-agnostic code review guidance for Ouroboros: severity levels, review focus, and comment format."
---

# Generic Code Review

Use this together with `code-review.instructions.md`, which holds the Ouroboros-specific checks.

## Severity

- **Blocker**: must be fixed before merge. Security flaws, data loss or corruption, races, breaking changes to a public interface, leaked secrets.
- **Should fix**: worth resolving or discussing before merge. Missing tests for new behavior, clear performance problems, designs that diverge from existing patterns.
- **Nit**: optional. Naming, readability, small documentation gaps.

## What to look at

1. **Correctness**: does the change do what its description says? Check edge cases, error paths and concurrency.
2. **Security**: input validation, access control, injection, secrets in code or logs. See `security-and-owasp.instructions.md`.
3. **Tests**: new behavior and bug fixes come with tests. Tests are deterministic, independent of each other, and mock only external systems.
4. **Design**: respects existing module boundaries and patterns, avoids duplicated logic, and documents public APIs.
5. **Readability**: clear names, small functions, shallow nesting. Comments explain why, not what.
6. **Compatibility**: wire formats (CBOR, FlatBuffers), config files, and RPC or REST schemas stay compatible, or the break is called out explicitly.

## Comment format

State the severity, the problem, why it matters, and a concrete suggestion.

```markdown
**[Blocker] Short title**

What is wrong and where.

**Why this matters:** the impact if it ships.

**Suggestion:** a concrete change, with code when it helps.
```
