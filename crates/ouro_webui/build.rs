// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

// Build script for ouro_webui.
//
// Runs `npm run build` in the webui directory when source files change,
// producing the `webui/dist` directory that rust-embed embeds into the binary.
// If Node.js / npm is not installed, the build is silently skipped so that
// a stale (or missing) dist directory still allows `cargo build` to succeed.

use std::path::Path;
use std::process::Command;

fn main() {
    let webui_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../webui");

    // Re-run this build script when any webui source file changes.
    print_rerun_if_changed_recursive(&webui_dir.join("src"));

    // The map tile settings are baked into the bundle at build time.
    println!("cargo:rerun-if-env-changed=VITE_MAP_TILE_URL");
    println!("cargo:rerun-if-env-changed=VITE_MAP_ATTRIBUTION");

    // Skip webui build if npm is not available.
    let npm = if cfg!(target_os = "windows") {
        "npm.cmd"
    } else {
        "npm"
    };

    if Command::new(npm).arg("--version").output().is_err() {
        println!("cargo:warning=npm not found; skipping webui build");
        return;
    }

    // Ensure node_modules exist.
    if !webui_dir.join("node_modules").exists() {
        println!("cargo:warning=Running npm install in webui/");
        let status = Command::new(npm)
            .arg("install")
            .current_dir(&webui_dir)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                println!(
                    "cargo:warning=npm install failed (exit {}); skipping webui build",
                    s
                );
                return;
            }
            Err(e) => {
                println!(
                    "cargo:warning=npm install error: {}; skipping webui build",
                    e
                );
                return;
            }
        }
    }

    // Run the production build.
    let status = Command::new(npm)
        .args(["run", "build"])
        .current_dir(&webui_dir)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            println!("cargo:warning=npm run build failed (exit {})", s);
        }
        Err(e) => {
            println!("cargo:warning=npm run build error: {}", e);
        }
    }
}

fn print_rerun_if_changed_recursive(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                print_rerun_if_changed_recursive(&path);
            } else {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
