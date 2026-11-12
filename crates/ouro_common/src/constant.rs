// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

/// Constants for daemon operations.
pub mod daemon {
    /// Metrics key prefix for heartbeat-related metrics.
    pub const PREFIX_METRICS_HEARTBEAT: &str = "heartbeat.";
    /// Metrics key prefix for metadata-related metrics.
    pub const PREFIX_METRICS_METADATA: &str = "metadata.";
}

/// Constants shared across all Ouroboros crates.
pub mod core {
    /// Default buffer size for `tokio::sync::mpsc` channels.
    pub const MPSC_BUFSIZ: usize = 16;
    /// Environment variable that, when set to `"1"` or `"true"`, causes the
    /// process to panic on any `InternalError` instead of returning it.
    pub const ENV_PANIC_ON_ERROR: &str = "OURO_PANIC_ON_ERROR";
    /// Size of `usize` in bytes on the target platform, used for hex address formatting.
    pub const SIZE_OF_USIZE: usize = std::mem::size_of::<usize>();
}

/// Default Unix domain socket path for the daemon CLI endpoint.
pub const CLI_UNIX_DOMAIN_SOCKET_PATH_DEFAULT: &str = "/tmp/ourodaemon.sock";
/// Default TCP address for the daemon CLI endpoint.
pub const CLI_TCP_SOCKET_ADDR_DEFAULT: &str = "127.0.0.1:9251";
