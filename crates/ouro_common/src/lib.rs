// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Common utilities and shared types for the Ouroboros system.
//!
//! This crate provides foundational types, serialization helpers, topic-key utilities,
//! and diagnostics used across all Ouroboros crates.
//!
//! # Modules
//!
//! - [`constant`]: System-wide constant definitions
//! - [`io`]: CBOR / TOML serialization and deserialization traits
//! - [`topic`]: Zenoh key-space definitions and key generation for Device Shadow operations
//! - [`types`]: Shared newtype wrappers (`SockAddr`, `Payload`, `Topic`, `UnixTimestamp`, etc.)
//! - [`log_lock`]: Lock-event tracing helpers for debugging contention

pub mod constant;
pub mod io;
pub mod log_lock;
pub mod suggest;
pub mod topic;
pub mod types;

use std::backtrace;

use crate::types::UnixTimestamp;

#[allow(unused_macros)] // Used conditionally in debug builds
#[cfg(debug_assertions)]
macro_rules! debug {
    ($x:expr) => {
        dbg!($x)
    };
}

#[allow(unused_macros)] // Used conditionally in release builds
#[cfg(not(debug_assertions))]
macro_rules! debug {
    ($x:expr) => {
        std::convert::identity($x)
    };
}

/// Logs all environment variables at debug level.
///
/// This macro checks if debug logging is enabled and, if so, collects all environment
/// variables and logs them. This is useful for debugging configuration issues or
/// understanding the runtime environment.
///
/// # Examples
///
/// ```rust,no_run
/// use ouro_common::log_env_vars;
///
/// fn main() {
///     log_env_vars!();
/// }
/// ```
///
/// # Notes
///
/// - Only logs when debug level logging is enabled to avoid performance overhead
/// - Environment variables are collected into a HashMap before logging
/// - Each variable is logged on a separate line with the format "  KEY = VALUE"
#[macro_export]
macro_rules! log_env_vars {
    () => {
        if tracing::enabled!(tracing::Level::DEBUG) {
            use std::collections::HashMap;
            let env_vars: HashMap<String, String> = std::env::vars().collect();
            tracing::debug!("Environment variables:");
            for (key, value) in &env_vars {
                tracing::debug!("  {} = {}", key, value);
            }
        }
    };
}

/// Returns the raw memory address of a reference as a `usize`.
#[inline]
pub fn addr<T>(x: &T) -> usize {
    x as *const _ as usize
}

/// Returns the current time as a [`UnixTimestamp`] (milliseconds since the Unix epoch).
pub fn unix_epoch_now() -> UnixTimestamp {
    UnixTimestamp::now()
}

/// Walks the call stack to find the first Ouroboros frame outside the caller's own module.
///
/// Used for diagnostic lock-event logging to identify which component acquired a lock.
pub fn get_caller(my_type_name: &str) -> String {
    let my_path = my_type_name.split("::").collect::<Vec<&str>>();

    let my_mod_name = if my_path.len() >= 3 * 2 {
        my_path[..my_path.len() - 3 * 2].join("::")
    } else {
        return "unknown(mod)".to_string();
    };

    let frames = backtrace::Backtrace::capture()
        .to_string()
        .lines()
        .collect::<Vec<&str>>()
        .chunks(2)
        .map(|chunk| {
            chunk
                .iter()
                .map(|s| (*s).trim())
                .collect::<Vec<&str>>()
                .join(" ")
        })
        .collect::<Vec<String>>();

    let mut found = false;
    for frame in frames {
        let frame = frame.trim();

        if found {
            if !frame.contains(&my_mod_name) && frame.contains("ouro") {
                return frame.to_string();
            }
        } else if frame.contains(&my_mod_name) {
            found = true;
        }
    }
    "unknown(caller)".to_string()
}

/// ANSI SGR color codes for terminal output.
pub enum AnsiColor {
    Red = 31,
    Green = 32,
    Yellow = 33,
    Blue = 34,
    Magenta = 35,
    Cyan = 36,
    White = 37,
    BrightBlack = 90,
    BrightRed = 91,
    BrightGreen = 92,
    BrightYellow = 93,
    BrightBlue = 94,
    BrightMagenta = 95,
    BrightCyan = 96,
    BrightWhite = 97,
}

/// Returns `text` wrapped in the given ANSI SGR code when the environment supports ANSI,
/// otherwise returns `text` unmodified.
/// Uses stderr because tracing-subscriber's fmt layer writes to stderr by default.
#[inline]
pub fn ansi_color(code: AnsiColor, text: &str) -> String {
    format!("\x1b[{}m{}\x1b[0m", code as u8, text)
}
