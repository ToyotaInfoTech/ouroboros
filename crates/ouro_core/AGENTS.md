# ouro_core — Agent Instructions

## Purpose

Runtime framework providing core abstractions for all Ouroboros components.

## Key Abstractions

### ComponentImpl / ComponentImplExt (`runtime/comp.rs`)

- `ComponentImpl`: Lifecycle trait with `on_created` → `on_active` → `on_inactive` → `on_discarded`
- `ComponentImplExt`: Extends `ComponentImpl` with event-driven `gather_actions()` → auto `tokio::select!` loop
- **Always prefer `ComponentImplExt`** for new tasks — avoid raw `tokio::spawn`

### SharableAction (`runtime/types.rs`)

Four action types returned from `gather_actions()`:
- `SharableAction::Instant` — runs once immediately
- `SharableAction::Event` — async trigger (e.g., channel receive, Zenoh subscription)
- `SharableAction::Periodic` — timer-based recurring action
- `SharableAction::Operation` — one-shot async operation

Use `define_resource_trigger_action!` macro for resource-triggered events:
```rust
define_resource_trigger_action! {
    with { resources } when: trigger_expr, then: action_fn
}
```

### InternalError / InternalResult (`error.rs`)

Unified error type. Usage:
```rust
// Wrap a typed error
("context", io_error).into()  // → InternalError with InternalErrorKind::Io

// Wrap a non-typed error
("context", InternalErrorKind::Network).into()
```

Key variants: `Io`, `Database`, `DeCbor`, `SerCbor`, `SerdeJson`, `Network`, `Protocol`, `Runtime`, `InvalidConfig`, `Bug`

### RPC (`comm/rpc.rs`)

- `RpcDispatcher` / `MethodHandler` traits for binary RPC
- `client` module: `Client<Req, Resp>` for UDS/TCP RPC calls
- `messaging.rs`: `RpcClient` / `RpcServer` traits for Zenoh-based distributed RPC

### ResourceContext (`runtime/context.rs`)

Manages DataSource, Client, Server, and Repository resource maps for component access.

## Module Structure

```
src/
├── comm/           # Communication (rpc, messaging, net)
├── runtime/        # Component lifecycle, actions, context, executor
│   ├── comp.rs     # ComponentImpl, ComponentImplExt
│   ├── types.rs    # SharableAction, ActionFactory
│   ├── context.rs  # ResourceContext
│   ├── builder.rs  # Component builder
│   ├── executor.rs # Action executor
│   └── rdb.rs      # DB transaction abstraction
├── supervisor/     # Task supervision (daemon feature)
└── error.rs        # InternalError, InternalErrorKind
```
