# Agent Guidelines

## Core Principles

- Always start tasks with 3 or more steps in Plan mode
- Never write code without reading it first. Always review existing code before making changes

## Project Overview

Ouroboros is a distributed edge-cloud system for vehicle telemetry collection and management. Edge devices (vehicles) collect sensor data and send it to a cloud daemon via Zenoh messaging. Real-time monitoring is provided via a WebUI dashboard, and CLI tools operate over UDS.

## Architecture

### Crate Dependencies

```
ouro_common (shared types, constants, IO)
  └─ ouro_state (TOML config deserialization)
      └─ ouro_schema (RPC/data schemas, rpc!/icc! macros)
          └─ ouro_core (runtime framework)
              ├─ ouro_daemon → ouro_daemon_main (ourodaemon)
              ├─ ouro_edge → ouro_edge_main (ouroedge)
              ├─ ouro_webui (axum + WebSocket + REST)
              └─ ouro_cli_main (ourocli)
```

### Communication Protocols

| Protocol | Purpose |
| --- | --- |
| Zenoh | pub/sub + RPC between edge and daemon |
| UDS | Local CLI-to-daemon communication; distributed traffic simulation-to-edge IPC |
| WebSocket | Real-time events from daemon to WebUI |
| CBOR | Serialization for RPC payloads |
| FlatBuffers | Serialization for edge IPC |

### Tech Stack

| Layer | Technologies |
| --- | --- |
| Rust | tokio 1.52, Zenoh 1.9, axum 0.8, clap 4.6, tracing |
| DB | TimescaleDB (daemon), SQLite (edge) |
| Frontend | React 19, TypeScript 5.9, Vite 8, Tailwind CSS 4, TanStack Router/Query, zustand |
| Python | 3.13, Ray 2.52, SUMO (traci) |

## Build & Test

```bash
cargo build                # Build all Rust crates
cargo test                 # Run Rust tests
cargo clippy               # Lint Rust
pytest python/tests/       # Run Python tests
cd webui && npm run build  # Build frontend
cd webui && npm run dev    # Start frontend dev server
cargo make gen             # Generate FlatBuffers code
cargo make bweb            # Build WebUI
cargo make cup / cdown     # Start/stop containers (TimescaleDB/PostgreSQL, Grafana)
cargo make zup / zdown     # Start/stop Zenoh daemon
cargo make oup / odown     # Start/stop ourodaemon
cargo make aup / adown     # Start/stop all services
cargo make stat            # Show system status

# Remote execution (prepend REMOTE=<host> to any task)
REMOTE=ourocloud cargo make cup    # Start containers on remote host
REMOTE=ourocloud cargo make oup    # Start ourodaemon on remote host
REMOTE=ourocloud cargo make stat   # Show status on remote host

# File sync (rsync-based)
bin/ouro-sync ourocloud            # Sync to ourocloud using role-based rules
bin/ouro-sync --role cloud         # Sync to all cloud-role hosts
bin/ouro-sync --role all           # Sync to all hosts
bin/ouro-sync ourocloud --dry-run  # Preview without transferring
```

## Environment Variables

Load environment variables by sourcing the `setenv` script:

```bash
source setenv local   # Local development (default)
source setenv ray     # Ray cluster deployment
```

| Variable | Description |
| --- | --- |
| `WORLD_MODE` | `local` or `ray`. Selects config files (`etc/*_${WORLD_MODE}.toml`) |
| `BUILD_PROFILE` | Rust build profile (`debug` / `release`, default: `debug`) |
| `OURO_HOME` | Path to the project root |
| `RUST_LOG` | Rust log level |

### Configuration File Naming Convention

Config files are selected automatically based on `WORLD_MODE`:

| File | Purpose |
| --- | --- |
| `etc/daemon_config_local.toml` | Daemon config for local development |
| `etc/daemon_config_ray.toml` | Daemon config for Ray cluster deployment |
| `etc/edge_config_base_local.toml` | Base edge config for local development |
| `etc/edge_config_base_ray.toml` | Base edge config for Ray cluster deployment |
| `etc/edge_config_full_local.toml` | Full edge config for local development |
| `etc/edge_config_full_ray.toml` | Full edge config for Ray cluster deployment |

## Common Task Patterns

### Adding a New RPC Method

1. Add the method to the `rpc!` macro in `crates/ouro_schema/src/schema/cli.rs`
2. Define Request/Response types in the same file or an appropriate module
3. Add a handler to the `CliHandler` implementation in `crates/ouro_daemon_main/src/agent.rs`
4. Add a CLI subcommand to `crates/ouro_cli_main/src/main.rs` (if needed)

### Adding a New Daemon Task

1. Create a new file under `crates/ouro_daemon_main/src/task/`
2. Implement the `ComponentImplExt` trait and define actions in `gather_actions()`
3. Add config fields to `DaemonTaskSpec` in `ouro_state`
4. Register the task in `agent.rs`

### Adding a New Edge Task

1. Create a new file under `crates/ouro_edge_main/src/`
2. Implement `ComponentImplExt`
3. Add config fields to `EdgeTaskSpec` in `ouro_state`

### Python World (Ray) Configuration

Key options for `bin/run-launch` and `python -m ouroboros.main`:

| Option | Default | Description |
| --- | --- | --- |
| `--max-places` | 50 | Maximum number of Place actors |
| `--num-cpus-per-place` | 0.5 | CPU allocation per Place (Ray `num_cpus`) |
| `--num-gpus-per-place` | 0.0 | GPU allocation per Place (Ray `num_gpus`) |
| `--renderer-name` | — | Renderer name (e.g. `wgpu`) |
| `-ew` / `--world-mode` | — | World mode (`local` or `ray`) |

Place actors are created on demand. If the cluster lacks sufficient GPUs, it automatically falls back to CPU-only mode (pre-reservation via PlacementGroup has been removed).

## Git

### Commit Messages

- Write commit messages in English
- Generate concise and clear Git commit messages in English based on the provided diff
- Use the imperative mood (e.g. `Add feature`, not `Added feature` or `Adds feature`)
- If the diff contains multiple changes, write a summary message that captures all the key points
- Avoid vague words without context (e.g. `Update`, `Fix`) — state concretely what was changed or fixed
- Keep the subject line under 50 characters; add a blank line and body for details if needed

## Programming Languages

### Comments

- All comments and documentation must be written in English

### Language-Agnostic Rules

### Language-Specific Rules

#### Python

- Guard expensive `debug`-level log output so it is skipped when the log level is disabled
- Add docstrings to describe the purpose, arguments, and return values of classes and methods. Use the following format:

```python
"""
Purpose of this function/method
Args:
    variable_name (type): Description of what the variable holds
Returns:
    type: Description of the return value
"""
```

#### Rust

- Add doc comments to describe the purpose, arguments, and return values of modules, structs, traits, and methods.
- Doc comment syntax:

| Usage | Syntax |
| --- | --- |
| Module/crate description (top of file) | `//!` |
| Function/struct/method description | `///` |
| Field description | `///` |

- Prefer implementing `From`/`Into` traits for type conversions
- Prefer `InternalResult<T>` for all fallible functions
- If `InternalErrorKind` has a variant that wraps the source error type, wrap the error in that variant and return it
- If no wrapping variant exists, choose the most appropriate non-wrapping variant from `InternalErrorKind`
- Implement `ComponentImpl` or, where needed, `ComponentImplExt` for application components
- Prefer implementing `ComponentImplExt::gather_actions()` and handling events asynchronously via `SharableAction::Event` inside a `tokio::select!` loop, rather than spawning raw `tokio::spawn` tasks
- Use the `State` + `StateMachine` design pattern for multi-state async workflows (see `agent.rs` in `ouro_daemon_main`)
- For RPC used by CLI and similar clients, prefer the following macros, libraries, and patterns:
  - `rpc!` macro in `${workspace}/crates/ouro_common/src/lib.rs`
  - `cli_handle` module in `${workspace}/crates/ouro_daemon_main/src/agent.rs`
  - RPC client module in `${workspace}/crates/ouro_core/src/comm/rpc.rs`
- Version number follows [Semantic Versioning 2.0.0](https://semver.org/) and is managed via `version` in `Cargo.toml` (`[workspace.package]`); keep `pyproject.toml` in sync
  - Do not bump versions in feature PRs; maintainers do it for a release (see "Versioning" in `CONTRIBUTING.md`)

#### TypeScript (WebUI)

- Framework: React 19 + TypeScript 5.9 + Vite 8 + Tailwind CSS 4
- Routing: TanStack Router (code-based, not file-based; see `webui/src/router.ts`)
- Server state: TanStack Query (fetching and caching API data)
- Client state: zustand (UI-local state)
- Forms: react-hook-form + zod (validation)
- API client: `webui/src/api/client.ts` (fetch-based)
- WebSocket: `webui/src/hooks/useWebSocket.ts` (real-time event reception)
- Page structure: feature-based directories under `webui/src/pages/` (dashboard, edges, metadata, tags)
- Components: organized under `webui/src/components/` into `layout/` and `ui/`
