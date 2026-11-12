---
applyTo: "**"
description: "Security guidance for Ouroboros: trust boundaries, input handling, secrets, and dependency hygiene."
---

# Security

Ouroboros is a research prototype, but write code defensively anyway.

## Trust boundaries

- Treat data from every boundary as untrusted: edge to daemon (Zenoh), CLI to daemon (UDS or TCP), browser to WebUI (REST and WebSocket), and the Python world to the edge (UDS).
- After deserializing CBOR, FlatBuffers, JSON or TOML, validate sizes, counts and ranges before use.
- Return an error instead of panicking on bad input: use `InternalResult`, and no `unwrap()` or `expect()` in production paths.

## Rust

- Build SQL only with sqlx bind parameters. Never format values into a query string, and take dynamic identifiers from an allow-list.
- Reject path components such as `..` in client-supplied paths and keep resolved paths under the expected base directory.
- The WebUI REST and WebSocket API has no authentication layer today. Do not expose it to untrusted networks, and design access control before adding privileged endpoints.
- Do not use `unsafe` without documenting the invariant that makes it sound.
- Do not log secrets, tokens, credentials or full payloads. Log through `tracing` with minimal context.

## WebUI

- Do not use `dangerouslySetInnerHTML` with data that comes from the API.
- Validate forms with zod, and do not trust values read from the URL or `localStorage`.
- Never put secrets in `VITE_*` variables. They are compiled into the public bundle.
- Third-party content such as map tiles, fonts or CDN scripts must be opt-in configuration, not hard-coded.

## Python

- Do not use `eval`, `exec` or `pickle` on external data.
- Call `subprocess` with an argument list and never with `shell=True` plus interpolated input.

## Secrets and configuration

- Keep credentials, tokens and keys out of the repository. `.env` files that hold credentials stay untracked, and only `examples/dot.env.example` with placeholders is committed.
- Configuration TOML reads credentials through `${VAR:-default}` expansion.
- If a secret is committed by mistake, rotate it. Removing the commit is not enough.

## Dependencies

- Keep the CI checks green: `cargo deny`, `cargo audit`, `npm audit` and the license checks.
- Prefer maintained packages, commit the lockfiles, and check that a new dependency's license is compatible with Apache-2.0.

## Questions to ask in review

- Who can reach this code path, and what can they send?
- What happens with empty, huge or malformed input?
- Could a secret end up in a log line, error message or bundle?
- Does the change need a new entry in `NOTICE` or `REUSE.toml`?
