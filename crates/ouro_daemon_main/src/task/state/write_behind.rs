// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Write-behind caching implementation for device shadow storage.
//!
//! This module provides a write-behind (write-back) caching strategy that:
//! - Immediately writes to fast in-memory transient storage
//! - Asynchronously persists changes to the backend database
//! - Maintains per-edge ordering guarantees via topic-based queuing
//!
//! This approach optimizes for low-latency reads and writes while ensuring
//! eventual durability of shadow state.

use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use async_trait::async_trait;
use completion::CompletionFuture;
use derivative::Derivative;
use ouro_core::{
    runtime::{executor, rdb},
    InternalResult,
};
use ouro_schema::schema::shadow::EdgeShadow;
use tracing::instrument;

use crate::task::state::{
    persistent::PersistentShadowStore,
    store::{self, ShadowStore},
    transient::TransientShadowStore,
};

/// Wraps a Send Future into a Send CompletionFuture for the executor.
/// This is a lightweight helper that avoids defining a separate wrapper struct.
fn into_completion_future<F>(
    future: F,
) -> Pin<Box<dyn CompletionFuture<Output = InternalResult<Option<EdgeShadow>>> + Send>>
where
    F: Future<Output = InternalResult<Option<EdgeShadow>>> + Send + 'static,
{
    struct Wrapper<F>(F);

    impl<F> CompletionFuture for Wrapper<F>
    where
        F: Future<Output = InternalResult<Option<EdgeShadow>>> + Send,
    {
        type Output = InternalResult<Option<EdgeShadow>>;

        unsafe fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.map_unchecked_mut(|s| &mut s.0).poll(cx)
        }

        unsafe fn poll_cancel(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            Poll::Ready(())
        }
    }

    Box::pin(Wrapper(future))
}

/// Write-behind shadow store that caches writes in transient storage
/// and asynchronously persists them to the backend database.
///
/// This store provides a two-tier caching strategy:
/// 1. **Transient layer**: Fast in-memory storage for immediate access
/// 2. **Persistent layer**: Durable database storage via async background writes
///
/// Write operations return immediately after updating the transient layer,
/// while persistence happens asynchronously in the background. Read operations
/// are served entirely from the transient layer for low latency.
///
/// # Ordering Guarantees
///
/// Operations for the same `edge_id` are queued and executed in order,
/// ensuring consistency for per-device state.
///
/// # Example
///
/// ```ignore
/// let backend = Arc::new(rdb::Rdb::new(pool));
/// let store = WriteBehindShadowStore::new(backend);
///
/// // Insert returns immediately after transient write
/// store.insert("edge-1".to_string(), shadow).await?;
///
/// // Read from transient layer (fast)
/// let shadow = store.get("edge-1").await?;
/// ```
#[derive(Derivative, Clone)]
#[derivative(Debug)]
#[allow(clippy::type_complexity)]
pub struct WriteBehindShadowStore {
    pub name: String,
    /// In-memory cache for fast read/write access.
    transient: TransientShadowStore,
    /// Persistent storage backend for durable writes.
    persistent: PersistentShadowStore,
    /// Executor for async persistence operations.
    /// `Box<dyn CompletionFuture>` is required for type erasure since each
    /// completion_async_move! generates a different concrete type.
    /// Output type includes `Option<DeviceShadow>` for future extensibility.
    #[derivative(Debug = "ignore")]
    executor: Arc<
        executor::TopicQueueExecutor<
            Pin<Box<dyn CompletionFuture<Output = InternalResult<Option<EdgeShadow>>> + Send>>,
        >,
    >,
}

impl WriteBehindShadowStore {
    /// Creates a new write-behind shadow store.
    ///
    /// # Arguments
    ///
    /// * `name` - Base name for the shadow store components
    ///
    /// # Returns
    ///
    /// A new `WriteBehindShadowStore` instance with initialized transient
    /// and persistent layers.
    #[instrument(skip(name, rdb))]
    pub(crate) fn new(name: String, rdb: Arc<rdb::Rdb<sqlx::postgres::Postgres>>) -> Self {
        let transient = TransientShadowStore::new(format!("{}-transient", name));
        let persistent = PersistentShadowStore::new(format!("{}-persistent", name), rdb);
        let executor = Arc::new(
            executor::TopicQueueExecutorBuilder::new()
                .idle_queue_eviction_time(Duration::from_millis(10000))
                .build(),
        );
        Self {
            name,
            transient,
            persistent,
            executor,
        }
    }
}

#[async_trait]
impl ShadowStore for WriteBehindShadowStore {
    #[instrument(skip(self, shadow))]
    async fn insert_shadow(&self, shadow: EdgeShadow) -> InternalResult<()> {
        let persistent = self.persistent.clone();
        let shadow_clone = shadow.clone();

        let edge_id_submit = shadow.edge_id.clone();

        self.executor.submit(
            edge_id_submit.as_str(),
            into_completion_future(async move {
                persistent.insert_shadow(shadow_clone).await?;
                Ok(None)
            }),
        );

        self.transient.insert_shadow(shadow).await
    }

    #[instrument(skip(self, edge_id))]
    async fn get_shadow(&self, edge_id: &str) -> InternalResult<Option<EdgeShadow>> {
        self.transient.get_shadow(edge_id).await
    }

    #[instrument(skip(self))]
    async fn list_shadow_ids(&self) -> InternalResult<Vec<String>> {
        self.transient.list_shadow_ids().await
    }

    #[instrument(skip(self, filters))]
    async fn get_shadow_all(
        &self,
        filters: &[store::Filter],
    ) -> InternalResult<HashMap<String, EdgeShadow>> {
        self.transient.get_shadow_all(filters).await
    }

    #[instrument(skip(self, shadow))]
    async fn update_shadow(&self, shadow: EdgeShadow) -> InternalResult<()> {
        let persistent = self.persistent.clone();
        let shadow_clone = shadow.clone();

        let edge_id_submit = shadow.edge_id.clone();
        self.executor.submit(
            edge_id_submit.as_str(),
            into_completion_future(async move {
                persistent.update_shadow(shadow_clone).await?;
                Ok(None)
            }),
        );

        self.transient.update_shadow(shadow).await
    }

    #[instrument(skip(self, edge_id))]
    async fn delete_shadow(&self, edge_id: &str) -> InternalResult<()> {
        let persistent = self.persistent.clone();

        let edge_id_submit = edge_id.to_string();

        self.executor.submit(
            edge_id,
            into_completion_future(async move {
                persistent.delete_shadow(&edge_id_submit).await?;
                Ok(None)
            }),
        );

        self.transient.delete_shadow(edge_id).await
    }
}
