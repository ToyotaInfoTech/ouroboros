// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Resource context traits for dependency resolution.
//!
//! Defines the interfaces for resolving runtime resources (data sources,
//! servers, clients) and creating components from specifications.

use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

#[cfg(feature = "daemon")]
use crate::runtime::types::BuiltServer;
use crate::{
    error::{InternalError, InternalErrorKind},
    runtime::types::{BuiltClient, BuiltDataSource},
    InternalResult,
};
use async_trait::async_trait;
use ouro_schema::schema::agent_control::AgentCommand;
#[cfg(feature = "daemon")]
use ouro_schema::schema::cli::icc::IccRequestWrapper;
#[cfg(feature = "daemon")]
use ouro_state::core::WithServer;
use ouro_state::core::{types::State, DataSourceRef, WithClient, WithDataSource};
use tokio::sync::{mpsc, Mutex, MutexGuard};
use tracing::instrument;

/// Resource resolution context for accessing built runtime resources.
///
/// Provides methods to resolve data sources, servers, and clients
/// from either inline definitions or named references.
#[async_trait]
pub trait ResourceContext: Send + Sync {
    /// Resolves a data source from a definition or reference.
    ///
    /// # Arguments
    /// * `with_data_source` - Inline definition or named reference
    ///
    /// # Returns
    /// * `Ok(Arc<BuiltDataSource>)` on success
    /// * `Err(InternalError)` if resolution fails
    async fn with_data_source(
        &self,
        with_data_source: &WithDataSource,
    ) -> InternalResult<Arc<BuiltDataSource>>;

    /// Resolves a server from a definition or reference.
    ///
    /// # Arguments
    /// * `with_server` - Inline definition or named reference
    ///
    /// # Returns
    /// * `Ok(Arc<BuiltServer>)` on success
    /// * `Err(InternalError)` if resolution fails
    #[cfg(feature = "daemon")]
    async fn with_server(&self, with_server: &WithServer) -> InternalResult<Arc<BuiltServer>>;

    /// Resolves a client from a definition or reference.
    ///
    /// # Arguments
    /// * `with_client` - Inline definition or named reference
    ///
    /// # Returns
    /// * `Ok(Arc<BuiltClient>)` on success
    /// * `Err(InternalError)` if resolution fails
    async fn with_client(&self, with_client: &WithClient) -> InternalResult<Arc<BuiltClient>>;

    async fn send_command(&self, command: AgentCommand) -> InternalResult<()>;
}

/// Type alias for an async builder function that takes a state reference and returns a built resource.
pub type Builder<S, B> =
    dyn Fn(Box<S>) -> Pin<Box<dyn Future<Output = InternalResult<B>> + Send>> + Send + Sync;

pub struct ResourceContextImpl {
    pub data_source_map: Mutex<HashMap<String, Arc<BuiltDataSource>>>,
    pub client_map: Mutex<HashMap<String, Arc<BuiltClient>>>,
    #[cfg(feature = "daemon")]
    pub server_map: Mutex<HashMap<String, Arc<BuiltServer>>>,
    #[cfg(feature = "daemon")]
    pub icc_sender_map: Mutex<HashMap<String, mpsc::Sender<IccRequestWrapper>>>,
    pub command_tx: mpsc::Sender<AgentCommand>,
}

impl ResourceContextImpl {
    pub fn new(command_tx: mpsc::Sender<AgentCommand>) -> Arc<Self> {
        Arc::new(Self {
            data_source_map: Mutex::new(HashMap::new()),
            client_map: Mutex::new(HashMap::new()),
            #[cfg(feature = "daemon")]
            server_map: Mutex::new(HashMap::new()),
            #[cfg(feature = "daemon")]
            icc_sender_map: Mutex::new(HashMap::new()),
            command_tx,
        })
    }

    #[cfg(feature = "daemon")]
    pub async fn register_icc_channel(
        &self,
        name: &str,
        tx: mpsc::Sender<IccRequestWrapper>,
    ) -> InternalResult<()> {
        let mut _guard = self.icc_sender_map.lock().await;

        if _guard.contains_key(name) {
            tracing::error!("ICC channel with name '{}' is already registered", name);
            return Err((
                format!("ICC channel with name '{}' is already registered", name),
                InternalErrorKind::InvalidConfig,
            )
                .into());
        }

        _guard.insert(name.to_string(), tx);
        Ok(())
    }

    #[cfg(feature = "daemon")]
    pub async fn icc_channnel_names(&self) -> InternalResult<Vec<String>> {
        let _guard = self.icc_sender_map.lock().await;
        Ok(_guard.keys().cloned().collect())
    }

    #[cfg(feature = "daemon")]
    pub async fn lookup_icc_channel(
        &self,
        name: &str,
    ) -> InternalResult<mpsc::Sender<IccRequestWrapper>> {
        let _guard = self.icc_sender_map.lock().await;

        _guard.get(name).cloned().ok_or_else(|| {
            (
                format!("ICC channel with name '{}' not found", name),
                InternalErrorKind::InvalidConfig,
            )
                .into()
        })
    }

    pub async fn build_resource_map<S, B>(
        &self,
        source_map: &HashMap<String, S>,
        target_map: &mut MutexGuard<'_, HashMap<String, Arc<B>>>,
        try_from: &Builder<S, B>,
    ) -> InternalResult<()>
    where
        S: State + Send + Sync,
    {
        //let mut _guard = target_map.lock().await;
        let futures = source_map
            .iter()
            .map(|(name, state)| async move {
                let data_source = try_from(Box::new(state.clone())).await?;
                Ok::<(String, Arc<B>), InternalError>((name.clone(), Arc::new(data_source)))
            })
            .collect::<Vec<_>>();
        let results: Vec<Result<(String, Arc<B>), _>> = futures::future::join_all(futures).await;
        for result in results {
            let (name, data_source) = result?;
            target_map.insert(name, data_source);
        }
        Ok(())
    }
}

#[async_trait]
impl ResourceContext for ResourceContextImpl {
    async fn with_data_source(
        &self,
        with_data_source: &WithDataSource,
    ) -> InternalResult<Arc<BuiltDataSource>> {
        match with_data_source {
            WithDataSource::Def { def: data_source } => {
                let data_source = BuiltDataSource::try_from_data_source_spec(data_source).await?;
                Ok(Arc::new(data_source))
            }
            WithDataSource::Ref {
                r#ref:
                    DataSourceRef::SqlDb {
                        r#ref: data_source_ref,
                        init_sql,
                    },
            } => {
                let data_source = self
                    .data_source_map
                    .lock()
                    .await
                    .get(data_source_ref.as_str())
                    .cloned()
                    .ok_or_else(|| {
                        (
                            format!("Data source reference '{}' not found", data_source_ref),
                            InternalErrorKind::InvalidConfig,
                        )
                            .into()
                    })?;

                match data_source.as_ref() {
                    BuiltDataSource::SqliteDb(rdb) => {
                        rdb.begin()
                            .await?
                            .pipe(|tx, _| async move {
                                for sql in init_sql {
                                    sqlx::query(sql).execute(&mut **tx).await.map_err(|e| {
                                        (
                                            format!(
                                                "Failed to execute init SQL for data source '{}'",
                                                data_source_ref
                                            ),
                                            e,
                                        )
                                            .into()
                                    })?;
                                }
                                Ok::<(), InternalError>(())
                            })
                            .await?
                            .commit()
                            .await?;
                    }
                    BuiltDataSource::SqliteTempDb(rdb, ..) => {
                        rdb.begin()
                            .await?
                            .pipe(|tx, _| async move {
                                for sql in init_sql {
                                    sqlx::query(sql).execute(&mut **tx).await.map_err(|e| {
                                        (
                                            format!(
                                                "Failed to execute init SQL for data source '{}'",
                                                data_source_ref
                                            ),
                                            e,
                                        )
                                            .into()
                                    })?;
                                }
                                Ok::<(), InternalError>(())
                            })
                            .await?
                            .commit()
                            .await?;
                    }
                    #[cfg(feature = "daemon")]
                    BuiltDataSource::PostgresDb(rdb) => {
                        rdb.begin()
                            .await?
                            .pipe(|tx, _| async move {
                                for sql in init_sql {
                                    sqlx::query(sql).execute(&mut **tx).await.map_err(|e| {
                                        (
                                            format!(
                                                "Failed to execute init SQL for data source '{}'",
                                                data_source_ref
                                            ),
                                            e,
                                        )
                                            .into()
                                    })?;
                                }
                                Ok::<(), InternalError>(())
                            })
                            .await?
                            .commit()
                            .await?;
                    }
                    _ => {
                        return Err((
                            format!(
                                "Data source reference '{}' is not a SQL database",
                                data_source_ref
                            ),
                            InternalErrorKind::InvalidConfig,
                        )
                            .into())
                    }
                }

                Ok(data_source)
            }
            WithDataSource::Ref {
                r#ref:
                    DataSourceRef::Network {
                        r#ref: data_source_ref,
                    },
            } => self
                .data_source_map
                .lock()
                .await
                .get(data_source_ref.as_str())
                .cloned()
                .ok_or_else(|| {
                    (
                        format!("Data source reference '{}' not found", data_source_ref),
                        InternalErrorKind::InvalidConfig,
                    )
                        .into()
                }),
        }
    }

    #[cfg(feature = "daemon")]
    async fn with_server(&self, with_server: &WithServer) -> InternalResult<Arc<BuiltServer>> {
        match with_server {
            WithServer::Def { def: server } => {
                let server: BuiltServer = BuiltServer::from_server_spec(server.clone());
                Ok(Arc::new(server))
            }
            WithServer::Ref { r#ref: server_ref } => self
                .server_map
                .lock()
                .await
                .get(server_ref.as_str())
                .cloned()
                .ok_or_else(|| {
                    (
                        format!("Server reference '{}' not found", server_ref),
                        InternalErrorKind::InvalidConfig,
                    )
                        .into()
                }),
        }
    }

    async fn with_client(&self, with_client: &WithClient) -> InternalResult<Arc<BuiltClient>> {
        match with_client {
            WithClient::Def { def: client } => {
                let client = BuiltClient::try_from_client_spec(client).await?;
                Ok(Arc::new(client))
            }
            WithClient::Ref { r#ref: client_ref } => self
                .client_map
                .lock()
                .await
                .get(client_ref.as_str())
                .cloned()
                .ok_or_else(|| {
                    (
                        format!("Client reference '{}' not found", client_ref),
                        InternalErrorKind::InvalidConfig,
                    )
                        .into()
                }),
        }
    }

    #[instrument(skip(self))]
    async fn send_command(&self, command: AgentCommand) -> InternalResult<()> {
        self.command_tx.send(command).await.map_err(|e| {
            (
                format!("Failed to send command: {:?}", e),
                InternalErrorKind::InternalCommunication,
            )
                .into()
        })
    }
}

/// Factory trait for creating runtime components from state specifications.
///
/// # Type Parameters
/// * `StateImpl` - State type used to configure the component
/// * `ContextImpl` - Resource context providing access to dependent resources
#[async_trait]
pub trait ResourceFactory<StateImpl, ContextImpl>
where
    StateImpl: State,
    ContextImpl: ResourceContext,
{
    /// The output type produced by the factory.
    type Output;

    /// Creates a new component instance from the given state.
    ///
    /// # Arguments
    /// * `state` - Configuration specification for the component
    /// * `context` - Resource context for resolving dependencies
    ///
    /// # Returns
    /// * `Ok(Self::Output)` on success
    /// * `Err(InternalError)` if creation fails
    async fn create(
        &self,
        state: &StateImpl,
        context: &Arc<ContextImpl>,
    ) -> InternalResult<Self::Output>;
}
