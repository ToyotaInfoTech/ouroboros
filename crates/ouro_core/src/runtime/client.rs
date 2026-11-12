// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Client connection management for network endpoints.
//!
//! Provides connection factories for Zenoh sessions, database pools (Postgres/SQLite),
//! and retryable client wrappers with configurable backoff strategies.

use ouro_state::core::{self, NetworkEndpointSpec};
use tracing::instrument;

use std::{any::type_name_of_val, sync::Arc};

use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, sqlite::SqlitePoolOptions};
use std::future::Future;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use crate::{comm::messaging, error::InternalErrorKind, InternalResult};

/// A resolved network endpoint, ready to communicate.
///
/// Wraps the underlying connection pool or messaging session, depending
/// on the data source or client specification that produced it.
#[derive(Debug, Clone)]
pub enum Endpoint {
    Messaging(Arc<dyn messaging::Session>),
    Sqlite(sqlx::SqlitePool),
    Postgres(sqlx::PgPool),
}

impl Endpoint {
    #[instrument(skip(self))]
    #[allow(clippy::result_large_err)]
    pub fn into_messaging(self) -> InternalResult<Arc<dyn messaging::Session>> {
        match self {
            Endpoint::Messaging(session) => Ok(session),
            _ => Err((
                "Endpoint is not a messaging session",
                InternalErrorKind::TypeConversion,
            )
                .into()),
        }
    }

    #[instrument(skip(self))]
    #[allow(clippy::result_large_err)]
    pub fn into_postgres(self) -> InternalResult<sqlx::PgPool> {
        match self {
            Endpoint::Postgres(pool) => Ok(pool),
            _ => Err((
                "Endpoint is not a Postgres pool",
                InternalErrorKind::TypeConversion,
            )
                .into()),
        }
    }

    #[instrument(skip(self))]
    #[allow(clippy::result_large_err)]
    pub fn into_sqlite(self) -> InternalResult<sqlx::SqlitePool> {
        match self {
            Endpoint::Sqlite(pool) => Ok(pool),
            _ => Err((
                "Endpoint is not a Sqlite pool",
                InternalErrorKind::TypeConversion,
            )
                .into()),
        }
    }
}

impl Endpoint {
    /// Build a Transport from a ClientSpec asynchronously.
    ///
    /// # Arguments
    /// * `spec` - The client specification
    ///
    /// # Returns
    /// * `Ok(Transport)` - The constructed transport
    /// * `Err(InternalError)` - If transport construction fails
    #[instrument(skip(spec))]
    pub async fn from_client_spec(spec: core::ClientSpec) -> InternalResult<Self> {
        match spec {
            core::ClientSpec {
                endpoint: NetworkEndpointSpec::Zenoh { endpoints, .. },
                retry,
                ..
            } => Ok(Endpoint::Messaging(
                messaging::Transport::Zenoh(endpoints).open(retry).await?,
            )),
            core::ClientSpec {
                name,
                endpoint: NetworkEndpointSpec::Http { .. },
                ..
            } => {
                return Err((
                    format!("HTTP client endpoint not yet supported: {}", name),
                    InternalErrorKind::NotImplemented,
                )
                    .into());
            }
            core::ClientSpec {
                name,
                endpoint: NetworkEndpointSpec::Mqtt { .. },
                ..
            } => {
                return Err((
                    format!("MQTT client endpoint not yet supported: {}", name),
                    InternalErrorKind::NotImplemented,
                )
                    .into());
            }
        }
    }

    /// Build a Transport from a DataSourceSpec asynchronously.
    ///
    /// # Arguments
    /// * `spec` - The data source specification
    ///
    /// # Returns
    /// * `Ok(Transport)` - The constructed transport
    /// * `Err(InternalError)` - If transport construction fails
    #[instrument(skip(spec))]
    pub async fn from_data_source_spec(spec: core::DataSourceSpec) -> InternalResult<Self> {
        match spec {
            core::DataSourceSpec::SqlDb { config, .. } => {
                Endpoint::from_sql_conn_spec(config).await
            }
            core::DataSourceSpec::Network { config, .. } => {
                Endpoint::from_network_endpoint(config).await
            }
        }
    }

    /// Build a Transport from a NetworkEndpointSpec asynchronously.
    #[instrument(skip(endpoint))]
    async fn from_network_endpoint(endpoint: core::NetworkEndpointSpec) -> InternalResult<Self> {
        match endpoint {
            NetworkEndpointSpec::Zenoh { endpoints, .. } => Ok(Endpoint::Messaging(
                messaging::Transport::Zenoh(endpoints).open(None).await?,
            )),
            NetworkEndpointSpec::Http { .. } => Err((
                "HTTP endpoint not yet supported",
                InternalErrorKind::NotImplemented,
            )
                .into()),
            NetworkEndpointSpec::Mqtt { .. } => Err((
                "MQTT endpoint not yet supported",
                InternalErrorKind::NotImplemented,
            )
                .into()),
        }
    }

    #[instrument(skip(conn_spec))]
    async fn from_sql_conn_spec(conn_spec: core::SqlConnSpec) -> InternalResult<Self> {
        match conn_spec {
            core::SqlConnSpec::Postgres {
                database_url,
                max_connections,
                init_sql,
            } => {
                let pool = PgPoolOptions::new()
                    .max_connections(max_connections)
                    .connect_lazy(&database_url)
                    // The URL embeds the password, so it must not appear in errors.
                    .map_err(|e| ("Failed to connect to PostgresDB", e).into())?;

                if !init_sql.is_empty() {
                    tracing::warn!(
                        "Executing init SQL for endpoint is not supported: {:?}",
                        init_sql
                    );
                }

                Ok(Endpoint::Postgres(pool))
            }
            core::SqlConnSpec::Sqlite {
                database_url,
                init_sql,
            } => {
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect_lazy(&database_url)
                    .map_err(|e| {
                        (
                            format!("Failed to connect to SqliteDB: {}", &database_url).as_str(),
                            e,
                        )
                            .into()
                    })?;

                if !init_sql.is_empty() {
                    tracing::warn!(
                        "Executing init SQL for endpoint is not supported: {:?}",
                        init_sql
                    );
                }

                Ok(Endpoint::Sqlite(pool))
            }
            core::SqlConnSpec::SqliteTempFile { .. } => Err((
                "SqliteTempFile cannot be used as a network endpoint",
                crate::error::InternalErrorKind::NotImplemented,
            )
                .into()),
        }
    }
}

#[async_trait]
pub trait Callable
where
    Self: Sized + Sync,
{
    type State: core::types::CallableState;
    type Transport: Clone + Send + Sync;

    async fn new(name: &str, state: Self::State) -> InternalResult<Self>;

    fn name(&self) -> &str;

    #[allow(clippy::result_large_err)]
    fn transport(&self) -> InternalResult<Self::Transport>;

    async fn call<UserTask, Fut, UserValue>(
        &self,
        user_task: UserTask,
    ) -> InternalResult<UserValue>
    where
        UserTask: FnOnce(Self::Transport) -> Fut + Clone + Send, // Closure that returns a Future
        Fut: Future<Output = InternalResult<UserValue>> + Send,
        UserValue: Send;
}

#[async_trait]
trait RetryableCallable: Callable
where
    Self: Sized + Sync,
{
    /// Call a function with retry and timeout support
    ///
    /// Executes the provided async function with timeout and exponential backoff retry logic
    /// based on the client's retry configuration.
    ///
    /// # Arguments
    /// * `func` - Async function to execute
    ///
    /// # Returns
    /// * `Ok(R)` - Result from successful function execution
    /// * `Err(_)` - If all retry attempts fail or timeout is reached
    ///
    /// # Type Parameters
    /// * `F` - Async function type that returns a Future
    /// * `Fut` - Future type returned by the function
    /// * `R` - Result type of the function

    #[instrument(skip(self, retry, user_task))]
    async fn call_with_retry<UserTask, Fut, UserValue>(
        &self,
        retry: &core::RetryStrategySpec,
        user_task: UserTask,
    ) -> InternalResult<UserValue>
    where
        UserTask: FnOnce(Self::Transport) -> Fut + Clone + Send, // Closure that returns a Future
        Fut: Future<Output = InternalResult<UserValue>> + Send,
        UserValue: Send,
    {
        let name = self.name().to_string();
        let mut backoff_ms = retry.initial_backoff_ms;
        let max_backoff_ms = retry.max_backoff_ms.unwrap_or(u64::MAX);

        for attempt in 1..=retry.max_attempts {
            let user_task = user_task.clone();
            let transport = self.transport()?;
            // Apply timeout if configured
            let result = match retry.retry_timeout_ms {
                Some(timeout_ms) if timeout_ms > 0 => {
                    match timeout(Duration::from_millis(timeout_ms), user_task(transport)).await {
                        Ok(result) => result,
                        Err(_) => {
                            tracing::warn!(
                                "Callable '{}' call timed out after {} ms (attempt {}/{})",
                                name,
                                timeout_ms,
                                attempt,
                                retry.max_attempts
                            );
                            Err((
                                format!(
                                    "Callable '{}' call timed out after {} ms (attempt {}/{})",
                                    name, timeout_ms, attempt, retry.max_attempts
                                ),
                                InternalErrorKind::Timeout,
                            )
                                .into())
                        }
                    }
                }
                _ => user_task(transport).await,
            };

            match result {
                Ok(value) => return Ok(value),
                Err(e) => {
                    if attempt >= retry.max_attempts {
                        tracing::error!(
                            "Callable '{}' call failed after {} attempts: {:?}",
                            name,
                            retry.max_attempts,
                            e
                        );
                        return Err((
                            format!(
                                "Callable '{}' call failed after {} attempts",
                                name, retry.max_attempts
                            ),
                            InternalErrorKind::RetryExhausted,
                        )
                            .into());
                    }

                    tracing::warn!(
                        "Callable '{}' call failed (attempt {}/{}): {}, retrying in {} ms...",
                        name,
                        attempt,
                        retry.max_attempts,
                        e,
                        backoff_ms
                    );

                    sleep(Duration::from_millis(backoff_ms)).await;

                    // Calculate next backoff with exponential increase
                    backoff_ms =
                        ((backoff_ms as f64 * retry.backoff_multiplier) as u64).min(max_backoff_ms);
                }
            }
        }

        Err((
            format!(
                "Callable '{}' exceeded maximum retry attempts: {}",
                name, retry.max_attempts,
            ),
            InternalErrorKind::Runtime,
        )
            .into())
    }
}

#[derive(Debug, Clone)]
pub struct RetryableClient {
    pub name: String,
    pub state: core::ClientSpec,
    transport: Option<Endpoint>,
}

#[async_trait]
impl Callable for RetryableClient {
    type State = core::ClientSpec;
    type Transport = Endpoint;

    #[instrument(skip(name, state))]
    async fn new(name: &str, state: Self::State) -> InternalResult<Self> {
        let transport = Endpoint::from_client_spec(state.clone()).await?;
        let transport = Some(transport);
        Ok(Self {
            name: name.into(),
            state,
            transport,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn transport(&self) -> InternalResult<Self::Transport> {
        self.transport.clone().ok_or_else(|| {
            (
                format!(
                    "No transport available for {} '{}'",
                    type_name_of_val(self),
                    self.name
                ),
                InternalErrorKind::Runtime,
            )
                .into()
        })
    }

    /// Call a function with retry and timeout support
    ///
    /// Executes the provided async function with timeout and exponential backoff retry logic
    /// based on the client's retry configuration.
    ///
    /// # Arguments
    /// * `func` - Async function to execute
    ///
    /// # Returns
    /// * `Ok(R)` - Result from successful function execution
    /// * `Err(_)` - If all retry attempts fail or timeout is reached
    ///
    /// # Type Parameters
    /// * `F` - Async function type that returns a Future
    /// * `Fut` - Future type returned by the function
    /// * `R` - Result type of the function
    async fn call<UserTask, Fut, UserValue>(&self, user_task: UserTask) -> InternalResult<UserValue>
    where
        UserTask: FnOnce(Endpoint) -> Fut + Clone + Send, // Closure that returns a Future
        Fut: Future<Output = InternalResult<UserValue>> + Send,
        UserValue: Send,
    {
        self.call_with_retry(self.state.retry.as_ref().unwrap(), user_task)
            .await
    }
}

#[async_trait]
impl RetryableCallable for RetryableClient {}

#[derive(Debug, Clone)]
pub struct CallableDataSource {
    pub name: String,
    pub state: core::DataSourceSpec,
    pub transport: Option<Endpoint>,
}

#[async_trait]
impl Callable for CallableDataSource {
    type State = core::DataSourceSpec;
    type Transport = Endpoint;

    #[instrument(skip(name, state))]
    async fn new(name: &str, state: Self::State) -> InternalResult<Self> {
        let transport = Endpoint::from_data_source_spec(state.clone()).await?;
        let transport = Some(transport);
        //let transport: Transport = Transport::from_data_source_spec(state.clone()).await?;
        Ok(Self {
            name: name.into(),
            state,
            transport,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn transport(&self) -> InternalResult<Self::Transport> {
        self.transport
            .clone()
            .ok_or_else(|| ("No transport available", InternalErrorKind::Runtime).into())
    }

    #[instrument(skip(self, user_task))]
    async fn call<UserTask, Fut, UserValue>(&self, user_task: UserTask) -> InternalResult<UserValue>
    where
        UserTask: FnOnce(Self::Transport) -> Fut + Clone + Send, // Closure that returns a Future
        Fut: Future<Output = InternalResult<UserValue>> + Send,
        UserValue: Send,
    {
        let retry = match &self.state {
            core::DataSourceSpec::SqlDb { retry, .. } => retry,
            core::DataSourceSpec::Network { retry, .. } => retry,
        };

        self.call_with_retry(retry, user_task).await
    }
}

#[async_trait]
impl RetryableCallable for CallableDataSource {}

#[derive(Debug, Clone)]
pub struct BasicClient {
    pub name: String,
    pub state: core::ClientSpec,
    transport: Option<Endpoint>,
}

#[async_trait]
impl Callable for BasicClient {
    type State = core::ClientSpec;
    type Transport = Endpoint;

    #[instrument(skip(name, state))]
    async fn new(name: &str, state: Self::State) -> InternalResult<Self> {
        let transport = Endpoint::from_client_spec(state.clone()).await?;
        let transport = Some(transport);
        Ok(Self {
            name: name.into(),
            state,
            transport,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn transport(&self) -> InternalResult<Self::Transport> {
        self.transport
            .clone()
            .ok_or_else(|| ("No transport available", InternalErrorKind::Runtime).into())
    }

    /// Call a function with retry and timeout support
    ///
    /// Executes the provided async function with timeout and exponential backoff retry logic
    /// based on the client's retry configuration.
    ///
    /// # Arguments
    /// * `func` - Async function to execute
    ///
    /// # Returns
    /// * `Ok(R)` - Result from successful function execution
    /// * `Err(_)` - If all retry attempts fail or timeout is reached
    ///
    /// # Type Parameters
    /// * `F` - Async function type that returns a Future
    /// * `Fut` - Future type returned by the function
    /// * `R` - Result type of the function
    #[instrument(skip(self, user_task))]
    async fn call<UserTask, Fut, UserValue>(&self, user_task: UserTask) -> InternalResult<UserValue>
    where
        UserTask: FnOnce(Endpoint) -> Fut + Clone + Send, // Closure that returns a Future
        Fut: Future<Output = InternalResult<UserValue>> + Send,
        UserValue: Send,
    {
        let transport = self.transport()?;
        let user_task = user_task.clone();
        user_task(transport).await
    }
}
