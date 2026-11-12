// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc};

use async_trait::async_trait;
use ouro_common::{
    constant::core::MPSC_BUFSIZ,
    types::{AnySharable, UnixTimestamp},
};
use ouro_core::{
    comm::rpc::MethodHandler,
    define_resource_trigger_action,
    error::{InternalError, InternalErrorKind},
    get_system_info,
    runtime::{
        comp::{CompContext, CompGuard, ComponentImpl, ComponentImplExt},
        context::ResourceContext as _,
        rdb::Rdb,
        types::{BuiltClient, BuiltDataSource, SharableAction},
    },
    InternalResult,
};
use ouro_edge::runtime::context::EdgeResourceContextImpl;
use ouro_edge::{gen::peer_generated::fb::peer as fbp, runtime::context::EdgeTaskFactory};
use ouro_schema::schema::{
    agent_control::{AgentCommand, AgentCommandKind, RootStateKind},
    shadow::{report::EdgeStatus, EdgeStateReport},
    traj::CameraId,
};
use ouro_state::edge::{EdgeSpec, EdgeTaskSpec, UploadDataSpec};
use tokio::sync::{
    mpsc::{channel, Receiver},
    Mutex,
};
use tracing::instrument;

use crate::{
    task::{
        heartbeat::HeartbeatTask, metadata::MetadataSendTask, state::report::StateReportTask,
        tag_watch, watch::CommandWatchTask,
    },
    types::BuiltEdgeTask,
};

mod sql {
    pub(crate) const _SQL_INSERT_STATE: &str = r#"
    INSERT INTO system_state (ts, state)
    VALUES (strftime('%Y-%m-%d %H:%M:%f', ?, 'unixepoch'), ?)
    "#;

    pub(crate) const _SQL_INSERT_CAMERA_IMAGE: &str = r#"
    INSERT INTO system_state_camera (ts, camera_id, image)
    VALUES (strftime('%Y-%m-%d %H:%M:%f', ?, 'unixepoch'), ?, ?)
    "#;

    pub(crate) const _SQL_COUNT_SYSTEM_STATE: &str = r#"
    SELECT COUNT(*) FROM system_state
    "#;
}

/// Edge Agent managing local operations and communication with daemon.
///
/// The Agent handles local state, RPC processing, heartbeat publishing,
/// and coordination with the daemon.
pub struct EdgeAgent {
    pub context: Arc<EdgeResourceContextImpl>,
    rdb: Arc<Rdb<sqlx::Sqlite>>,
    rx: Mutex<Receiver<AgentCommand>>,
}

#[async_trait]
impl ComponentImpl for EdgeAgent {
    type StateImpl = EdgeSpec;
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
impl ComponentImplExt for EdgeAgent {
    #[instrument(skip(self, _state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        _state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let action = define_resource_trigger_action!(
            with {
                let this = self.clone();
                let context = context.clone();
            }
            when: let result = this.rx.lock().await.recv().await,
            then: _handle_command(result, context)
        );

        Ok(vec![action])
    }
}

#[instrument(skip(result, context))]
async fn _handle_command(
    result: Option<AgentCommand>,
    context: Arc<CompContext<EdgeAgent>>,
) -> InternalResult<()> {
    let message = result.ok_or_else(|| {
        (
            "Command channel closed unexpectedly",
            InternalErrorKind::Runtime,
        )
            .into()
    })?;

    match &message.payload {
        AgentCommandKind::Shutdown => {
            context
                .status
                .lock()
                .await
                .clone_from(&EdgeStatus::Stopping);
            tracing::info!(msg = %message, "Received, shutting down tasks.");
            let mut _guard = context.task_map.lock().await;
            for (name, task) in _guard.drain() {
                tracing::info!("  Aborting task: {}", name);
                let task = BuiltEdgeTask::try_from_any_sharable(task)?;
                task.discard().await?;
            }
            context.status.lock().await.clone_from(&EdgeStatus::Down);
            tracing::info!(msg = %message, "done: all tasks shut down");

            tokio::spawn(async {
                // Delay briefly to allow the shutdown response to be sent back before exiting.
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                std::process::exit(0);
            });
        }
        AgentCommandKind::Start => {
            context
                .status
                .lock()
                .await
                .clone_from(&EdgeStatus::Starting);
            tracing::info!(msg = %message, "Received, activating tasks.");
            let mut _guard = context.task_map.lock().await;
            for (name, task) in _guard.iter() {
                tracing::info!("  Starting task: {}", name);
                let task = BuiltEdgeTask::try_from_any_sharable(task.clone())?;
                task.activate().await?;
            }
            tracing::info!(msg = %message, "done: all tasks activated");
            context.status.lock().await.clone_from(&EdgeStatus::Up);
        }
        AgentCommandKind::Suspend => {
            context
                .status
                .lock()
                .await
                .clone_from(&EdgeStatus::Stopping);
            tracing::info!(msg = %message, "Received, suspending tasks.");
            let mut _guard = context.task_map.lock().await;
            for (name, task) in _guard.iter() {
                let task = BuiltEdgeTask::try_from_any_sharable(task.clone())?;
                if matches!(
                    *task,
                    BuiltEdgeTask::CommandWatch(_)
                        | BuiltEdgeTask::TagWatch(_)
                        | BuiltEdgeTask::StateReport(_)
                ) {
                    tracing::info!(
                        "  Skipping CommandWatch/TagWatch/StateReport task: '{}'",
                        name
                    );
                    continue;
                }
                tracing::info!("  Suspending task: '{}'", name);
                task.deactivate().await?;
            }
            tracing::info!(msg = %message, "done: all tasks suspended");
            context
                .status
                .lock()
                .await
                .clone_from(&EdgeStatus::Suspended);
        }
        AgentCommandKind::Restart => {
            context
                .status
                .lock()
                .await
                .clone_from(&EdgeStatus::Stopping);
            tracing::info!(msg = %message, "Received, restarting tasks.");
            let mut _guard = context.task_map.lock().await;
            for (name, task) in _guard.iter() {
                tracing::info!("  Suspending task: '{}'", name);
                let task = BuiltEdgeTask::try_from_any_sharable(task.clone())?;
                context
                    .status
                    .lock()
                    .await
                    .clone_from(&EdgeStatus::Stopping);
                task.deactivate().await?;
            }
            context
                .status
                .lock()
                .await
                .clone_from(&EdgeStatus::Suspended);
            context
                .status
                .lock()
                .await
                .clone_from(&EdgeStatus::Starting);
            for (name, task) in _guard.iter() {
                tracing::info!("  Starting task: '{}'", name);
                let task = BuiltEdgeTask::try_from_any_sharable(task.clone())?;
                task.activate().await?;
            }
            context.status.lock().await.clone_from(&EdgeStatus::Up);
            tracing::info!(msg = %message, "done: all tasks restarted");
        }
        AgentCommandKind::Reconfigure(new_state) => {
            tracing::info!(msg = %message, "Received, reconfiguring tasks.");
            match *new_state.clone() {
                RootStateKind::Edge(mut new_state) => {
                    let (current_state, new_state) = {
                        // Update root state
                        let root_state = context.root();
                        let mut _guard = root_state.lock().await;
                        let current_state = _guard.clone();
                        new_state.runtime.edge_id = _guard.runtime.edge_id.clone();
                        new_state.runtime.model = _guard.runtime.model.clone();
                        *_guard = new_state.clone();
                        (current_state, new_state)
                    };

                    let removed_data_source_keys = {
                        // Data source diff
                        let mut _guard = context.data_source_map.lock().await;
                        let new_keys: HashSet<String> =
                            new_state.data_source.keys().cloned().collect();
                        let current_keys: HashSet<String> =
                            current_state.data_source.keys().cloned().collect();

                        let added_keys: Vec<String> =
                            new_keys.difference(&current_keys).cloned().collect();
                        let removed_keys: Vec<String> =
                            current_keys.difference(&new_keys).cloned().collect();

                        tracing::info!(
                            "  Data source diff — added: {:?}, removed: {:?}",
                            added_keys,
                            removed_keys
                        );

                        for key in added_keys {
                            tracing::info!("  Adding new data source: {}", key);
                            if let Some(state) = new_state.data_source.get(&key) {
                                let data_source =
                                    BuiltDataSource::try_from_data_source_spec(state).await?;
                                _guard.insert(key.clone(), Arc::new(data_source));
                            }
                        }

                        removed_keys
                    };

                    let removed_client_keys = {
                        // Client diff
                        let mut _guard = context.client_map.lock().await;
                        let new_keys: HashSet<String> = new_state.client.keys().cloned().collect();
                        let current_keys: HashSet<String> =
                            current_state.client.keys().cloned().collect();

                        let added_keys: Vec<String> =
                            new_keys.difference(&current_keys).cloned().collect();
                        let removed_keys: Vec<String> =
                            current_keys.difference(&new_keys).cloned().collect();

                        tracing::info!(
                            "  Client diff — added: {:?}, removed: {:?}",
                            added_keys,
                            removed_keys
                        );

                        for key in added_keys {
                            tracing::info!("  Adding new client: {}", key);
                            if let Some(state) = new_state.client.get(&key) {
                                let built_client: BuiltClient =
                                    BuiltClient::try_from_client_spec(state).await?;
                                _guard.insert(key.clone(), Arc::new(built_client));
                            }
                        }

                        removed_keys
                    };

                    {
                        // Reconfigure tasks based on new state
                        let mut _guard = context.task_map.lock().await;

                        // Identify tasks to add and remove by comparing spec keys with current task map keys.
                        let new_keys: HashSet<String> = new_state.task.keys().cloned().collect();
                        let current_keys: HashSet<String> = _guard.keys().cloned().collect();

                        let added_keys: Vec<String> =
                            new_keys.difference(&current_keys).cloned().collect();
                        let removed_keys: Vec<String> =
                            current_keys.difference(&new_keys).cloned().collect();

                        tracing::info!(
                            "  Task diff — added: {:?}, removed: {:?}",
                            added_keys,
                            removed_keys
                        );

                        for key in removed_keys {
                            tracing::info!("  Discarding removed task: {}", key);
                            if let Some(task) = _guard.remove(&key) {
                                let task = BuiltEdgeTask::try_from_any_sharable(task)?;
                                task.discard().await?;
                            }
                        }

                        for key in added_keys {
                            tracing::info!("  Adding new task: {}", key);
                            if let Some(task) = new_state.task.get(&key) {
                                let task = context.create_task(task).await?;
                                _guard.insert(key.clone(), task);
                            }
                        }

                        tracing::debug!(
                            "  Checking existing tasks for spec changes: {:?}",
                            _guard.keys()
                        );
                        for (name, task) in _guard.iter() {
                            tracing::debug!("  Checking task '{}' for spec changes", name);

                            let task = BuiltEdgeTask::try_from_any_sharable(task.clone())?;

                            let result = task.state().await;

                            let new_task_spec = new_state
                                .task
                                .get(name)
                                .ok_or_else(|| {
                                    (
                                        format!("No task config found for task '{}'", name),
                                        InternalErrorKind::InvalidConfig,
                                    )
                                        .into()
                                })?
                                .clone();

                            let should_reconfigure = match &result {
                                Err(_) => false,
                                Ok(current_spec) => *current_spec != new_task_spec,
                            };

                            if should_reconfigure {
                                tracing::info!("  Reconfiguring task '{}' with new spec", name,);
                                // For simplicity, we just discard and recreate the task here.
                                // A more sophisticated implementation could attempt in-place updates.
                                task.reconfigure(new_task_spec).await?;
                            } else {
                                tracing::info!(
                                    "  Task '{}' spec unchanged, skipping reconfiguration",
                                    name
                                );
                            }
                            tracing::debug!("  Finished checking task '{}' for spec changes", name,);
                        }
                    }

                    {
                        // Remove data sources that are no longer in the new state
                        let mut _guard = context.data_source_map.lock().await;
                        for key in removed_data_source_keys {
                            tracing::info!("  Removing data source: {}", key);
                            _guard.remove(&key);
                        }
                    }

                    {
                        // Remove clients that are no longer in the new state
                        let mut _guard = context.client_map.lock().await;
                        for key in removed_client_keys {
                            tracing::info!("  Removing client: {}", key);
                            _guard.remove(&key);
                        }
                    }
                }
                _ => {
                    tracing::warn!("Unsupported reconfiguration target: {:?}", new_state);
                }
            }
            tracing::info!(msg = %message, "done: reconfigured tasks if needed");
        }
    }

    Ok(())
}

impl EdgeAgent {
    #[instrument(skip(context))]
    pub(crate) async fn new(
        context: Arc<EdgeResourceContextImpl>,
        rx: Receiver<AgentCommand>,
    ) -> InternalResult<Self> {
        let with_data_source = context.root().lock().await.runtime.with_data_source.clone();
        let data_source = context.with_data_source(&with_data_source).await?;
        let rdb = match data_source.as_ref() {
            BuiltDataSource::SqliteDb(rdb) => rdb.clone(),
            _ => {
                return Err((
                    "Requires an Sqlite data source, found different type",
                    InternalErrorKind::TypeConversion,
                )
                    .into());
            }
        };

        Ok(EdgeAgent {
            context,
            rdb,
            rx: Mutex::new(rx),
        })
    }

    // Example method that could be called via RPC
    #[instrument(skip(self, params))]
    pub async fn step(&self, params: &[u8]) -> InternalResult<Box<[u8]>> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Agent step called with params_length={}", params.len());
        }

        let peer = flatbuffers::root::<fbp::Peer>(params)
            .map_err(|e| Into::<InternalError>::into(("parse flatbuffers peer", e)))?;

        let json_value = peer_to_json(peer);

        // Optional: keep logging as a string.
        let json_text = serde_json::to_string(&json_value)
            .map_err(|e| Into::<InternalError>::into(("serialize peer to json", e)))?;

        // Extract camera images from the FlatBuffer.
        let camera_images: Vec<(CameraId, Vec<u8>)> = peer
            .camera_images()
            .map(|v| {
                (0..v.len())
                    .filter_map(|i| {
                        let ci = v.get(i);
                        let cam_id: CameraId = ci.camera_id().into();
                        let image = ci.image()?.bytes().to_vec();
                        Some((cam_id, image))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let num_rows = self
            .insert_system_state(peer.ts(), &json_text, &camera_images)
            .await?;

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("system state row count={}", num_rows);
        }

        Ok(vec![].into_boxed_slice())
    }

    #[instrument(skip(self, json_text, camera_images))]
    async fn insert_system_state(
        &self,
        ts: f64,
        json_text: &str,
        camera_images: &[(CameraId, Vec<u8>)],
    ) -> InternalResult<i64> {
        let json_text = json_text.to_string();
        let camera_images = camera_images.to_vec();
        let num_rows = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                sqlx::query(sql::_SQL_INSERT_STATE)
                    .bind(ts)
                    .bind(&json_text)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| Into::<InternalError>::into(("insert system state", e)))?;

                for (camera_id, image) in &camera_images {
                    sqlx::query(sql::_SQL_INSERT_CAMERA_IMAGE)
                        .bind(ts)
                        .bind(camera_id.to_string())
                        .bind(image)
                        .execute(&mut **tx)
                        .await
                        .map_err(|e| Into::<InternalError>::into(("insert camera image", e)))?;
                }

                let num_rows: i64 = sqlx::query_scalar(sql::_SQL_COUNT_SYSTEM_STATE)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| Into::<InternalError>::into(("count system state", e)))?;

                Ok(num_rows)
            })
            .await?
            .commit()
            .await?;
        match num_rows {
            None => Err((
                "No rows in system_state after inserting one",
                InternalErrorKind::Runtime,
            )
                .into()),
            Some(num_rows) => Ok(num_rows),
        }
    }
}

pub struct AgentSkeleton {
    agent: Arc<EdgeAgent>,
}

impl AgentSkeleton {
    #[instrument(skip(agent))]
    pub(crate) fn new(agent: Arc<EdgeAgent>) -> Self {
        AgentSkeleton { agent }
    }
}

#[async_trait]
impl MethodHandler for AgentSkeleton {
    #[instrument(skip(self, params))]
    async fn handle(&self, method: &str, params: Box<[u8]>) -> InternalResult<Box<[u8]>> {
        let result = match method {
            "step" => self.agent.step(&params).await,
            "stop" => self
                .agent
                .context
                .send_command(AgentCommand {
                    timestamp: UnixTimestamp::now(),
                    payload: AgentCommandKind::Shutdown,
                })
                .await
                .map(|_| Vec::new().into_boxed_slice()),
            _ => Err((
                format!("Unknown method: {}", method),
                InternalErrorKind::Protocol,
            )
                .into()),
        };
        result
    }
}

impl EdgeAgent {
    #[instrument(skip(edge_spec))]
    pub async fn try_from(edge_spec: &EdgeSpec) -> InternalResult<Arc<EdgeAgent>> {
        let (tx, rx) = channel::<AgentCommand>(MPSC_BUFSIZ);
        let task_factory = EdgeTaskFactoryImpl::new();
        let context = EdgeResourceContextImpl::new(edge_spec, task_factory, tx);

        // Build data source
        {
            fn try_from(
                state: Box<ouro_state::core::DataSourceSpec>,
            ) -> Pin<Box<dyn Future<Output = InternalResult<BuiltDataSource>> + Send>> {
                Box::pin(async move { BuiltDataSource::try_from_data_source_spec(&state).await })
            }

            let data_source = edge_spec.data_source.clone();
            let mut data_source_map = context.data_source_map.lock().await;
            context
                .build_resource_map(&data_source, &mut data_source_map, &try_from)
                .await?;
        }

        // Build clients
        {
            fn try_from(
                state: Box<ouro_state::core::ClientSpec>,
            ) -> Pin<Box<dyn Future<Output = InternalResult<BuiltClient>> + Send>> {
                Box::pin(async move { BuiltClient::try_from_client_spec(&state).await })
            }

            let client = edge_spec.client.clone();
            let mut client_map = context.client_map.lock().await;
            context
                .build_resource_map(&client, &mut client_map, &try_from)
                .await?;
        }

        // Build tasks
        {
            let context1 = context.clone();
            let mut _guard = context1.task_map.lock().await;
            let futures: Vec<_> = edge_spec
                .task
                .iter()
                .map(|(name, task)| {
                    let context1 = context.clone();
                    async move {
                        let built_task = context1.create_task(task).await?;
                        Ok::<(String, AnySharable), InternalError>((name.clone(), built_task))
                    }
                })
                .collect();

            let results: Vec<Result<(String, AnySharable), _>> =
                futures::future::join_all(futures).await;
            for result in results {
                let (name, task) = result?;
                _guard.insert(name, task);
            }
        }

        let agent = Arc::new(EdgeAgent::new(context.clone(), rx).await?);

        context.status.lock().await.clone_from(&EdgeStatus::Up);

        Ok(agent)
    }
}

struct EdgeTaskFactoryImpl {}

impl EdgeTaskFactoryImpl {
    pub fn new() -> Arc<Self> {
        Arc::new(EdgeTaskFactoryImpl {})
    }
}

#[async_trait]
impl EdgeTaskFactory for EdgeTaskFactoryImpl {
    async fn create_task(
        &self,
        state: &EdgeTaskSpec,
        context: &Arc<EdgeResourceContextImpl>,
    ) -> InternalResult<AnySharable> {
        match state {
            EdgeTaskSpec::DataUpload { name, data, .. } => {
                tracing::info!("Building DataUpload task: {}", name);

                match data {
                    UploadDataSpec::Heartbeat => {
                        tracing::info!("Building Heartbeat task: {}", name);
                        let task = HeartbeatTask::new(name);

                        let comp_guard = CompGuard::new(name, task, state.clone(), context).await?;

                        Ok(Arc::new(BuiltEdgeTask::Heartbeat(comp_guard)))
                    }

                    UploadDataSpec::State => {
                        tracing::info!("Building State report task: {}", name);

                        let report = EdgeStateReport::new(
                            &context.root().lock().await.clone(),
                            EdgeStatus::Starting,
                            get_system_info()?,
                            vec![],
                        );

                        let task = StateReportTask::new(report);

                        let comp_guard = CompGuard::new(name, task, state.clone(), context).await?;

                        Ok(Arc::new(BuiltEdgeTask::StateReport(comp_guard)))
                    }

                    UploadDataSpec::Trajectory { .. } => {
                        tracing::info!("Building Trajectory task: {}", name);

                        let task = MetadataSendTask::new();

                        let comp_guard = CompGuard::new(name, task, state.clone(), context).await?;

                        Ok(Arc::new(BuiltEdgeTask::Metadata(comp_guard)))
                    }
                }
            }
            EdgeTaskSpec::CommandWatch { name, .. } => {
                tracing::info!("Building CommandWatch task: {}", name);

                let task = CommandWatchTask::new();

                let comp_guard = CompGuard::new(name, task, state.clone(), context).await?;

                Ok(Arc::new(BuiltEdgeTask::CommandWatch(comp_guard)))
            }
            EdgeTaskSpec::TagWatch { name, .. } => {
                tracing::info!("Building TagWatch task: {}", name);

                let task = tag_watch::TagWatchTask::new();

                let comp_guard = CompGuard::new(name, task, state.clone(), context).await?;

                Ok(Arc::new(BuiltEdgeTask::TagWatch(comp_guard)))
            }
        }
    }
}

#[instrument[skip(peer)]]
fn peer_to_json(peer: fbp::Peer<'_>) -> serde_json::Value {
    // Convert a FlatBuffers Peer view into owned JSON data.
    let leader_json = peer.leader().map(|l| {
        serde_json::json!({
            "vin": l.vin(),
            "dist": l.dist(),
        })
    });

    let next_tls_json = peer
        .next_tls()
        .map(|v| {
            (0..v.len())
                .map(|i| {
                    let t = v.get(i);
                    serde_json::json!({
                        "id": t.id(),
                        "idx": t.idx(),
                        "dist": t.dist(),
                        "signal": t.signal(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let neighbor_vehicles_json = peer
        .neighbor_vehicles()
        .map(|v| {
            (0..v.len())
                .map(|i| {
                    let n = v.get(i);
                    serde_json::json!({
                        "vin": n.vin(),
                        "x": n.x(),
                        "y": n.y(),
                        "lon": n.lon(),
                        "lat": n.lat(),
                        "speed": n.speed(),
                        "accel": n.accel(),
                        "road_id": n.road_id(),
                        "lane_id": n.lane_id(),
                        "lane_idx": n.lane_idx(),
                        "angle": n.angle(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    serde_json::json!({
        "vin": peer.vin(),
        "status": peer.status(),
        "ts": peer.ts(),

        // Add other scalar fields as needed (examples):
        "x": peer.x(),
        "y": peer.y(),
        "lon": peer.lon(),
        "lat": peer.lat(),
        "angle": peer.angle(),
        "speed": peer.speed(),
        "accel": peer.accel(),

        "lane_id": peer.lane_id(),
        "lane_idx": peer.lane_idx(),
        "lane_pos": peer.lane_pos(),
        "road_id": peer.road_id(),

        "wait_time": peer.wait_time(),
        "dist": peer.dist(),

        "fuel_cons": peer.fuel_cons(),
        "co2": peer.co2(),
        "co": peer.co(),
        "nox": peer.nox(),
        "pmx": peer.pmx(),
        "noise": peer.noise(),

        "type_id": peer.type_id(),

        "leader": leader_json,
        "next_tls": next_tls_json,
        "neighbor_vehicles": neighbor_vehicles_json,
        "active_behavior": peer.active_behavior(),
    })
}
