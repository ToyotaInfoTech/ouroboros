// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

/// Device Shadow: combines desired config and reported state.
use super::*;
use std::time::Instant;

/// Device Shadow for an edge device.
///
/// Maintains both desired configuration (from daemon) and reported state (from edge).
/// Similar to AWS IoT Device Shadow concept.
#[derive(Debug, Clone)]
pub struct EdgeShadow {
    /// Edge device identifier.
    pub edge_id: String,

    /// Reported state (edge → daemon).
    pub last_report: Option<EdgeStateReport>,

    /// Desired configuration (daemon → edge).
    pub desired_state: Option<EdgeStateMessage>,

    /// Last time we received any communication from this edge.
    pub last_seen: Instant,
}

impl EdgeShadow {
    /// Create a new device shadow.
    ///
    /// # Arguments
    /// * `edge_id` - Unique edge device identifier
    /// * `last_report` - Last reported state from edge
    ///
    /// # Returns
    /// * `EdgeShadow` - New device shadow
    pub fn new(edge_id: &str, last_report: Option<EdgeStateReport>) -> Self {
        Self {
            edge_id: edge_id.to_string(),
            last_report,
            desired_state: None,
            last_seen: Instant::now(),
        }
    }

    /// Update the reported state from edge.
    ///
    /// # Arguments
    /// * `report` - New reported state
    pub fn update_reported(&mut self, report: EdgeStateReport) {
        self.last_report = Some(report);
        self.last_seen = Instant::now();
    }

    /// Update the desired configuration.
    ///
    /// # Arguments
    /// * `desired_state` - New desired configuration
    pub fn update_desired(&mut self, desired_state: EdgeStateMessage) {
        self.desired_state = Some(desired_state);
    }

    /// Check if the edge is synchronized (reported.config_version == desired.version).
    ///
    /// # Returns
    /// * `bool` - True if synchronized
    pub fn is_synced(&self) -> bool {
        // Sync check is not yet implemented; conservatively report as not synced.
        false
    }

    /// Get time elapsed since last communication from edge.
    ///
    /// # Returns
    /// * `Duration` - Time since last_seen
    pub fn time_since_last_seen(&self) -> std::time::Duration {
        self.last_seen.elapsed()
    }

    /// Check if edge is considered offline (no communication for timeout period).
    ///
    /// # Arguments
    /// * `timeout` - Timeout duration
    ///
    /// # Returns
    /// * `bool` - True if offline
    pub fn is_offline(&self, timeout: std::time::Duration) -> bool {
        self.time_since_last_seen() > timeout
    }

    /// Touch the last_seen timestamp (e.g., from heartbeat).
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
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
