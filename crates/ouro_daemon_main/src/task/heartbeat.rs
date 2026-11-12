// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Heartbeat monitoring for edge devices.
//!
//! Subscribes to heartbeat messages from edge devices and tracks their
//! liveness status by updating the system state database.

use ouro_common::{
    constant::{self},
    types::UnixTimestamp,
};
use ouro_core::{
    comm::messaging::TopicMessage,
    define_resource_trigger_action,
    error::{InternalError, InternalErrorKind},
    runtime::{
        client::Callable,
        comp::{CompContext, ComponentImpl, ComponentImplExt},
        metrics::{self, Latency, Throughput},
        types::SharableAction,
    },
    InternalResult,
};
use ouro_daemon::runtime::context::DaemonResourceContextImpl;
use ouro_schema::schema::shadow::EdgeShadow;
use ouro_state::daemon::{DaemonTaskSpec, IngestionDataSpec};
use ouro_webui::event::DashboardEvent;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::instrument;

use async_trait::async_trait;

use crate::task::state::store::{BuiltShadowStore, ShadowStore as _};
use crate::task::{icc::register_icc_handler, system_state};
use crate::types::BuiltRepository;
use ouro_common::topic;
use ouro_core::runtime::rdb;
use ouro_daemon::runtime::context::DaemonResourceContext as _;
use ouro_daemon::runtime::types::BuiltMetricsStore;

use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::metrics::Metrics as _;

/// Background task for monitoring edge device heartbeats.
///
/// Subscribes to heartbeat messages via Zenoh and:
/// - Updates the `system_state` table with the latest timestamp.
/// - Auto-registers unknown edges in the shadow store on first heartbeat.
/// - Updates `last_seen` on the shadow for known edges.
#[derive(Debug, Clone)]
pub struct HeartbeatTask {
    metrics_store: Arc<BuiltMetricsStore>,
    event_tx: broadcast::Sender<DashboardEvent>,
}

#[async_trait]
impl ComponentImpl for HeartbeatTask {
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
impl ComponentImplExt for HeartbeatTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = vec![];

        let metrics_store = self.metrics_store.clone();

        match state {
            DaemonTaskSpec::DataIngestion {
                name,
                enabled,
                with_data_source,
                with_client,
                data: IngestionDataSpec::Heartbeat { with_repository },
                ..
            } => {
                if !enabled {
                    return Ok(vec![]);
                }
                let this = self.clone();

                // Resolve shadow store for auto-registration.
                let repository = context.with_repository(with_repository).await?;
                let repository = BuiltRepository::try_from_any_sharable(repository)?;
                let shadow_store = match repository.as_ref() {
                    BuiltRepository::ShadowStore(store) => store.clone(),
                    _ => {
                        return Err((
                            "Invalid repository type for HeartbeatTask",
                            InternalErrorKind::TypeConversion,
                        )
                            .into())
                    }
                };

                {
                    let data_source = context.with_data_source(with_data_source).await?;
                    let rdb = data_source.as_postgres()?;

                    let client = context.with_client(with_client).await?;
                    let client = client.as_retryable()?;

                    let mut throughput = metrics::Throughput::new(
                        constant::daemon::PREFIX_METRICS_HEARTBEAT,
                        time::Duration::seconds(5),
                        metrics_store.clone(),
                    );
                    throughput.start();
                    let throughput = Arc::new(throughput);

                    let latency = Arc::new(metrics::Latency::new(
                        constant::daemon::PREFIX_METRICS_HEARTBEAT,
                        metrics_store.clone(),
                    ));
                    let session = client.transport()?.into_messaging()?;
                    let subscriber = session
                        .topic_pattern_subscriber(&topic::PATTERN_LIVENESS_STATUS)
                        .await?;
                    let continuous_errors = Arc::new(AtomicU32::new(0u32));
                    let event_tx = self.event_tx.clone();

                    let action = define_resource_trigger_action!(
                        with {
                            let subscriber = subscriber.clone();
                            let rdb = rdb.clone();
                            let throughput = throughput.clone();
                            let latency = latency.clone();
                            let continuous_errors = continuous_errors.clone();
                            let shadow_store = shadow_store.clone();
                            let event_tx = event_tx.clone();
                        }
                        when: let result = subscriber.recv_async().await,
                        then: _recv_heartbeat(result, rdb, throughput, latency, continuous_errors, shadow_store, event_tx)
                    );
                    actions.push(action);
                }

                actions.push(register_icc_handler(&this, name, &context).await?);
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

impl HeartbeatTask {
    /// Creates a new heartbeat monitoring task.
    ///
    /// # Arguments
    ///
    /// * `metrics_store` - Metrics store for tracking statistics
    /// * `event_tx` - Broadcast sender for dashboard events
    ///
    /// # Returns
    ///
    /// * `Self` - New heartbeat task instance
    #[instrument(skip(metrics_store, event_tx))]
    pub(crate) fn new(
        metrics_store: Arc<BuiltMetricsStore>,
        event_tx: broadcast::Sender<DashboardEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            metrics_store,
            event_tx,
        })
    }
}

/// Processes a single liveness heartbeat message.
///
/// - Updates the `system_state` table with the latest timestamp.
/// - If a shadow store is provided: auto-registers unknown edges on first
///   heartbeat, and updates `last_seen` for already-registered edges.
///
/// # Arguments
///
/// * `result` - Incoming topic message result
/// * `rdb` - PostgreSQL connection for `system_state` updates
/// * `throughput` - Throughput metrics tracker
/// * `latency` - Latency metrics tracker
/// * `continuous_errors` - Counter for consecutive errors
/// * `shadow_store` - Optional shadow store for edge auto-registration
/// * `event_tx` - Broadcast sender for dashboard events
#[instrument(skip(
    result,
    rdb,
    throughput,
    latency,
    continuous_errors,
    shadow_store,
    event_tx
))]
async fn _recv_heartbeat(
    result: InternalResult<TopicMessage>,
    rdb: Arc<rdb::Rdb<sqlx::postgres::Postgres>>,
    throughput: Arc<Throughput<BuiltMetricsStore>>,
    latency: Arc<Latency<BuiltMetricsStore>>,
    continuous_errors: Arc<AtomicU32>,
    shadow_store: Arc<BuiltShadowStore>,
    event_tx: broadcast::Sender<DashboardEvent>,
) -> InternalResult<()> {
    let start_time = std::time::Instant::now();

    let (edge_id, payload) = match result {
        Ok(TopicMessage(topic, payload)) => {
            let Some(edge_id) = topic.split('/').nth(1) else {
                // Increment continuous_errors counter if VIN is invalid
                continuous_errors.fetch_add(1, Ordering::SeqCst);
                tracing::error!("Invalid heartbeat topic expression: {}", topic);
                throughput.report_one(false).await;
                return Ok(());
            };

            let edge_id = edge_id.to_string();

            (edge_id, payload)
        }

        Err(e) => {
            continuous_errors.fetch_add(1, Ordering::SeqCst);
            throughput.report_one(false).await;
            tracing::error!("Error receiving heartbeat sample: {:?}", e);
            // Sleep briefly before retrying to avoid tight error loop
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            return Ok(());
        }
    };

    if payload.len() < 8 {
        continuous_errors.fetch_add(1, Ordering::SeqCst);
        tracing::error!(edge_id = %edge_id, len = payload.len(), "Heartbeat payload too short");
        throughput.report_one(false).await;
        return Ok(());
    }

    let ts_bytes: [u8; 8] = payload[..8].try_into().map_err(|_| {
        Into::<InternalError>::into((
            "Failed to parse heartbeat timestamp",
            InternalErrorKind::Parse,
        ))
    })?;
    let ts = UnixTimestamp::from(u64::from_le_bytes(ts_bytes) as i64);
    tracing::debug!(edge_id = %edge_id, ts=%ts, "received heartbeat");

    // Update shadow store: auto-register or touch last_seen.
    match shadow_store.get_shadow(&edge_id).await {
        Ok(Some(mut shadow)) => {
            // identified edge - update last_seen
            shadow.touch();

            if let Err(e) = shadow_store.update_shadow(shadow).await {
                tracing::error!(edge_id = %edge_id, error = ?e, "Failed to touch shadow last_seen");
            }
        }

        Ok(None) => {
            // unknown edge - auto-register in shadow store
            tracing::info!(edge_id = %edge_id, "First heartbeat — auto-registering edge in shadow store");

            let shadow = EdgeShadow::new(&edge_id);
            if let Err(e) = shadow_store.insert_shadow(shadow).await {
                tracing::error!(edge_id = %edge_id, error = ?e, "Failed to auto-register edge shadow");
            } else {
                let _ = event_tx.send(DashboardEvent::ShadowUpdated {
                    edge_id: edge_id.clone(),
                });
            }
        }

        Err(e) => {
            // Failed to query shadow store - log error but continue processing heartbeats
            tracing::error!(vin = %edge_id, error = ?e, "Failed to query shadow store");
        }
    }

    // Update system_state table (fire and forget).
    let continuous_errors_for_spawn = continuous_errors.clone();
    // fire and forget
    tokio::spawn(async move {
        let edge_id_moved = edge_id.clone();
        let result = rdb
            .transaction(|tx| async move {
                system_state::mark_update(tx, &edge_id_moved).await?;
                Ok(())
            })
            .await;

        match result {
            Ok(_) => {
                continuous_errors_for_spawn.store(0, Ordering::SeqCst);
                throughput.report_one(true).await;
                let elapsed = std::time::Instant::now() - start_time;
                latency.report_one(elapsed).await;
            }
            Err(e) => {
                continuous_errors_for_spawn.fetch_add(1, Ordering::SeqCst);
                throughput.report_one(false).await;
                tracing::error!(edge_id = %edge_id, error = ?e, "Error updating heartbeat");
            }
        }
    });

    Ok(())
}

#[instrument(skip(config))]
#[allow(clippy::result_large_err)]
fn _get_max_continuous_errors(config: &DaemonTaskSpec) -> InternalResult<u32> {
    match config {
        DaemonTaskSpec::DataIngestion { behavior, .. } => Ok(behavior.max_continuous_errors),
        _ => Err((
            "Invalid task config for HeartbeatTask",
            InternalErrorKind::TypeConversion,
        )
            .into()),
    }
}

mod icc {
    use std::sync::{Arc, LazyLock};

    use async_trait::async_trait;
    use futures::lock::Mutex;
    use ouro_common::constant;
    use ouro_core::runtime::metrics::provider::MetricsProvider;
    use ouro_core::runtime::metrics::store::MetricsStore;
    use ouro_core::runtime::types::BuiltMetricsProvider;
    use ouro_schema::operation_descriptions;
    use ouro_schema::schema::cli::{
        icc::IccHandler, unsupported_operation, DescribeResp, Description,
    };
    use ouro_schema::schema::cli::{ListResp, OperateReq, OperateResp};
    use ouro_schema::Void;
    use tracing::instrument;

    use crate::task::heartbeat::HeartbeatTask;

    /// Canonical operation descriptions for HeartbeatTask.
    fn descriptions() -> Vec<Description> {
        operation_descriptions![
            /// Receive ICC requests for the heartbeat task.
            "get_metrics" {
                inputs {}
                output {
                    /// List of metric keys currently tracked by the heartbeat task.
                }
            }
        ]
    }

    #[async_trait]
    impl IccHandler for HeartbeatTask {
        #[instrument(skip(self, _req))]
        async fn describe(&self, _req: Void) -> anyhow::Result<DescribeResp> {
            Ok(DescribeResp {
                descriptions: descriptions(),
            })
        }

        #[instrument(skip(self))]
        async fn operate(&self, req: OperateReq) -> anyhow::Result<OperateResp> {
            static METRICS_PROVIDER: LazyLock<Mutex<Option<Arc<BuiltMetricsProvider>>>> =
                LazyLock::new(|| Mutex::new(None));

            let mut _guard = METRICS_PROVIDER.lock().await;
            if _guard.is_none() {
                tracing::debug!("Initializing metrics provider for ICC handler");
                *_guard = Some(self.metrics_store.new_provider());
            }
            let provider = _guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Metrics provider not initialized"))?;

            match req.operation_name.as_str() {
                "get_metrics" => {
                    let metrics = provider
                        .get_all_metrics(1000, Some(constant::daemon::PREFIX_METRICS_HEARTBEAT))
                        .await;
                    tracing::debug!("Fetched metrics for ICC request: {:?}", metrics);
                    let output = serde_value::to_value(metrics)
                        .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?;
                    Ok(OperateResp { output })
                }
                _ => Err(unsupported_operation(&req.operation_name, &descriptions())),
            }
        }

        #[instrument(skip(self, _req))]
        async fn list(&self, _req: Void) -> anyhow::Result<ListResp> {
            Err(anyhow::anyhow!(
                "List operation not supported for '{}'",
                stringify!(HeartbeatTask)
            ))
        }
    }
}
