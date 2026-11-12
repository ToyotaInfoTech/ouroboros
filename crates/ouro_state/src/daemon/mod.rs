// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Daemon configuration specifications.
//!
//! Defines the TOML-deserializable configuration types for the daemon process,
//! including data sources, repositories, clients, servers, tasks, and metrics.

pub mod types;

use crate::core::types::CompState;
use crate::core::{default_enabled, default_heartbeat_timeout_secs, uuid_gen, StateMetadata};
use crate::core::{ClientSpec, DataSourceSpec, ExecutorSpec, ServerSpec};
use crate::core::{WithClient, WithDataSource, WithServer};
use crate::{
    deserialize_client_vec_to_map, deserialize_daemon_task_vec_to_map,
    deserialize_data_source_vec_to_map, deserialize_executor_vec_to_map,
    deserialize_repository_vec_to_map, deserialize_server_vec_to_map,
};
use ouro_common::io::FromTomlFile;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use toml;

/// Repository specification for device shadow storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RepositorySpec {
    /// Shadow store repository variant.
    ShadowStore {
        /// Repository name.
        name: String,
        /// Shadow store backend specification.
        shadow_store: ShadowStoreSpec,
    },
    /// Tag store repository variant for configuration tags.
    TagStore {
        /// Repository name.
        name: String,
        /// Tag store backend specification.
        tag_store: TagStoreSpec,
    },
}

impl RepositorySpec {
    /// Returns the repository name.
    pub fn name(&self) -> &str {
        match self {
            RepositorySpec::ShadowStore { name, .. } => name.as_str(),
            RepositorySpec::TagStore { name, .. } => name.as_str(),
        }
    }
}

/// Tag store backend specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TagStoreSpec {
    /// Data source reference for database connection.
    pub with_data_source: WithDataSource,
}

/// Shadow store backend specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShadowStoreSpec {
    /// In-memory transient storage.
    Transient,
    /// Database-backed persistent storage.
    Persistent {
        /// Data source reference for database connection.
        with_data_source: WithDataSource,
    },
    /// Write-behind caching with async persistence.
    WriteBehind {
        /// Data source reference for database connection.
        with_data_source: WithDataSource,
    },
}

/// Root TOML configuration structure.
///
/// Top-level structure matching the TOML file format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonSpec {
    pub metadata: StateMetadata,

    /// Runtime configuration.
    pub runtime: RuntimeSpec,

    pub metrics_store: MetricsStore,

    pub cli_endpoint: Option<CliEndpoint>,

    /// Data source configurations.
    #[serde(default, deserialize_with = "deserialize_data_source_vec_to_map")]
    pub data_source: HashMap<String, DataSourceSpec>,

    /// Repository source configurations.
    #[serde(default, deserialize_with = "deserialize_repository_vec_to_map")]
    pub repository: HashMap<String, RepositorySpec>,

    /// Client configurations.
    #[serde(default, deserialize_with = "deserialize_client_vec_to_map")]
    pub client: HashMap<String, ClientSpec>,

    /// Server configurations.
    #[serde(default, deserialize_with = "deserialize_server_vec_to_map")]
    pub server: HashMap<String, ServerSpec>,

    /// Executor configurations.
    #[serde(default, deserialize_with = "deserialize_executor_vec_to_map")]
    pub executor: HashMap<String, ExecutorSpec>,

    /// Task configurations.
    #[serde(default, deserialize_with = "deserialize_daemon_task_vec_to_map")]
    pub task: HashMap<String, DaemonTaskSpec>,
}

impl CompState for DaemonSpec {
    fn is_enabled(&self) -> bool {
        true
    }
}

impl FromTomlFile for DaemonSpec {}

/// Runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeSpec {
    /// Unique daemon identifier.
    pub daemon_id: String,
    /// Runtime version string.
    pub version: String,
}

/// Task configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonTaskSpec {
    DataIngestion {
        /// Task name.
        name: String,

        /// Whether task is enabled.
        #[serde(default = "default_enabled")]
        enabled: bool,

        with_client: WithClient,

        /// Task data configuration.
        data: IngestionDataSpec,

        /// Task behavior configuration.
        behavior: BehaviorSpec,

        with_data_source: WithDataSource,

        attribute: Option<toml::Value>,
    },
    RepositoryIngestion {
        /// Task name.
        name: String,

        /// Whether task is enabled.
        #[serde(default = "default_enabled")]
        enabled: bool,

        with_client: WithClient,

        with_repository: WithRepository,

        with_tag_repository: WithRepository,

        /// Task data configuration.
        repository_data: RepositoryDataSpec,

        /// Task behavior configuration.
        behavior: BehaviorSpec,

        /// Seconds after which an unresponsive edge is marked as Down.
        /// Defaults to 120 seconds.
        #[serde(default = "default_heartbeat_timeout_secs")]
        heartbeat_timeout_secs: u64,

        attribute: Option<toml::Value>,
    },
    Metrics {
        name: String,
        enabled: bool,
        store: MetricsStore,
        with_server: WithServer,
    },
    /// Tag-based configuration management task.
    TagManagement {
        /// Task name.
        name: String,

        /// Whether task is enabled.
        #[serde(default = "default_enabled")]
        enabled: bool,

        /// Client for Zenoh pub/sub.
        with_client: WithClient,

        /// Tag store repository reference.
        with_tag_repository: WithRepository,

        /// Shadow store repository reference (for resolving edge lists).
        with_shadow_repository: WithRepository,

        /// Path to the default edge state TOML file.
        default_edge_state_path: std::path::PathBuf,

        /// Tags to automatically register into the tag store on daemon startup.
        #[serde(default)]
        predefined_tag: Vec<PredefinedTagSpec>,

        /// Task behavior configuration.
        behavior: BehaviorSpec,
    },
    /// WebUI dashboard server task.
    WebUi {
        /// Task name.
        name: String,

        /// Whether task is enabled.
        #[serde(default = "default_enabled")]
        enabled: bool,

        /// HTTP server configuration.
        with_server: WithServer,

        /// Optional data source for database access (e.g. metadata queries).
        with_data_source: Option<WithDataSource>,

        /// Bearer token required for `/api/v1`; empty or unset disables authentication.
        #[serde(default)]
        auth_token: Option<String>,
    },
}

impl CompState for DaemonTaskSpec {
    fn is_enabled(&self) -> bool {
        match self {
            DaemonTaskSpec::DataIngestion { enabled, .. } => *enabled,
            DaemonTaskSpec::RepositoryIngestion { enabled, .. } => *enabled,
            DaemonTaskSpec::Metrics { enabled, .. } => *enabled,
            DaemonTaskSpec::TagManagement { enabled, .. } => *enabled,
            DaemonTaskSpec::WebUi { enabled, .. } => *enabled,
        }
    }
}

/// Repository definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WithRepository {
    Def { def: RepositorySpec },
    Ref { r#ref: String },
}

/// Ingestion data type for data ingestion tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IngestionDataSpec {
    /// Heartbeat liveness data.
    Heartbeat {
        /// Shadow store repository for auto-registering edges on heartbeat.
        with_repository: WithRepository,
    },
    /// Trajectory telemetry data.
    Trajectory,
}

/// Repository data type for repository ingestion tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RepositoryDataSpec {
    /// Device shadow state data.
    State,
}

/// Task behavior configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BehaviorSpec {
    /// Maximum continuous errors before task dies.
    pub max_continuous_errors: u32,
}

/// A predefined configuration tag to be automatically registered on daemon startup.
///
/// Each entry is loaded from a TOML file and upserted into the tag store
/// before any publish occurs, so edges immediately receive the correct config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PredefinedTagSpec {
    #[serde(default = "uuid_gen")]
    pub id: String,

    pub display_name: String,

    /// Merge priority.  Lower value = applied first.
    pub priority: u32,

    /// Path to a TOML file whose contents are deserialized as `EdgeSpecPatch`.
    pub path: std::path::PathBuf,
}

/// CLI RPC endpoint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliEndpoint {
    UnixDomainSocket { path: Option<String> },
    TcpSocket { addr: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_parse() {
        let toml_str = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../etc/daemon_config_local.toml"
        ))
        .expect("failed to read daemon_config_local.toml");
        let config: DaemonSpec = toml::from_str(&toml_str)
            .unwrap_or_else(|e| panic!("Failed to parse TOML config: {}", e));
        assert_eq!(config.runtime.daemon_id, "center-daemon");
    }
}

/// Metrics store configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MetricsStore {
    /// In-memory metrics storage.
    InMemory {
        /// Maximum number of data points retained per metric.
        max_points_per_metric: Option<usize>,
    },
}
