# Contributing to Ouroboros

> **Project status: Experimental research platform.**
> This is not production-ready software. Expect breaking changes and incomplete features.
> Contributions of all kinds — code, documentation, ideas, and bug reports — are welcome.

## Table of Contents

- [Getting Started](#getting-started)
- [Reporting Issues](#reporting-issues)
- [Contributing Code](#contributing-code)
- [Code Style](#code-style)
- [Versioning](#versioning)
- [Contributor License Agreement](#contributor-license-agreement)

---

## Getting Started

### Prerequisites

| Tool | Version |
| ------ | --------- |
| Rust | stable (see `rust-toolchain.toml`) |
| Python | 3.13 |
| Node.js | 20+ |
| [cargo-make](https://github.com/sagiegurari/cargo-make) | latest |

### Build

```bash
# Rust
cargo build

# WebUI
cd webui && npm install && npm run build

# Python dependencies
pip install -e ".[dev]"

# Generate FlatBuffers code (required before first build)
cargo make gen
```

### Run tests

```bash
cargo test                  # Rust
pytest python/tests/        # Python
cd webui && npm test        # Frontend
```

---

## Reporting Issues

- **Bug reports**: Open a GitHub Issue with steps to reproduce, expected behavior, and actual behavior.
- **Feature requests**: Open a GitHub Issue describing the use case and why it fits the project's direction.
- **Security vulnerabilities**: Do **not** open a public issue. See [SECURITY.md](SECURITY.md).

### Language

- Write issues and pull requests in English when you can, so that more people can follow the discussion.
  Japanese is also welcome; maintainers may answer in either language.
- Code comments, documentation and commit messages must be in English.

---

## Contributing Code

1. Fork the repository and create a branch from `develop`.
2. Make your changes. Keep commits focused and atomic.
3. Ensure all checks pass locally:

   ```bash
   cargo clippy -- -D warnings
   cargo fmt --check
   cargo test
   cd webui && npm run lint
   pytest python/tests/
   ```

4. Open a pull request against `develop` with a clear description of what and why.

### Commit message format

Use the imperative mood and keep the subject line under 50 characters.

```text
Add Kafka client to daemon messaging layer
Fix edge reconnect logic on network timeout
```

---

## Code Style

| Layer | Standard |
| ------- | ---------- |
| Rust | `cargo fmt` + `cargo clippy -D warnings` |
| Python | Ruff / PEP 8 |
| TypeScript | ESLint (`npm run lint`) |
| All | English for comments, doc comments, and commit messages |

All source files must include an SPDX header:

```rust
// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0
```

Files that cannot carry a header (documents, lock files, generated code, third-party works) are annotated in `REUSE.toml`. Run `reuse lint` to check REUSE compliance (CI runs it as well).

---

## Versioning

Ouroboros follows [Semantic Versioning 2.0.0](https://semver.org/) (`MAJOR.MINOR.PATCH`).
The version lives in `[workspace.package]` of the root `Cargo.toml`, and `pyproject.toml` carries the same number.

While the version is `0.y.z` (the current state), the API is unstable:

- Bump `y` for breaking changes.
- Bump `z` for backward-compatible features and bug fixes.

From `1.0.0` on, breaking changes bump `MAJOR`, features bump `MINOR`, and fixes bump `PATCH`.

The following count as public interfaces, so changing them incompatibly is a breaking change:

- CLI commands and options, and the CLI/daemon RPC protocol.
- REST and WebSocket API of the WebUI.
- Wire formats (CBOR, FlatBuffers) and the TOML configuration schema.

Contributors do not bump the version in feature or fix PRs. Maintainers bump it in a release PR, tag the merge commit `vMAJOR.MINOR.PATCH`, and publish a GitHub Release with the notes.
Mention breaking changes in your PR description so they can be reflected in the notes.

---

## Contributor License Agreement

By submitting a pull request you agree to the terms of the [Contributor License Agreement](CLA.md).
If your contribution is made as part of employment, please make sure your employer has permitted it (see Section 5 of the CLA) before submitting.
