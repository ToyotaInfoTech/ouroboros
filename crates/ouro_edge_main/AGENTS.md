# ouro_edge_main — Agent Instructions

## Purpose

Edge-side agent (`ouroedge`) running on vehicle devices. Collects sensor data, manages local storage, sends telemetry to daemon.

## Key Components

### EdgeAgent

Main entry point. Implements `ComponentImpl` + `ComponentImplExt`.
- Manages FlatBuffers IPC for local sensor data
- Communicates with daemon via Zenoh RPC

### AgentSkeleton

RPC skeleton for receiving commands from external processes (Python, SUMO).
- Uses FlatBuffers for IPC serialization
- UDS-based local communication

### Metadata Send

Periodic task that:
1. Collects unacked metadata from local SQLite
2. Serializes to CBOR
3. Sends via Zenoh RPC to daemon
4. Marks as acked on success

## Communication

- **Inbound**: FlatBuffers IPC over UDS (from Python/SUMO processes)
- **Outbound**: Zenoh RPC to daemon (CBOR-encoded metadata)
- **Local DB**: SQLite for metadata, system_state, camera_image

## Module Structure

```
src/
├── main.rs         # Entry point, EdgeAgent
├── metadata.rs     # Metadata collection and upload task
└── ...
```

## Dependencies

- `ouro_edge` — Edge runtime library (IPC, RPC handler, FlatBuffers gen)
- `ouro_core` — ComponentImpl, InternalError
- `ouro_schema` — Message types, RPC definitions
