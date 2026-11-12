---
name: cli-development
description: Guidance for adding and maintaining ourocli commands and CLI-side RPC dispatch behavior.
---

# CLI Development

## When to Use

Use this skill when adding or changing commands in the CLI binary.

## Add a CLI Command

### Workflow
1. Add a new subcommand variant to the `Command` enum in `crates/ouro_cli_main/src/main.rs`.
2. Define command args with clap attributes and defaults.
3. Add dispatch match logic in the main command handler.
4. Build and send `CliRequest` through `Client<CliRequest, CliResponse>`.
5. Render output according to `--format` (`toml` or `json`).
6. If operation-oriented, add operation descriptions in the schema layer.

### Implementation rules
- Prefer explicit typed request construction before `client.call(req).await?`.
- Keep output formatting deterministic and machine-readable for `toml` and `json` mode.
- Support UDS default endpoint behavior unless intentionally changed.
- Use `InternalResult` for command handling errors.

### RPC note
If the command needs a new RPC contract, perform RPC method addition in the schema and daemon handler first.

### References
- `crates/ouro_cli_main/src/main.rs`
- `crates/ouro_schema/src/schema/cli.rs`
- `crates/ouro_core/src/comm/rpc.rs`

## Validation Checklist
- Command enum updated.
- Dispatch match arm added.
- Request/Response path confirmed.
- Output format behavior verified.
- Operation metadata updated when applicable.

## Quick Checklist
- Command enum and dispatch arm are both implemented.
- RPC request/response path is valid for the command.
- `--format` output is deterministic for `toml` and `json`.
- Operation description metadata is added when required.
