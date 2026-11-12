# ouro_cli_main — Agent Instructions

## Purpose

CLI tool (`ourocli`) for interacting with `ourodaemon` over UDS or TCP.

## Command Structure

```
ourocli [-s socket] <command>

Commands:
  show      Show daemon state (target: state | runtime)
  echo      Echo test message
  list      List registered tasks
  describe  Describe a specific task
  operate   Execute an operation on a task
```

### Socket Addressing

- UDS (default): `unix:/tmp/ourodaemon.sock`
- TCP: `tcp:host:port`

### Operate Command

```bash
ourocli operate --name task_name --operation op_name key1=value1 key2=value2
ourocli operate --name task_name --operation op_name file://config.toml
ourocli operate --name task_name --operation op_name file://params.json
```

Input supports `key=value` pairs or file references (`file://path.toml`, `file://path.json`).

## RPC Communication

Uses `Client<CliRequest, CliResponse>` from `ouro_core::comm::rpc::client`.
- Frames: `LengthDelimitedCodec` + CBOR
- All request/response types defined in `ouro_schema::schema::cli`

## Adding a New Command

1. Add variant to `Command` enum (with clap attributes)
2. Add match arm in dispatch logic
3. Construct appropriate `CliRequest` variant
4. Call `client.call(req).await`
5. Format response for display (`--format text|json`)

## `operation_descriptions!` Macro

Use this macro in `ouro_schema::schema::cli` to define operation metadata that `describe` and `operate` commands can discover at runtime.
