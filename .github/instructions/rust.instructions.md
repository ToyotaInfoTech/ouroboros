---
applyTo: "**/*.rs"
---

# Rust Coding Standards

## Error Handling
- Use `InternalResult<T>` as the return type for all fallible functions.
- Wrap errors with `("context message", error_or_kind).into()`.
- If `InternalErrorKind` has a wrapping variant for the source error type, use it (e.g., `Io` for `io::Error`, `Database` for `sqlx::Error`).
- Otherwise, use a descriptive non-wrapping variant: `Network`, `Protocol`, `Runtime`, `InvalidConfig`, `Bug`, etc.
- Reference: `crates/ouro_core/src/error.rs`

## Component Pattern
- Implement `ComponentImplExt` trait with `gather_actions()` for long-running tasks.
- Return `Vec<SharableAction>` — the framework handles `tokio::select!` multiplexing.
- Use `SharableAction::Event` for async triggers, `SharableAction::Periodic` for timers.
- Use `define_resource_trigger_action!` macro where applicable.
- Never spawn new `tokio::spawn` tasks directly — use actions instead.
- Reference: `crates/ouro_core/src/runtime/comp.rs`, `crates/ouro_core/src/runtime/types.rs`

## RPC
- Define RPC interfaces via the `rpc!` macro in `crates/ouro_schema/src/schema/`.
- `rpc!` generates: `{Prefix}Request` enum, `{Prefix}Response` enum, `{Prefix}Handler` trait, `dispatch()` fn.
- Implement handler methods in the corresponding `*Handler` trait.
- Request/Response types must derive `Serialize` + `Deserialize` (CBOR encoding).
- Reference: `crates/ouro_schema/src/schema/mod.rs`, `crates/ouro_schema/src/schema/cli.rs`

## State Machines
- Use `State` + `StateMachine` pattern for multi-step async workflows.
- Reference: `crates/ouro_daemon_main/src/agent.rs` (cli_rpc module)

## Type Conversions
- Implement `From` / `Into` traits for type conversions (not ad-hoc conversion functions).

## Documentation
- `//!` at the top of each module file.
- `///` on all public structs, traits, methods, and fields.
- Write all comments and documentation in English.

## Logging
- Use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`, `trace!`).
- Do not use `println!` or `eprintln!` in library/application code.
