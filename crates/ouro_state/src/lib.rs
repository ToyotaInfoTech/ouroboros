// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! State specifications and configuration models for the Ouroboros system.
//!
//! This crate defines the configuration and state types for the core runtime,
//! daemon, and edge components, including TOML-deserializable specifications.

use std::collections::HashMap;

use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserializer,
};

use crate::{
    core::{ClientSpec, DataSourceSpec, ExecutorSpec, ServerSpec},
    daemon::{DaemonTaskSpec, RepositorySpec},
    edge::{EdgeTaskSpec, TriggerStrategySpec},
};

pub mod core;
pub mod daemon;
pub mod edge;

/// Deserialize a sequence or map of DataSourceSpec into a HashMap keyed by name.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_data_source_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, DataSourceSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, DataSourceSpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of DataSourceSpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(item) = seq.next_element::<DataSourceSpec>()? {
                let name = match &item {
                    DataSourceSpec::SqlDb { name, .. } => name.clone(),
                    DataSourceSpec::Network { name, .. } => name.clone(),
                };
                map.insert(name, item);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, DataSourceSpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize a sequence or map of ClientSpec into a HashMap keyed by name.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_client_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, ClientSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, ClientSpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of ClientSpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(item) = seq.next_element::<ClientSpec>()? {
                map.insert(item.name.clone(), item);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, ClientSpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize a sequence or map of ServerSpec into a HashMap keyed by name.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_server_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, ServerSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, ServerSpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of ServerSpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(item) = seq.next_element::<ServerSpec>()? {
                map.insert(item.name.clone(), item);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, ServerSpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize a sequence or map of DaemonTaskSpec into a HashMap keyed by name.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_daemon_task_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, DaemonTaskSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, DaemonTaskSpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of DaemonTaskSpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(task) = seq.next_element::<DaemonTaskSpec>()? {
                let name = match &task {
                    DaemonTaskSpec::DataIngestion { name, .. } => name.clone(),
                    DaemonTaskSpec::RepositoryIngestion { name, .. } => name.clone(),
                    DaemonTaskSpec::Metrics { name, .. } => name.clone(),
                    DaemonTaskSpec::TagManagement { name, .. } => name.clone(),
                    DaemonTaskSpec::WebUi { name, .. } => name.clone(),
                };
                map.insert(name, task);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, DaemonTaskSpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize a sequence or map of EdgeTaskSpec into a HashMap keyed by name.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_edge_task_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, EdgeTaskSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, EdgeTaskSpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of EdgeTaskSpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(task) = seq.next_element::<EdgeTaskSpec>()? {
                let name = match &task {
                    EdgeTaskSpec::DataUpload { name, .. } => name.clone(),
                    EdgeTaskSpec::CommandWatch { name, .. } => name.clone(),
                    EdgeTaskSpec::TagWatch { name, .. } => name.clone(),
                };
                map.insert(name, task);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, EdgeTaskSpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize a sequence or map of TriggerStrategySpec into a HashMap keyed by uuid.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_trigger_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, TriggerStrategySpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, TriggerStrategySpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of TriggerStrategySpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(trigger) = seq.next_element::<TriggerStrategySpec>()? {
                let uuid = match &trigger {
                    TriggerStrategySpec::Periodic { uuid, .. } => uuid.clone(),
                    TriggerStrategySpec::BulkBytes { uuid, .. } => uuid.clone(),
                    TriggerStrategySpec::BulkCounts { uuid, .. } => uuid.clone(),
                    TriggerStrategySpec::Instant { uuid, .. } => uuid.clone(),
                };
                map.insert(uuid, trigger);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, TriggerStrategySpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize a sequence or map of RepositorySpec into a HashMap keyed by name.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_repository_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, RepositorySpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, RepositorySpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of RepositorySpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(item) = seq.next_element::<RepositorySpec>()? {
                let name = match &item {
                    RepositorySpec::ShadowStore { name, .. } => name.clone(),
                    RepositorySpec::TagStore { name, .. } => name.clone(),
                };
                map.insert(name, item);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, RepositorySpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize a sequence or map of ExecutorSpec into a HashMap keyed by name.
/// Accepts both TOML array format and CBOR map format.
pub(crate) fn deserialize_executor_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, ExecutorSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SeqOrMapVisitor;

    impl<'de> Visitor<'de> for SeqOrMapVisitor {
        type Value = HashMap<String, ExecutorSpec>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or map of ExecutorSpec")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut map = HashMap::new();
            while let Some(item) = seq.next_element::<ExecutorSpec>()? {
                map.insert(item.name.clone(), item);
            }
            Ok(map)
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
            let mut map = HashMap::new();
            while let Some((k, v)) = access.next_entry::<String, ExecutorSpec>()? {
                map.insert(k, v);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(SeqOrMapVisitor)
}

/// Deserialize an optional sequence or map of DataSourceSpec into an Option<HashMap>.
/// Accepts both TOML array format (`[[data_source]]`) and JSON object format (`{"name": {...}}`).
pub(crate) fn deserialize_opt_data_source_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<Option<HashMap<String, Option<DataSourceSpec>>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_data_source_vec_to_map(deserializer)
        .map(|map| Some(map.into_iter().map(|(k, v)| (k, Some(v))).collect()))
}

/// Deserialize an optional sequence or map of ClientSpec into an Option<HashMap>.
/// Accepts both TOML array format (`[[client]]`) and JSON object format (`{"name": {...}}`).
pub(crate) fn deserialize_opt_client_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<Option<HashMap<String, Option<ClientSpec>>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_client_vec_to_map(deserializer)
        .map(|map| Some(map.into_iter().map(|(k, v)| (k, Some(v))).collect()))
}

/// Deserialize an optional sequence or map of EdgeTaskSpec into an Option<HashMap>.
/// Accepts both TOML array format (`[[task]]`) and JSON object format (`{"name": {...}}`).
pub(crate) fn deserialize_opt_edge_task_vec_to_map<'de, D>(
    deserializer: D,
) -> Result<Option<HashMap<String, Option<EdgeTaskSpec>>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_edge_task_vec_to_map(deserializer)
        .map(|map| Some(map.into_iter().map(|(k, v)| (k, Some(v))).collect()))
}
