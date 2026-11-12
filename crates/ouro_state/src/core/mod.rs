// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Core configuration specifications shared across daemon and edge.
//!
//! Defines data source, client, server, and executor configuration types
//! that are referenced by both daemon and edge state specifications.

pub mod types;

use std::ops::Deref;

use ouro_common::{
    io::{FromCbor, ToCbor},
    types::SockAddr,
};
use serde::{Deserialize, Serialize};

use crate::edge::patch::EdgeSpecPatch;

/// Retry policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryStrategySpec
where
    Self: Send + Sync,
{
    /// Maximum number of retry attempts.
    pub max_attempts: u32,

    pub retry_timeout_ms: Option<u64>,

    /// Initial backoff duration in milliseconds.
    pub initial_backoff_ms: u64,

    /// Maximum backoff duration in milliseconds.
    pub max_backoff_ms: Option<u64>,

    /// Backoff multiplier (e.g., 2.0 for exponential).
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,
}

fn default_backoff_multiplier() -> f64 {
    2.0
}

/// Data source specification.
///
/// Defines where data is sourced from: SQL databases or network endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataSourceSpec
where
    Self: Send + Sync,
{
    /// SQL database data source.
    SqlDb {
        /// Data source name.
        name: String,
        /// SQL connection configuration.
        config: SqlConnSpec,
        /// Retry strategy for connection attempts.
        retry: RetryStrategySpec,
    },
    /// Network-based data source.
    Network {
        /// Data source name.
        name: String,
        /// Network endpoint configuration.
        config: NetworkEndpointSpec,
        /// Retry strategy for connection attempts.
        retry: RetryStrategySpec,
    },
}

/// Data source specification.
///
/// Defines where data is sourced from: SQL databases or network endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataSourceRef
where
    Self: Send + Sync,
{
    /// Reference to a SQL database data source.
    /// The string value is the name of the data source to reference.
    /// This allows for referencing data sources defined elsewhere in the configuration.
    /// For example, a data source defined in a global section of the configuration can be referenced by name here.
    /// This promotes reuse and modularity in the configuration, allowing multiple components to reference the same data source without duplication.
    /// The referenced data source must be defined in the configuration for this reference to be valid.
    SqlDb {
        r#ref: String,
        #[serde(default)]
        init_sql: Vec<String>,
    },
    /// Reference to a network-based data source.
    /// Similar to `SqlDbRef`, this allows referencing a network data source defined elsewhere in
    ///    the configuration by name. The `r#ref` field contains the name of the data source to reference.  
    /// This promotes reuse and modularity in the configuration, allowing multiple components to reference the same data source without duplication.
    /// The referenced data source must be defined in the configuration for this reference to be valid.
    Network { r#ref: String },
}

/// SQL database connection specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SqlConnSpec
where
    Self: Send + Sync,
{
    /// PostgreSQL connection.
    Postgres {
        /// PostgreSQL connection URL.
        database_url: String,
        /// Maximum number of connections in the pool.
        #[serde(default = "default_max_connections")]
        max_connections: u32,
        /// SQL statements to execute on initialization.
        #[serde(default)]
        init_sql: Vec<String>,
    },
    /// SQLite connection.
    Sqlite {
        /// SQLite database file URL.
        database_url: String,
        /// SQL statements to execute on initialization.
        #[serde(default)]
        init_sql: Vec<String>,
    },
    /// Temporary file-backed SQLite connection.
    ///
    /// The database is stored in an OS-managed temporary file. The file is
    /// created automatically at runtime and deleted when the process exits
    /// (i.e., when the owning [`BuiltDataSource`] is dropped).
    /// No `database_url` is required — the path is generated at build time.
    SqliteTempFile {
        /// SQL statements to execute on initialization.
        #[serde(default)]
        init_sql: Vec<String>,
    },
}

fn default_max_connections() -> u32 {
    10
}

/// Network endpoint specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetworkEndpointSpec
where
    Self: Send + Sync,
{
    /// Zenoh pub/sub endpoint.
    Zenoh {
        /// List of Zenoh router endpoints.
        endpoints: Vec<SockAddr>,
    },
    /// HTTP endpoint.
    Http {
        /// HTTP URL.
        url: String,
    },
    /// MQTT endpoint.
    Mqtt {
        /// MQTT broker address.
        broker_address: String,
        /// MQTT topic.
        topic: String,
    },
}

/// Zenoh communication mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ZenohMode {
    /// Publish messages.
    Publisher,
    /// Subscribe to messages.
    Subscriber,
    /// Query for data.
    Querier,
    /// Respond to queries.
    Queryable,
}

/// Server endpoint specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEndpointSpec
where
    Self: Send + Sync,
{
    /// Zenoh server endpoint.
    Zenoh { endpoints: Vec<String> },
    /// HTTP server endpoint.
    Http { bind_addrs: Vec<String> },
    /// MQTT server endpoint.
    Mqtt { bind_addrs: Vec<String> },
}

/// Client specification for network communication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientSpec
where
    Self: Send + Sync,
{
    /// Client name.
    pub name: String,
    /// Network endpoint configuration.
    pub endpoint: NetworkEndpointSpec,
    /// Optional retry strategy.
    pub retry: Option<RetryStrategySpec>,
}

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ServerSpec
where
    Self: Send + Sync,
{
    /// Server name.
    pub name: String,

    /// Endpoint configuration.
    pub endpoint: Vec<ServerEndpointSpec>,
}

/// Executor configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutorSpec
where
    Self: Send + Sync,
{
    /// Executor name.
    pub name: String,

    /// Maximum worker threads.
    #[serde(default = "default_max_workers")]
    pub max_workers: usize,

    /// Thread name prefix.
    #[serde(default = "default_thread_name_prefix")]
    pub thread_name_prefix: String,

    /// Idle worker check interval in seconds.
    #[serde(default = "default_idle_worker_check_interval")]
    pub idle_worker_check_interval_secs: u64,

    /// Idle worker eviction time in seconds.
    #[serde(default = "default_idle_worker_eviction_time")]
    pub idle_worker_eviction_time_secs: u64,

    /// Idle queue check interval in seconds.
    #[serde(default = "default_idle_queue_check_interval")]
    pub idle_queue_check_interval_secs: u64,

    /// Idle queue eviction time in seconds.
    #[serde(default = "default_idle_queue_eviction_time")]
    pub idle_queue_eviction_time_secs: u64,
}

fn default_thread_name_prefix() -> String {
    "worker_".to_string()
}

fn default_max_workers() -> usize {
    std::cmp::min(32, num_cpus::get())
}

fn default_idle_worker_check_interval() -> u64 {
    20
}

fn default_idle_worker_eviction_time() -> u64 {
    60
}

fn default_idle_queue_check_interval() -> u64 {
    20
}

fn default_idle_queue_eviction_time() -> u64 {
    60
}

/// Returns the default `enabled` value (`true`).
pub fn default_enabled() -> bool {
    true
}

/// Returns the default heartbeat timeout in seconds (60).
pub fn default_heartbeat_timeout_secs() -> u64 {
    60
}

/// Generates a UUID v7 string.
pub fn uuid_gen() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Data source reference: either an inline definition or a named reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum WithDataSource {
    /// Inline data source definition.
    Def { def: DataSourceSpec },
    /// Named reference to an existing data source.
    Ref { r#ref: DataSourceRef },
}

/// Client reference: either an inline definition or a named reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WithClient {
    /// Inline client definition.
    Def { def: ClientSpec },
    /// Named reference to an existing client.
    Ref { r#ref: String },
}

/// Server reference: either an inline definition or a named reference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WithServer {
    /// Inline server definition.
    Def { def: ServerSpec },
    /// Named reference to an existing server.
    Ref { r#ref: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SchemaVersion(pub u32);

impl std::fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        SchemaVersion(1)
    }
}

impl Deref for SchemaVersion {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<SchemaVersion> for u32 {
    fn from(val: SchemaVersion) -> u32 {
        val.0
    }
}
impl From<u32> for SchemaVersion {
    fn from(value: u32) -> Self {
        SchemaVersion(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct StateEpoch(pub u64);

impl Default for StateEpoch {
    fn default() -> Self {
        StateEpoch(1)
    }
}

impl Deref for StateEpoch {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<StateEpoch> for u64 {
    fn from(val: StateEpoch) -> u64 {
        val.0
    }
}
impl From<u64> for StateEpoch {
    fn from(value: u64) -> Self {
        StateEpoch(value)
    }
}
impl std::fmt::Display for StateEpoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The current schema version for edge state reports.
/// Edge reports with an older version trigger no tag resync (future: trigger OTA/firmware update).
pub const CURRENT_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub struct StateMetadata {
    /// Schema version for this state. Used for compatibility checks and migrations.
    pub schema_version: SchemaVersion,
    /// State epoch
    pub epoch: TagAssignment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    #[serde(default = "uuid_gen")]
    pub id: String,

    #[serde(default)]
    pub epoch: StateEpoch,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<EdgeSpecPatch>,
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.epoch == other.epoch
    }
}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(
            self.priority
                .unwrap_or(0)
                .cmp(&other.priority.unwrap_or(0))
                .then_with(|| self.id.cmp(&other.id)),
        )
    }
}

impl std::hash::Hash for Tag {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.epoch.hash(state);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct TagAssignment(pub Vec<Tag>);

impl From<Vec<Tag>> for TagAssignment {
    fn from(tags: Vec<Tag>) -> Self {
        TagAssignment(tags)
    }
}

impl From<Option<Vec<Tag>>> for TagAssignment {
    fn from(tags: Option<Vec<Tag>>) -> Self {
        TagAssignment::from(tags.unwrap_or_default())
    }
}

impl ToCbor for TagAssignment {}
impl FromCbor for TagAssignment {}

impl std::ops::Deref for TagAssignment {
    type Target = Vec<Tag>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for TagAssignment {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TagAssignment {
    /// Return a copy with all tags stripped of display_name, priority, and patch.
    ///
    /// Used when building StateReport to minimize payload size.
    pub fn stripped(&self) -> Self {
        TagAssignment(self.iter().map(|t| t.stripped()).collect())
    }
}

impl Tag {
    /// Return a copy with display_name, priority, and patch set to None.
    pub fn stripped(&self) -> Self {
        Tag {
            id: self.id.clone(),
            epoch: self.epoch,
            display_name: None,
            priority: None,
            patch: None,
        }
    }
}

impl ToCbor for Tag {}
impl FromCbor for Tag {}
