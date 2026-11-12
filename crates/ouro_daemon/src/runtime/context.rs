// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, ops::Deref, sync::Arc};

use crate::runtime::types::BuiltMetricsStore;
use async_trait::async_trait;
use ouro_common::types::AnySharable;
use ouro_core::runtime::context::ResourceContextImpl;
use ouro_core::runtime::{metrics::store::MetricsStore as _, types::BuiltMetricsProvider};
use ouro_core::{error::InternalErrorKind, InternalResult};
use ouro_schema::schema::agent_control::AgentCommand;
use ouro_state::daemon::{DaemonSpec, DaemonTaskSpec, RepositorySpec, WithRepository};
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use tracing::instrument;

/// Factory trait for creating daemon task components.
///
/// Implemented externally (e.g., in `ouro_daemon_task`) to decouple concrete
/// task types from the daemon infrastructure. `ouro_daemon` never references
/// concrete task types directly.
#[async_trait]
pub trait DaemonTaskFactory: Send + Sync {
    /// Creates a task component from the given spec.
    ///
    /// # Arguments
    /// * `spec` - Task specification
    /// * `context` - Resource context
    ///
    /// # Returns
    /// An opaque Arc that keeps the created task alive.
    async fn create_task(
        &self,
        spec: &DaemonTaskSpec,
        context: &Arc<DaemonResourceContextImpl>,
    ) -> InternalResult<AnySharable>;
}

#[async_trait]
pub trait RepositoryFactory: Send + Sync {
    async fn create_repository(
        &self,
        spec: &RepositorySpec,
        context: &Arc<DaemonResourceContextImpl>,
    ) -> InternalResult<AnySharable>;
}

#[async_trait]
pub trait DaemonResourceContext {
    async fn with_repository(
        self: &Arc<Self>,
        with_repository: &WithRepository,
    ) -> InternalResult<AnySharable>;
}

/// A daemon-specific resource context that extends [`ResourceContextImpl`]
/// with metrics, repositories, and task lifecycle management.
pub struct DaemonResourceContextImpl {
    current_state: Arc<Mutex<DaemonSpec>>,
    metrics_store: Arc<BuiltMetricsStore>,
    metrics_provider: Arc<BuiltMetricsProvider>,
    inner: Arc<ResourceContextImpl>,
    pub repository_map: Mutex<HashMap<String, AnySharable>>,
    pub task_map: Mutex<HashMap<String, AnySharable>>,
    repository_factory: Arc<dyn RepositoryFactory>,
    task_factory: Arc<dyn DaemonTaskFactory>,
}

impl Deref for DaemonResourceContextImpl {
    type Target = ResourceContextImpl;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DaemonResourceContextImpl {
    /// Creates a new DaemonResourceContext with an injected task factory.
    ///
    /// # Arguments
    /// * `current_state` - Daemon configuration spec
    /// * `repository_factory` - Factory for creating repository components
    /// * `task_factory` - Factory for creating task components
    ///
    /// # Returns
    /// * `Arc<Self>` - New resource context

    #[instrument(skip(current_state, repository_factory, task_factory))]
    pub fn new(
        current_state: &DaemonSpec,
        repository_factory: Arc<dyn RepositoryFactory>,
        task_factory: Arc<dyn DaemonTaskFactory>,
        command_tx: Sender<AgentCommand>,
    ) -> Arc<Self> {
        let metrics_store: Arc<BuiltMetricsStore> =
            Arc::new(current_state.metrics_store.clone().into());
        let metrics_provider = metrics_store.new_provider();
        Arc::new(Self {
            current_state: Arc::new(Mutex::new(current_state.clone())),
            metrics_store,
            metrics_provider,
            inner: ResourceContextImpl::new(command_tx),
            repository_map: Mutex::new(HashMap::new()),
            task_map: Mutex::new(HashMap::new()),
            repository_factory,
            task_factory,
        })
    }

    pub fn current_state(&self) -> Arc<Mutex<DaemonSpec>> {
        self.current_state.clone()
    }

    /// Returns a reference to the metrics store.
    pub fn metrics_store(&self) -> &Arc<BuiltMetricsStore> {
        &self.metrics_store
    }

    /// Returns a reference to the metrics provider.
    pub fn metrics_provider(&self) -> &Arc<BuiltMetricsProvider> {
        &self.metrics_provider
    }

    #[instrument(skip(self, spec))]
    pub async fn create_task(
        self: &Arc<Self>,
        spec: &DaemonTaskSpec,
    ) -> InternalResult<AnySharable> {
        self.task_factory.create_task(spec, self).await
    }
}

#[async_trait]
impl DaemonResourceContext for DaemonResourceContextImpl {
    #[instrument(skip(self, with_repository))]
    async fn with_repository(
        self: &Arc<Self>,
        with_repository: &WithRepository,
    ) -> InternalResult<AnySharable> {
        match with_repository {
            WithRepository::Def { def: repository } => {
                let built_repository = self
                    .repository_factory
                    .create_repository(repository, self)
                    .await?;

                self.repository_map
                    .lock()
                    .await
                    .insert(repository.name().to_string(), built_repository.clone());

                Ok(built_repository)
            }
            WithRepository::Ref {
                r#ref: repository_ref,
            } => {
                let guard = self.repository_map.lock().await;
                let repository = guard.get(repository_ref.as_str()).cloned().ok_or_else(|| {
                    (
                        format!("Repository reference '{repository_ref}' not found",).as_str(),
                        InternalErrorKind::InvalidConfig,
                    )
                        .into()
                })?;
                Ok(repository)
            }
        }
    }
}

#[async_trait]
impl RepositoryFactory for DaemonResourceContextImpl {
    #[instrument(skip(self, spec, context))]
    async fn create_repository(
        &self,
        spec: &RepositorySpec,
        context: &Arc<DaemonResourceContextImpl>,
    ) -> InternalResult<AnySharable> {
        self.repository_factory
            .create_repository(spec, context)
            .await
    }
}
