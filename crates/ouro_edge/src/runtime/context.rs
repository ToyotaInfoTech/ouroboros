// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use ouro_common::types::AnySharable;
use ouro_core::runtime::context::ResourceContextImpl;
use ouro_core::InternalResult;
use ouro_schema::schema::agent_control::AgentCommand;
use ouro_schema::schema::shadow::report::EdgeStatus;
use ouro_state::core::TagAssignment;
use ouro_state::edge::{EdgeSpec, EdgeTaskSpec};
use std::{collections::HashMap, ops::Deref, sync::Arc};
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock};
use tracing::instrument;

/// An edge-specific resource context that extends [`ResourceContextImpl`]
/// with edge root configuration and task lifecycle management.
pub struct EdgeResourceContextImpl {
    root: Arc<Mutex<EdgeSpec>>,
    /// Immutable snapshot of the EdgeSpec at startup.
    /// Used as the merge base in [`TagWatchTask`] so that removing all tags
    /// always reverts to the original configuration rather than to a
    /// previously-patched state.
    base_state: Arc<EdgeSpec>,
    inner_context: Arc<ResourceContextImpl>,
    pub task_map: Mutex<HashMap<String, AnySharable>>,
    task_factory: Arc<dyn EdgeTaskFactory>,
    pub status: Arc<Mutex<EdgeStatus>>,
    /// Shared desired state from TagWatchTask.
    /// Set by TagWatchTask when an EdgeStateSyncIntent is received,
    /// read by StateReportTask when generating reports.
    pub tag_desired_state: Arc<RwLock<Option<TagAssignment>>>,
}

impl Deref for EdgeResourceContextImpl {
    type Target = ResourceContextImpl;

    fn deref(&self) -> &Self::Target {
        &self.inner_context
    }
}

impl EdgeResourceContextImpl {
    pub fn new(
        root: &EdgeSpec,
        task_factory: Arc<dyn EdgeTaskFactory>,
        tx: Sender<AgentCommand>,
    ) -> Arc<Self> {
        Arc::new(Self {
            root: Arc::new(Mutex::new(root.clone())),
            base_state: Arc::new(root.clone()),
            inner_context: ResourceContextImpl::new(tx),
            task_map: Mutex::new(HashMap::new()),
            task_factory,
            status: Arc::new(Mutex::new(EdgeStatus::Starting)),
            tag_desired_state: Arc::new(RwLock::new(None)),
        })
    }

    pub fn root(&self) -> Arc<Mutex<EdgeSpec>> {
        self.root.clone()
    }

    /// Returns an immutable snapshot of the [`EdgeSpec`] captured at startup.
    ///
    /// This is the merge base used by `TagWatchTask` so that removing all tag
    /// assignments always reverts to the original TOML configuration rather than
    /// a previously-patched state.
    pub fn base_state(&self) -> Arc<EdgeSpec> {
        self.base_state.clone()
    }

    #[instrument(skip(self, state))]
    pub async fn create_task(
        self: &Arc<Self>,
        state: &EdgeTaskSpec,
    ) -> InternalResult<AnySharable> {
        self.task_factory.create_task(state, self).await
    }
}

/// Factory trait for creating edge task components.
///
/// Implemented externally (e.g., in `ouro_edge_task`) to decouple concrete
/// task types from the edge infrastructure. `ouro_edge` never references
/// concrete task types directly.
#[async_trait]
pub trait EdgeTaskFactory: Send + Sync {
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
        spec: &EdgeTaskSpec,
        context: &Arc<EdgeResourceContextImpl>,
    ) -> InternalResult<AnySharable>;
}
