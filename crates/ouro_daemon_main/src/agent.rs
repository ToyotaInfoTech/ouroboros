// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use crate::task::icc::register_icc_handler;
/// CLI RPC handling module for the Daemon Agent.
///
/// This module implements the Unix Domain Socket (UDS) based RPC communication
/// layer between the daemon and CLI clients. It manages the lifecycle of
/// individual client connections using a state machine approach.
///
/// # Architecture
///
/// The module uses a state machine ([`StateMachine`]) per connection, cycling through:
/// - [`State::Connected`]: Actively reading requests and dispatching responses.
/// - [`State::Finished`]: Connection has been closed or errored out.
///
/// Each connection is enqueued into a shared MPSC channel and processed
/// sequentially via [`_handle_machine`]. The framing protocol uses
/// [`LengthDelimitedCodec`] over a [`UnixStream`], with CBOR serialization
/// for request/response payloads.
///
/// # Flow
///
/// 1. A new UDS connection is accepted by the outer [`DaemonAgent`].
/// 2. [`StateMachine::start`] wraps the stream in a framed codec and enqueues
///    the machine for processing.
/// 3. [`_handle_machine`] dequeues the machine, calls [`StateMachine::trigger`]
///    to read and dispatch the next message, and re-queues the machine if the
///    connection is still active.
/// 4. [`StateMachine::process`] deserializes the incoming CBOR request,
///    dispatches it via [`dispatch`], serializes the response, and sends it back.
use crate::{DaemonTaskFactoryImpl, RepositoryFactoryImpl};
use async_trait::async_trait;
use ouro_common::constant::core::MPSC_BUFSIZ;
use ouro_common::types::AnySharable;
use ouro_core::comm::net::Listener;
use ouro_core::error::InternalError;
use ouro_core::runtime::comp::{CompContext, ComponentImpl, ComponentImplExt};
use ouro_core::runtime::types::{BuiltClient, BuiltServer};
use ouro_core::runtime::types::{BuiltDataSource, SharableAction};
use ouro_core::{define_resource_trigger_action, InternalResult};
use ouro_daemon::runtime::context::{DaemonResourceContextImpl, RepositoryFactory};
use ouro_schema::schema::agent_control::AgentCommand;
use ouro_state::daemon::DaemonSpec;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver};
use tokio::sync::Mutex;
use tracing::instrument;

/// Daemon Agent managing local operations and communication with daemon.
///
/// The Agent handles local state, RPC processing, heartbeat publishing,
/// and coordination with the daemon.
pub struct DaemonAgent {
    // TODO might want to remove pub
    pub context: Arc<DaemonResourceContextImpl>,
    rx: Mutex<Receiver<AgentCommand>>,
}

impl DaemonAgent {
    /// Create a new DaemonAgent instance.
    fn new(context: Arc<DaemonResourceContextImpl>, rx: Receiver<AgentCommand>) -> Arc<Self> {
        Arc::new(DaemonAgent {
            context: context.clone(),
            rx: Mutex::new(rx),
        })
    }

    /// Build an Agent from a DaemonSpec and an injected task factory.
    ///
    /// # Arguments
    /// * `config` - Daemon configuration spec
    /// * `task_factory` - Factory for creating task components
    ///
    /// # Returns
    /// * `InternalResult<DaemonAgent>` - Fully initialized Agent
    #[instrument(skip(config))]
    pub async fn try_from(config: &DaemonSpec) -> InternalResult<Arc<Self>> {
        let repository_factory = RepositoryFactoryImpl::new();
        let task_factory = Arc::new(DaemonTaskFactoryImpl::new());

        let (command_tx, command_rx) = channel::<AgentCommand>(MPSC_BUFSIZ);
        let context =
            DaemonResourceContextImpl::new(config, repository_factory, task_factory, command_tx);

        // Build data source
        {
            fn try_from(
                state: Box<ouro_state::core::DataSourceSpec>,
            ) -> Pin<Box<dyn Future<Output = InternalResult<BuiltDataSource>> + Send>> {
                Box::pin(async move { BuiltDataSource::try_from_data_source_spec(&state).await })
            }

            let data_source = config.data_source.clone();
            let mut data_source_map = context.data_source_map.lock().await;
            context
                .build_resource_map(&data_source, &mut data_source_map, &try_from)
                .await?;
        }

        // Build clients
        {
            let client = config.client.clone();
            fn try_from(
                state: Box<ouro_state::core::ClientSpec>,
            ) -> Pin<Box<dyn Future<Output = InternalResult<BuiltClient>> + Send>> {
                Box::pin(async move { BuiltClient::try_from_client_spec(&state).await })
            }

            let mut client_map = context.client_map.lock().await;
            context
                .build_resource_map(&client, &mut client_map, &try_from)
                .await?;
        }

        // Build servers
        {
            let server = config.server.clone();
            let context1 = context.clone();
            let mut _guard = context1.server_map.lock().await;
            _guard.extend(server.iter().map(|(name, state)| {
                (
                    name.clone(),
                    Arc::new(BuiltServer::from_server_spec(state.clone())),
                )
            }));
        }

        // Build repositories
        {
            let repository = config.repository.clone();
            let context1 = context.clone();
            let mut _guard = context1.repository_map.lock().await;
            let futures = repository
                .iter()
                .map(|(name, repository)| {
                    let context1 = context.clone();
                    let context2 = context.clone();
                    async move {
                        let repository = context1.create_repository(repository, &context2).await?;
                        Ok::<(String, AnySharable), InternalError>((name.clone(), repository))
                    }
                })
                .collect::<Vec<_>>();
            let results: Vec<Result<(String, AnySharable), InternalError>> =
                futures::future::join_all(futures).await;
            for result in results {
                let (name, built_repository) = result?;
                _guard.insert(name, built_repository);
            }
        }

        // Build tasks
        {
            let task = config.task.clone();
            let context1 = context.clone();
            let mut _guard = context1.task_map.lock().await;
            let futures = task
                .iter()
                .map(|(name, task)| {
                    let context1 = context.clone();
                    async move {
                        let built_task = context1.create_task(task).await?;
                        Ok::<(String, AnySharable), InternalError>((name.clone(), built_task))
                    }
                })
                .collect::<Vec<_>>();
            let results: Vec<Result<(String, AnySharable), InternalError>> =
                futures::future::join_all(futures).await;
            for result in results {
                let (name, built_task) = result?;
                _guard.insert(name, built_task);
            }
        }

        Ok(DaemonAgent::new(context, command_rx))
    }
}

#[async_trait]
impl ComponentImpl for DaemonAgent {
    type StateImpl = DaemonSpec;
    type ContextImpl = DaemonResourceContextImpl;

    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<tokio::task::JoinHandle<()>>> {
        ComponentImplExt::on_active(self, state, context).await
    }
}

#[async_trait]
impl ComponentImplExt for DaemonAgent {
    #[instrument(skip(self, _state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        _state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = vec![];

        let listener = Listener::try_from({
            let state = context.current_state();
            let _guard = state.lock().await;
            let cli_endpoint = _guard.cli_endpoint.clone();

            tracing::debug!("Daemon CLI endpoint configuration: {:?}", cli_endpoint);

            cli_endpoint
        })
        .await?;

        {
            let this = self.clone();
            let context = context.clone();
            let action = define_resource_trigger_action!(
                with {
                    let this = this.clone();
                    let context = context.clone();
                }
                when: let result = this.rx.lock().await.recv().await,
                then: _handle_command(result, context)
            );
            actions.push(action);
        }

        let listener = match listener {
            Some(listener) => Arc::new(listener),
            None => {
                tracing::info!("No CLI RPC listener configured");
                return Ok(actions);
            }
        };

        {
            let this = self.clone();
            let action = define_resource_trigger_action!(
                with {
                    let listener = listener.clone();
                    let this = this.clone();
                }
                when: let stream = listener.accept().await.ok(),
                then: Box::pin(cli_rpc::start(stream, this))
            );
            actions.push(action);
        }

        {
            let context = context.clone();
            let this = self.clone();
            let name = context
                .current_state()
                .lock()
                .await
                .runtime
                .daemon_id
                .clone();
            actions.push(register_icc_handler(&this, &name, &context).await?);
        }

        Ok(actions)
    }
}

#[instrument(skip(result, _context))]
async fn _handle_command(
    result: Option<AgentCommand>,
    _context: Arc<CompContext<DaemonAgent>>,
) -> InternalResult<()> {
    if let Some(command) = result {
        tracing::info!("Received command: {:?}", command);
    }
    Ok(())
}

mod cli_rpc {
    use futures::{SinkExt, StreamExt};
    use ouro_common::io::FromCbor as _;
    use ouro_common::io::ToCbor as _;
    use ouro_core::comm::net::TransportStream;
    use ouro_core::error::InternalError;
    use ouro_core::InternalResult;
    use ouro_schema::schema::cli::CliHandler;
    use ouro_schema::schema::cli::{dispatch, CliRequest};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_util::codec::{Framed, LengthDelimitedCodec};
    use tracing::instrument;

    /// Starts a new state machine for an accepted connection.
    ///
    /// # Arguments
    /// * `result` - The result of accepting a new connection.
    /// * `cli_handler` - The daemon agent CLI handler.
    #[instrument(skip(stream, cli_handler))]
    pub(super) async fn start<H: CliHandler>(
        stream: Option<Box<dyn TransportStream>>,
        cli_handler: Arc<H>,
    ) -> InternalResult<()> {
        let stream = match stream {
            Some(stream) => stream,
            None => return Ok(()),
        };

        tokio::spawn(async move {
            tracing::info!("Accepted connection");

            let framed = Arc::new(Mutex::new(Framed::new(stream, LengthDelimitedCodec::new())));

            loop {
                let cli_handler = cli_handler.clone();
                let result = framed.lock().await.next().await;
                match result {
                    Some(Ok(request_bytes)) => {
                        let request = CliRequest::from_cbor(request_bytes.as_ref())
                            .map_err(|e| ("Failed to deserialize request", e).into())?;

                        tracing::debug!("Received request: {:?}", request);

                        let response = dispatch(cli_handler.as_ref(), request).await;
                        let response_bytes = response
                            .to_cbor()
                            .map_err(|e| ("Failed to serialize response", e).into())?;

                        framed
                            .lock()
                            .await
                            .send(response_bytes.into())
                            .await
                            .map_err(|e| ("Failed to send response", e).into())?;
                    }
                    Some(Err(e)) => {
                        tracing::error!("Error reading from connection: {:?}", e);
                        return Ok::<(), InternalError>(());
                    }
                    None => return Ok::<(), InternalError>(()),
                }
            }
        });
        Ok(())
    }
}

mod icc {
    use async_trait::async_trait;
    use ouro_schema::{
        operation_descriptions,
        schema::cli::{icc::IccHandler, DescribeResp, ListResp, OperateReq, OperateResp, TaskInfo},
        Void,
    };
    use tracing::instrument;

    use crate::{agent::DaemonAgent, types::BuiltDaemonTask};

    #[async_trait]
    impl IccHandler for DaemonAgent {
        #[instrument(skip(self, _req))]
        async fn list(&self, _req: Void) -> anyhow::Result<ListResp> {
            let _guard = self.context.task_map.lock().await;
            let icc_channel_names = self.context.icc_channnel_names().await.unwrap_or_default();
            let tasks = _guard
                .iter()
                .filter(|(name, _)| icc_channel_names.contains(name))
                .filter_map(|(name, task)| {
                    BuiltDaemonTask::try_from_any_sharable(task.clone())
                        .ok()
                        .map(|built| TaskInfo {
                            name: name.clone(),
                            r#type: format!("{}", built),
                        })
                })
                .collect();
            Ok(ListResp { tasks })
        }

        #[instrument(skip(self, _req))]
        async fn describe(&self, _req: Void) -> anyhow::Result<DescribeResp> {
            Ok(DescribeResp {
                descriptions: operation_descriptions![],
            })
        }

        #[instrument(skip(self))]
        async fn operate(&self, _req: OperateReq) -> anyhow::Result<OperateResp> {
            Err(anyhow::anyhow!(
                "Operate not implemented for '{}'",
                stringify!(DaemonAgent)
            ))
        }
    }
}
