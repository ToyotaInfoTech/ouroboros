// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Edge-side state reporter for Device Shadow.
//!
//! This module provides periodic state publishing functionality for edge devices,
//! enabling them to report their current status to the daemon via Zenoh.

use async_trait::async_trait;
use ouro_common::io::ToCbor as _;
use ouro_common::topic;
use ouro_common::types::{Payload, Topic};
use ouro_common::unix_epoch_now;
use ouro_core::comm::messaging::{Publisher, Response, RpcHandle};
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::client::{Callable as _, RetryableClient};
use ouro_core::runtime::comp::{CompContext, ComponentImpl, ComponentImplExt};
use ouro_core::runtime::context::ResourceContext;
use ouro_core::runtime::types::{ActionFactory, SharableAction};
use ouro_core::{define_resource_trigger_action, get_system_info, InternalResult};
use ouro_edge::runtime::context::EdgeResourceContextImpl;
use ouro_schema::schema::shadow::report::TaskState;
use ouro_schema::schema::shadow::EdgeStateReport;
use ouro_state::edge::{EdgeTaskSpec, TriggerStrategySpec, UploadDataSpec};
use std::sync::Arc;
use std::vec;
use tokio::sync::RwLock;
use tracing::instrument;

use crate::types::BuiltEdgeTask;

/// State report task for edge devices.
///
/// Manages periodic publication of device state to the daemon. The reporter
/// monitors configuration changes and adjusts the publish interval dynamically.
/// State updates are published via Zenoh to the daemon's status subscriber.
#[derive(Debug, Clone)]
pub struct StateReportTask {
    report: Arc<RwLock<EdgeStateReport>>,
}

#[async_trait]
impl ComponentImpl for StateReportTask {
    type StateImpl = EdgeTaskSpec;
    type ContextImpl = EdgeResourceContextImpl;

    #[instrument(skip(self, state, context))]
    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<tokio::task::JoinHandle<()>>> {
        ComponentImplExt::on_active(self, state, context).await
    }
}

#[async_trait]
impl ComponentImplExt for StateReportTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = Vec::new();

        match state {
            EdgeTaskSpec::DataUpload {
                enabled,
                data,
                with_client,
                behavior,
                ..
            } => {
                if !enabled {
                    return Ok(vec![]);
                }

                // Check that the target data is State
                if data != &UploadDataSpec::State {
                    return Err((
                        "Invalid data type for StateReporterTask",
                        InternalErrorKind::TypeConversion,
                    )
                        .into());
                }

                let triggers = behavior.trigger.values().cloned();
                for trigger in triggers {
                    let context = context.clone();
                    match trigger {
                        TriggerStrategySpec::Periodic {
                            enabled,
                            interval_secs,
                            ..
                        } => {
                            if !enabled {
                                continue;
                            }

                            let client = context.with_client(with_client).await?;
                            let client = client.as_retryable()?;
                            let edge_id = context.root().lock().await.runtime.edge_id.clone();
                            let topic = topic::status_key(edge_id.as_str());
                            let session = client.transport()?.into_messaging()?;
                            let publisher = session.publisher(&topic).await?;

                            let report = self.report.clone();

                            let action = SharableAction::Periodic {
                                interval_secs,
                                factory: {
                                    let factory: ActionFactory = Arc::new(move || {
                                        Box::pin(_periodic_state_report_task(
                                            edge_id.clone(),
                                            client.clone(),
                                            publisher.clone(),
                                            report.clone(),
                                            context.clone(),
                                        ))
                                    });
                                    factory
                                },
                            };

                            actions.push(action);
                        }
                        TriggerStrategySpec::Instant { enabled, .. } => {
                            if !enabled {
                                continue;
                            }
                            let client = context.with_client(with_client).await?;
                            let client = client.as_retryable()?;
                            let session = client.transport()?.into_messaging()?;

                            let edge_id = context.root().lock().await.runtime.edge_id.clone();
                            let topic = topic::status_key(edge_id.as_str());
                            let report = self.report.clone();
                            let rpc_server = session.topic_server(&topic).await?;

                            let action = define_resource_trigger_action!(
                                with {
                                    let rpc_server = rpc_server.clone();
                                    let edge_id = edge_id.clone();
                                    let topic = topic.clone();
                                    let report = report.clone();
                                }
                                when: let result = rpc_server.recv_async().await,
                                then: _request_based_report_task(edge_id, topic, report, result)
                            );

                            actions.push(action);
                        }
                        _ => {
                            return Err((
                                "StateReporterTask only supports Periodic and Instant trigger strategies",
                                InternalErrorKind::TypeConversion,
                            )
                                .into());
                        }
                    }
                }
            }
            _ => {
                return Err((
                    "Invalid task config for StateReportTask",
                    InternalErrorKind::InvalidConfig,
                )
                    .into());
            }
        }

        Ok(actions)
    }
}

impl StateReportTask {
    /// Create a new state report task.
    ///
    /// # Arguments
    ///
    /// * `report` - Shared state report to be published
    ///
    /// # Returns
    ///
    /// A new `StateReportTask` instance ready to be started
    #[instrument(skip(report))]
    pub fn new(report: Arc<RwLock<EdgeStateReport>>) -> Arc<Self> {
        Arc::new(StateReportTask { report })
    }
}

/// Core state reporting loop that publishes device state at configured intervals.
///
/// This async function runs indefinitely, periodically publishing the edge device's
/// current state to the daemon via Zenoh. It automatically updates timestamps before
/// each publish and dynamically adjusts the publish interval when configuration changes.
///
/// # Arguments
///
/// * `edge_id` - Unique edge device identifier
/// * `outbox` - Zenoh outbox for declaring publishers and sending messages
/// * `current_config` - Shared configuration containing the publish interval
///
/// # Returns
///
/// `Result<()>` - Returns Ok on successful initialization, though the loop runs indefinitely
///
/// # Errors
///
/// Returns error if publisher declaration fails
#[instrument(skip(client, publisher, report, context))]
async fn _periodic_state_report_task(
    edge_id: String,
    client: Arc<RetryableClient>,
    publisher: Arc<dyn Publisher>,
    report: Arc<RwLock<EdgeStateReport>>,
    context: Arc<CompContext<StateReportTask>>,
) -> InternalResult<()> {
    _update_report(report.clone(), context).await?;
    let cbor_bytes = _as_cbor_bytes(report.clone()).await?;

    tracing::debug!(edge_id = %edge_id, length = %cbor_bytes.len(), "Publishing cbor data");

    client
        .call(|_endpoint| async move {
            // Publish the state report
            match publisher.put(&cbor_bytes).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to publish state report");
                    Err(e)
                }
            }
        })
        .await?;

    Ok(())
}

async fn _update_report(
    report: Arc<RwLock<EdgeStateReport>>,
    context: Arc<CompContext<StateReportTask>>,
) -> InternalResult<()> {
    let mut _guard = report.write().await;

    // Snapshot the current root state metadata and strip it (id+epoch only).
    _guard.current_state = context.root().lock().await.metadata.clone();
    _guard.current_state.epoch = _guard.current_state.epoch.stripped();

    // Set desired_state from TagWatchTask's last received intent (stripped).
    // If the desired state matches current_state, set to None.
    {
        let ds = context.tag_desired_state.read().await;
        _guard.desired_state = ds.as_ref().map(|tags| {
            let epoch = tags.stripped();
            ouro_state::core::StateMetadata {
                schema_version: _guard.current_state.schema_version,
                epoch,
            }
        });
        // If desired == current, no need to report desired.
        if _guard.desired_state.as_ref().map(|d| &d.epoch) == Some(&_guard.current_state.epoch) {
            _guard.desired_state = None;
        }
    }

    // Update timestamp and system info before publishing
    _guard.timestamp = unix_epoch_now();
    _guard.system_info = get_system_info()?;
    _guard.status = *context.status.lock().await;

    _guard.task_states.clear();
    for (name, task) in context.task_map.lock().await.iter() {
        match BuiltEdgeTask::try_from_any_sharable(task.clone()) {
            Ok(built) => {
                _guard.task_states.push(TaskState {
                    name: name.clone(),
                    status: built.comp_status().await.into(),
                });
            }
            Err(e) => {
                tracing::error!(
                    task = %name,
                    error = %e,
                    "Failed to downcast task to BuiltEdgeTask for status reporting",
                );
            }
        }
    }

    Ok(())
}

#[instrument(skip(report))]
async fn _as_cbor_bytes(report: Arc<RwLock<EdgeStateReport>>) -> InternalResult<Vec<u8>> {
    let mut _guard = report.read().await;

    let cbor_bytes = _guard.to_cbor().map_err(|e| {
        (
            format!("Failed to serialize state report to CBOR: {}", e),
            e,
        )
            .into()
    })?;

    Ok(cbor_bytes)
}

#[instrument(skip(report, result))]
async fn _request_based_report_task(
    edge_id: String,
    topic: Topic,
    report: Arc<RwLock<EdgeStateReport>>,
    result: InternalResult<Arc<dyn RpcHandle>>,
) -> InternalResult<()> {
    let topic = topic.clone();
    match result {
        Ok(handle) => {
            tracing::info!(edge_id = %edge_id, "Received state report request: {:?}", handle);
            // Handle the request and send the current state
            let cbor_bytes = Payload(_as_cbor_bytes(report.clone()).await?);
            match handle.response(&Response::Success(topic, cbor_bytes)).await {
                Ok(_) => {
                    tracing::debug!(edge_id = %edge_id, "Edge replied to state request");
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "Edge '{}' failed to reply to state request",
                        edge_id
                    );
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to receive state report request");
        }
    }
    Ok(())
}
