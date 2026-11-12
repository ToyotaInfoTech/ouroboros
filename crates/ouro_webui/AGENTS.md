# ouro_webui — Agent Instructions

## Purpose

WebUI server providing REST API and WebSocket real-time events for the dashboard frontend.

## Architecture

- **Framework**: axum 0.8 + tower middleware
- **Static files**: Embedded via `rust-embed` (built WebUI assets from `webui/` directory)
- **WebSocket**: Real-time `DashboardEvent` streaming to connected clients

## Key Components

### API Routes (`api/`)

REST endpoints for dashboard data. Uses `AppState` for shared daemon state access.
- axum extractors: `State`, `Path`, `Query`, `Json`
- Routes registered in router (check `api/mod.rs`)

### Events (`event/`)

`DashboardEvent` enum — WebSocket events pushed to frontend:
- Edge status changes
- Telemetry updates
- System state changes

### Static Files (`static_files/`)

Uses `rust-embed` to serve the built WebUI. The frontend is built separately (`cargo make bweb`) and embedded at compile time.

## Adding a New API Endpoint

1. Add handler function in `api/` with appropriate axum extractors
2. Register route in the router
3. Define request/response types
4. Add corresponding frontend API client function in `webui/src/api/client.ts`

## Build

```bash
# Build frontend first (required for rust-embed)
cargo make bweb

# Then build the Rust crate
cargo build
```
