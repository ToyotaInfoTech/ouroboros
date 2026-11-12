// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Telemetry metadata management for edge devices.
//!
//! Handles storage and querying of telemetry metadata from edge devices
//! using a TimescaleDB backend for time-series data.

use async_trait::async_trait;
use ouro_common::types::Payload;
use ouro_common::{constant, topic};
use ouro_core::comm::messaging::{self, RpcHandle, TopicMessage};
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::client::{Callable, RetryableClient};
use ouro_core::runtime::comp::{ComponentImpl, ComponentImplExt};
use ouro_core::runtime::metrics::{self, Latency, Throughput};
use ouro_core::runtime::types::SharableAction;
use ouro_core::{define_resource_trigger_action, InternalResult};
use ouro_daemon::runtime::context::DaemonResourceContextImpl;
use ouro_daemon::runtime::types::BuiltMetricsStore;
use ouro_schema::schema::traj::EventMetadata;
use ouro_state::daemon::{DaemonTaskSpec, IngestionDataSpec};
use sqlx::Postgres;
use std::sync::Arc;
use std::vec;
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::task::icc::register_icc_handler;
use crate::task::system_state;
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::metrics::Metrics as _;
use ouro_core::{
    runtime::comp::CompContext,
    runtime::{rdb, types::BuiltDataSource},
};

/// SQL queries for metadata table management.
pub(crate) mod sql {

    #[allow(dead_code)] // API
    pub(crate) const INSERT_METADATA: &str = r#"
        INSERT INTO telemetry(vin, start_time, end_time, model, metadata)
        VALUES ($1, $2, $3, $4, $5::jsonb)
        ON CONFLICT (vin, start_time) DO UPDATE SET model = EXCLUDED.model, metadata = EXCLUDED.metadata;
    "#;

    #[allow(dead_code)] // API
    pub(crate) const INSERT_CAMERA_IMAGE_BULK: &str = r#"
        INSERT INTO camera_image(vin, start_time, camera_id, ts, image)
        SELECT * FROM UNNEST(
            $1::text[],
            $2::timestamptz[],
            $3::text[],
            $4::timestamptz[],
            $5::bytea[]
        )
        ON CONFLICT DO NOTHING;
    "#;
}

/// Database manager for telemetry metadata.
///
/// Manages a TimescaleDB hypertable for storing time-series telemetry
/// metadata from edge devices.
pub(crate) struct MetadataDB {
    /// Database connection pool.
    rdb: Arc<rdb::Rdb<Postgres>>,
}

impl MetadataDB {
    /// Creates a new metadata database manager.
    ///
    /// Initializes the database schema including the telemetry table,
    /// TimescaleDB hypertable, and necessary indexes.
    ///
    /// # Arguments
    ///
    /// * `rdb` - Database connection pool
    ///
    /// # Returns
    ///
    /// * `Result<Self>` - New metadata DB instance on success
    #[instrument(skip(rdb))]
    pub(crate) fn new(rdb: Arc<rdb::Rdb<Postgres>>) -> Self {
        Self { rdb }
    }
}

impl MetadataDB {
    /// Inserts telemetry metadata and associated camera frames into the database.
    ///
    /// # Arguments
    ///
    /// * `vin` - Vehicle Identification Number
    /// * `start_time` - Start timestamp of the telemetry period
    /// * `end_time` - End timestamp of the telemetry period
    /// * `model` - Model identifier for the telemetry data
    /// * `json` - JSON-formatted telemetry metadata
    /// * `camera_frames` - Optional camera frames to store in camera_image table
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok on success, error otherwise
    #[instrument(skip(self, vin, start_time, end_time, model, json, camera_frames))]
    async fn insert_metadata(
        &self,
        vin: &str,
        start_time: time::OffsetDateTime,
        end_time: time::OffsetDateTime,
        model: &str,
        json: &str,
        camera_frames: Option<&[ouro_schema::schema::traj::CameraFrame]>,
    ) -> InternalResult<()> {
        let vin_owned = vin.to_string();
        let model_owned = model.to_string();
        let json_owned = json.to_string();
        let camera_frame_count = camera_frames.map(|f| f.len()).unwrap_or(0);
        let camera_frames_owned = camera_frames.map(|f| f.to_vec());

        let t_total = std::time::Instant::now();

        let tx = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| {
                let vin_owned = vin_owned.clone();
                async move {
                    let t = std::time::Instant::now();
                    system_state::mark_update(tx, &vin_owned).await?;
                    tracing::debug!(elapsed_ms = t.elapsed().as_millis() as u64, "mark_update");
                    Ok(())
                }
            })
            .await?
            .pipe(|tx, _| async move {
                let t = std::time::Instant::now();
                sqlx::query(sql::INSERT_METADATA)
                    .bind(&vin_owned)
                    .bind(start_time)
                    .bind(end_time)
                    .bind(&model_owned)
                    .bind(&json_owned)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to insert metadata", e).into())?;
                tracing::debug!(
                    elapsed_ms = t.elapsed().as_millis() as u64,
                    "insert_telemetry"
                );

                if let Some(ref frames) = camera_frames_owned {
                    let t_frames = std::time::Instant::now();
                    let vins: Vec<&str> = frames.iter().map(|_| vin_owned.as_str()).collect();
                    let start_times: Vec<time::OffsetDateTime> =
                        frames.iter().map(|_| start_time).collect();
                    let camera_ids: Vec<String> =
                        frames.iter().map(|f| f.camera_id.to_string()).collect();
                    let timestamps: Vec<time::OffsetDateTime> =
                        frames.iter().map(|f| f.timestamp).collect();
                    let images: Vec<&[u8]> = frames.iter().map(|f| f.image.as_slice()).collect();
                    sqlx::query(sql::INSERT_CAMERA_IMAGE_BULK)
                        .bind(&vins)
                        .bind(&start_times)
                        .bind(&camera_ids)
                        .bind(&timestamps)
                        .bind(&images)
                        .execute(&mut **tx)
                        .await
                        .map_err(|e| ("Failed to bulk insert camera_image", e).into())?;
                    tracing::debug!(
                        elapsed_ms = t_frames.elapsed().as_millis() as u64,
                        frame_count = frames.len(),
                        "insert_camera_images"
                    );
                }

                Ok(())
            })
            .await?;

        let t_commit = std::time::Instant::now();
        tx.commit().await?;
        tracing::debug!(elapsed_ms = t_commit.elapsed().as_millis() as u64, "commit");

        tracing::debug!(
            total_ms = t_total.elapsed().as_millis() as u64,
            camera_frame_count,
            "insert_metadata complete"
        );

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TrajectoryTask {
    name: String,
    metrics_store: Arc<BuiltMetricsStore>,
}

impl TrajectoryTask {
    /// Creates a new trajectory ingestion task.
    ///
    /// # Arguments
    ///
    /// * `name` - Task name
    /// * `metrics_store` - Metrics store for tracking statistics
    ///
    /// # Returns
    ///
    /// * `Self` - New trajectory task instance
    #[instrument(skip(name, metrics_store))]
    pub fn new(name: &str, metrics_store: Arc<BuiltMetricsStore>) -> Arc<Self> {
        Arc::new(Self {
            name: name.into(),
            metrics_store,
        })
    }
}

#[async_trait]
impl ComponentImpl for TrajectoryTask {
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
impl ComponentImplExt for TrajectoryTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = vec![];

        match state {
            DaemonTaskSpec::DataIngestion {
                name,
                enabled,
                with_client,
                data,
                behavior: _behavior,
                with_data_source,
                attribute: _attribute,
                ..
            } => {
                if !enabled {
                    tracing::info!("TrajectoryTask '{}' is disabled.", self.name);
                    return Ok(vec![]);
                }

                if *data != IngestionDataSpec::Trajectory {
                    tracing::error!(
                        "TrajectoryTask '{}' received incompatible data type: {:?}",
                        self.name,
                        data
                    );
                    return Err((
                        "Incompatible data type for TrajectoryTask",
                        InternalErrorKind::TypeConversion,
                    )
                        .into());
                }

                let client = context.with_client(with_client).await?;
                let client: Arc<RetryableClient> = client.as_ref().try_into()?;

                let data_source: Arc<BuiltDataSource> =
                    context.with_data_source(with_data_source).await?;
                let rdb: Arc<rdb::Rdb<Postgres>> = data_source.as_ref().try_into()?;
                let metadata_db = Arc::new(MetadataDB::new(rdb));

                let metrics_store = self.metrics_store.clone();

                let mut throughput = metrics::Throughput::new(
                    constant::daemon::PREFIX_METRICS_METADATA,
                    time::Duration::seconds(5),
                    metrics_store.clone(),
                );
                throughput.start();
                let throughput = Arc::new(throughput);

                let latency = metrics::Latency::new(
                    constant::daemon::PREFIX_METRICS_METADATA,
                    metrics_store.clone(),
                );
                let latency = Arc::new(latency);

                let metadata_db = metadata_db.clone();

                let session = client.transport()?.into_messaging()?.clone();
                let rpc_server = session
                    .topic_pattern_server(&topic::PATTERN_METADATA_LOG)
                    .await?;

                let rpc_server = rpc_server.clone();

                let action = define_resource_trigger_action!(
                    with {
                        let metadata_db = metadata_db.clone();
                        let rpc_server = rpc_server.clone();
                        let throughput = throughput.clone();
                        let latency = latency.clone();
                    }
                    when: let result = rpc_server.recv_async().await,
                    then: _recv_metadata(
                        metadata_db,
                        result,
                        throughput,
                        latency,
                    )
                );
                actions.push(action);

                let this = self.clone();
                actions.push(register_icc_handler(&this, name, &context).await?);
            }
            _ => {
                return Err((
                    "Incompatible state for TrajectoryTask",
                    InternalErrorKind::TypeConversion,
                )
                    .into());
            }
        }

        Ok(actions)
    }
}

/// Starts the metadata query handler.
///
/// Creates a Zenoh queryable that listens for metadata queries from edge devices
/// and processes them with concurrency control.
///
/// # Arguments
///
/// * `metadata_db` - Metadata database manager
/// * `rpc_server` - RPC server for receiving metadata requests
/// * `semaphore` - Semaphore for concurrency control
/// * `throughput` - Throughput metrics tracker
/// * `latency` - Latency metrics tracker
///
/// # Returns
///
/// * `Result<()>` - Ok on success, error otherwise
#[instrument(skip(metadata_db, result, throughput, latency))]
pub(crate) async fn _recv_metadata(
    metadata_db: Arc<MetadataDB>,
    result: InternalResult<Arc<dyn RpcHandle>>,
    throughput: Arc<Throughput<BuiltMetricsStore>>,
    latency: Arc<Latency<BuiltMetricsStore>>,
) -> InternalResult<()> {
    /// Helper function to log an error and send an error reply to a query.
    ///
    /// # Arguments
    ///
    /// * `handle` - RPC handle to reply to
    /// * `log_msg` - Message to log
    /// * `response_msg` - Message to send in the reply
    #[instrument(skip(handle, log_msg, response_msg))]
    async fn send_error_response(
        handle: &Arc<dyn messaging::RpcHandle>,
        log_msg: &str,
        response_msg: &Payload,
    ) {
        tracing::error!("{}", log_msg);

        if let Err(e) = handle
            .response(&messaging::Response::Error(response_msg.clone()))
            .await
        {
            tracing::error!("Failed to send error response: {:?}", e);
        }
    }

    let start_time = std::time::Instant::now();

    let (handle, topic, payload) = match result {
        Ok(handle) => {
            let request = match handle.request() {
                Ok(request) => request,
                Err(e) => {
                    throughput.report_one(false).await;

                    send_error_response(
                        &handle,
                        &format!("Failed to receive request: {:?}", e),
                        &Payload(b"Failed to receive request".to_vec()),
                    )
                    .await;

                    return Ok(());
                }
            };
            match request {
                TopicMessage(topic, payload) => (handle, topic, payload),
            }
        }
        _ => {
            tracing::error!("Failed to receive metadata query: {:?}", result.err());
            throughput.report_one(false).await;
            return Ok(());
        }
    };

    let result = ciborium::from_reader(payload.as_slice());
    if let Err(e) = result {
        throughput.report_one(false).await;

        send_error_response(
            &handle,
            &format!("Failed to deserialize payload: {:?}", e),
            &Payload(b"Invalid payload".to_vec()),
        )
        .await;

        return Ok(());
    }

    let mut metadata: EventMetadata = match result {
        Ok(m) => m,
        Err(_e) => {
            // Already logged and responded with error above via early return;
            // this branch is unreachable, but avoids unwrap.
            throughput.report_one(false).await;
            return Ok(());
        }
    };

    // Extract camera frames before serializing to JSON so they are not
    // duplicated in the telemetry.metadata JSONB column.  Camera frames are
    // stored separately in the camera_image table by insert_metadata.
    let camera_frames = metadata.camera_frames.take();

    let json = match serde_json::to_string(&metadata) {
        Ok(j) => j,
        Err(e) => {
            throughput.report_one(false).await;

            send_error_response(
                &handle,
                &format!("Failed to serialize metadata to JSON: {:?}", e),
                &Payload(b"Failed to serialize metadata".to_vec()),
            )
            .await;

            return Ok(());
        }
    };

    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!(
            "[{}] Received metadata (id={})",
            &topic,
            metadata.vehicle.vin
        );
    }

    let result = metadata_db
        .insert_metadata(
            &metadata.vehicle.vin,
            metadata.start_time,
            metadata.end_time,
            &metadata.vehicle.model,
            &json,
            camera_frames.as_deref(),
        )
        .await;

    match result {
        Ok(()) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    "[{}] Inserted metadata (id={}) successfully",
                    &topic,
                    metadata.vehicle.vin
                );
            }
            let topic_for_err = topic.clone();
            if let Err(e) = handle
                .response(&messaging::Response::Success(
                    topic,
                    Payload(b"Ok".to_vec()),
                ))
                .await
            {
                throughput.report_one(false).await;
                tracing::error!(
                    "[{}] Failed to send reply for {}: {:?}",
                    &topic_for_err,
                    metadata.vehicle.vin,
                    e
                );
            } else {
                throughput.report_one(true).await;
                latency
                    .report_one(std::time::Instant::now() - start_time)
                    .await;
            }
        }
        Err(e) => {
            throughput.report_one(false).await;
            send_error_response(
                &handle,
                format!("Server failed to store metadata: {:?}", e).as_str(),
                &Payload(b"Failed to store metadata".to_vec()),
            )
            .await;
        }
    }
    Ok(())
}

mod icc {
    use std::sync::{Arc, LazyLock};

    use async_trait::async_trait;
    use ouro_common::constant;
    use ouro_core::runtime::{
        metrics::{provider::MetricsProvider as _, store::MetricsStore as _},
        types::BuiltMetricsProvider,
    };
    use ouro_schema::{
        operation_descriptions,
        schema::cli::{
            icc::IccHandler, unsupported_operation, DescribeResp, Description, ListResp,
            OperateReq, OperateResp,
        },
        Void,
    };
    use tokio::sync::Mutex;
    use tracing::instrument;

    use crate::task::metadata::TrajectoryTask;

    /// Canonical operation descriptions for TrajectoryTask.
    fn descriptions() -> Vec<Description> {
        operation_descriptions![
            /// Receive ICC requests for the metadata task.
            "get_metrics" {
                inputs {}
                output {
                    /// List of metric keys currently tracked by the metadata task.
                }
            }
        ]
    }

    #[async_trait]
    impl IccHandler for TrajectoryTask {
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
            // SAFETY: `_guard` is guaranteed to be `Some` after the initialization above.
            let provider = _guard
                .as_ref()
                .expect("metrics provider must be initialized");

            match req.operation_name.as_str() {
                "get_metrics" => {
                    let metrics = provider
                        .get_all_metrics(1000, Some(constant::daemon::PREFIX_METRICS_METADATA))
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
                stringify!(TrajectoryTask)
            ))
        }
    }
}
