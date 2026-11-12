# ouro_daemon_main — Agent Instructions

## Purpose

Cloud-side control server (`ourodaemon`). Manages edge devices, persists telemetry, serves WebUI.

## Key Components

### DaemonAgent (`agent.rs`)

Main entry point. Implements `ComponentImpl` + `ComponentImplExt`.
- `gather_actions()` sets up CLI RPC listener (UDS), ICC channels, system commands
- CLI connections handled via `cli_rpc` module with StateMachine pattern

### StateMachine Pattern (`agent.rs`, `cli_rpc` module)

Per-connection state machine for UDS RPC:
```
State::Connected → (process requests) → State::Finished
```
- Uses `LengthDelimitedCodec` + CBOR for framing
- Each connection dispatched to `CliHandler` trait implementation

### CliHandler Implementation (`agent.rs`)

Handles all CLI RPC methods (Version, Echo, Show, List, Describe, Operate).
Add new handler methods here when extending the CLI RPC interface.

### Tasks (`task/`)

| Task | File | Purpose |
|---|---|---|
| Heartbeat | `heartbeat.rs` | Periodic heartbeat |
| Metadata | `metadata.rs` | Telemetry metadata ingestion via Zenoh RPC |
| Metrics | `metrics.rs` | System metrics collection |
| SystemState | `system_state.rs` | System state management |
| WebUI | `webui.rs` | axum web server + WebSocket |
| State managers | `state/` | Shadow/Tag management with write-behind persistence |

### Adding a New Task

1. Create `task/{name}.rs`
2. Implement `ComponentImplExt` with `gather_actions()`
3. Add `pub mod {name};` in `task/mod.rs`
4. Register in `DaemonAgent` (agent.rs)
5. Add config to `ouro_state::DaemonTaskSpec` if needed

## Module Structure

```
src/
├── agent.rs        # DaemonAgent, CliHandler, cli_rpc StateMachine
├── task/
│   ├── mod.rs      # Task module + ICC handler registration
│   ├── heartbeat.rs
│   ├── metadata.rs
│   ├── metrics.rs
│   ├── system_state.rs
│   ├── webui.rs
│   └── state/      # Shadow/Tag management
└── types.rs        # BuiltDaemonTask, BuiltRepository
```
