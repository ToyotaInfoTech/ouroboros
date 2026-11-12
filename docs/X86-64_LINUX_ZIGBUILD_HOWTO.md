X64 Linux Zigbuild HOWTO
========================

This document explains how to build Linux x86_64 GNU binaries on macOS
for this repository using `cargo-zigbuild`.

Target
------

- Rust target triple: `x86_64-unknown-linux-gnu`
- Host OS in this guide: macOS

Prerequisites
-------------

1. Install Zig.

 ```bash
 brew install zig
 ```

2. Verify Zig is available.

 ```bash
 zig version
 ```

3. Install cargo-zigbuild.

 ```bash
 cargo install cargo-zigbuild
 ```

4. Add the Rust target.

 ```bash
 rustup target add x86_64-unknown-linux-gnu
 ```

Build Commands
--------------

Build all workspace crates in release mode for Linux x86_64:

```bash
cargo zigbuild --workspace --release --target x86_64-unknown-linux-gnu
```

Build a specific package only:

```bash
cargo zigbuild -p ouro_edge --release --target x86_64-unknown-linux-gnu
```

Build a specific binary target:

```bash
cargo zigbuild -p ouro_daemon --bin ourodaemon --release --target x86_64-unknown-linux-gnu
```

Output Location
---------------

Artifacts are created under:

```text
target/x86_64-unknown-linux-gnu/release/
```

For example:

- `target/x86_64-unknown-linux-gnu/release/ouroedge`
- `target/x86_64-unknown-linux-gnu/release/ourodaemon`

Quick Verification
------------------

Check that the binary is Linux x86_64 ELF:

```bash
file target/x86_64-unknown-linux-gnu/release/ouroedge
```

Expected output includes:

```text
ELF 64-bit LSB executable, x86-64
```

Troubleshooting
---------------

1. `zig: command not found`
 - Zig is not installed or not in your `PATH`.
 - Reinstall with Homebrew and open a new shell.

2. `no such command: zigbuild`
 - `cargo-zigbuild` is not installed, or `~/.cargo/bin` is not in `PATH`.

3. Build still uses host target
 - Confirm you passed:
   `--target x86_64-unknown-linux-gnu`

4. CI reproducibility
 - Pin versions in CI for Rust toolchain and Zig to avoid environment drift.

Notes
-----

- `cargo install cargo-zigbuild` installs `cargo-zigbuild`, not Zig itself.
- This method avoids setting up a full Linux GNU linker/sysroot manually on macOS.
