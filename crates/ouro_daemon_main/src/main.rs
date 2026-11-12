// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Ouroboros Daemon binary.
//!
//! The daemon is responsible for managing edge devices and coordinating
//! system-wide operations.

pub mod agent;
pub mod task;
pub mod types;

use std::path::Path;
use std::sync::Arc;

use crate::agent::DaemonAgent;
use crate::task::state::manager::ShadowManagerTask;
use crate::task::state::tag_manager::TagManagerTask;
use crate::task::{heartbeat, metadata, metrics, state, webui};
use crate::types::{BuiltDaemonTask, BuiltRepository};
use async_trait::async_trait;
use clap::{self, Parser};
use ouro_common::io::FromTomlFile as _;
use ouro_common::types::AnySharable;
use ouro_core::error::{panic_if_env_set, InternalErrorKind};
use ouro_core::runtime::comp::CompGuard;
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::{setup_tracing, InternalResult};
use ouro_daemon::runtime::context::{
    DaemonResourceContextImpl, DaemonTaskFactory, RepositoryFactory,
};
use ouro_state::daemon::{
    DaemonTaskSpec, IngestionDataSpec, RepositoryDataSpec, RepositorySpec, ShadowStoreSpec,
};
use ouro_webui::event::DashboardEvent;
use tokio::sync::broadcast;

use ouro_state::daemon::DaemonSpec;
use ouro_state::edge::EdgeSpec;
use tracing::instrument;

#[tokio::main]
async fn main() {
    setup_tracing();
    if let Err(e) = run().await {
        panic_if_env_set(&e);
    }
}

#[instrument]
async fn run() -> InternalResult<()> {
    tracing::info!(
        "******** [[ Ouroboros Daemon (pid:{}) ]] ********",
        std::process::id(),
    );

    #[derive(clap::Parser, Debug)]
    #[clap(
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
        author = env!("CARGO_PKG_AUTHORS"),
        about = env!("CARGO_PKG_DESCRIPTION"),
        arg_required_else_help = true,
        long_about = None
    )]
    struct Args {
        /// Edge Identification Number
        #[clap(
            short = 'i',
            long,
            default_value = "default_id",
            help = "Edge Identification Number"
        )]
        id: String,

        #[clap(short = 'c', long, help = "Configuration TOML file path")]
        config: String,

        #[clap(short = 'l', long, help = "Check configuration file syntax and exit")]
        lint: bool,
    }

    let args = Args::parse();
    tracing::debug!("Parsed command-line arguments: {:?}", args);

    // Parse TOML configuration file
    let state = DaemonSpec::from_toml_file(&args.config).map_err(|e| {
        (
            format!("Failed to parse toml file: {}: {:?}", args.config, e).as_str(),
            InternalErrorKind::Parse,
        )
            .into()
    })?;

    if args.lint {
        println!("Syntax is valid: {}", args.config);
        println!("Loaded daemon config:\n{:#?}", state);
    }

    let agent = DaemonAgent::try_from(&state).await?;

    {
        let _guard = agent.context.task_map.lock().await;
        for (name, task) in _guard.iter() {
            tracing::info!("Initialized task: {}", name);
            let task = task.clone();
            let _task = BuiltDaemonTask::try_from_any_sharable(task)?;
        }
    }

    let context = agent.context.clone();
    let _agent = CompGuard::new("ourodaemon", agent.clone(), state, &context).await?;

    if args.lint {
        println!("An instance of Agent was successfully created and ready to start");
        std::process::exit(0);
    }

    tracing::info!("Ouroboros Daemon is running.");

    loop {
        tokio::time::sleep(tokio::time::Duration::MAX).await;
    }
}

/// Concrete factory for daemon tasks.
///
/// Implements DaemonTaskFactory to create all supported task types.
pub struct DaemonTaskFactoryImpl {
    event_tx: broadcast::Sender<DashboardEvent>,
}

impl DaemonTaskFactoryImpl {
    /// Creates a new DaemonTaskFactoryImpl.
    ///
    /// Returns:
    ///     Self: New factory instance
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (event_tx, _) = ouro_webui::event_channel();
        Self { event_tx }
    }
}

#[async_trait]
impl DaemonTaskFactory for DaemonTaskFactoryImpl {
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
        task_state: &DaemonTaskSpec,
        context: &Arc<DaemonResourceContextImpl>,
    ) -> InternalResult<AnySharable> {
        match task_state {
            DaemonTaskSpec::DataIngestion { name, data, .. } => match data {
                IngestionDataSpec::Heartbeat { .. } => {
                    let task = heartbeat::HeartbeatTask::new(
                        context.metrics_store().clone(),
                        self.event_tx.clone(),
                    );
                    let comp_guard =
                        CompGuard::new(name, task, task_state.clone(), context).await?;
                    Ok(Arc::new(BuiltDaemonTask::Heartbeat(comp_guard)) as AnySharable)
                }
                IngestionDataSpec::Trajectory => {
                    let task = metadata::TrajectoryTask::new(name, context.metrics_store().clone());
                    let comp_guard =
                        CompGuard::new(name, task, task_state.clone(), context).await?;
                    Ok(Arc::new(BuiltDaemonTask::Trajectory(comp_guard)) as AnySharable)
                }
            },
            DaemonTaskSpec::RepositoryIngestion {
                name,
                repository_data,
                ..
            } => match repository_data {
                RepositoryDataSpec::State => {
                    let task = ShadowManagerTask::new(self.event_tx.clone());
                    let comp_guard =
                        CompGuard::new(name, task, task_state.clone(), context).await?;
                    Ok(Arc::new(BuiltDaemonTask::State(comp_guard)) as AnySharable)
                }
            },
            DaemonTaskSpec::Metrics { name, .. } => {
                let task =
                    metrics::JsonHttpMetricsServerTask::new(context.metrics_provider().clone());
                let comp_guard = CompGuard::new(name, task, task_state.clone(), context).await?;
                Ok(Arc::new(BuiltDaemonTask::Metrics(comp_guard)) as AnySharable)
            }
            DaemonTaskSpec::TagManagement {
                name,
                default_edge_state_path,
                predefined_tag,
                ..
            } => {
                let default_edge_state: EdgeSpec =
                    _load_default_edge_state(default_edge_state_path)?;
                let task = TagManagerTask::new(default_edge_state, predefined_tag.clone());
                let comp_guard = CompGuard::new(name, task, task_state.clone(), context).await?;
                Ok(Arc::new(BuiltDaemonTask::Tag(comp_guard)) as AnySharable)
            }
            DaemonTaskSpec::WebUi { name, .. } => {
                let task = webui::WebUiTask::new(self.event_tx.clone());
                let comp_guard = CompGuard::new(name, task, task_state.clone(), context).await?;
                Ok(Arc::new(BuiltDaemonTask::WebUi(comp_guard)) as AnySharable)
            }
        }
    }
}

pub struct RepositoryFactoryImpl {}

impl RepositoryFactoryImpl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {})
    }
}

#[async_trait]
impl RepositoryFactory for RepositoryFactoryImpl {
    async fn create_repository(
        &self,
        state: &RepositorySpec,
        context: &Arc<DaemonResourceContextImpl>,
    ) -> InternalResult<AnySharable> {
        match state {
            RepositorySpec::ShadowStore { name, shadow_store } => match shadow_store {
                ShadowStoreSpec::Transient => {
                    let store = state::transient::TransientShadowStore::new(name.to_string());
                    Ok(Arc::new(BuiltRepository::ShadowStore(Arc::new(
                        state::store::BuiltShadowStore::Transient(store),
                    ))) as AnySharable)
                }
                ShadowStoreSpec::Persistent {
                    with_data_source, ..
                } => {
                    let data_source = context.with_data_source(with_data_source).await?;
                    let rdb = data_source.as_postgres()?;
                    let store =
                        state::persistent::PersistentShadowStore::new(name.to_string(), rdb);
                    Ok(Arc::new(BuiltRepository::ShadowStore(Arc::new(
                        state::store::BuiltShadowStore::Persistent(store),
                    ))) as AnySharable)
                }
                ShadowStoreSpec::WriteBehind {
                    with_data_source, ..
                } => {
                    let data_source = context.with_data_source(with_data_source).await?;
                    let rdb = data_source.as_postgres()?;
                    let store =
                        state::write_behind::WriteBehindShadowStore::new(name.to_string(), rdb);
                    Ok(Arc::new(BuiltRepository::ShadowStore(Arc::new(
                        state::store::BuiltShadowStore::WriteBehind(store),
                    ))) as AnySharable)
                }
            },
            RepositorySpec::TagStore { name, tag_store } => {
                let data_source = context
                    .with_data_source(&tag_store.with_data_source)
                    .await?;
                let rdb = data_source.as_postgres()?;
                let store = state::tag_persistent::PersistentTagStore::new(name.to_string(), rdb);
                Ok(Arc::new(BuiltRepository::TagStore(Arc::new(
                    state::tag_store::BuiltTagStore::Persistent(store),
                ))) as AnySharable)
            }
        }
    }
}

#[instrument]
#[allow(clippy::result_large_err)]
fn _load_default_edge_state(config_toml: &Path) -> InternalResult<EdgeSpec> {
    tracing::debug!(
        config_toml = ?config_toml,
        "Loading default edge state from TOML file",
    );
    let config_toml = std::fs::read_to_string(config_toml).map_err(|e| {
        (
            format!("Failed to read edge config TOML file: {:?}", config_toml).as_str(),
            e,
        )
            .into()
    })?;
    let edge_spec: EdgeSpec = toml::from_str(config_toml.as_str()).map_err(|e| {
        (
            format!("Failed to parse edge config TOML file: {:?}", config_toml).as_str(),
            e,
        )
            .into()
    })?;
    Ok(edge_spec)
}

mod cli_handler {
    use async_trait::async_trait;
    use ouro_core::get_system_info;
    use ouro_schema::schema::cli::icc::{IccRequest, IccRequestWrapper, IccResponse};
    use ouro_schema::schema::cli::{
        CliHandler, DescribeReq, DescribeResp, EchoReq, EchoResp, ListResp, OperateReq,
        OperateResp, ShowReq, ShowResp, ShowTarget, ShowTargetKind, VersionReq, VersionResp,
        VersionResult,
    };
    use ouro_schema::Void;
    use tracing::instrument;

    use crate::agent::DaemonAgent;

    /// Sends an ICC command to the named task and returns the result.
    ///
    /// The daemon acts as a pure routing layer — it never inspects the
    /// ICC command or result payloads.
    async fn forward_icc(
        agent: &DaemonAgent,
        task_name: &str,
        request: IccRequest,
    ) -> anyhow::Result<IccResponse> {
        let tx = agent
            .context
            .lookup_icc_channel(task_name)
            .await
            .map_err(|e| anyhow::anyhow!("Task '{}' not found: {:?}", task_name, e))?;

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        tx.send(IccRequestWrapper {
            request,
            response_tx,
        })
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to send ICC request to task '{}': {:?}",
                task_name,
                e
            )
        })?;

        response_rx
            .await
            .map_err(|_| anyhow::anyhow!("Task '{}' did not respond", task_name))
    }

    #[async_trait]
    impl CliHandler for DaemonAgent {
        #[instrument(skip(self))]
        async fn version(&self, req: VersionReq) -> anyhow::Result<VersionResp> {
            tracing::debug!("Received version request: {:?}", req);
            let version_req = VersionReq::default();

            if req.version() != version_req.version() {
                Ok(VersionResp {
                    result: VersionResult::VersionMismatch {
                        message: format!(
                            "Version mismatch: client version '{}' is not compatible with server version '{}'",
                            req.version(), version_req.version()
                        ),
                    },
                })
            } else {
                Ok(VersionResp {
                    result: VersionResult::VersionOk {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                    },
                })
            }
        }

        #[instrument(skip(self))]
        async fn echo(&self, req: EchoReq) -> anyhow::Result<EchoResp> {
            tracing::debug!("Received echo request: {:?}", req);
            Ok(EchoResp {
                message: format!("Hello, {}!", req.message),
            })
        }

        #[instrument(skip(self))]
        async fn show(&self, req: ShowReq) -> anyhow::Result<ShowResp> {
            tracing::debug!("Received show request: {:?}", req);
            match req.target {
                ShowTargetKind::State => {
                    let state = self.context.current_state().lock().await.clone();
                    Ok(ShowResp {
                        target: ShowTarget::State(Box::new(state)),
                    })
                }
                ShowTargetKind::Runtime => match get_system_info() {
                    Ok(system_info) => Ok(ShowResp {
                        target: ShowTarget::Runtime(system_info),
                    }),
                    Err(e) => {
                        tracing::error!("Failed to get system info: {:?}", e);
                        Err(anyhow::anyhow!("Failed to get system info: {:?}", e))
                    }
                },
                ShowTargetKind::Edge => {
                    // let edge_shadow = self.context.edge_shadow().lock().await.clone();
                    Ok(ShowResp {
                        target: ShowTarget::Edge(vec![]),
                    })
                }
            }
        }

        #[instrument(skip(self))]
        async fn list(&self, _req: Void) -> anyhow::Result<ListResp> {
            tracing::debug!("Received list request");
            let task_name = self
                .context
                .current_state()
                .lock()
                .await
                .runtime
                .daemon_id
                .clone();
            match forward_icc(self, &task_name, IccRequest::List(Void)).await? {
                IccResponse::List(resp) => Ok(resp),
                IccResponse::Error(e) => Err(anyhow::Error::msg(e)),
                other => Err(anyhow::anyhow!("Unexpected ICC response: {:?}", other)),
            }
        }

        #[instrument(skip(self))]
        async fn describe(&self, req: DescribeReq) -> anyhow::Result<DescribeResp> {
            tracing::debug!("Received describe request: {:?}", req);
            let task_name = req.name.clone();
            match forward_icc(self, &task_name, IccRequest::Describe(Void)).await? {
                IccResponse::Describe(resp) => Ok(resp),
                IccResponse::Error(e) => Err(anyhow::Error::msg(e)),
                other => Err(anyhow::anyhow!("Unexpected ICC response: {:?}", other)),
            }
        }

        #[instrument(skip(self))]
        async fn operate(&self, req: OperateReq) -> anyhow::Result<OperateResp> {
            tracing::debug!("Received operate request: {:?}", req);
            let task_name = req.name.clone();
            match forward_icc(self, &task_name, IccRequest::Operate(req)).await? {
                IccResponse::Operate(resp) => Ok(resp),
                IccResponse::Error(e) => Err(anyhow::Error::msg(e)),
                other => Err(anyhow::anyhow!("Unexpected ICC response: {:?}", other)),
            }
        }
    }
}
