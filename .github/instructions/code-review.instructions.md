---
applyTo: "**"
---

# Code Review Checklist

## Error Handling
- [ ] Uses `InternalError` / `InternalResult` — no `anyhow`, `.unwrap()`, or `.expect()` in production code.
- [ ] Errors wrapped with descriptive context: `("context", root_cause_error_supported).into()` or `("context", error_or_kind).into()`.

## Architecture Compliance
- [ ] `ComponentImplExt` pattern used for long-running tasks — no raw `tokio::spawn`.
- [ ] `rpc!` macro used for RPC interface definitions.
- [ ] `StateMachine` pattern for multi-state async workflows.

## Documentation
- [ ] Doc comments in English (`///` for items, `//!` for modules).
- [ ] Public APIs have doc comments describing purpose, arguments, and return values.

## Security & Quality
- [ ] No hardcoded secrets or credentials.
- [ ] Input validation on public-facing APIs.
- [ ] Uses `tracing` for logging (not `println!` or `eprintln!`).
- [ ] No unnecessary `.clone()` or allocations.
