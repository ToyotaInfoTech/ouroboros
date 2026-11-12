# Python ouroboros — Agent Instructions

## Purpose

Python modules for edge simulation, SUMO integration, and Ray-based distributed processing.

## Module Structure

```console
python/ouroboros/
├── __init__.py
├── main.py          # Entry point (sumo local/remote modes)
├── util.py          # Shared utilities
├── edge/            # Edge device simulation
├── gen/             # Generated code (FlatBuffers)
├── message/         # Message types and serialization
├── router/          # Routing and communication
└── world/           # World simulation (SUMO integration)
```

## Tech Stack

- Python 3.13
- Ray 2.52 — distributed computing cluster
- SUMO (traci/sumolib) — traffic simulation
- FlatBuffers — IPC serialization (edge communication)
- CBOR (cbor2) — metadata serialization
- numpy, pandas, opencv — data processing

## Communication

- Communicates with `ouroedge` via FlatBuffers IPC over UDS
- SUMO provides vehicle state via `traci` API
- Ray handles distributed task scheduling

## Conventions

- Docstrings in Google style format:

  ```python
  """
  Function purpose.
  Args:
      param (type): Description.
  Returns:
      type: Description.
  """
  ```

- Guard expensive debug logging: `if logger.isEnabledFor(logging.DEBUG):`
- Comments and documentation in English
- Tests in `python/tests/` using pytest + pytest-asyncio
