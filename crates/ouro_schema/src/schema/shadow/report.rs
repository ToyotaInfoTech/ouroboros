// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use ouro_common::types::UnixTimestamp;
use ouro_state::core::StateMetadata;
use serde::{Deserialize, Serialize};

/// Edge device status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum EdgeStatus {
    /// Edge is running normally.
    Up,
    /// Edge encountered an error.
    Error,
    /// Edge is starting up.
    Starting,
    /// Edge is stopping.
    Stopping,
    /// Edge is stopped.
    Down,
    /// Edge is suspended.
    Suspended,
}

/// Reported state from an edge device.
///
/// This represents the current state of the edge device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeStateReport {
    pub current_state: StateMetadata,

    pub desired_state: Option<StateMetadata>,

    /// Edge device identifier.
    pub edge_id: String,

    /// Edge device location.
    #[serde(default)]
    pub place_id: Option<String>,

    /// Timestamp when this report was generated (Unix milliseconds).
    pub timestamp: UnixTimestamp,

    /// Current operational status.
    pub status: EdgeStatus,

    /// System resource snapshot captured when this report was created.
    pub system_info: SystemInfo,

    #[serde(default)]
    pub task_states: Vec<TaskState>,
}

/// Per-CPU core information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuInfo {
    /// CPU name (e.g. "cpu0").
    pub name: String,
    /// CPU usage in percent (0.0–100.0).
    pub usage: f32,
    /// CPU frequency in MHz.
    pub frequency_mhz: u64,
    /// CPU brand string (e.g. "Intel(R) Core(TM) i7-...").
    pub brand: String,
    /// CPU vendor ID.
    pub vendor_id: String,
}

/// System load averages (1-min / 5-min / 15-min).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Serializable snapshot of `sysinfo::System`.
///
/// `sysinfo::System` does not implement `serde::Serialize`/`Deserialize` natively,
/// so this type captures the relevant fields and can be converted with `From<&System>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemInfo {
    /// Global CPU usage in percent.
    pub global_cpu_usage: f32,
    /// Per-core CPU information.
    pub cpus: Vec<CpuInfo>,

    /// Total physical memory in bytes.
    pub total_memory: u64,
    /// Used physical memory in bytes.
    pub used_memory: u64,
    /// Available physical memory in bytes.
    pub available_memory: u64,

    /// Total swap space in bytes.
    pub total_swap: u64,
    /// Used swap space in bytes.
    pub used_swap: u64,

    /// System uptime in seconds.
    pub uptime: u64,
    /// Load averages.
    pub load_average: LoadAverage,

    // OS metadata
    pub os_name: Option<String>,
    pub kernel_version: Option<String>,
    pub os_version: Option<String>,
    pub host_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskState {
    /// Unique identifier for the task.
    pub name: String,

    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Created,
    Active(UnixTimestamp),
    Inactive(UnixTimestamp),
    Discarded,
}
