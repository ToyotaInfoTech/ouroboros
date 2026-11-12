// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Server infrastructure for handling incoming connections.
//!
//! Provides multi-protocol server abstractions with endpoint management
//! and HTTP server implementation via axum.

use std::sync::Arc;

use crate::{
    runtime::{server, types::BuiltServerEndpoint},
    InternalResult,
};
use async_trait::async_trait;
use ouro_state::core::{self, ServerSpec};
use tracing::instrument;

/// Multi-protocol server instance for handling incoming connections.
///
/// Holds the server name, configuration, and endpoints.
#[derive(Debug)]
pub struct Server {
    /// Server name
    pub name: String,
    pub endpoints: Vec<BuiltServerEndpoint>,
}

impl Server {
    /// Create a new server instance.
    ///
    /// # Arguments
    /// * `name` - Server name
    /// * `config` - Shared server configuration
    /// * `endpoints` - List of built server endpoints
    ///
    /// # Returns
    /// * `Self` - New server instance
    pub(crate) fn new(state: ServerSpec) -> Self {
        let endpoints: Vec<BuiltServerEndpoint> = state
            .endpoint
            .iter()
            .map(|endpoint| match endpoint {
                core::ServerEndpointSpec::Http { bind_addrs } => {
                    BuiltServerEndpoint::HttpServerEndpoint(Arc::new(
                        server::HttpServerEndpoint::new(bind_addrs.clone()),
                    ))
                }
                _ => unimplemented!(),
            })
            .collect();
        Self {
            name: state.name,
            endpoints,
        }
    }
}

impl HttpServerEndpoint {
    /// Create a new HTTP server endpoint with the given bind addresses.
    pub fn new(bind_addrs: Vec<String>) -> Self {
        Self { bind_addrs }
    }
}

/// Trait for server endpoint implementations.
///
/// Provides methods for constructing and starting endpoint listeners.
#[allow(dead_code)]
#[async_trait]
pub trait ServerEndpoint {
    type Router;
    // Create a new endpoint instance with the given bind addresses.
    //fn new(bind_addrs: Vec<String>) -> Self;

    /// Bind to the configured address and start listening for incoming connections.
    ///
    /// # Arguments
    /// * `router` - Router instance to handle requests
    ///
    /// # Returns
    /// A future that resolves when the server starts listening.
    async fn listen(&self, router: &Self::Router) -> InternalResult<()>;
}

/// HTTP server endpoint implementation using axum.
#[derive(Debug, Clone)]
pub struct HttpServerEndpoint {
    /// List of bind addresses
    #[allow(dead_code)]
    bind_addrs: Vec<String>,
}

#[async_trait]
impl ServerEndpoint for HttpServerEndpoint {
    type Router = axum::Router;
    /// Bind to the configured addresses and start the HTTP server using axum.
    ///
    /// # Arguments
    /// * `router` - axum router instance
    ///
    /// # Returns
    /// * `InternalResult<()>` - Ok on success, error otherwise
    #[instrument(skip(self, router))]
    async fn listen(&self, router: &Self::Router) -> InternalResult<()> {
        let bind_addrs: Vec<std::net::SocketAddr> = self
            .bind_addrs
            .iter()
            .map(|addr| addr.as_str().parse())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ("Failed to parse bind address", e).into())?;

        let router = router.clone();
        let bind_addrs = bind_addrs.clone();
        let listener = tokio::net::TcpListener::bind(&bind_addrs[..])
            .await
            .map_err(|e| {
                (
                    format!("Failed to bind HTTP server to addresses: {:?}", bind_addrs),
                    e,
                )
                    .into()
            })?;
        for bind_addr in bind_addrs.iter() {
            tracing::info!("HTTP server listening on http://{}/", bind_addr);
        }
        axum::serve(listener, router.clone())
            .await
            .map_err(|e| ("Failed to start HTTP server", e).into())?;
        Ok(())
    }
}
