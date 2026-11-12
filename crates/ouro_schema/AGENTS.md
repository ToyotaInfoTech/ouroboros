# ouro_schema — Agent Instructions

## Purpose

Defines RPC/data schemas and communication message types for the entire system.

## Key Macros

### `rpc!` Macro (`schema/mod.rs`)

Generates strongly-typed RPC interface:
```rust
rpc! {
    Prefix,
    MethodA(MethodAReq) -> MethodAResp,
    MethodB(MethodBReq) -> MethodBResp,
}
// Generates: PrefixRequest enum, PrefixResponse enum, PrefixHandler trait, dispatch() fn
```

### `icc!` Macro (`schema/mod.rs`)

Inter-Component Communication — same pattern but with `RequestWrapper` + oneshot response channel.

### `operation_descriptions!` Macro (`schema/cli.rs`)

Defines CLI/ICC operation metadata from doc comments:
```rust
operation_descriptions! {
    /// Operation description
    "operation_name" {
        inputs { /// arg docs "arg_name" }
        output { /// output docs }
    }
}
```

## Adding a New RPC Method

1. Define `Req`/`Resp` structs with `#[derive(Debug, Clone, Serialize, Deserialize)]`
2. Add to `rpc!` macro in `schema/cli.rs` (or `schema/cli/rpc.rs`)
3. Implement `{Prefix}Handler` trait method in the daemon/edge
4. All types must be CBOR-serializable (serde `Serialize` + `Deserialize`)

## Module Structure

```
src/schema/
├── mod.rs          # rpc! and icc! macro definitions
├── cli.rs          # CLI RPC (CliRequest, CliResponse, CliHandler)
├── shadow/         # Device Shadow schema
├── traj.rs         # Trajectory telemetry
└── agent_control.rs
```

## Feature Flags

- `daemon`: Enables `serde-value` and `erased-serde` for dynamic serialization (CLI describe/operate)
