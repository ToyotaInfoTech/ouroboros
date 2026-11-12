// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

// build.rs
// This build script generates Rust and Python code from all fb/*.fbs FlatBuffers schemas before building.
// All comments and docs are in English (see AGENTS.md).

use std::fs;
use std::process::Command;

fn main() {
    // let rust_out_dir = "crates/edge/src/gen";
    // let python_out_dir = "python/ouroboros/gen";
    // let fb_schema_dir = "schema/fb";

    // // Find all .fbs files in fb/
    // let schemas = match fs::read_dir(fb_schema_dir) {
    //     Ok(entries) => entries
    //         .filter_map(|e| e.ok())
    //         .map(|e| e.path())
    //         .filter(|p| p.extension().map(|ext| ext == "fbs").unwrap_or(false))
    //         .collect::<Vec<_>>(),
    //     Err(_) => {
    //         panic!("Could not read fb/ directory");
    //     }
    // };

    // if schemas.is_empty() {
    //     println!("cargo:warning=No .fbs files found in fb/");
    //     return;
    // }

    // // Rerun if any schema changes
    // for schema in &schemas {
    //     println!("cargo:rerun-if-changed={}", schema.display());
    // }

    // // Generate Rust and Python code for each schema
    // for schema in &schemas {
    //     let schema_str = schema.to_str().expect("Invalid schema path");
    //     // Rust
    //     let status = Command::new("flatc")
    //         .args(["--rust", "-o", rust_out_dir, schema_str])
    //         .status()
    //         .expect("Failed to run flatc for Rust");
    //     if !status.success() {
    //         panic!("flatc failed for Rust with exit code: {}", status);
    //     }
    //     // Python
    //     let status = Command::new("flatc")
    //         .args([
    //             "--python",
    //             "--gen-onefile",
    //             "-o",
    //             python_out_dir,
    //             schema_str,
    //         ])
    //         .status()
    //         .expect("Failed to run flatc for Python");
    //     if !status.success() {
    //         panic!("flatc failed for Python with exit code: {}", status);
    //     }
    // }
}
