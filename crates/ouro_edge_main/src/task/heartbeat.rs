// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use async_trait::async_trait;
use ouro_core::comm::messaging::Publisher;
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::client::RetryableClient;
use ouro_core::runtime::types::{ActionFactory, SharableAction};
use ouro_core::InternalResult;
use ouro_edge::runtime::context::EdgeResourceContextImpl;
use tokio::task::JoinHandle;

use ouro_common::topic;
use ouro_core::runtime::client::Callable as _;
use ouro_core::runtime::comp::{CompContext, ComponentImpl, ComponentImplExt};
use ouro_core::runtime::context::ResourceContext as _;
use ouro_state::edge::{EdgeTaskSpec, TriggerStrategySpec, UploadDataSpec};
use tracing::instrument;

#[allow(dead_code)] // API
#[derive(Debug, Clone)]
pub struct HeartbeatTask {
    name: String,
}

impl HeartbeatTask {
    #[instrument(skip(name))]
    pub fn new(name: &str) -> Arc<Self> {
        Arc::new(Self { name: name.into() })
    }
}

#[async_trait]
impl ComponentImpl for HeartbeatTask {
    type StateImpl = EdgeTaskSpec;
    type ContextImpl = EdgeResourceContextImpl;

    #[instrument(skip(self, state, context))]
    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>> {
        ComponentImplExt::on_active(self, state, context).await
    }
}

#[async_trait]
impl ComponentImplExt for HeartbeatTask {
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

                if data != &UploadDataSpec::Heartbeat {
                    return Err((
                        "Invalid data type for HeartbeatTask",
                        InternalErrorKind::TypeConversion,
                    )
                        .into());
                }

                let triggers = behavior.trigger.values().cloned();
                for trigger in triggers {
                    match trigger {
                        TriggerStrategySpec::Periodic {
                            enabled,
                            interval_secs,
                            ..
                        } => {
                            if !enabled {
                                continue;
                            }

                            let (client, publisher) = {
                                let client = context.with_client(with_client).await?;
                                let client = client.as_retryable()?;

                                // Get edge ID and construct topic
                                let edge_id = context.root().lock().await.runtime.edge_id.clone();
                                let topic = topic::liveness_status_key(edge_id.as_str());

                                let session = client.transport()?.into_messaging()?;
                                let publisher = session.publisher(&topic).await?;

                                (client, publisher)
                            };

                            let action = SharableAction::Periodic {
                                interval_secs,
                                factory: {
                                    let factory: ActionFactory = Arc::new(move || {
                                        Box::pin(_heartbeat_task(client.clone(), publisher.clone()))
                                    });
                                    factory
                                },
                            };

                            actions.push(action);
                        }
                        _ => {
                            return Err((
                                "HeartbeatTask only supports Periodic trigger strategy",
                                InternalErrorKind::TypeConversion,
                            )
                                .into());
                        }
                    }
                }
            }
            _ => {
                return Err((
                    "Invalid task config for HeartbeatTask",
                    InternalErrorKind::InvalidConfig,
                )
                    .into());
            }
        }

        Ok(actions)
    }
}

/// Internal heartbeat task implementation
///
/// Publishes periodic heartbeat messages to Zenoh at the configured interval.
///
/// # Arguments
/// * `trigger` - Trigger strategy configuration
/// * `runtime_config` - Runtime configuration containing the edge ID
/// * `task_config` - Task configuration with interval settings
/// * `client` - Zenoh client for publishing
///
/// # Returns
/// * `Result<()>` - Never returns on success; runs indefinitely
///
/// # Errors
/// Returns an error if:
/// - Publisher declaration fails
/// - Client type is not TransientPublisher
#[instrument(skip(client, publisher))]
async fn _heartbeat_task(
    client: Arc<RetryableClient>,
    publisher: Arc<dyn Publisher>,
) -> InternalResult<()> {
    // Get current timestamp in milliseconds (consistent with UnixTimestamp semantics)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_millis() as u64;

    let publisher = publisher.clone();
    client
        .call(|_endpoint| async move {
            // Publish timestamp as heartbeat
            match publisher.put(now.to_le_bytes().as_slice()).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    tracing::error!("Failed to publish heartbeat: {:?}", e);
                    Err(e)
                }
            }
        })
        .await?;

    Ok(())
}
