// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Storage abstraction for device shadows.
//!
//! This module defines the storage interface and implementations for managing
//! device shadow state across different storage backends.

use async_trait::async_trait;

use ouro_core::InternalResult;
use ouro_schema::schema::shadow::EdgeShadow;
use tracing::instrument;

use std::{collections::HashMap, time::Duration};

use crate::task::state::{
    persistent::PersistentShadowStore, transient::TransientShadowStore,
    write_behind::WriteBehindShadowStore,
};

/// Filter criteria for querying device shadows.
pub enum Filter {
    /// Filter for devices that have been offline for longer than the specified duration.
    Offline(Duration),
    /// Filter for devices whose reported state is out of sync with desired state.
    OutOfSync,
}

/// Trait for device shadow storage operations.
///
/// Defines the interface for storing and retrieving device shadows across
/// different storage backends.
#[async_trait]
pub trait ShadowStore {
    /// Inserts a new device shadow.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    /// * `shadow` - Device shadow to store
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok on success, error otherwise
    async fn insert_shadow(&self, shadow: EdgeShadow) -> InternalResult<()>;

    /// Retrieves a device shadow by edge ID.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    ///
    /// # Returns
    ///
    /// * `Result<Option<DeviceShadow>>` - Shadow if found, None otherwise
    async fn get_shadow(&self, edge_id: &str) -> InternalResult<Option<EdgeShadow>>;

    async fn list_shadow_ids(&self) -> InternalResult<Vec<String>>;

    /// Retrieves all device shadows matching the specified filters.
    ///
    /// # Arguments
    ///
    /// * `filters` - Array of filter criteria to apply
    ///
    /// # Returns
    ///
    /// * `Result<Option<HashMap<String, DeviceShadow>>>` - Map of shadows if any exist
    async fn get_shadow_all(
        &self,
        filters: &[Filter],
    ) -> InternalResult<HashMap<String, EdgeShadow>>;

    /// Updates an existing device shadow.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    /// * `shadow` - Updated device shadow
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok on success, error otherwise
    async fn update_shadow(&self, shadow: EdgeShadow) -> InternalResult<()>;

    /// Deletes a device shadow.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok on success, error otherwise
    async fn delete_shadow(&self, edge_id: &str) -> InternalResult<()>;
}

/// Enumeration of available shadow storage implementations.
///
/// Provides a type-safe way to use different storage backends without
/// trait objects, enabling efficient pattern matching.
#[derive(Debug, Clone)]
pub enum BuiltShadowStore {
    /// In-memory transient storage for testing or lightweight use cases.
    Transient(TransientShadowStore),
    /// Database-backed persistent storage for durable state.
    Persistent(PersistentShadowStore),
    /// Write-behind caching strategy combining transient and persistent storage.
    WriteBehind(WriteBehindShadowStore),
}

#[async_trait]
impl ShadowStore for BuiltShadowStore {
    /// Inserts a new device shadow.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    /// * `shadow` - Device shadow to store
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok on success, error otherwise
    #[instrument(skip(self, shadow))]
    async fn insert_shadow(&self, shadow: EdgeShadow) -> InternalResult<()> {
        match self {
            BuiltShadowStore::Transient(store) => store.insert_shadow(shadow).await,
            BuiltShadowStore::Persistent(store) => store.insert_shadow(shadow).await,
            BuiltShadowStore::WriteBehind(store) => store.insert_shadow(shadow).await,
        }
    }

    /// Retrieves a device shadow by edge ID.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    ///
    /// # Returns
    ///
    /// * `Result<Option<DeviceShadow>>` - Shadow if found, None otherwise
    #[instrument(skip(self, edge_id))]
    async fn get_shadow(&self, edge_id: &str) -> InternalResult<Option<EdgeShadow>> {
        match self {
            BuiltShadowStore::Transient(store) => store.get_shadow(edge_id).await,
            BuiltShadowStore::Persistent(store) => store.get_shadow(edge_id).await,
            BuiltShadowStore::WriteBehind(store) => store.get_shadow(edge_id).await,
        }
    }

    async fn list_shadow_ids(&self) -> InternalResult<Vec<String>> {
        match self {
            BuiltShadowStore::Transient(store) => store.list_shadow_ids().await,
            BuiltShadowStore::Persistent(store) => store.list_shadow_ids().await,
            BuiltShadowStore::WriteBehind(store) => store.list_shadow_ids().await,
        }
    }

    /// Retrieves all device shadows matching the specified filters.
    ///
    /// # Arguments
    ///
    /// * `filters` - Array of filter criteria to apply
    ///
    /// # Returns
    ///
    /// * `Result<HashMap<String, DeviceShadow>>` - Map of shadows if any exist
    #[instrument(skip(self, filters))]
    async fn get_shadow_all(
        &self,
        filters: &[Filter],
    ) -> InternalResult<HashMap<String, EdgeShadow>> {
        match self {
            BuiltShadowStore::Transient(store) => store.get_shadow_all(filters).await,
            BuiltShadowStore::Persistent(store) => store.get_shadow_all(filters).await,
            BuiltShadowStore::WriteBehind(store) => store.get_shadow_all(filters).await,
        }
    }

    /// Updates an existing device shadow.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    /// * `shadow` - Updated device shadow
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok on success, error otherwise
    #[instrument(skip(self, shadow))]
    async fn update_shadow(&self, shadow: EdgeShadow) -> InternalResult<()> {
        match self {
            BuiltShadowStore::Transient(store) => store.update_shadow(shadow).await,
            BuiltShadowStore::Persistent(store) => store.update_shadow(shadow).await,
            BuiltShadowStore::WriteBehind(store) => store.update_shadow(shadow).await,
        }
    }

    /// Deletes a device shadow.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok on success, error otherwise
    #[instrument(skip(self, edge_id))]
    async fn delete_shadow(&self, edge_id: &str) -> InternalResult<()> {
        match self {
            BuiltShadowStore::Transient(store) => store.delete_shadow(edge_id).await,
            BuiltShadowStore::Persistent(store) => store.delete_shadow(edge_id).await,
            BuiltShadowStore::WriteBehind(store) => store.delete_shadow(edge_id).await,
        }
    }
}
