// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Storage abstraction for configuration tags.
//!
//! Defines the storage interface for managing config tags and
//! edge-to-tag assignments.

use std::collections::HashMap;

use async_trait::async_trait;

use ouro_core::InternalResult;
use ouro_state::core::{Tag, TagAssignment};
use tracing::instrument;

use crate::task::state::tag_persistent::PersistentTagStore;

/// Trait for configuration tag storage operations.
///
/// Provides CRUD for tags and assignment management between
/// edges and tags.
#[async_trait]
pub trait TagStore {
    /// Insert a new configuration tag only if display_name does not already exist.
    async fn insert_tag_if_not_exists(&self, tag: &Tag) -> InternalResult<()>;

    /// Insert a new configuration tag.
    async fn insert_tag(&self, tag: &Tag) -> InternalResult<()>;

    /// Retrieve a tag by its identifier.
    async fn get_tag(&self, tag_id: &str) -> InternalResult<Option<Tag>>;

    /// Retrieve a tag by its display name.
    async fn get_tag_by_display_name(&self, display_name: &str) -> InternalResult<Option<Tag>>;

    /// List all configuration tags.
    async fn list_tags(&self) -> InternalResult<Vec<Tag>>;

    /// Update an existing tag (upsert semantics).
    async fn update_tag(&self, tag: &Tag) -> InternalResult<()>;

    /// Delete a tag by its identifier.
    async fn delete_tag(&self, tag_id: &str) -> InternalResult<()>;

    /// Assign a tag to an edge device.
    async fn assign_tag(&self, edge_id: &str, tag_id: &str) -> InternalResult<()>;

    /// Remove a tag assignment from an edge device.
    async fn unassign_tag(&self, edge_id: &str, tag_id: &str) -> InternalResult<()>;

    /// Retrieve all tags assigned to a specific edge, ordered by priority.
    async fn get_tags_for_edge(&self, edge_id: &str) -> InternalResult<TagAssignment>;

    /// Retrieve all edge IDs that have a specific tag assigned.
    async fn get_edges_for_tag(&self, tag_id: &str) -> InternalResult<Vec<String>>;

    /// Retrieve all edge-tag assignments grouped by edge ID.
    async fn list_all_assignments(&self) -> InternalResult<HashMap<String, TagAssignment>>;
}

/// Enumeration of available tag storage implementations.
#[derive(Debug, Clone)]
pub enum BuiltTagStore {
    /// Database-backed persistent storage.
    Persistent(PersistentTagStore),
}

#[async_trait]
impl TagStore for BuiltTagStore {
    #[instrument(skip(self, tag))]
    async fn insert_tag_if_not_exists(&self, tag: &Tag) -> InternalResult<()> {
        match self {
            BuiltTagStore::Persistent(store) => store.insert_tag_if_not_exists(tag).await,
        }
    }

    #[instrument(skip(self, tag))]
    async fn insert_tag(&self, tag: &Tag) -> InternalResult<()> {
        match self {
            BuiltTagStore::Persistent(store) => store.insert_tag(tag).await,
        }
    }

    #[instrument(skip(self, tag_id))]
    async fn get_tag(&self, tag_id: &str) -> InternalResult<Option<Tag>> {
        match self {
            BuiltTagStore::Persistent(store) => store.get_tag(tag_id).await,
        }
    }

    #[instrument(skip(self, display_name))]
    async fn get_tag_by_display_name(&self, display_name: &str) -> InternalResult<Option<Tag>> {
        match self {
            BuiltTagStore::Persistent(store) => store.get_tag_by_display_name(display_name).await,
        }
    }

    #[instrument(skip(self))]
    async fn list_tags(&self) -> InternalResult<Vec<Tag>> {
        match self {
            BuiltTagStore::Persistent(store) => store.list_tags().await,
        }
    }

    #[instrument(skip(self, tag))]
    async fn update_tag(&self, tag: &Tag) -> InternalResult<()> {
        match self {
            BuiltTagStore::Persistent(store) => store.update_tag(tag).await,
        }
    }

    #[instrument(skip(self, tag_id))]
    async fn delete_tag(&self, tag_id: &str) -> InternalResult<()> {
        match self {
            BuiltTagStore::Persistent(store) => store.delete_tag(tag_id).await,
        }
    }

    #[instrument(skip(self, edge_id, tag_id))]
    async fn assign_tag(&self, edge_id: &str, tag_id: &str) -> InternalResult<()> {
        match self {
            BuiltTagStore::Persistent(store) => store.assign_tag(edge_id, tag_id).await,
        }
    }

    #[instrument(skip(self, edge_id, tag_id))]
    async fn unassign_tag(&self, edge_id: &str, tag_id: &str) -> InternalResult<()> {
        match self {
            BuiltTagStore::Persistent(store) => store.unassign_tag(edge_id, tag_id).await,
        }
    }

    #[instrument(skip(self, edge_id))]
    async fn get_tags_for_edge(&self, edge_id: &str) -> InternalResult<TagAssignment> {
        match self {
            BuiltTagStore::Persistent(store) => store.get_tags_for_edge(edge_id).await,
        }
    }

    #[instrument(skip(self, tag_id))]
    async fn get_edges_for_tag(&self, tag_id: &str) -> InternalResult<Vec<String>> {
        match self {
            BuiltTagStore::Persistent(store) => store.get_edges_for_tag(tag_id).await,
        }
    }

    #[instrument(skip(self))]
    async fn list_all_assignments(&self) -> InternalResult<HashMap<String, TagAssignment>> {
        match self {
            BuiltTagStore::Persistent(store) => store.list_all_assignments().await,
        }
    }
}
