---
name: rust-development
description: Guidance for Rust development in Ouroboros, including daemon task design, RPC method additions, InternalError handling, and Rust code review.
---

# Rust Development

## When to Use

Use this skill when working on Rust code in this repository, especially for daemon tasks, CLI-daemon RPC methods, and code quality reviews.

## Add a Daemon Task

### Workflow
1. Create a new task module under `crates/ouro_daemon_main/src/task/`.
2. Implement `ComponentImpl` for identity and component type bindings.
3. Implement `ComponentImplExt::gather_actions()` and return `Vec<SharableAction>`.
4. Register the module in `crates/ouro_daemon_main/src/task/mod.rs`.
5. Add or update task configuration fields in `ouro_state` when needed.
6. Register the task in `crates/ouro_daemon_main/src/agent.rs`.

### Implementation rules
- Prefer `ComponentImplExt` for long-running/event-driven work.
- Use `SharableAction::Event` and `SharableAction::Periodic`.
- Do not use raw `tokio::spawn` in daemon task code.
- Use `InternalResult` for fallible functions.
- Add English doc comments: `//!` at module top, `///` for public APIs.

### References
- `crates/ouro_core/src/runtime/comp.rs`
- `crates/ouro_core/src/runtime/types.rs`
- `crates/ouro_daemon_main/src/task/heartbeat.rs`
- `crates/ouro_daemon_main/src/task/metadata.rs`
- `crates/ouro_daemon_main/src/task/webui.rs`
- `crates/ouro_daemon_main/src/agent.rs`

## Add an RPC Method

### Workflow
1. Define request/response types in `crates/ouro_schema/src/schema/cli.rs` or related cli submodule.
2. Add the method to the `rpc!` interface.
3. Implement the handler in the daemon-side `CliHandler` implementation.
4. If user-facing, add CLI command handling that calls `Client<CliRequest, CliResponse>`.
5. If operation-based, add operation description metadata.

### Implementation rules
- Request/Response types should derive `Serialize` and `Deserialize`.
- Handler methods should return `InternalResult<ResponseType>`.
- Wrap errors with context via `("context", error_or_kind).into()`.
- Keep RPC comments and docs in English.

### References
- `crates/ouro_common/src/lib.rs`
- `crates/ouro_schema/src/schema/mod.rs`
- `crates/ouro_schema/src/schema/cli.rs`
- `crates/ouro_daemon_main/src/agent.rs`
- `crates/ouro_cli_main/src/main.rs`
- `crates/ouro_core/src/comm/rpc.rs`

## Fix InternalError Handling

### Preferred patterns
- Use `InternalResult<T>` instead of `anyhow::Result`.
- Convert source errors with `map_err(|e| ("context", e).into())?`.
- Use descriptive non-wrapping `InternalErrorKind` where no typed wrapper exists.

### Variant guidance
- `std::io::Error` -> `Io`
- `sqlx::Error` -> `Database`
- `serde_json::Error` -> `SerdeJson`
- `toml::de::Error` -> `DeToml`
- `toml::ser::Error` -> `SerToml`
- `ciborium::de::Error` -> `DeCbor`
- `ciborium::ser::Error` -> `SerCbor`
- `flatbuffers::InvalidFlatbuffer` -> `SerdeFlatbuf`
- `std::net::AddrParseError` -> `InvalidAddress`

### Anti-patterns to remove
- `.unwrap()` and `.expect()` in production paths.
- Converting failures to `Option` with `.ok()` when errors are needed for diagnosis.
- Returning `Result<T, Box<dyn Error>>` in core paths.

### References
- `crates/ouro_core/src/error.rs`

## Rust Code Review Checklist

### Error handling
- `InternalResult` used in fallible APIs.
- Contextual wrapping applied on propagated errors.
- No `unwrap`/`expect` in production code.

### Architecture compliance
- Long-running tasks follow `ComponentImplExt` and `SharableAction` patterns.
- RPC contracts defined via `rpc!`.
- Multi-state async flows use explicit `State`/`StateMachine` style where appropriate.

### Documentation and quality
- Public APIs have `///` comments.
- Modules use `//!` docs.
- Logging uses `tracing` macros, not `println!`.
- No hardcoded secrets.
- Validate boundary inputs.

### Async robustness
- Avoid ownership/capture mistakes in action factories.
- Preserve root-cause errors for resource initialization failures.

### References
- `crates/ouro_core/src/error.rs`
- `crates/ouro_core/src/runtime/comp.rs`
- `crates/ouro_core/src/runtime/types.rs`
- `crates/ouro_schema/src/schema/mod.rs`
- `crates/ouro_daemon_main/src/agent.rs`

## Validation Checklist
- Task additions follow `ComponentImplExt` and action-based execution.
- RPC methods are declared in `rpc!` and handled via `CliHandler`.
- Error paths use `InternalResult` and contextual wrapping.
- Public Rust APIs include required English documentation comments.
- Production paths avoid `unwrap`, `expect`, and silent error dropping.

## Quick Checklist
- Uses `InternalResult` and wraps errors with context.
- Follows `ComponentImplExt` with action-driven async flow.
- Defines RPC contracts with `rpc!` and matching handler methods.
- Avoids `unwrap`, `expect`, and `Result::ok()` for runtime failures.
- Keeps module/public API docs and logging conventions.
