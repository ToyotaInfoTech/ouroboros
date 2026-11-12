// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! In-memory transient storage for device shadows.
//!
//! Provides a fast, non-persistent storage backend suitable for testing,
//! development, or scenarios where shadow state durability is not required.

use async_trait::async_trait;
use ouro_core::InternalResult;
use ouro_schema::schema::shadow::EdgeShadow;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::instrument;

use crate::task::state::store::{Filter, ShadowStore};

/// In-memory implementation of ShadowStore for testing or lightweight use cases.
///
/// All shadow data is stored in memory and will be lost when the process terminates.
/// This implementation provides fast read/write operations without database overhead.
#[derive(Debug, Clone)]
pub struct TransientShadowStore {
    pub name: String,
    /// Thread-safe map of device shadows indexed by edge ID.
    pub shadows: Arc<RwLock<HashMap<String, EdgeShadow>>>,
}

impl TransientShadowStore {
    /// Creates a new transient shadow store.
    ///
    /// # Returns
    ///
    /// * `Self` - New transient store instance with empty shadow map
    #[instrument(skip(name))]
    pub fn new(name: String) -> Self {
        Self {
            name,
            shadows: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ShadowStore for TransientShadowStore {
    #[instrument(skip(self, shadow))]
    async fn insert_shadow(&self, shadow: EdgeShadow) -> InternalResult<()> {
        self.shadows
            .write()
            .await
            .insert(shadow.edge_id.clone(), shadow);
        Ok(())
    }

    #[instrument(skip(self, edge_id))]
    async fn get_shadow(&self, edge_id: &str) -> InternalResult<Option<EdgeShadow>> {
        let shadows = self.shadows.read().await;
        Ok(shadows.get(edge_id).cloned())
    }

    #[instrument(skip(self))]
    async fn list_shadow_ids(&self) -> InternalResult<Vec<String>> {
        let shadows = self.shadows.read().await;
        Ok(shadows.keys().cloned().collect())
    }

    #[instrument(skip(self, filters))]
    async fn get_shadow_all(
        &self,
        filters: &[Filter],
    ) -> InternalResult<HashMap<String, EdgeShadow>> {
        let shadows = self.shadows.read().await;

        if !shadows.is_empty() {
            let mut result: HashMap<String, EdgeShadow> = shadows.clone();

            for filter in filters {
                match filter {
                    Filter::Offline(offline_timeout) => {
                        result.retain(|_, shadow| shadow.is_offline(*offline_timeout));
                    }
                    Filter::OutOfSync => {
                        result.retain(|_, shadow| !shadow.is_synced());
                    }
                }
            }
            return Ok(result);
        }
        Ok(HashMap::new())
    }

    #[instrument(skip(self, shadow))]
    async fn update_shadow(&self, shadow: EdgeShadow) -> InternalResult<()> {
        if let Some(existing_shadow) = self.shadows.write().await.get_mut(&shadow.edge_id) {
            *existing_shadow = shadow;
        } else {
            // TODO fix this registration-basis
            // We tentatively allow registering non-existing shadows by inserting them.
            self.shadows
                .write()
                .await
                .insert(shadow.edge_id.clone(), shadow);
        }
        Ok(())
    }

    #[instrument(skip(self, edge_id))]
    async fn delete_shadow(&self, edge_id: &str) -> InternalResult<()> {
        let removed = self.shadows.write().await.remove(edge_id);
        if removed.is_none() {
            return Err((
                format!("Shadow not found: '{}')", edge_id),
                ouro_core::error::InternalErrorKind::NoSuchKey,
            )
                .into());
        }
        Ok(())
    }
}
