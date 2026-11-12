// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Hierarchical configuration management for edge and daemon.
//!
//! Provides type-safe, extensible configuration with granular locking
//! for concurrent access across multiple tasks.

pub mod merge;
pub mod patch;
pub mod types;
use std::collections::HashMap;

use crate::core::types::CompState;
use crate::core::uuid_gen;
use crate::core::{default_enabled, StateMetadata, WithClient, WithDataSource};
use crate::core::{ClientSpec, DataSourceSpec};
use crate::{
    deserialize_client_vec_to_map, deserialize_data_source_vec_to_map,
    deserialize_edge_task_vec_to_map, deserialize_trigger_vec_to_map,
};
use ouro_common::io::{FromCbor, FromTomlFile, ToCbor};
use serde::{Deserialize, Serialize};

/// Root configuration container for an edge device.
///
/// Organizes configuration into logical domains (network, runtime, tasks)
/// with independent locking for each domain to minimize contention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeSpec
where
    Self: Send + Sync,
{
    pub metadata: StateMetadata,

    /// Network and communication settings.
    pub runtime: RuntimeSpec,

    #[serde(default, deserialize_with = "deserialize_data_source_vec_to_map")]
    pub data_source: HashMap<String, DataSourceSpec>,

    #[serde(default, deserialize_with = "deserialize_client_vec_to_map")]
    pub client: HashMap<String, ClientSpec>,

    #[serde(default)]
    pub development: HashMap<String, toml::Value>,

    /// Task-specific configurations.
    #[serde(default, deserialize_with = "deserialize_edge_task_vec_to_map")]
    pub task: HashMap<String, EdgeTaskSpec>,
}

impl CompState for EdgeSpec {
    fn is_enabled(&self) -> bool {
        true
    }
}

impl FromTomlFile for EdgeSpec {}
impl FromCbor for EdgeSpec {}
impl ToCbor for EdgeSpec {}

/// Network and communication configuration for edge tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum EdgeTaskSpec
where
    Self: Send + Sync,
{
    /// Data upload task from edge to daemon.
    DataUpload {
        /// Task name.
        name: String,
        /// Whether the task is enabled.
        #[serde(default = "default_enabled")]
        enabled: bool,
        /// Task behavior configuration.
        behavior: BehaviorSpec,
        /// Client to upload through.
        with_client: WithClient,
        /// Type of data to upload.
        data: UploadDataSpec,
    },
    CommandWatch {
        /// Task name.
        name: String,

        /// Client to listen through.
        with_client: WithClient,
    },
    /// Tag-based configuration watch task.
    TagWatch {
        /// Task name.
        name: String,

        /// Client to subscribe through.
        with_client: WithClient,
    },
}

impl CompState for EdgeTaskSpec {
    fn is_enabled(&self) -> bool {
        match self {
            EdgeTaskSpec::DataUpload { enabled, .. } => *enabled,
            EdgeTaskSpec::CommandWatch { .. } => true,
            EdgeTaskSpec::TagWatch { .. } => true,
        }
    }
}

/// Behavior configuration for edge tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BehaviorSpec {
    /// Maximum consecutive errors before the task is terminated.
    pub max_continuous_errors: Option<u32>,

    /// Trigger strategies controlling when the task executes.
    #[serde(default, deserialize_with = "deserialize_trigger_vec_to_map")]
    pub trigger: HashMap<String, TriggerStrategySpec>,
}

/// Runtime configuration for the edge device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeSpec {
    /// Edge device unique identifier.
    #[serde(default = "uuid_gen")]
    pub edge_id: String,

    /// Device model identifier.
    pub model: String,

    /// Runtime version string.
    pub version: String,

    /// Reference to the system state data source.
    pub with_data_source: WithDataSource,

    /// Edge device location.
    #[serde(default)]
    pub place_id: Option<String>,
}

/// Data upload type specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum UploadDataSpec {
    /// Heartbeat liveness data.
    Heartbeat,
    /// Device state report.
    State,
    /// Trajectory telemetry data.
    Trajectory {
        /// Data source for trajectory and metadata tables (`system_state`, `metadata`).
        with_data_source: WithDataSource,
        /// Optional separate data source for camera image storage.
        ///
        /// When present, camera frames collected during trajectory processing are
        /// persisted to this data source via `INSERT INTO camera_image`. The data
        /// source should be a `sqlite_temp_file` instance so that the storage is
        /// automatically reclaimed on process exit.
        /// When absent, camera images are embedded in CBOR only and not inserted
        /// into a separate table.
        #[serde(default)]
        with_camera_data_source: Option<WithDataSource>,
    },
}

/// Trigger strategy for controlling task execution timing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerStrategySpec {
    /// Time-based periodic trigger.
    Periodic {
        #[serde(default = "uuid_gen")]
        uuid: String,
        /// Enable/disable metadata publishing.
        #[serde(default = "default_enabled")]
        enabled: bool,

        /// Publishing interval in seconds.
        interval_secs: u64,
    },

    BulkBytes {
        #[serde(default = "uuid_gen")]
        uuid: String,
        /// Enable/disable metadata publishing.
        #[serde(default = "default_enabled")]
        enabled: bool,

        /// Publishing interval in seconds.
        bulk_bytes: u64,

        /// Publishing interval in seconds.
        max_delay_secs: i64,
    },

    BulkCounts {
        #[serde(default = "uuid_gen")]
        uuid: String,
        /// Enable/disable metadata publishing.
        #[serde(default = "default_enabled")]
        enabled: bool,

        /// Publishing interval in seconds.
        max_counts: u64,
    },

    Instant {
        #[serde(default = "uuid_gen")]
        uuid: String,
        /// Enable/disable metadata publishing.
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_parse() {
        let toml_str = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../etc/edge_config_base_local.toml"
        ))
        .expect("failed to read edge_config_base_local.toml");
        let config: EdgeSpec = toml::from_str(&toml_str)
            .unwrap_or_else(|e| panic!("Failed to parse TOML config: {}", e));
        assert_eq!(config.runtime.edge_id, "auto");
    }
}
