// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use async_trait::async_trait;
use ouro_common::topic;
use ouro_core::runtime::client::Callable as _;
use ouro_core::runtime::comp::{CompContext, ComponentImpl};
use ouro_core::runtime::context::ResourceContext;
use ouro_core::runtime::rdb;
use ouro_core::{
    comm::messaging,
    error::{InternalError, InternalErrorKind},
    runtime::{
        client::RetryableClient,
        comp::ComponentImplExt,
        types::{ActionFactory, SharableAction},
    },
    InternalResult,
};
use ouro_edge::runtime::context::EdgeResourceContextImpl;
use ouro_schema::schema::traj;
use ouro_state::edge::{EdgeTaskSpec, TriggerStrategySpec, UploadDataSpec};
use sqlx::Row;
use time::OffsetDateTime;
use tokio::task::JoinHandle;
use tracing::instrument;

/// Minimum number of trailing rows to retain in `system_state` after cleanup.
/// Rows beyond this count from the tail are eligible for deletion once processed
/// into `metadata`.
const MIN_TRAILING_ROWS: i64 = 100;

mod sql {
    #[allow(dead_code)] // API
    pub(crate) const INSERT_METADATA: &str = r#"
    INSERT INTO metadata (start_time, end_time, cbor)
    VALUES (strftime('%Y-%m-%d %H:%M:%f', ?, 'unixepoch'),
            strftime('%Y-%m-%d %H:%M:%f', ?, 'unixepoch'), ?);
    "#;

    pub(crate) const INSERT_CAMERA_IMAGE: &str = r#"
    INSERT INTO camera_image (metadata_id, camera_id, ts, image)
    VALUES (?, ?, strftime('%Y-%m-%d %H:%M:%f', ?, 'unixepoch'), ?);
    "#;

    /// Select camera images subsampled to 1Hz within a time window.
    /// Picks the first frame per integer-second from system_state_camera.
    #[allow(dead_code)] // API
    pub(crate) const SELECT_CAMERA_IMAGES_1HZ: &str = r#"
    SELECT sc.camera_id, sc.image, unixepoch(sc.ts) AS timestamp
    FROM system_state_camera sc
    INNER JOIN (
        SELECT ts FROM system_state
        WHERE unixepoch(ts) > ? AND unixepoch(ts) <= ?
        GROUP BY CAST(unixepoch(ts) AS INTEGER)
        HAVING ts = MIN(ts)
    ) sampled ON sc.ts = sampled.ts
    ORDER BY sc.ts, sc.camera_id
    "#;

    /// Marks a metadata row as acknowledged.
    #[allow(dead_code)] // API
    pub(crate) const ACK_METADATA: &str = r#"
    UPDATE metadata SET acked = 1 WHERE id = ?;
    "#;

    /// Deletes all acknowledged metadata rows except the one with the latest
    /// `end_time`. The surviving row serves as a persistent watermark so that
    /// `_collect_event_metadata` can resume from the correct position after a
    /// system restart.
    #[allow(dead_code)] // API
    pub(crate) const DELETE_ACKED_NON_LATEST: &str = r#"
    DELETE FROM metadata
    WHERE acked = 1
      AND end_time < (SELECT max(end_time) FROM metadata WHERE acked = 1);
    "#;

    /// Deletes `system_state` rows that have already been processed into `metadata`,
    /// while retaining at least `MIN_TRAILING_ROWS` rows from the tail.
    ///
    /// A row is eligible for deletion when:
    /// - Its `ts` is at or before the maximum `end_time` recorded in `metadata` (processed watermark), AND
    /// - It is not among the last `?` (`MIN_TRAILING_ROWS`) rows ordered by `ts` descending.
    ///
    /// If `metadata` is empty or `system_state` has fewer rows than `MIN_TRAILING_ROWS`,
    /// the subqueries return NULL and no rows are deleted.
    #[allow(dead_code)] // API
    pub(crate) const DELETE_SYSTEM_STATE_PROCESSED: &str = r#"
    DELETE FROM system_state
    WHERE ts <= (SELECT max(end_time) FROM metadata)
      AND ts < (
        SELECT ts FROM system_state
        ORDER BY ts DESC
        LIMIT 1 OFFSET ?
      )
    "#;
}

#[derive(Debug, Clone)]
pub struct MetadataSendTask {}

impl MetadataSendTask {
    #[instrument]
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(MetadataSendTask {})
    }
}

#[async_trait]
impl ComponentImpl for MetadataSendTask {
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
impl ComponentImplExt for MetadataSendTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = Vec::new();

        let state = state.clone();
        match &state {
            EdgeTaskSpec::DataUpload {
                enabled,
                behavior,
                with_client,
                data,
                ..
            } => {
                if !enabled {
                    return Ok(vec![]);
                }

                let (rdb, camera_rdb) = {
                    if let UploadDataSpec::Trajectory {
                        with_data_source,
                        with_camera_data_source,
                    } = data
                    {
                        let data_source = context.with_data_source(with_data_source).await?;
                        let rdb = data_source.as_sqlite()?;
                        let camera_rdb: Option<Arc<rdb::Rdb<sqlx::Sqlite>>> =
                            match with_camera_data_source {
                                Some(data_source) => {
                                    let data_source = context.with_data_source(data_source).await?;
                                    Some(data_source.as_sqlite()?)
                                }
                                None => None,
                            };
                        (rdb, camera_rdb)
                    } else {
                        return Err(Into::<InternalError>::into((
                            "Invalid data type for MetadataSendTask",
                            InternalErrorKind::TypeConversion,
                        )));
                    }
                };

                let client = context.with_client(with_client).await?;
                let client = client.as_retryable()?;
                let session = client.transport()?.into_messaging()?;
                // Get edge ID and construct key
                let edge_id = context.root().lock().await.runtime.edge_id.clone();
                let model = context.root().lock().await.runtime.model.clone();
                let vehicle = traj::Vehicle {
                    vin: edge_id.clone(),
                    model: model.clone(),
                };
                let topic = topic::metadata_log_key(edge_id.as_str());
                let rpc_client = session.rpc_client(&topic).await?;
                // Clone triggers to end the borrow of `state` before spawning tasks
                let triggers: Vec<_> = behavior.trigger.values().cloned().collect();

                for trigger in triggers {
                    let vehicle = vehicle.clone();
                    let state = state.clone();
                    let client = client.clone();
                    let rpc_client = rpc_client.clone();
                    let rdb = rdb.clone();
                    let camera_rdb = camera_rdb.clone();
                    match trigger {
                        TriggerStrategySpec::Periodic {
                            uuid,
                            enabled,
                            interval_secs,
                            ..
                        } => {
                            if !enabled {
                                continue;
                            }

                            tracing::info!(
                                "Starting metadata send task for edge '{}' with topic '{}'",
                                edge_id,
                                topic
                            );

                            let continuous_errors = Arc::new(AtomicU32::new(0u32));

                            let action = SharableAction::Periodic {
                                interval_secs,
                                factory: {
                                    let factory: ActionFactory = Arc::new(move || {
                                        Box::pin(_metadata_send_task(
                                            vehicle.clone(),
                                            uuid.clone(),
                                            state.clone(),
                                            client.clone(),
                                            rpc_client.clone(),
                                            rdb.clone(),
                                            camera_rdb.clone(),
                                            continuous_errors.clone(),
                                        ))
                                    });
                                    factory
                                },
                            };

                            actions.push(action);
                        }
                        _ => {
                            return Err((
                                "MetadataSendTask only supports Periodic trigger strategy",
                                InternalErrorKind::TypeConversion,
                            )
                                .into());
                        }
                    }
                }
            }
            _ => {
                return Err((
                    "Invalid task config for MetadataSendTask",
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
#[allow(clippy::too_many_arguments)]
async fn _metadata_send_task(
    vehicle: traj::Vehicle,
    uuid: String,
    state: EdgeTaskSpec,
    client: Arc<RetryableClient>,
    rpc_client: Arc<dyn messaging::RpcClient>,
    rdb: Arc<rdb::Rdb<sqlx::Sqlite>>,
    camera_rdb: Option<Arc<rdb::Rdb<sqlx::Sqlite>>>,
    continuous_errors: Arc<AtomicU32>,
) -> InternalResult<()> {
    // Returns only metadata IDs — no large CBOR blobs are loaded here.
    let metadata_ids =
        _collect_event_metadata(&vehicle, &uuid, &state, &rdb, camera_rdb.as_ref()).await?;

    if metadata_ids.is_empty() {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("No metadata logs to send");
        }
        return Ok(());
    } else if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!("Collected metadata logs to send: {}", metadata_ids.len());
    }

    let continuous_errors_for_call = Arc::clone(&continuous_errors);
    client
        .call(|_endpoint| async move {
            let continuous_errors = Arc::clone(&continuous_errors_for_call);

            for id in metadata_ids {
                // Fetch the CBOR for this single row in a short, focused transaction.
                // The DB connection is released before the Zenoh RPC call, so
                // insert_system_state() is never blocked by a long-running send.
                let cbor: Vec<u8> = rdb
                    .begin()
                    .await?
                    .pipe(move |tx, _| async move {
                        let cbor: Vec<u8> = sqlx::query_scalar(
                            "SELECT cbor FROM metadata WHERE id = ?",
                        )
                        .bind(id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_err(|e| {
                            Into::<InternalError>::into(("Failed to fetch metadata cbor", e))
                        })?;
                        Ok(cbor)
                    })
                    .await?
                    .commit()
                    .await?
                    .ok_or_else(|| {
                        Into::<InternalError>::into((
                            "No cbor for metadata id",
                            InternalErrorKind::Runtime,
                        ))
                    })?;

                let continuous_errors = continuous_errors.clone();
                let _ack_payload = match rpc_client.call(cbor.as_slice()).await {
                    Ok(messaging::TopicMessage(_, ack_payload)) => ack_payload,
                    Err(e) => {
                        match e.kind() {
                            InternalErrorKind::Network | InternalErrorKind::Protocol => {
                                tracing::error!("Failed to send metadata (id={}): {:?}", id, e);
                                continuous_errors.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                            e => {
                                tracing::error!("Failed to send metadata (id={}): Unexpected error: should be a bug: {:?}", id, e);
                                return Err(Into::<InternalError>::into((
                                    format!("Unexpected error: should be a bug: {:?}", e),
                                    InternalErrorKind::Bug,
                                )));
                            }
                        }
                    }
                };
                rdb.transaction(|tx| async move {
                    sqlx::query(sql::ACK_METADATA)
                        .bind(id)
                        .execute(&mut **tx)
                        .await
                        .map_err(|e| Into::<InternalError>::into(("Failed to ack metadata", e)))?;
                    sqlx::query(sql::DELETE_ACKED_NON_LATEST)
                        .execute(&mut **tx)
                        .await
                        .map_err(|e| {
                            Into::<InternalError>::into(("Failed to delete old acked metadata", e))
                        })?;
                    Ok(())
                })
                .await?;

                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Marked metadata (id={}) as acked", id);
                }

                continuous_errors.store(0, Ordering::Relaxed);
            }
            Ok(())
        })
        .await?;
    Ok(())
}

async fn _collect_event_metadata(
    vehicle: &traj::Vehicle,
    uuid: &str,
    state: &EdgeTaskSpec,
    rdb: &Arc<rdb::Rdb<sqlx::Sqlite>>,
    camera_rdb: Option<&Arc<rdb::Rdb<sqlx::Sqlite>>>,
) -> InternalResult<Vec<i64>> {
    macro_rules! fetch_one_time_or_epoch {
        ($tx:expr, $sql:expr) => {{
            let result = sqlx::query($sql).fetch_one(&mut **$tx).await;
            match result {
                Ok(row) => OffsetDateTime::from_unix_timestamp(row.get(0)).map_err(|e| {
                    Into::<InternalError>::into((
                        format!("Failed to get unix timestamp from row: {:?}", e),
                        InternalErrorKind::Parse,
                    ))
                }),
                Err(sqlx::Error::RowNotFound) => {
                    // No metadata exists yet; use epoch as the starting point
                    Ok(OffsetDateTime::UNIX_EPOCH)
                }
                Err(e) => Err(Into::<InternalError>::into((
                    "fetch metadata from database",
                    e,
                ))),
            }
        }};
    }

    let interval_secs = match state {
        EdgeTaskSpec::DataUpload {
            name: _name,
            enabled,
            behavior,
            ..
        } => {
            // Extract interval from task config
            behavior
                .trigger
                .get(uuid)
                .and_then(|trigger| match trigger {
                    TriggerStrategySpec::Periodic {
                        uuid: _,
                        enabled: _,
                        interval_secs,
                    } => {
                        if !enabled {
                            // Disabled trigger: no interval to schedule.
                            return None;
                        }
                        Some(*interval_secs)
                    }
                    _ => None,
                })
                .unwrap_or(60) // TODO: tentatively default to 60s if not found. fix it
        }
        _ => {
            return Err((
                "Invalid task config for MetadataSendTask",
                InternalErrorKind::InvalidConfig,
            )
                .into());
        }
    };

    // Row from system_state with location and optional active behavior.
    #[derive(Debug, Clone, sqlx::FromRow)]
    struct StateRow {
        timestamp: OffsetDateTime,
        longitude: f64,
        latitude: f64,
        active_behavior: Option<String>,
    }

    // Pre-built metadata window ready for camera frame attachment.
    struct MetadataWindow {
        start_ts: i64,
        end_ts: i64,
        emd: traj::EventMetadata,
    }

    // Tx1: Fetch unacked metadata IDs (fast, short transaction).
    // CBORs (which contain large JPEG blobs) are loaded individually just
    // before each Zenoh send.
    let mut metadata_ids: Vec<i64> = rdb
        .begin()
        .await?
        .pipe(|tx, _| async move {
            let sql = "SELECT id FROM metadata WHERE acked = 0";
            let ids: Vec<i64> =
                sqlx::query_scalar(sql)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| {
                        Into::<InternalError>::into(("Failed to fetch unacked metadata ids", e))
                    })?;
            Ok(ids)
        })
        .await?
        .commit()
        .await?
        .unwrap_or_default();

    // Tx2: Determine the processing window and fetch system_state text rows
    // (no JPEG blobs). The connection is released immediately after this query
    // completes, so `insert_system_state` is never blocked during the CPU-
    // bound window-building phase.
    let state_rows: Vec<StateRow> = {
        let result: Option<Vec<StateRow>> = rdb
            .begin()
            .await?
            .pipe(move |tx, _| async move {
                // Get last end_time from metadata (the latest acked row is
                // always retained as a persistent watermark, so this query
                // reliably returns the processing position even after a
                // system restart).
                let sql = "SELECT unixepoch(end_time) FROM metadata WHERE id = (SELECT max(id) FROM metadata)";
                let mut last_md_ts = fetch_one_time_or_epoch!(tx, sql)?;

                if last_md_ts == OffsetDateTime::UNIX_EPOCH {
                    // No metadata yet: start from the earliest system_state row.
                    let sql = "SELECT unixepoch(min(ts)) FROM system_state";
                    last_md_ts = fetch_one_time_or_epoch!(tx, sql)?;
                    if last_md_ts == OffsetDateTime::UNIX_EPOCH {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!("No system_state data available to process");
                        }
                        return Ok::<Option<Vec<StateRow>>, InternalError>(None);
                    }
                    last_md_ts = last_md_ts.saturating_sub(time::Duration::seconds(1));
                }

                // Get last ts in system_state
                let sql = "SELECT unixepoch(max(ts)) FROM system_state";
                let last_state_ts = fetch_one_time_or_epoch!(tx, sql)?;
                let next_last_md_ts =
                    last_md_ts + time::Duration::seconds(interval_secs as i64);
                if last_state_ts < next_last_md_ts {
                    // No new data to process.
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            "No new system_state data to process (last_state_ts: {}, next_last_md_ts: {})",
                            last_state_ts, next_last_md_ts
                        );
                    }
                    return Ok(None);
                }

                // Fetch system_state rows (text fields only, no JPEG blobs).
                let sql = r#"
                    SELECT unixepoch(ts) AS timestamp,
                           CAST(json_extract(state, '$.lon') AS REAL) AS longitude,
                           CAST(json_extract(state, '$.lat') AS REAL) AS latitude,
                           json_extract(state, '$.active_behavior') AS active_behavior
                    FROM system_state
                    WHERE unixepoch(ts) > ? AND unixepoch(ts) <= ?
                "#;
                let rows: Vec<StateRow> = sqlx::query_as::<_, StateRow>(sql)
                    .bind(last_md_ts.unix_timestamp())
                    .bind(next_last_md_ts.unix_timestamp())
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| {
                        Into::<InternalError>::into((
                            "Failed to fetch state rows from system_state",
                            e,
                        ))
                    })?;

                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        "Processing {} state rows from system_state in ({}, {}]",
                        rows.len(),
                        last_md_ts
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_else(|_| last_md_ts.to_string()),
                        next_last_md_ts
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_else(|_| next_last_md_ts.to_string())
                    );
                }

                Ok(Some(rows))
            })
            .await?
            .commit()
            .await?
            .flatten(); // Option<Option<T>> -> Option<T>

        match result {
            None => return Ok(metadata_ids),
            Some(rows) => rows,
        }
    };

    // Flush the tracked behavior span into the behaviors vec.
    fn flush_behavior(
        current: &mut Option<(String, OffsetDateTime)>,
        end_time: OffsetDateTime,
        behaviors: &mut Vec<traj::ActiveBehavior>,
    ) {
        if let Some((name, start_time)) = current.take() {
            behaviors.push(traj::ActiveBehavior {
                name,
                start_time,
                end_time,
            });
        }
    }

    // Build metadata windows in memory — no DB access needed here.
    // Each window covers up to `interval_secs` sim-seconds of trajectory data.
    let mut windows: Vec<MetadataWindow> = Vec::new();
    let mut emd: traj::EventMetadata = traj::EventMetadata::new(vehicle, "bulk_system_state");
    let mut current_behavior: Option<(String, OffsetDateTime)> = None;
    let mut behaviors: Vec<traj::ActiveBehavior> = Vec::new();

    for row in state_rows {
        let location = traj::LocationFix {
            timestamp: row.timestamp,
            longitude: row.longitude,
            latitude: row.latitude,
        };

        if emd.start_time == OffsetDateTime::UNIX_EPOCH {
            emd.start_time = location.timestamp;
        } else if location.timestamp
            > emd.start_time + time::Duration::seconds(interval_secs as i64)
        {
            if let Some(last_fix) = emd.trajectory().location_fixes.last() {
                emd.end_time = last_fix.timestamp;
            }

            // Flush any in-progress behavior span for this window.
            flush_behavior(&mut current_behavior, emd.end_time, &mut behaviors);
            if !behaviors.is_empty() {
                emd.active_behaviors = Some(std::mem::take(&mut behaviors));
            }

            let finished_emd = std::mem::replace(
                &mut emd,
                traj::EventMetadata::new(vehicle, "bulk_system_state"),
            );
            windows.push(MetadataWindow {
                start_ts: finished_emd.start_time.unix_timestamp(),
                end_ts: finished_emd.end_time.unix_timestamp(),
                emd: finished_emd,
            });

            // If there was an active behavior at window boundary,
            // start a new span in the next window.
            if let Some(ref ab) = row.active_behavior {
                current_behavior = Some((ab.clone(), row.timestamp));
            }
        }

        // Track behavior spans.
        match (&row.active_behavior, &current_behavior) {
            (Some(ab), Some((cur_name, _))) if ab == cur_name => {
                // Same behavior continues — nothing to do.
            }
            (Some(ab), _) => {
                // New or different behavior.
                flush_behavior(&mut current_behavior, row.timestamp, &mut behaviors);
                current_behavior = Some((ab.clone(), row.timestamp));
            }
            (None, _) => {
                // Behavior ended.
                flush_behavior(&mut current_behavior, row.timestamp, &mut behaviors);
            }
        }

        emd.trajectory().add_location_fix(location);
    }

    // Push final (incomplete) window if it has data.
    if emd.start_time != OffsetDateTime::UNIX_EPOCH {
        if let Some(last_fix) = emd.trajectory().location_fixes.last() {
            emd.end_time = last_fix.timestamp;
        }
        flush_behavior(&mut current_behavior, emd.end_time, &mut behaviors);
        if !behaviors.is_empty() {
            emd.active_behaviors = Some(behaviors);
        }
        windows.push(MetadataWindow {
            start_ts: emd.start_time.unix_timestamp(),
            end_ts: emd.end_time.unix_timestamp(),
            emd,
        });
    }

    // For each window, use short-lived transactions so that
    // `insert_system_state` can acquire the connection between operations.
    for window in windows {
        // Tx3: Fetch 1Hz-subsampled camera frames (short transaction).
        // The connection is released before the CPU-bound CBOR serialization.
        let start_ts = window.start_ts;
        let end_ts = window.end_ts;
        let frames: Vec<traj::CameraFrame> = rdb
            .begin()
            .await?
            .pipe(|tx, _| async move { _fetch_camera_frames(&mut *tx, start_ts, end_ts).await })
            .await?
            .commit()
            .await?
            .unwrap_or_default();

        let mut emd = window.emd;
        if !frames.is_empty() {
            emd.camera_frames = Some(frames);
        }

        // Serialize CBOR outside the transaction: this is CPU-bound and can
        // be 100ms–seconds for large EventMetadata with many camera frames.
        // Keeping it outside avoids holding the single `:memory:` connection
        // during serialization.
        let cbor_bytes = {
            let mut buf = Vec::new();
            ciborium::into_writer(&emd, &mut buf).map_err(|e| {
                Into::<InternalError>::into(("Failed to serialize metadata to cbor", e))
            })?;
            buf
        };
        let emd_start = emd.start_time.unix_timestamp();
        let emd_end = emd.end_time.unix_timestamp();

        // Take camera frames out of emd after CBOR serialization so they can
        // be moved into the Tx4b closure without cloning the image blobs.
        // CBOR already captured the frames above, so emd.camera_frames is no
        // longer needed for serialization.
        let camera_frames: Option<Vec<traj::CameraFrame>> = emd.camera_frames.take();

        // Tx4: Insert the serialized CBOR into the metadata table (short transaction).
        let id: i64 = rdb
            .begin()
            .await?
            .pipe(move |tx, _| async move {
                sqlx::query(sql::INSERT_METADATA)
                    .bind(emd_start)
                    .bind(emd_end)
                    .bind(&cbor_bytes)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| Into::<InternalError>::into(("insert metadata record", e)))?;

                let id: i64 = sqlx::query("SELECT last_insert_rowid()")
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| {
                        Into::<InternalError>::into(("Failed to fetch last insert rowid", e))
                    })?
                    .get(0);
                Ok(id)
            })
            .await?
            .commit()
            .await?
            .ok_or_else(|| {
                Into::<InternalError>::into((
                    "No ID returned from metadata insert",
                    InternalErrorKind::Runtime,
                ))
            })?;

        // Tx4b (optional): Insert camera frames into the separate camera_image
        // store. This runs only when `with_camera_data_source` is configured.
        //
        // On failure, a compensating transaction removes the metadata row
        // inserted in Tx4 so that the two stores remain consistent. The
        // failed window is not added to `metadata_ids` and will be retried
        // on the next periodic invocation.
        if let (Some(camera_rdb), Some(frames)) =
            (camera_rdb, camera_frames.as_ref().filter(|f| !f.is_empty()))
        {
            let result = camera_rdb
                .transaction(|tx| async move {
                    for frame in frames {
                        sqlx::query(sql::INSERT_CAMERA_IMAGE)
                            .bind(id)
                            .bind(frame.camera_id.to_string().as_str())
                            .bind(frame.timestamp.unix_timestamp())
                            .bind(frame.image.as_slice())
                            .execute(&mut **tx)
                            .await
                            .map_err(|e| Into::<InternalError>::into(("insert camera image", e)))?;
                    }
                    Ok(())
                })
                .await;

            match result {
                Ok(()) => metadata_ids.push(id), // Only add to metadata_ids if camera frame insertion succeeded.
                Err(e) => {
                    tracing::warn!(
                        "Failed to insert camera frames for metadata id={}: {:?}. \
                         Running compensating transaction to remove the metadata row.",
                        id,
                        e
                    );
                    // Compensating transaction: undo Tx4 so the two stores stay consistent.
                    if let Err(comp_err) = rdb
                        .transaction(|tx| async move {
                            sqlx::query("DELETE FROM metadata WHERE id = ?")
                                .bind(id)
                                .execute(&mut **tx)
                                .await
                                .map_err(|e| {
                                    Into::<InternalError>::into((
                                        "compensating delete of metadata row",
                                        e,
                                    ))
                                })?;
                            Ok(())
                        })
                        .await
                    {
                        tracing::error!(
                            "Compensating transaction failed for metadata id={}: {:?}",
                            id,
                            comp_err
                        );
                    }
                }
            }
        };
    }

    // Tx5: Delete processed system_state rows (short transaction).
    let rows_affected = rdb
        .begin()
        .await?
        .pipe(|tx, _| async move {
            let rows = sqlx::query(sql::DELETE_SYSTEM_STATE_PROCESSED)
                .bind(MIN_TRAILING_ROWS)
                .execute(&mut **tx)
                .await
                .map_err(|e| {
                    Into::<InternalError>::into(("Failed to delete processed system_state rows", e))
                })?
                .rows_affected();
            Ok(rows)
        })
        .await?
        .commit()
        .await?
        .unwrap_or(0);

    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!("Deleted {} processed system_state rows", rows_affected);
    }

    Ok(metadata_ids)
}

/// Fetch 1Hz subsampled camera frames from system_state_camera.
///
/// Picks the first frame per integer-second within the given time window
/// `(start_ts, end_ts]` from `system_state_camera`, joining against
/// `system_state` to use the same timestamp set as the trajectory data.
///
/// # Arguments
/// * `tx` - Active SQLite transaction
/// * `start_ts` - Exclusive window start as Unix timestamp
/// * `end_ts` - Inclusive window end as Unix timestamp
///
/// # Returns
/// Camera frames ordered by timestamp and camera ID.
async fn _fetch_camera_frames(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    start_ts: i64,
    end_ts: i64,
) -> InternalResult<Vec<traj::CameraFrame>> {
    let rows: Vec<(String, Vec<u8>, i64)> = sqlx::query_as(sql::SELECT_CAMERA_IMAGES_1HZ)
        .bind(start_ts)
        .bind(end_ts)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| Into::<InternalError>::into(("fetch camera frames", e)))?;
    let mut frames = Vec::with_capacity(rows.len());
    for (camera_id, image, ts) in rows {
        let timestamp = OffsetDateTime::from_unix_timestamp(ts).map_err(|e| {
            Into::<InternalError>::into((
                format!("camera frame timestamp: {:?}", e),
                InternalErrorKind::Parse,
            ))
        })?;
        let camera_id: traj::CameraId = camera_id
            .parse()
            .map_err(|e: String| Into::<InternalError>::into((e, InternalErrorKind::Parse)))?;
        frames.push(traj::CameraFrame {
            camera_id,
            timestamp,
            image,
        });
    }
    Ok(frames)
}
