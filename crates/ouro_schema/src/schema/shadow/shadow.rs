// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use ouro_common::types::UnixTimestamp;
use ouro_state::edge::EdgeSpec;
use serde::{Deserialize, Serialize};
use tracing::instrument;

/// Device Shadow: combines desired config and reported state.
use super::*;

/// Device Shadow for an edge device.
///
/// Maintains both desired configuration (from daemon) and reported state (from edge).
/// Similar to AWS IoT Device Shadow concept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeShadow {
    /// Edge device identifier.
    pub edge_id: String,

    /// Reported state (edge → daemon).
    pub last_report: Option<EdgeStateReport>,

    /// Desired configuration (daemon → edge).
    pub desired_state: Option<EdgeSpec>,

    /// Last time we received any communication from this edge.
    pub last_seen: UnixTimestamp,
}

impl EdgeShadow {
    /// Create a new device shadow.
    ///
    /// # Arguments
    /// * `edge_id` - Unique edge device identifier
    ///
    /// # Returns
    /// * `EdgeShadow` - New device shadow
    #[instrument(skip(edge_id))]
    pub fn new(edge_id: &str) -> Self {
        Self {
            edge_id: edge_id.into(),
            last_report: None,
            desired_state: None,
            last_seen: UnixTimestamp::now(),
        }
    }

    /// Update the reported state from edge.
    ///
    /// # Arguments
    /// * `report` - New reported state
    #[instrument(skip(self, report))]
    pub fn update_reported(&mut self, report: EdgeStateReport) {
        self.last_report = Some(report);
        self.last_seen = UnixTimestamp::now();
    }

    /// Update the desired configuration.
    ///
    /// # Arguments
    /// * `desired_state` - New desired configuration
    #[instrument(skip(self, desired_state))]
    pub fn update_desired(&mut self, desired_state: EdgeSpec) {
        self.desired_state = Some(desired_state);
    }

    /// Check if the edge is synchronized (reported.config_version == desired.version).
    ///
    /// # Returns
    /// * `bool` - True if synchronized
    #[instrument(skip(self))]
    pub fn is_synced(&self) -> bool {
        // Sync check is not yet implemented; conservatively report as not synced.
        false
    }

    /// Get time elapsed since last communication from edge.
    ///
    /// # Returns
    /// * `Duration` - Time since last_seen
    #[instrument(skip(self))]
    pub fn time_since_last_seen(&self) -> std::time::Duration {
        std::time::Duration::from_millis(UnixTimestamp::now().0 as u64 - self.last_seen.0 as u64)
    }

    /// Check if edge is considered offline (no communication for timeout period).
    ///
    /// # Arguments
    /// * `timeout` - Timeout duration
    ///
    /// # Returns
    /// * `bool` - True if offline
    #[instrument(skip(self, timeout))]
    pub fn is_offline(&self, timeout: std::time::Duration) -> bool {
        self.time_since_last_seen() > timeout
    }

    /// Touch the last_seen timestamp (e.g., from heartbeat).
    #[instrument(skip(self))]
    pub fn touch(&mut self) {
        self.last_seen = UnixTimestamp::now();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_shadow_sync_status() {
        // TODO implement
    }

    #[test]
    fn test_offline_detection() {
        // TODO implement
    }

    #[test]
    fn test_touch_updates_last_seen() {
        // TODO implement
    }
}
