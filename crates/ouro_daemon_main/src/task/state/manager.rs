// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use ouro_common::io::FromCbor as _;
use ouro_common::topic;
use ouro_core::comm::messaging::{self, TopicMessage};
use ouro_core::define_resource_trigger_action;
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::client::Callable as _;
use ouro_core::runtime::client::RetryableClient;
use ouro_core::runtime::comp::ComponentImplExt;
use ouro_core::runtime::comp::{CompContext, ComponentImpl};
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::types::{ActionFactory, SharableAction};
use ouro_core::InternalResult;
use ouro_daemon::runtime::context::DaemonResourceContext as _;
use ouro_daemon::runtime::context::DaemonResourceContextImpl;
use ouro_schema::schema::shadow::report::EdgeStatus;
use ouro_schema::schema::shadow::{EdgeShadow, EdgeStateReport};
use ouro_state::daemon::DaemonTaskSpec;
use ouro_webui::event::DashboardEvent;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::task::icc::register_icc_handler;
use crate::task::state::manager::icc::ShadowIccHandler;
use crate::task::state::store::BuiltShadowStore;
use crate::task::state::store::Filter;
use crate::task::state::store::ShadowStore as _;
use crate::task::state::tag_manager;
use crate::task::state::tag_store::BuiltTagStore;
use crate::types::BuiltRepository;

/// Core operations for device shadow management.
///
/// This trait defines the interface for managing device shadows, including
/// registration, configuration updates, querying, and filtering operations.
/// All methods return `impl Future` to support async operations while maintaining
/// trait object safety and composability.
#[async_trait]
trait ShadowManager {
    /// Register a new edge device with desired configuration.
    ///
    /// # Arguments
    /// * `edge_id` - Unique edge device identifier
    /// * `config` - Desired configuration
    ///
    /// # Returns
    /// * `Result<EdgeShadow>` - Ok with the newly created shadow on success
    async fn register_edge(
        edge_id: &str,
        store: Arc<BuiltShadowStore>,
    ) -> InternalResult<EdgeShadow>;
}

/// Shadow manager task for daemon.
///
/// This is the public-facing struct that manages the lifecycle of background tasks
/// for device shadow operations. It maintains DeviceShadow instances for all edge
/// devices and handles:
/// - Configuration distribution (daemon → edge)
/// - State collection (edge → daemon)
/// - Heartbeat monitoring
///
/// The actual business logic is delegated to `ShadowManagerInner`, which is wrapped
/// in an `Arc` to enable safe sharing across multiple async tasks.
#[derive(Clone)]
pub struct ShadowManagerTask {
    event_tx: broadcast::Sender<DashboardEvent>,
}

impl ShadowManagerTask {
    /// Create a new shadow manager task.
    pub(crate) fn new(event_tx: broadcast::Sender<DashboardEvent>) -> Arc<Self> {
        Arc::new(Self { event_tx })
    }

    /// Background task that subscribes to state updates from all edge devices.
    ///
    /// This method subscribes to the Zenoh topic pattern `edge/*/status` and processes
    /// reported state messages from edge devices. For each received state:
    /// 1. Deserializes the CBOR-encoded `ReportedState`
    /// 2. Updates the corresponding device shadow in the store
    /// 3. Auto-registers unknown devices with default configuration
    ///
    /// The loop runs indefinitely until an error occurs or the subscriber is closed.
    /// Deserialization errors are logged but do not terminate the loop.
    ///
    /// # Arguments
    ///
    /// * `uuid` - Unique identifier for the edge device
    /// * `context` - Arc-wrapped context for accessing the ShadowManagerTask
    /// * `manager` - Arc-wrapped manager for auto-registration of unknown devices
    /// * `session` - Zenoh session for subscribing to status updates
    /// * `store` - Storage backend for updating device shadows
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Should never return normally; only on subscriber error
    /// * `Err` - If subscription fails or subscriber encounters an error
    #[instrument(skip(result, _client, store, event_tx, tag_sync))]
    async fn _recv_state(
        result: InternalResult<TopicMessage>,
        _client: Arc<RetryableClient>,
        store: Arc<BuiltShadowStore>,
        event_tx: broadcast::Sender<DashboardEvent>,
        tag_sync: (Arc<BuiltTagStore>, Arc<RetryableClient>),
    ) -> InternalResult<()> {
        let request = match result {
            Ok(request) => request,
            Err(e) => {
                tracing::error!(error = %e, "ShadowManagerTask failed to receive state update");
                return Ok(());
            }
        };

        let report = match request {
            TopicMessage(_topic, payload) => match EdgeStateReport::from_cbor(&payload) {
                Ok(state) => state,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to decode reported state");
                    return Ok(());
                }
            },
        };

        let edge_id = report.edge_id.clone();
        let status = format!("{:?}", report.status);
        // Clone metadata before state is potentially moved into update_reported.
        let current_state = report.current_state.clone();
        let desired_state = report.desired_state.clone();
        tracing::debug!(edge_id = %edge_id, status = %status, current_state = ?current_state, desired_state = ?desired_state, "ShadowManager received state from edge");

        let shadow = store.get_shadow(&edge_id).await;

        if let Ok(Some(mut shadow)) = shadow {
            shadow.update_reported(report);
            store.update_shadow(shadow).await?;
        } else {
            tracing::warn!(
                edge_id = %edge_id,
                "Received state from unknown edge, registering it for now"
            );

            let mut shadow = Self::register_edge(&edge_id, store.clone()).await?;

            shadow.update_reported(report);
            store.update_shadow(shadow).await?;
        }

        let _ = event_tx.send(DashboardEvent::EdgeStateChanged {
            edge_id: edge_id.clone(),
            status,
        });

        {
            let (tag_store, client) = tag_sync;
            if let Err(e) = tag_manager::sync_state(
                &edge_id,
                &current_state,
                desired_state.as_ref(),
                tag_store,
                client,
            )
            .await
            {
                tracing::error!(edge_id = %edge_id, error = ?e, "Tag resync failed");
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ComponentImpl for ShadowManagerTask {
    type StateImpl = DaemonTaskSpec;
    type ContextImpl = DaemonResourceContextImpl;

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
impl ComponentImplExt for ShadowManagerTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = vec![];

        match state {
            DaemonTaskSpec::RepositoryIngestion {
                name,
                enabled,
                with_client,
                with_repository,
                with_tag_repository,
                heartbeat_timeout_secs,
                ..
            } => {
                if !enabled {
                    return Ok(vec![]);
                }

                let _this = self.clone();

                let client = context.with_client(with_client).await?;
                let client = Arc::<RetryableClient>::try_from(client.as_ref())?;
                let session: Arc<dyn messaging::Session> = client.transport()?.into_messaging()?;
                let topic = topic::PATTERN_STATUS;
                let subscriber = session.topic_pattern_subscriber(&topic).await?;

                let repository = context.with_repository(with_repository).await?;
                let repository = BuiltRepository::try_from_any_sharable(repository)?;

                let store2 = match repository.as_ref() {
                    BuiltRepository::ShadowStore(store) => store.clone(),
                    _ => {
                        return Err((
                            "ShadowManagerTask requires ShadowStore repository",
                            InternalErrorKind::InvalidConfig,
                        )
                            .into());
                    }
                };

                // Clone event_tx before moving self into closures below.
                let event_tx = self.event_tx.clone();
                let event_tx2 = self.event_tx.clone();

                // Optionally resolve tag store for automatic tag epoch resync.
                let tag_resync: (Arc<BuiltTagStore>, Arc<RetryableClient>) = {
                    let tag_repository = context.with_repository(with_tag_repository).await?;
                    let tag_repository = BuiltRepository::try_from_any_sharable(tag_repository)?;
                    match tag_repository.as_ref() {
                        BuiltRepository::TagStore(ts) => (ts.clone(), client.clone()),
                        _ => {
                            return Err((
                                "with_tag_repository does not resolve to a TagStore",
                                InternalErrorKind::InvalidConfig,
                            )
                                .into());
                        }
                    }
                };

                let action = define_resource_trigger_action!(
                    with {
                        let client = client.clone();
                        let subscriber = subscriber.clone();
                        let repository = repository.clone();
                        let event_tx = event_tx.clone();
                        let tag_resync = tag_resync.clone();
                        let store = match repository.as_ref() {
                            BuiltRepository::ShadowStore(store) => store.clone(),
                            _ => unreachable!("validated above"),
                        };
                    }
                    when: let result = client
                        .call(move |_transport| async move { subscriber.recv_async().await })
                        .await,
                    then: Self::_recv_state(
                        result,
                        client,
                        store,
                        event_tx,
                        tag_resync,
                    )
                );
                actions.push(action);

                {
                    let icc_handler = Arc::new(ShadowIccHandler {
                        store: store2.clone(),
                        session: session.clone(),
                    });
                    let action = register_icc_handler(&icc_handler, name, &context).await?;
                    actions.push(action);
                }

                {
                    // Periodic heartbeat timeout check: mark unresponsive edges as Down.
                    let timeout = Duration::from_secs(*heartbeat_timeout_secs);
                    let store_for_check = store2.clone();
                    let event_tx_for_check = event_tx2;
                    let heartbeat_check = SharableAction::Periodic {
                        interval_secs: *heartbeat_timeout_secs,
                        factory: {
                            let factory: ActionFactory = Arc::new(move || {
                                Box::pin(Self::_check_offline_edges(
                                    store_for_check.clone(),
                                    timeout,
                                    event_tx_for_check.clone(),
                                ))
                            });
                            factory
                        },
                    };
                    actions.push(heartbeat_check);
                }
            }
            _ => {
                return Err((
                    "ShadowManagerTask requires a shadow data store configuration",
                    InternalErrorKind::InvalidConfig,
                )
                    .into());
            }
        }

        Ok(actions)
    }
}

impl ShadowManagerTask {
    /// Marks edges that have not reported within `timeout` as `Down`.
    #[instrument(skip(store, event_tx))]
    async fn _check_offline_edges(
        store: Arc<BuiltShadowStore>,
        timeout: Duration,
        event_tx: broadcast::Sender<DashboardEvent>,
    ) -> InternalResult<()> {
        let offline = store.get_shadow_all(&[Filter::Offline(timeout)]).await?;
        for (edge_id, mut shadow) in offline {
            let current_status = shadow.last_report.as_ref().map(|r| r.status);
            if current_status == Some(EdgeStatus::Down) {
                continue;
            }
            tracing::info!(
                edge_id = %edge_id,
                "Edge heartbeat timed out, marking as Down"
            );
            if let Some(ref mut report) = shadow.last_report {
                report.status = EdgeStatus::Down;
            }
            store.update_shadow(shadow).await?;
            let _ = event_tx.send(DashboardEvent::EdgeStateChanged {
                edge_id: edge_id.clone(),
                status: "Down".to_string(),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl ShadowManager for ShadowManagerTask {
    /// Register a new edge device with desired configuration.
    ///
    /// # Arguments
    /// * `edge_id` - Unique edge device identifier
    /// * `config` - Desired configuration
    ///
    /// # Returns
    /// * `Result<()>` - Ok on success
    #[instrument(skip(store))]
    async fn register_edge(
        edge_id: &str,
        store: Arc<BuiltShadowStore>,
    ) -> InternalResult<EdgeShadow> {
        tracing::info!("ShadowManager registering edge: {}", edge_id.to_string());

        let shadow = EdgeShadow::new(edge_id);

        store.insert_shadow(shadow.clone()).await?;

        Ok(shadow)
    }
}

mod icc {
    use std::sync::Arc;

    use async_trait::async_trait;
    use ouro_common::{io::ToCbor as _, topic};
    use ouro_core::comm::messaging::Session;
    use ouro_schema::{
        operation_descriptions,
        schema::{
            agent_control::{AgentCommand, AgentCommandKind},
            cli::{
                icc::IccHandler, require_input, unsupported_operation, DescribeResp, Description,
                ListResp, OperateReq, OperateResp,
            },
        },
        Void,
    };
    use tracing::instrument;

    use crate::task::state::store::{BuiltShadowStore, ShadowStore as _};

    pub(super) struct ShadowIccHandler {
        pub(super) store: Arc<BuiltShadowStore>,
        pub(super) session: Arc<dyn Session>,
    }

    /// Canonical operation descriptions for ShadowIccHandler.
    ///
    /// This is the single source of truth for operation names, inputs, and
    /// documentation. Both `describe()` and `operate()` derive their behaviour
    /// from the list returned here.
    fn descriptions() -> Vec<Description> {
        operation_descriptions![
            /// Receive ICC requests for the shadow task.
            "list_shadow_ids" {
                inputs {}
                output {
                    /// List of all registered shadow IDs
                }
            },
            /// Get the shadow for a specific edge by its shadow ID.
            "get_shadow" {
                inputs {
                    /// Shadow ID to query
                    "shadow_id"
                }
                output {
                    /// The edge shadow corresponding to the requested shadow ID, if it exists
                }
            },
            /// Get all shadows.
            "get_all_shadows" {
                inputs {}
                output {
                    /// Map of all shadow IDs to their corresponding edge shadows
                }
            },
            /// Delete a shadow by its shadow ID.
            "delete_shadow" {
                inputs {
                    /// Shadow ID to delete
                    "shadow_id"
                }
                output {
                    /// "success" if the shadow was deleted, or an error message if the shadow ID does not exist
                }
            },
            /// Send a lifecycle command to an edge device.
            "send_command" {
                inputs {
                    /// Target edge ID to send the command to
                    "edge_id",
                    /// Command kind: one of "start", "shutdown", "restart", "suspend"
                    "command_kind"
                }
                output {
                    /// returns ok and shadow_id if the command was sent successfully, or error, shadow_id, and reason, if the command failed
                }
            }
        ]
    }

    #[async_trait]
    impl IccHandler for ShadowIccHandler {
        #[instrument(skip(self, _req))]
        async fn describe(&self, _req: Void) -> anyhow::Result<DescribeResp> {
            Ok(DescribeResp {
                descriptions: descriptions(),
            })
        }

        #[instrument(skip(self))]
        async fn operate(&self, req: OperateReq) -> anyhow::Result<OperateResp> {
            let descs = descriptions();
            match req.operation_name.as_str() {
                "list_shadow_ids" => {
                    let shadow_ids = self
                        .store
                        .list_shadow_ids()
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to list shadow IDs: {:?}", e))?;
                    tracing::debug!("Fetched shadow IDs for ICC request: {:?}", shadow_ids);
                    let output = serde_value::to_value(shadow_ids)
                        .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?;
                    Ok(OperateResp { output })
                }
                "get_shadow" => {
                    let shadow_id =
                        require_input(&req.inputs, "shadow_id", &req.operation_name, &descs)?;
                    let shadow = self.store.get_shadow(shadow_id).await.map_err(|e| {
                        anyhow::anyhow!("Failed to get shadow '{}': {:?}", shadow_id, e)
                    })?;
                    tracing::debug!(
                        "Fetched shadow for ICC request (shadow_id={}): {:?}",
                        shadow_id,
                        shadow
                    );
                    let output = serde_value::to_value(shadow)
                        .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?;
                    Ok(OperateResp { output })
                }
                "get_all_shadows" => {
                    let shadows = self
                        .store
                        .get_shadow_all(&[])
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to get all shadows: {:?}", e))?;
                    tracing::debug!("Fetched all {} shadows for ICC request", shadows.len());
                    let output = serde_value::to_value(shadows)
                        .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?;
                    Ok(OperateResp { output })
                }
                "delete_shadow" => {
                    let shadow_id =
                        require_input(&req.inputs, "shadow_id", &req.operation_name, &descs)?;
                    let output = match self.store.delete_shadow(shadow_id).await {
                        Ok(()) => {
                            tracing::debug!(
                                "Deleted shadow for ICC request (shadow_id={})",
                                shadow_id
                            );
                            let mut map = std::collections::HashMap::new();
                            map.insert("result", "ok".to_string());
                            map.insert("shadow_id", shadow_id.to_string());
                            serde_value::to_value(map)
                                .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                ouro_core::error::InternalErrorKind::NoSuchKey
                            ) =>
                        {
                            tracing::debug!(
                                "Shadow not found for delete ICC request (shadow_id={})",
                                shadow_id
                            );
                            let mut map = std::collections::HashMap::new();
                            map.insert("result", "error".to_string());
                            map.insert("shadow_id", shadow_id.to_string());
                            map.insert("reason", "shadow not found".to_string());
                            serde_value::to_value(map)
                                .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "Failed to delete shadow '{}': {:?}",
                                shadow_id,
                                e
                            ));
                        }
                    };
                    Ok(OperateResp { output })
                }
                "send_command" => {
                    let edge_id =
                        require_input(&req.inputs, "edge_id", &req.operation_name, &descs)?;
                    let command_kind_str =
                        require_input(&req.inputs, "command_kind", &req.operation_name, &descs)?;

                    let kind = match command_kind_str {
                        "start" => AgentCommandKind::Start,
                        "shutdown" => AgentCommandKind::Shutdown,
                        "restart" => AgentCommandKind::Restart,
                        "suspend" => AgentCommandKind::Suspend,
                        other => {
                            return Err(anyhow::anyhow!(
                                "Unknown command_kind '{}': expected one of start, shutdown, restart, suspend",
                                other
                            ))
                        }
                    };
                    let command = AgentCommand::new(kind);

                    let publisher = self
                        .session
                        .publisher(&topic::command_key(edge_id))
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to create publisher for edge '{}': {:?}",
                                edge_id,
                                e
                            )
                        })?;

                    let cbor_bytes = command.to_cbor().map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to serialize command for edge '{}': {:?}",
                            edge_id,
                            e
                        )
                    })?;

                    publisher.put(&cbor_bytes).await.map_err(|e| {
                        anyhow::anyhow!("Failed to publish command to edge '{}': {:?}", edge_id, e)
                    })?;

                    Ok(OperateResp {
                        output: serde_value::to_value(Void {})?,
                    })
                }
                _ => Err(unsupported_operation(&req.operation_name, &descs)),
            }
        }

        #[instrument(skip(self, _req))]
        async fn list(&self, _req: Void) -> anyhow::Result<ListResp> {
            Err(anyhow::anyhow!(
                "List operation not supported for '{}'",
                stringify!(ShadowIccHandler)
            ))
        }
    }
}
