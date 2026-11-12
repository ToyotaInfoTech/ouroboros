// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::schema::shadow::report::{
    CpuInfo, EdgeStateReport, EdgeStatus, LoadAverage, SystemInfo, TaskState,
};
use ouro_common::{
    io::{FromCbor, ToCbor},
    unix_epoch_now,
};
use ouro_state::edge::EdgeSpec;
use sysinfo::System;

impl FromCbor for EdgeStateReport {}
impl ToCbor for EdgeStateReport {}

impl From<&System> for SystemInfo {
    /// Capture a serializable snapshot from a `sysinfo::System` reference.
    ///
    /// The caller is responsible for calling `System::refresh_all()` (or appropriate
    /// refresh methods) before converting so that the data is up-to-date.
    fn from(sys: &System) -> Self {
        let cpus = sys
            .cpus()
            .iter()
            .map(|c| CpuInfo {
                name: c.name().to_owned(),
                usage: c.cpu_usage(),
                frequency_mhz: c.frequency(),
                brand: c.brand().to_owned(),
                vendor_id: c.vendor_id().to_owned(),
            })
            .collect();

        let load_average = System::load_average();
        Self {
            global_cpu_usage: sys.global_cpu_usage(),
            cpus,
            total_memory: sys.total_memory(),
            used_memory: sys.used_memory(),
            available_memory: sys.available_memory(),
            total_swap: sys.total_swap(),
            used_swap: sys.used_swap(),
            uptime: System::uptime(),
            load_average: LoadAverage {
                one: load_average.one,
                five: load_average.five,
                fifteen: load_average.fifteen,
            },
            os_name: System::name(),
            kernel_version: System::kernel_version(),
            os_version: System::os_version(),
            host_name: System::host_name(),
        }
    }
}

impl EdgeStateReport {
    /// Create a new state report with default values and current timestamp.
    /// The caller can then populate the `state` field with the actual edge state before publishing.
    /// This is a convenience method to create a report with the necessary metadata fields initialized.
    /// # Arguments
    /// * `edge_id` - Unique edge device identifier
    /// * `schema_version` - Schema version number
    /// * `epoch` - Current epoch
    /// * `status` - Current operational status of the edge device
    /// * `system_info` - Snapshot of system information at the time of report creation
    ///
    /// # Returns
    /// A new `EdgeStateReport` instance with default state and current timestamp
    /// The caller is expected to fill in the `state` field before publishing.
    /// This method is useful for quickly creating a report with the necessary metadata, while allowing the caller to set the actual state details separately.
    /// Example usage:
    /// ```ignore
    /// let mut report = EdgeStateReport::new("edge-123", SchemaVersion(1),
    ///     StateEpoch(100), EdgeStatus::Up, SystemInfo::from(&sys), vec![]);
    /// report.state = Some(EdgeState { ... }); // Fill in the actual state details
    /// ```
    pub fn new(
        state: &EdgeSpec,
        status: EdgeStatus,
        system_info: SystemInfo,
        task_states: Vec<TaskState>,
    ) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self {
            edge_id: state.runtime.edge_id.clone(),
            place_id: state.runtime.place_id.clone(),
            current_state: state.metadata.clone(),
            desired_state: None,
            timestamp: unix_epoch_now(),
            status,
            system_info,
            task_states,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ouro_common::io::{FromCbor, ToCbor};
    use sysinfo::System;

    impl ToCbor for SystemInfo {}
    impl FromCbor for SystemInfo {}

    /// Convert `System` to `SystemInfo`, print debug output, round-trip via JSON/CBOR,
    /// and verify equality between the original and restored values.
    #[test]
    fn systeminfo_json_cbor_roundtrip_matches() {
        let mut sys = System::new_all();
        sys.refresh_all();

        let info_a = SystemInfo::from(&sys);

        let json = serde_json::to_string(&info_a).expect("serialize SystemInfo to JSON");
        let json_pretty =
            serde_json::to_string_pretty(&info_a).expect("serialize SystemInfo to pretty JSON");

        // Debug output (pretty JSON).
        eprintln!("SystemInfo A (pretty JSON):\n{}", json_pretty);

        let cbor = info_a.to_cbor().expect("serialize SystemInfo to CBOR");
        let info_b = SystemInfo::from_cbor(&cbor).expect("deserialize SystemInfo from CBOR");

        assert!(!json.is_empty(), "JSON output should not be empty");
        assert_eq!(info_a, info_b, "SystemInfo should round-trip via CBOR");
    }
}
