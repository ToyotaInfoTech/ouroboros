// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

/// Edge-side configuration subscriber for Device Shadow.
///
/// Subscribes to desired configuration from daemon and applies it locally.
use async_trait::async_trait;
use ouro_common::io::FromCbor as _;
use ouro_common::topic;
use ouro_core::comm::messaging::TopicMessage;
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::client::Callable as _;
use ouro_core::runtime::comp::{CompContext, ComponentImpl, ComponentImplExt};
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::types::SharableAction;
use ouro_core::{define_resource_trigger_action, InternalResult};
use ouro_edge::runtime::context::EdgeResourceContextImpl;
use ouro_schema::schema::agent_control::AgentCommand;
use ouro_state::edge::EdgeTaskSpec;
use std::any::type_name_of_val;
use std::sync::Arc;
use tracing::instrument;

/// Configuration subscriber task for edge devices.
///
/// Subscribes to config updates from daemon and maintains local desired config.
#[derive(Debug, Clone)]
pub struct CommandWatchTask {}

impl CommandWatchTask {
    /// Create a new state watcher.
    ///
    /// # Arguments
    /// * `edge_id` - Unique edge device identifier
    /// * `outbox` - Zenoh outbox for communication
    ///
    /// # Returns
    /// * `Result<StateWatcherTask>` - New subscriber instance or error
    pub fn new() -> Arc<Self> {
        Arc::new(Self {})
    }
}

#[async_trait]
impl ComponentImpl for CommandWatchTask {
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
impl ComponentImplExt for CommandWatchTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &EdgeTaskSpec,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = Vec::new();

        match state {
            EdgeTaskSpec::CommandWatch { with_client, .. } => {
                let client = context.with_client(with_client).await?;
                let client = client.as_retryable()?;
                let edge_id = context.root().lock().await.runtime.edge_id.clone();
                let topic = topic::command_key(edge_id.as_str());
                let session = client.transport()?.into_messaging()?;
                let subscriber = session.topic_subscriber(&topic).await?;

                let action = define_resource_trigger_action!(
                    with {
                        let subscriber = subscriber.clone();
                        let context = context.clone();
                    }
                    when: let result = subscriber.recv_async().await,
                    then: _recv_command(result, context)
                );

                actions.push(action);
            }
            _ => {
                return Err((
                    "Invalid task config for CommandWatcherTask",
                    InternalErrorKind::InvalidConfig,
                )
                    .into());
            }
        }

        Ok(actions)
    }
}

/// Start subscribing to configuration updates.
///
/// # Returns
/// * `Result<()>` - Ok on success, Err on failure
#[instrument(skip(result, context))]
async fn _recv_command(
    result: InternalResult<TopicMessage>,
    context: Arc<CompContext<CommandWatchTask>>,
) -> InternalResult<()> {
    let TopicMessage(topic, payload) = result?;

    tracing::debug!(
        "Received payload as command on topic {}: len={}",
        topic,
        payload.len()
    );

    let command = AgentCommand::from_cbor(payload.as_slice())
        .map_err(|e| ("Failed to decode command payload", e).into())?;

    tracing::debug!(type_name = %type_name_of_val(&command), "Received a command");

    let context: &EdgeResourceContextImpl = context.as_ref();

    context.send_command(command).await?;

    Ok(())
}
