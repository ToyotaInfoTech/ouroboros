// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Database-backed persistent storage for configuration tags.
//!
//! Provides durable storage of config tags and edge-to-tag assignments
//! using PostgreSQL, mirroring the pattern of `PersistentShadowStore`.

use async_trait::async_trait;
use derivative::Derivative;
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::rdb;
use ouro_core::InternalResult;
use ouro_state::core::{StateEpoch, Tag, TagAssignment};
use ouro_state::edge::patch::EdgeSpecPatch;
use sqlx::Row as _;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

use crate::task::state::tag_store;

/// SQL: Insert a config tag only if the display_name does not already exist.
const INSERT_TAG_IF_NOT_EXISTS: &str = r#"
    INSERT INTO state_tags (tag_id, display_name, priority, patch, created_at, updated_at)
    VALUES ($1, $2, $3, $4, NOW(), NOW())
    ON CONFLICT (display_name) DO NOTHING
"#;

/// SQL: Insert or update a config tag.
const INSERT_OR_UPDATE_TAG: &str = r#"
    INSERT INTO state_tags (tag_id, display_name, priority, patch, created_at, updated_at)
    VALUES ($1, $2, $3, $4, NOW(), NOW())
    ON CONFLICT (tag_id) DO UPDATE SET
    display_name = $2, priority = $3, patch = $4, updated_at = NOW()
"#;

/// SQL: Retrieve a single config tag by ID.
const GET_TAG: &str =
    "SELECT tag_id, epoch, display_name, priority, patch FROM state_tags WHERE tag_id = $1";

/// SQL: Retrieve a single config tag by display name.
const GET_TAG_BY_DISPLAY_NAME: &str =
    "SELECT tag_id, epoch, display_name, priority, patch FROM state_tags WHERE display_name = $1";

/// SQL: List all config tags ordered by priority.
const LIST_TAGS: &str =
    "SELECT tag_id, epoch, display_name, priority, patch FROM state_tags ORDER BY priority ASC";

/// SQL: Update an existing config tag.
const UPDATE_TAG: &str = r#"
    UPDATE state_tags SET display_name = $2, priority = $3, patch = $4
    WHERE tag_id = $1
"#;

/// SQL: Delete a config tag.
const DELETE_TAG: &str = "DELETE FROM state_tags WHERE tag_id = $1";

/// SQL: Assign a tag to an edge.
const ASSIGN_TAG: &str = r#"
    INSERT INTO edge_tag_assignments (edge_id, tag_id)
    VALUES ($1, $2)
    ON CONFLICT (edge_id, tag_id) DO NOTHING
"#;

/// SQL: Remove a tag assignment from an edge.
const UNASSIGN_TAG: &str = "DELETE FROM edge_tag_assignments WHERE edge_id = $1 AND tag_id = $2";

/// SQL: Get all tags for an edge, joined with tag data, ordered by priority.
const GET_TAGS_FOR_EDGE: &str = r#"
    SELECT ct.tag_id, ct.epoch, ct.display_name, ct.priority, ct.patch
    FROM state_tags ct
    INNER JOIN edge_tag_assignments eta ON ct.tag_id = eta.tag_id
    WHERE eta.edge_id = $1
    ORDER BY ct.priority ASC
"#;

/// SQL: Get all edge IDs that have a specific tag assigned.
const GET_EDGES_FOR_TAG: &str = "SELECT edge_id FROM edge_tag_assignments WHERE tag_id = $1";

/// SQL: Get all edge-tag assignments with tag priority.
const LIST_ALL_ASSIGNMENTS: &str = r#"
    SELECT eta.edge_id, ct.tag_id, ct.epoch, ct.display_name, ct.priority, ct.patch
    FROM edge_tag_assignments eta
    INNER JOIN state_tags ct ON eta.tag_id = ct.tag_id
    ORDER BY eta.edge_id, ct.priority ASC
"#;

/// Database-backed persistent tag store.
///
/// Stores configuration tags and edge-tag assignments in PostgreSQL.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct PersistentTagStore
where
    Self: Send + Sync,
{
    pub name: String,
    /// Database connection pool for PostgreSQL operations.
    #[derivative(Debug = "ignore")]
    rdb: Arc<rdb::Rdb<sqlx::postgres::Postgres>>,
}

impl PersistentTagStore {
    /// Creates a new PersistentTagStore instance.
    ///
    /// # Arguments
    ///
    /// * `name` - Store name identifier
    /// * `rdb` - Arc-wrapped database connection pool for PostgreSQL
    ///
    /// # Returns
    ///
    /// A new PersistentTagStore instance
    #[instrument(skip(name, rdb))]
    pub(crate) fn new(name: String, rdb: Arc<rdb::Rdb<sqlx::postgres::Postgres>>) -> Self {
        Self { name, rdb }
    }
}

/// Parse a database row into a `ConfigTag`.
#[allow(clippy::result_large_err)]
fn row_to_state_tag(row: &sqlx::postgres::PgRow) -> InternalResult<Tag> {
    let id: String = row.get(0);
    let epoch: i64 = row.get(1);
    let epoch = StateEpoch(epoch as u64);
    let display_name: String = row.get(2);
    let priority: i32 = row.get(3);
    let patch_json: serde_json::Value = row.get(4);
    let patch: EdgeSpecPatch = serde_json::from_value(patch_json)
        .map_err(|e| ("Failed to deserialize tag patch", e).into())?;
    Ok(Tag {
        id,
        epoch,
        display_name: Some(display_name),
        priority: Some(priority as u32),
        patch: Some(patch),
    })
}

#[async_trait]
impl tag_store::TagStore for PersistentTagStore {
    /// Inserts a config tag only if no tag with the same display_name exists.
    ///
    /// Uses `ON CONFLICT (display_name) DO NOTHING` to make predefined tag
    /// registration idempotent across daemon restarts.
    #[instrument(skip(self, tag))]
    async fn insert_tag_if_not_exists(&self, tag: &Tag) -> InternalResult<()> {
        let priority = tag.priority.ok_or_else(|| {
            (
                "Tag priority is required for insertion",
                InternalErrorKind::IllegalArgument,
            )
                .into()
        })?;

        self.rdb
            .transaction(|tx| async move {
                let patch_json = serde_json::to_value(&tag.patch)
                    .map_err(|e| ("Failed to serialize tag patch", e).into())?;
                sqlx::query(INSERT_TAG_IF_NOT_EXISTS)
                    .bind(&tag.id)
                    .bind(&tag.display_name)
                    .bind(priority as i32)
                    .bind(patch_json)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to insert config tag", e).into())?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Inserts or updates a config tag in the database.
    #[instrument(skip(self, tag))]
    async fn insert_tag(&self, tag: &Tag) -> InternalResult<()> {
        let priority = tag.priority.ok_or_else(|| {
            (
                "Tag priority is required for insertion",
                InternalErrorKind::IllegalArgument,
            )
                .into()
        })?;

        self.rdb
            .transaction(|tx| async move {
                let patch_json = serde_json::to_value(&tag.patch)
                    .map_err(|e| ("Failed to serialize tag patch", e).into())?;
                sqlx::query(INSERT_OR_UPDATE_TAG)
                    .bind(&tag.id)
                    .bind(&tag.display_name)
                    .bind(priority as i32)
                    .bind(patch_json)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to insert/update config tag", e).into())?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Retrieves a config tag from the database by display name.
    #[instrument(skip(self, display_name))]
    async fn get_tag_by_display_name(&self, display_name: &str) -> InternalResult<Option<Tag>> {
        let tag = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let row = sqlx::query(GET_TAG_BY_DISPLAY_NAME)
                    .bind(display_name)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;
                match row {
                    Some(row) => Ok(Some(row_to_state_tag(&row)?)),
                    None => Ok(None),
                }
            })
            .await?
            .commit()
            .await?;

        Ok(tag.flatten())
    }

    /// Retrieves a config tag from the database.
    #[instrument(skip(self, tag_id))]
    async fn get_tag(&self, tag_id: &str) -> InternalResult<Option<Tag>> {
        let tag = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let row = sqlx::query(GET_TAG)
                    .bind(tag_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;
                match row {
                    Some(row) => Ok(Some(row_to_state_tag(&row)?)),
                    None => Ok(None),
                }
            })
            .await?
            .commit()
            .await?;

        Ok(tag.flatten())
    }

    /// Lists all configuration tags ordered by priority.
    #[instrument(skip(self))]
    async fn list_tags(&self) -> InternalResult<Vec<Tag>> {
        let tags = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let rows = sqlx::query(LIST_TAGS)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;
                let mut tags = Vec::with_capacity(rows.len());
                for row in &rows {
                    tags.push(row_to_state_tag(row)?);
                }
                Ok(tags)
            })
            .await?
            .commit()
            .await?;

        Ok(tags.unwrap_or_default())
    }

    /// Updates an existing config tag.
    #[instrument(skip(self, tag))]
    async fn update_tag(&self, tag: &Tag) -> InternalResult<()> {
        let priority = tag.priority.ok_or_else(|| {
            (
                "Tag priority is required for update",
                InternalErrorKind::IllegalArgument,
            )
                .into()
        })?;
        self.rdb
            .transaction(|tx| async move {
                let patch_json = serde_json::to_value(&tag.patch)
                    .map_err(|e| ("Failed to serialize tag patch", e).into())?;
                let result = sqlx::query(UPDATE_TAG)
                    .bind(&tag.id)
                    .bind(&tag.display_name)
                    .bind(priority as i32)
                    .bind(patch_json)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to update config tag", e).into())?;
                if result.rows_affected() == 0 {
                    return Err((
                        "No config tag found with the given tag_id",
                        InternalErrorKind::Runtime,
                    )
                        .into());
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Deletes a config tag (cascade removes assignments).
    #[instrument(skip(self, tag_id))]
    async fn delete_tag(&self, tag_id: &str) -> InternalResult<()> {
        self.rdb
            .transaction(|tx| async move {
                sqlx::query(DELETE_TAG)
                    .bind(tag_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to delete config tag", e).into())?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Assigns a tag to an edge device.
    #[instrument(skip(self, edge_id, tag_id))]
    async fn assign_tag(&self, edge_id: &str, tag_id: &str) -> InternalResult<()> {
        self.rdb
            .transaction(|tx| async move {
                sqlx::query(ASSIGN_TAG)
                    .bind(edge_id)
                    .bind(tag_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to assign tag to edge", e).into())?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Removes a tag assignment from an edge device.
    #[instrument(skip(self, edge_id, tag_id))]
    async fn unassign_tag(&self, edge_id: &str, tag_id: &str) -> InternalResult<()> {
        let edge_id_owned = edge_id.to_string();
        let tag_id_owned = tag_id.to_string();
        self.rdb
            .transaction(|tx| async move {
                let result = sqlx::query(UNASSIGN_TAG)
                    .bind(&edge_id_owned)
                    .bind(&tag_id_owned)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to unassign tag from edge", e).into())?;
                if result.rows_affected() == 0 {
                    return Err((
                        format!(
                            "Tag assignment not found: edge='{}', tag='{}'",
                            edge_id_owned, tag_id_owned
                        ),
                        InternalErrorKind::NoSuchKey,
                    )
                        .into());
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Retrieves all tags for a specific edge, ordered by priority.
    #[instrument(skip(self, edge_id))]
    async fn get_tags_for_edge(&self, edge_id: &str) -> InternalResult<TagAssignment> {
        let tags = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let rows = sqlx::query(GET_TAGS_FOR_EDGE)
                    .bind(edge_id)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;
                let mut tags = Vec::with_capacity(rows.len());
                for row in &rows {
                    tags.push(row_to_state_tag(row)?);
                }
                Ok(tags)
            })
            .await?
            .commit()
            .await?;

        Ok(tags.into())
    }

    /// Retrieves all edge IDs assigned to a specific tag.
    #[instrument(skip(self, tag_id))]
    async fn get_edges_for_tag(&self, tag_id: &str) -> InternalResult<Vec<String>> {
        let ids = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let rows = sqlx::query(GET_EDGES_FOR_TAG)
                    .bind(tag_id)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;
                Ok(rows.into_iter().map(|row| row.get(0)).collect())
            })
            .await?
            .commit()
            .await?;

        Ok(ids.unwrap_or_default())
    }

    /// Retrieves all edge-tag assignments grouped by edge ID.
    #[instrument(skip(self))]
    async fn list_all_assignments(&self) -> InternalResult<HashMap<String, TagAssignment>> {
        let map = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let rows = sqlx::query(LIST_ALL_ASSIGNMENTS)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;
                let mut map: HashMap<String, Vec<Tag>> = HashMap::new();
                for row in &rows {
                    let edge_id: String = row.get(0);
                    let id: String = row.get(1);
                    let epoch: i64 = row.get(2);
                    let epoch = StateEpoch(epoch as u64);
                    let display_name: String = row.get(3);
                    let priority: i32 = row.get(4);
                    let patch_json: serde_json::Value = row.get(5);
                    let patch: EdgeSpecPatch = serde_json::from_value(patch_json)
                        .map_err(|e| ("Failed to deserialize tag patch", e).into())?;
                    map.entry(edge_id).or_default().push(Tag {
                        id,
                        epoch,
                        display_name: Some(display_name),
                        priority: Some(priority as u32),
                        patch: Some(patch),
                    });
                }
                Ok(map)
            })
            .await?
            .commit()
            .await?;

        Ok(map
            .unwrap_or_default()
            .into_iter()
            .map(|(edge_id, tags)| (edge_id, TagAssignment(tags)))
            .collect())
    }
}
