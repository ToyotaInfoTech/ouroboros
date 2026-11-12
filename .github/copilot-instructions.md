# Copilot Instructions

## Project Overview

Ouroboros is a distributed edge-cloud system for vehicle telemetry collection and management.
Edge devices (e.g. vehicles, or other IoT devices) collect sensor data and send it to a cloud daemon via Zenoh messaging.
A WebUI dashboard provides real-time monitoring. CLI tools interact with the daemon over Unix Domain Sockets.

The Python layer (Ray cluster + SUMO traffic simulator) feeds simulated vehicle state to the edge layer via Unix Domain Sockets.

## Architecture

### Crate Dependency Graph

```
ouro_common          ← Shared types, constants, CBOR/TOML IO, rpc! macro
  └─ ouro_state      ← TOML config deserialization (DaemonSpec, EdgeSpec, etc.)
      └─ ouro_schema  ← RPC/data schemas, rpc!/icc! macros, message types
          └─ ouro_core ← Runtime framework (ComponentImpl, SharableAction, InternalError)
              ├─ ouro_daemon      ← Daemon-specific runtime (DaemonResourceContext)
              │   └─ ouro_daemon_main ← Cloud daemon binary (ourodaemon)
              ├─ ouro_edge        ← Edge-specific runtime (IPC, FlatBuffers)
              │   └─ ouro_edge_main   ← Edge binary (ouroedge)
              ├─ ouro_webui       ← WebUI server (axum + WebSocket + REST)
              └─ ouro_cli_main    ← CLI binary (ourocli)
```

### Key Binaries

| Binary | Crate | Purpose |
|---|---|---|
| `ourodaemon` | ouro_daemon_main | Cloud-side control server |
| `ouroedge` | ouro_edge_main | Edge-side agent on vehicle |
| `ourocli` | ouro_cli_main | CLI tool (UDS/TCP to daemon) |

### Communication

- **Zenoh**: Distributed pub/sub + querier/queryable RPC between edge and daemon
- **UDS**: Unix Domain Socket for local CLI-to-daemon communication, and Python Edge↔OBU IPC
- **WebSocket**: Real-time events from daemon to WebUI
- **CBOR**: Serialization for RPC payloads
- **FlatBuffers**: Serialization for edge IPC

## Key Patterns

### ComponentImpl / ComponentImplExt

All long-running tasks implement `ComponentImpl` (lifecycle) or `ComponentImplExt` (event-driven).
`ComponentImplExt::gather_actions()` returns `Vec<SharableAction>` which are multiplexed in a `tokio::select!` loop automatically. **Do not use raw `tokio::spawn`** in task code — use `SharableAction::Event` or `SharableAction::Periodic` instead.

Reference: `crates/ouro_core/src/runtime/comp.rs`, `crates/ouro_core/src/runtime/types.rs`

### rpc! Macro

Defines RPC interfaces generating Request/Response enums, Handler trait, and dispatch function.

```rust
rpc! {
    Cli,
    Version(VersionReq) -> VersionResp,
    Echo(EchoReq) -> EchoResp,
    // ...
}
// Generates: CliRequest, CliResponse, CliHandler trait, dispatch()
```

Reference: `crates/ouro_schema/src/schema/mod.rs`, `crates/ouro_schema/src/schema/cli.rs`

### InternalError / InternalResult

Unified error type. Wrap errors with `("context", error_or_kind).into()`.
Use `InternalErrorKind` variants that match the source error. For errors without a wrapping variant, use descriptive non-wrapping variants like `Network`, `Protocol`, `Runtime`, etc.

| Source error type | `InternalErrorKind` variant |
|---|---|
| `std::io::Error` | `Io` |
| `sqlx::Error` | `Database` |
| `serde_json::Error` | `SerdeJson` |
| `toml::de::Error` | `DeToml` |
| `toml::ser::Error` | `SerToml` |
| `ciborium::de::Error` | `DeCbor` |
| `ciborium::ser::Error` | `SerCbor` |
| `flatbuffers::InvalidFlatbuffer` | `SerdeFlatbuf` |
| `std::net::AddrParseError` | `InvalidAddress` |

Avoid `.unwrap()`, `.expect()`, and `.ok()` in production code paths.

Reference: `crates/ouro_core/src/error.rs`

### StateMachine Pattern

For multi-state async workflows, use `State` + `StateMachine` pattern.

Reference: `crates/ouro_daemon_main/src/agent.rs` (cli_rpc module)

## Common Task Workflows

### Add a Daemon Task

1. Create a new module under `crates/ouro_daemon_main/src/task/`.
2. Implement `ComponentImpl` (identity/type) and `ComponentImplExt::gather_actions()`.
3. Register the module in `crates/ouro_daemon_main/src/task/mod.rs`.
4. Add config fields to `ouro_state` (`DaemonTaskSpec`) if needed.
5. Register the task in `crates/ouro_daemon_main/src/agent.rs`.

Reference examples: `heartbeat.rs`, `metadata.rs`, `webui.rs` in `src/task/`.

### Add an RPC Method

1. Define request/response types in `crates/ouro_schema/src/schema/cli.rs`.
2. Add the method to the `rpc!` interface.
3. Implement the handler in the daemon-side `CliHandler` in `agent.rs`.
4. Add a CLI subcommand in `crates/ouro_cli_main/src/main.rs` calling `Client<CliRequest, CliResponse>`.

### Add a WebUI Feature

**Backend:** Add/extend a route module under `crates/ouro_webui/src/api/`, register it in the API router.

**Frontend:**
1. Create a page under `webui/src/pages/<feature>/`.
2. Add types in `webui/src/api/types.ts` and calls in `webui/src/api/client.ts`.
3. Register the route in `webui/src/router.ts` (code-based routing, not file-based).
4. Add a sidebar/nav entry if user-facing.

State management rules: server data → TanStack Query; UI-only state → zustand; realtime → `useWebSocket` hook.

## Tech Stack

| Layer | Technologies |
|---|---|
| Rust runtime | tokio 1.52, async/await |
| Messaging | Zenoh 1.9 |
| Web server | axum 0.8, tower |
| Database | TimescaleDB (daemon), SQLite (edge) |
| Serialization | CBOR (ciborium), FlatBuffers, serde_json, TOML |
| CLI | clap 4.6 |
| Logging | tracing + tracing-subscriber |
| Frontend | React 19, TypeScript 5.9, Vite 8, Tailwind CSS 4 |
| Routing (FE) | TanStack Router (code-based, `webui/src/router.ts`) |
| State (FE) | zustand, TanStack Query |
| Forms (FE) | react-hook-form + zod |
| Python | 3.13, Ray 2.52, SUMO (traci) |

## Build & Test Commands

```bash
# Rust
cargo build                              # Build all crates
cargo test                               # Run all Rust tests
cargo test -p <crate_name>               # Test a single crate
cargo test -p <crate_name> <test_name>   # Run a single test
cargo clippy                             # Lint

# WebUI
cd webui && npm install && npm run build   # Build frontend
cd webui && npm run dev                    # Dev server

# Python
pytest python/tests/                     # Run all Python tests
pytest python/tests/test_foo.py::test_bar  # Run a single test

# cargo-make tasks
cargo make gen                 # Generate FlatBuffers code
cargo make bweb                # Build WebUI
cargo make cup / cdown         # Start/Stop containers (TimescaleDB, Grafana)
cargo make zup / zdown         # Start/Stop Zenoh daemon
cargo make oup / odown         # Start/Stop ourodaemon
cargo make aup / adown         # Start/Stop all services
cargo make stat                # Show system status
cargo make sync                # Sync files to remote (requires REMOTE or SYNC_ROLE)
cargo make pdoc / rdoc         # Generate Python/Rust docs

# Remote execution (prepend REMOTE=<host> to any task)
REMOTE=ourocloud cargo make cup    # Start containers on ourocloud
REMOTE=ourocloud cargo make oup    # Start ourodaemon on ourocloud
REMOTE=ourocloud cargo make stat   # Show status on ourocloud

# File sync (rsync-based, replaces ansible deploy)
bin/ouro-sync ourocloud            # Sync to host using role rules
bin/ouro-sync --role cloud         # Sync to all cloud hosts
bin/ouro-sync --role all           # Sync to all hosts
bin/ouro-sync ourocloud --dry-run  # Preview sync
```

## Code Conventions

### All Languages
- All code comments and documentation must be written in **English**.
- Use `tracing` macros for logging (not `println!`).

### Rust
- Module/crate docs: `//!` at file top. Struct/fn/field docs: `///`.
- Use `From`/`Into` traits for type conversions.
- Use `InternalResult<T>` for all fallible functions; wrap with `("context", err).into()`.
- Prefer `ComponentImplExt` + `SharableAction` over `tokio::spawn`.

### Python
- Use Google-style docstrings (Args/Returns sections).
- Guard expensive `debug`-level logging with a level check before constructing the message.

### TypeScript
- Form validation: react-hook-form + zod.
- Keep API types and client calls co-located in `webui/src/api/`.

## Version Numbering

The project follows [Semantic Versioning 2.0.0](https://semver.org/). Version in root `Cargo.toml` (`[workspace.package]`); keep `pyproject.toml` in sync.
See "Versioning" in `CONTRIBUTING.md` for the bump rules.

## Git Commit Conventions

- Use imperative mood: `Add feature`, not `Added feature`.
- Subject line ≤ 50 characters; use body for details if needed.
- Avoid vague subjects (`Update`, `Fix`) — state what changed concretely.

## Skill Files

Detailed step-by-step workflows and review checklists are in `.github/skills/`:
- `rust-development/SKILL.md` — daemon tasks, RPC methods, error handling, code review checklist
- `cli-development/SKILL.md` — CLI command additions and dispatch patterns
- `webui-development/SKILL.md` — full-stack WebUI feature additions
