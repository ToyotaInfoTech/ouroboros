// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Builder utilities for constructing runtime components from specifications.
//!
//! Converts configuration specifications into built runtime instances
//! for data sources, clients, and servers.

use std::sync::Arc; // for row.get

use crate::error::InternalErrorKind;
use crate::runtime::client::Callable as _;
#[cfg(feature = "daemon")]
use crate::runtime::server;
#[cfg(feature = "daemon")]
use crate::runtime::types::BuiltServer;
use crate::runtime::types::{BuiltClient, BuiltDataSource};
use crate::runtime::{client, rdb};
use crate::InternalResult;
#[cfg(feature = "daemon")]
use ouro_state::core::ServerSpec;
use ouro_state::core::{ClientSpec, DataSourceSpec, NetworkEndpointSpec, SqlConnSpec};
use tracing::instrument;

impl BuiltDataSource {
    /// Asynchronously build a BuiltDataSource from SharedDataSource.
    ///
    /// # Returns
    /// * `Ok((String, Arc<BuiltDataSource>))` on success
    /// * `Err(anyhow::Error)` on failure
    #[instrument]
    pub async fn try_from_data_source_spec(
        state: &DataSourceSpec,
    ) -> InternalResult<BuiltDataSource> {
        match &state {
            DataSourceSpec::SqlDb {
                name: _,
                config: sql_conne_config,
                .. // TODO use retry
            } => {
                tracing::debug!("Building SqlDb data source");

                match &sql_conne_config {
                    SqlConnSpec::Sqlite { database_url, init_sql} => {
                        let pool = sqlx::sqlite::SqlitePoolOptions::new()
                            .max_connections(1)
                            .connect_lazy(&database_url.clone())
                            .map_err(|e| (format!("Failed to connect to Sqlite database: {}", &database_url).as_str(), e).into())?;
                        let rdb = rdb::Rdb::new(pool, init_sql.clone());

                        tracing::debug!("Built SqliteDb data source: {:?}", &sql_conne_config);

                        Ok(BuiltDataSource::SqliteDb(Arc::new(rdb)))
                    }
                    SqlConnSpec::SqliteTempFile { init_sql } => {
                        // Create an OS-managed temporary file. The file is deleted automatically
                        // when the returned `BuiltDataSource::SqliteTempDb` is dropped (i.e., on
                        // process exit or explicit teardown), because the `Arc<NamedTempFile>`
                        // inside the variant holds the cleanup handle.
                        let temp_file = tempfile::Builder::new()
                            .prefix("ouroedge_")
                            .suffix(".db")
                            .tempfile()
                            .map_err(|e| {
                                (
                                    "Failed to create temporary SQLite file",
                                    e,
                                ).into()
                            })?;
                        let url = format!("sqlite://{}", temp_file.path().display());
                        let pool = sqlx::sqlite::SqlitePoolOptions::new()
                            .max_connections(1)
                            .connect_lazy(&url)
                            .map_err(|e| {
                                (format!("Failed to connect to temp SQLite database: {}", &url).as_str(), e)
                                    .into()
                            })?;
                        let rdb = rdb::Rdb::new(pool, init_sql.clone());

                        tracing::debug!(
                            "Built SqliteTempDb data source at: {}",
                            temp_file.path().display()
                        );

                        Ok(BuiltDataSource::SqliteTempDb(
                            Arc::new(rdb),
                            Arc::new(temp_file),
                        ))
                    }
                    SqlConnSpec::Postgres {
                        database_url,
                        max_connections,
                        init_sql,
                    } => {
                        #[cfg(feature = "daemon")]
                        {
                            if database_url.contains("${") {
                                return Err((
                                    "Postgres database_url has an unexpanded placeholder; set DATABASE_URL",
                                    InternalErrorKind::InvalidConfig,
                                )
                                    .into());
                            }
                            // The URL embeds the password, so it must not appear in errors.
                            let pool = sqlx::postgres::PgPoolOptions::new()
                                .max_connections(*max_connections)
                                .connect_lazy(&database_url.clone())
                                .map_err(|e| ("Failed to connect to Postgres database", e).into())?;
                            let rdb = rdb::Rdb::new(pool, init_sql.clone());

                            Ok(BuiltDataSource::PostgresDb(Arc::new(rdb)))
                        }
                        #[cfg(not(feature = "daemon"))]
                        {
                            let _ = (database_url, max_connections, init_sql);
                            Err((
                                "Postgres data source requires the 'daemon' feature",
                                InternalErrorKind::NotImplemented,
                            ).into())
                        }
                    }
                }
            }
            DataSourceSpec::Network {
                name,
                config,
                .. // TODO use retry
             } => {
                tracing::info!("Building network data source: {}", name);
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Network config:\n{:#?}", config);
                }

                match config.clone() {
                    NetworkEndpointSpec::Zenoh { .. } => Ok(BuiltDataSource::ZenohSession(
                        Arc::new(client::CallableDataSource::new(name, state.clone()).await?),
                    )),
                    _ => {
                        return Err((
                            format!("Zenoh data source endpoint not yet supported: {}", name),
                            InternalErrorKind::NotImplemented,
                        )
                            .into());
                    }
                }
            }
        }
    }
}

#[cfg(feature = "daemon")]
impl BuiltServer {
    /// Builds a `BuiltServer` from a `ServerSpec`.
    ///
    /// # Arguments
    /// * `spec` - Server specification
    ///
    /// # Returns
    /// A wrapped `BuiltServer::MultiProtocolServer` instance
    pub fn from_server_spec(spec: ServerSpec) -> BuiltServer {
        let server = server::Server::new(spec);
        BuiltServer::MultiProtocolServer(Arc::new(server))
    }
}

impl BuiltClient {
    /// Asynchronously builds a `BuiltClient` from a `ClientSpec`.
    ///
    /// Creates either a `BasicClient` or `RetryableClient` based on
    /// whether a retry strategy is specified.
    ///
    /// # Arguments
    /// * `spec` - Client specification
    ///
    /// # Returns
    /// * `Ok(BuiltClient)` on success
    /// * `Err(InternalError)` if the endpoint type is unsupported
    #[instrument]
    pub async fn try_from_client_spec(spec: &ClientSpec) -> InternalResult<BuiltClient> {
        match spec.clone() {
            ClientSpec {
                name,
                endpoint: NetworkEndpointSpec::Zenoh { .. },
                retry,
            } => match retry {
                Some(_) => {
                    let client = client::RetryableClient::new(name.as_str(), spec.clone()).await?;
                    Ok(BuiltClient::Retryable(Arc::new(client)))
                }
                None => {
                    let client = client::BasicClient::new(name.as_str(), spec.clone()).await?;
                    Ok(BuiltClient::Basic(Arc::new(client)))
                }
            },
            ClientSpec {
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
            ClientSpec {
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
}
