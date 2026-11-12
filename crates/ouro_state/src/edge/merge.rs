// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Priority-based merge of [`EdgeSpecPatch`] layers onto a base [`EdgeSpec`].
//!
//! Patches are applied in ascending priority order (lower number = lower
//! priority).  HashMap fields are merged key-by-key; scalar fields use
//! last-writer-wins.  If two patches share the same priority **and**
//! conflict on the same key / scalar field, a [`MergeError`] is
//! returned.

use std::collections::HashMap;
use std::fmt;

use crate::core::{SchemaVersion, StateMetadata, Tag, TagAssignment};
use crate::edge::patch::{EdgeSpecPatch, RuntimeSpecPatch};
use crate::edge::{EdgeSpec, RuntimeSpec};

/// Priority value for ordering patches.  Lower = applied first.
pub type Priority = u32;

/// Error during merge when two patches at the same priority conflict.
#[derive(Debug, Clone)]
pub struct MergeError {
    pub conflicts: Vec<MergeConflict>,
}

/// A single conflict between two patches at the same priority.
#[derive(Debug, Clone)]
pub struct MergeConflict {
    /// Dotted path to the conflicting field (e.g. `"runtime.version"`).
    pub field: String,
    /// Priority level where the conflict occurred.
    pub priority: Priority,
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "merge conflicts: [")?;
        for (i, c) in self.conflicts.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}@priority={}", c.field, c.priority)?;
        }
        write!(f, "]")
    }
}

impl std::error::Error for MergeError {}

/// Merge an ordered list of `(priority, patch)` pairs onto a base `EdgeSpec`.
///
/// Patches are applied in ascending priority order.  Within the same
/// priority, conflicts are detected and reported.
///
/// Metadata rules:
/// - **schema_version**: All patches must share the same `schema_version`;
///   a mismatch is treated as a conflict.
/// - **epoch**: Built from each patch's `metadata.epoch` entries, collected
///   in ascending priority order.
/// - **timestamp**: The latest `timestamp` among all patches is adopted.
///
/// # Arguments
/// * `base`    – The starting `EdgeSpec` (e.g. the "default" config).
/// * `patches` – Slice of `(priority, patch)` pairs.  Need not be sorted;
///   the function sorts internally.
///
/// # Errors
/// Returns [`MergeError`] when two patches at the same priority set the
/// same scalar field or HashMap key to different values, or when
/// `schema_version` values disagree across patches.
pub fn merge_edge_spec(
    base: &EdgeSpec,
    patches: &[(Priority, EdgeSpecPatch)],
) -> Result<EdgeSpec, MergeError> {
    let mut result = base.clone();
    let mut conflicts: Vec<MergeConflict> = Vec::new();

    // Sort by priority ascending (stable sort keeps insertion order for ties).
    let mut sorted: Vec<_> = patches.iter().collect();
    sorted.sort_by_key(|(p, _)| *p);

    // Validate schema_version consistency across all patches.
    let mut schema_version: Option<&SchemaVersion> = None;
    for (priority, patch) in sorted.iter() {
        match schema_version {
            None => schema_version = Some(&patch.metadata.schema_version),
            Some(schema_version) if *schema_version != patch.metadata.schema_version => {
                conflicts.push(MergeConflict {
                    field: "metadata.schema_version".to_string(),
                    priority: *priority,
                });
            }
            _ => {}
        }
    }

    if !conflicts.is_empty() {
        return Err(MergeError { conflicts });
    }

    // Group consecutive patches that share the same priority.
    let mut i = 0;
    while i < sorted.len() {
        let current_priority = sorted[i].0;
        let mut j = i + 1;
        while j < sorted.len() && sorted[j].0 == current_priority {
            j += 1;
        }
        let group = &sorted[i..j];

        if group.len() == 1 {
            apply_patch(&mut result, &group[0].1);
        } else {
            detect_conflicts(group, current_priority, &mut conflicts);
            if conflicts.is_empty() {
                for (_, patch) in group {
                    apply_patch(&mut result, patch);
                }
            }
        }

        i = j;
    }

    if !conflicts.is_empty() {
        return Err(MergeError { conflicts });
    }

    // Build merged metadata.
    // epoch: collect each patch's epoch entries in priority-ascending order.
    let merged_epoch: Vec<Tag> = sorted
        .iter()
        .flat_map(|(_, patch)| patch.metadata.epoch.iter().cloned())
        .collect();

    result.metadata = StateMetadata {
        schema_version: schema_version
            .copied()
            .unwrap_or(base.metadata.schema_version),
        epoch: TagAssignment(merged_epoch),
    };

    Ok(result)
}

/// Apply a single patch onto the accumulator.
///
/// Semantics for `Option<HashMap<…>>` fields:
/// - `None`           → no change (field untouched)
/// - `Some(empty)`    → clear all entries in the target
/// - `Some(non-empty)` → merge keys (last-writer-wins per key)
fn apply_patch(target: &mut EdgeSpec, patch: &EdgeSpecPatch) {
    if let Some(ref rt) = patch.runtime {
        apply_runtime_patch(&mut target.runtime, rt);
    }
    if let Some(ref ds) = patch.data_source {
        merge_or_clear_hashmap(&mut target.data_source, ds);
    }
    if let Some(ref cl) = patch.client {
        merge_or_clear_hashmap(&mut target.client, cl);
    }
    if let Some(ref dev) = patch.development {
        merge_or_clear_hashmap(&mut target.development, dev);
    }
    if let Some(ref tasks) = patch.task {
        merge_or_clear_hashmap(&mut target.task, tasks);
    }
}

fn apply_runtime_patch(target: &mut RuntimeSpec, patch: &RuntimeSpecPatch) {
    if let Some(ref v) = patch.edge_id {
        target.edge_id = v.clone();
    }
    if let Some(ref v) = patch.model {
        target.model = v.clone();
    }
    if let Some(ref v) = patch.version {
        target.version = v.clone();
    }
    if let Some(ref v) = patch.with_data_source {
        target.with_data_source = v.clone();
    }
}

/// Merge or clear a HashMap field from a patch source.
///
/// - If `source` is empty, clear all entries in `target`.
/// - `Some(value)` → insert or overwrite the key in `target`.
/// - `None` → remove the key from `target`.
fn merge_or_clear_hashmap<V: Clone>(
    target: &mut HashMap<String, V>,
    source: &HashMap<String, Option<V>>,
) {
    if source.is_empty() {
        target.clear();
    } else {
        for (k, v) in source {
            match v {
                Some(val) => {
                    target.insert(k.clone(), val.clone());
                }
                None => {
                    target.remove(k);
                }
            }
        }
    }
}

/// Detect conflicts among patches that share the same priority.
fn detect_conflicts(
    group: &[&(Priority, EdgeSpecPatch)],
    priority: Priority,
    conflicts: &mut Vec<MergeConflict>,
) {
    // Runtime scalar fields
    detect_scalar_conflict(
        group
            .iter()
            .filter_map(|(_, p)| p.runtime.as_ref()?.edge_id.as_ref()),
        "runtime.edge_id",
        priority,
        conflicts,
    );
    detect_scalar_conflict(
        group
            .iter()
            .filter_map(|(_, p)| p.runtime.as_ref()?.model.as_ref()),
        "runtime.model",
        priority,
        conflicts,
    );
    detect_scalar_conflict(
        group
            .iter()
            .filter_map(|(_, p)| p.runtime.as_ref()?.version.as_ref()),
        "runtime.version",
        priority,
        conflicts,
    );
    detect_scalar_conflict(
        group
            .iter()
            .filter_map(|(_, p)| p.runtime.as_ref()?.with_data_source.as_ref()),
        "runtime.with_data_source",
        priority,
        conflicts,
    );

    // HashMap fields — detect overlapping keys with different values
    detect_map_conflicts(
        group.iter().filter_map(|(_, p)| p.data_source.as_ref()),
        "data_source",
        priority,
        conflicts,
    );
    detect_map_conflicts(
        group.iter().filter_map(|(_, p)| p.client.as_ref()),
        "client",
        priority,
        conflicts,
    );
    detect_map_conflicts(
        group.iter().filter_map(|(_, p)| p.task.as_ref()),
        "task",
        priority,
        conflicts,
    );
}

/// If more than one patch sets the same scalar to different values, report conflict.
fn detect_scalar_conflict<'a, T: PartialEq + 'a>(
    values: impl Iterator<Item = &'a T>,
    field: &str,
    priority: Priority,
    conflicts: &mut Vec<MergeConflict>,
) {
    let collected: Vec<_> = values.collect();
    if collected.len() > 1 {
        let first = collected[0];
        if collected.iter().skip(1).any(|v| *v != first) {
            conflicts.push(MergeConflict {
                field: field.to_string(),
                priority,
            });
        }
    }
}

/// Detect overlapping HashMap keys with different values among patches at the same priority.
fn detect_map_conflicts<'a, V: PartialEq + 'a>(
    maps: impl Iterator<Item = &'a HashMap<String, Option<V>>>,
    prefix: &str,
    priority: Priority,
    conflicts: &mut Vec<MergeConflict>,
) {
    let collected: Vec<_> = maps.collect();
    if collected.len() <= 1 {
        return;
    }
    // For each key that appears in more than one map, check value equality.
    let mut seen: HashMap<&str, &Option<V>> = HashMap::new();
    for map in &collected {
        for (k, v) in *map {
            if let Some(prev) = seen.get(k.as_str()) {
                if *prev != v {
                    conflicts.push(MergeConflict {
                        field: format!("{}.{}", prefix, k),
                        priority,
                    });
                }
            } else {
                seen.insert(k.as_str(), v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{DataSourceRef, SchemaVersion, StateEpoch, WithDataSource};
    use crate::edge::patch::{EdgeSpecPatch, RuntimeSpecPatch};
    use crate::edge::{EdgeSpec, RuntimeSpec};

    fn base_spec() -> EdgeSpec {
        EdgeSpec {
            metadata: StateMetadata {
                schema_version: SchemaVersion(1),
                epoch: TagAssignment(vec![Tag {
                    id: "base".to_string(),
                    epoch: StateEpoch(0),
                    display_name: None,
                    priority: None,
                    patch: None,
                }]),
            },
            runtime: RuntimeSpec {
                edge_id: "edge-001".to_string(),
                model: "typeA".to_string(),
                version: "1.0".to_string(),
                with_data_source: WithDataSource::Ref {
                    r#ref: DataSourceRef::SqlDb {
                        r#ref: "sqlite_memory".to_string(),
                        init_sql: vec![],
                    },
                },
                place_id: Some("place-001".to_string()),
            },
            data_source: HashMap::new(),
            client: HashMap::new(),
            development: HashMap::new(),
            task: HashMap::new(),
        }
    }

    fn patch_with_metadata(tag: &str, epoch: u64) -> EdgeSpecPatch {
        EdgeSpecPatch::new(StateMetadata {
            schema_version: SchemaVersion(1),
            epoch: TagAssignment(vec![Tag {
                id: tag.to_string(),
                epoch: StateEpoch(epoch),
                display_name: None,
                priority: None,
                patch: None,
            }]),
        })
    }

    #[test]
    fn test_single_patch_applies() {
        let base = base_spec();
        let mut patch = patch_with_metadata("tag-a", 1);
        patch.runtime = Some(RuntimeSpecPatch {
            version: Some("2.0".to_string()),
            ..Default::default()
        });

        let result = merge_edge_spec(&base, &[(10, patch)]).unwrap();
        assert_eq!(result.runtime.version, "2.0");
        assert_eq!(result.runtime.model, "typeA"); // unchanged
        assert_eq!(result.metadata.epoch.len(), 1);
        assert_eq!(result.metadata.epoch[0].id, "tag-a");
        assert_eq!(result.metadata.epoch[0].epoch, StateEpoch(1));
    }

    #[test]
    fn test_priority_ordering() {
        let base = base_spec();
        let mut low = patch_with_metadata("low-tag", 1);
        low.runtime = Some(RuntimeSpecPatch {
            version: Some("low".to_string()),
            ..Default::default()
        });
        let mut high = patch_with_metadata("high-tag", 2);
        high.runtime = Some(RuntimeSpecPatch {
            version: Some("high".to_string()),
            ..Default::default()
        });

        let result = merge_edge_spec(&base, &[(10, low), (20, high)]).unwrap();
        assert_eq!(result.runtime.version, "high"); // higher priority wins
                                                    // epoch entries in priority-ascending order
        assert_eq!(result.metadata.epoch.len(), 2);
        assert_eq!(result.metadata.epoch[0].id, "low-tag");
        assert_eq!(result.metadata.epoch[1].id, "high-tag");
    }

    #[test]
    fn test_hashmap_key_merge() {
        let base = base_spec();
        let mut patch_a = patch_with_metadata("a", 1);
        patch_a.development = Some(HashMap::from([(
            "key_a".to_string(),
            Some(toml::Value::String("a".to_string())),
        )]));
        let mut patch_b = patch_with_metadata("b", 1);
        patch_b.development = Some(HashMap::from([(
            "key_b".to_string(),
            Some(toml::Value::String("b".to_string())),
        )]));

        let result = merge_edge_spec(&base, &[(10, patch_a), (20, patch_b)]).unwrap();
        assert_eq!(result.development.len(), 2);
        assert!(result.development.contains_key("key_a"));
        assert!(result.development.contains_key("key_b"));
    }

    #[test]
    fn test_same_priority_conflict() {
        let base = base_spec();
        let mut p1 = patch_with_metadata("t1", 1);
        p1.runtime = Some(RuntimeSpecPatch {
            version: Some("v1".to_string()),
            ..Default::default()
        });
        let mut p2 = patch_with_metadata("t2", 1);
        p2.runtime = Some(RuntimeSpecPatch {
            version: Some("v2".to_string()),
            ..Default::default()
        });

        let err = merge_edge_spec(&base, &[(10, p1), (10, p2)]).unwrap_err();
        assert_eq!(err.conflicts.len(), 1);
        assert_eq!(err.conflicts[0].field, "runtime.version");
    }

    #[test]
    fn test_same_priority_no_conflict_when_equal() {
        let base = base_spec();
        let mut p1 = patch_with_metadata("t1", 1);
        p1.runtime = Some(RuntimeSpecPatch {
            version: Some("same".to_string()),
            ..Default::default()
        });
        let mut p2 = patch_with_metadata("t2", 1);
        p2.runtime = Some(RuntimeSpecPatch {
            version: Some("same".to_string()),
            ..Default::default()
        });

        let result = merge_edge_spec(&base, &[(10, p1), (10, p2)]).unwrap();
        assert_eq!(result.runtime.version, "same");
    }

    #[test]
    fn test_empty_patch_is_noop() {
        let base = base_spec();
        let empty = patch_with_metadata("empty", 1);

        let result = merge_edge_spec(&base, &[(10, empty)]).unwrap();
        assert_eq!(result.runtime.version, "1.0");
        assert_eq!(result.runtime.model, "typeA");
    }

    #[test]
    fn test_no_patches() {
        let base = base_spec();
        let result = merge_edge_spec(&base, &[]).unwrap();
        assert_eq!(result.runtime.version, "1.0");
        // No patches → base schema_version preserved, empty epoch
        assert_eq!(result.metadata.schema_version, SchemaVersion(1));
        assert!(result.metadata.epoch.is_empty());
    }

    #[test]
    fn test_some_empty_clears_hashmap() {
        let mut base = base_spec();
        base.development.insert(
            "existing".to_string(),
            toml::Value::String("val".to_string()),
        );

        let mut patch = patch_with_metadata("clear", 1);
        patch.development = Some(HashMap::new());

        let result = merge_edge_spec(&base, &[(10, patch)]).unwrap();
        assert!(result.development.is_empty());
    }

    #[test]
    fn test_none_leaves_hashmap_untouched() {
        let mut base = base_spec();
        base.development.insert(
            "existing".to_string(),
            toml::Value::String("val".to_string()),
        );

        let patch = patch_with_metadata("noop", 1);
        // development is None by default

        let result = merge_edge_spec(&base, &[(10, patch)]).unwrap();
        assert_eq!(result.development.len(), 1);
        assert!(result.development.contains_key("existing"));
    }

    #[test]
    fn test_clear_then_repopulate() {
        let mut base = base_spec();
        base.development.insert(
            "old_key".to_string(),
            toml::Value::String("old".to_string()),
        );

        let mut clear_patch = patch_with_metadata("clear", 1);
        clear_patch.development = Some(HashMap::new());

        let mut add_patch = patch_with_metadata("add", 2);
        add_patch.development = Some(HashMap::from([(
            "new_key".to_string(),
            Some(toml::Value::String("new".to_string())),
        )]));

        let result = merge_edge_spec(&base, &[(10, clear_patch), (20, add_patch)]).unwrap();
        assert_eq!(result.development.len(), 1);
        assert!(result.development.contains_key("new_key"));
        assert!(!result.development.contains_key("old_key"));
    }

    #[test]
    fn test_schema_version_mismatch_is_conflict() {
        let base = base_spec();
        let mut p1 = patch_with_metadata("t1", 1);
        p1.metadata.schema_version = SchemaVersion(1);
        let mut p2 = patch_with_metadata("t2", 1);
        p2.metadata.schema_version = SchemaVersion(2);

        let err = merge_edge_spec(&base, &[(10, p1), (20, p2)]).unwrap_err();
        assert!(err
            .conflicts
            .iter()
            .any(|c| c.field == "metadata.schema_version"));
    }
}
