// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Partial configuration patch types for edge devices.
//!
//! Provides `EdgeSpecPatch` — an all-optional mirror of `EdgeSpec` used
//! for tag-based configuration overlays.  Each field is `Option<T>` so
//! that only the fields present in a patch overwrite the base spec during
//! merge.

use std::collections::HashMap;

use ouro_common::io::{FromCbor, ToCbor};
use serde::{Deserialize, Serialize};

use crate::core::{ClientSpec, DataSourceSpec, StateMetadata, WithDataSource};
use crate::edge::EdgeTaskSpec;
use crate::{
    deserialize_opt_client_vec_to_map, deserialize_opt_data_source_vec_to_map,
    deserialize_opt_edge_task_vec_to_map,
};

/// Partial patch for [`super::EdgeSpec`].
///
/// All fields are optional — only `Some(…)` values are applied during merge.
/// `metadata` is intentionally excluded; it is regenerated after merge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeSpecPatch {
    /// Metadata for this patch.
    /// Must be explicitly provided; merged metadata is regenerated after merge.
    pub metadata: StateMetadata,

    /// Runtime configuration overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeSpecPatch>,

    /// Data-source overrides (key = data-source name).
    ///
    /// - `None` → no change
    /// - `Some(empty)` → clear all data sources
    /// - `Some(non-empty)` → merge keys into base
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_data_source_vec_to_map"
    )]
    pub data_source: Option<HashMap<String, Option<DataSourceSpec>>>,

    /// Client overrides (key = client name).
    ///
    /// - `None` → no change
    /// - `Some(empty)` → clear all clients
    /// - `Some(non-empty)` → merge keys into base
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_client_vec_to_map"
    )]
    pub client: Option<HashMap<String, Option<ClientSpec>>>,

    /// Development overrides (key = arbitrary name).
    ///
    /// - `None` → no change
    /// - `Some(empty)` → clear all development entries
    /// - `Some(non-empty)` → merge keys into base
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub development: Option<HashMap<String, Option<toml::Value>>>,

    /// Task overrides (key = task name).
    ///
    /// - `None` → no change
    /// - `Some(empty)` → clear all tasks
    /// - `Some(non-empty)` → merge keys into base
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_edge_task_vec_to_map"
    )]
    pub task: Option<HashMap<String, Option<EdgeTaskSpec>>>,
}

impl FromCbor for EdgeSpecPatch {}
impl ToCbor for EdgeSpecPatch {}

/// Partial patch for [`super::RuntimeSpec`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RuntimeSpecPatch {
    /// Edge device unique identifier override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,

    /// Device model identifier override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Runtime version string override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// System state data source reference override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with_data_source: Option<WithDataSource>,
}

impl EdgeSpecPatch {
    /// Creates a new patch with the given metadata and all optional fields set to `None`.
    pub fn new(metadata: StateMetadata) -> Self {
        Self {
            metadata,
            runtime: None,
            data_source: None,
            client: None,
            development: None,
            task: None,
        }
    }

    /// Returns `true` if every optional field is `None` (i.e. the patch is a no-op).
    pub fn is_empty(&self) -> bool {
        self.runtime.is_none()
            && self.data_source.is_none()
            && self.client.is_none()
            && self.development.is_none()
            && self.task.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{SchemaVersion, StateEpoch, Tag, TagAssignment};

    fn test_metadata() -> StateMetadata {
        StateMetadata {
            schema_version: SchemaVersion(1),
            epoch: TagAssignment(vec![Tag {
                id: "test".to_string(),
                epoch: StateEpoch(0),
                display_name: Some("test".to_string()),
                priority: Some(0),
                patch: None,
            }]),
        }
    }

    #[test]
    fn test_new_patch_is_empty() {
        let patch = EdgeSpecPatch::new(test_metadata());
        assert!(patch.is_empty());
    }

    #[test]
    fn test_patch_serde_roundtrip() {
        let mut patch = EdgeSpecPatch::new(test_metadata());
        patch.runtime = Some(RuntimeSpecPatch {
            version: Some("2.0".to_string()),
            ..Default::default()
        });
        let json = serde_json::to_string(&patch).unwrap();
        let restored: EdgeSpecPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(patch, restored);
    }

    #[test]
    fn test_skip_serializing_none_fields() {
        let patch = EdgeSpecPatch::new(test_metadata());
        let json = serde_json::to_string(&patch).unwrap();
        // metadata is always serialized, optional fields are skipped
        assert!(json.contains("schema_version"));
        assert!(!json.contains("runtime"));
    }
}
