// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Metrics collection and HTTP server for Grafana visualization.
//!
//! This module provides:
//! - A trait-based extensible metrics provider interface
//! - An HTTP server compatible with Grafana's JSON datasource
//! - Real-time metrics collection and serving

use async_trait::async_trait;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::server::ServerEndpoint as _;
use ouro_core::runtime::types::{BuiltMetricsProvider, BuiltServer, BuiltServerEndpoint};
use ouro_core::runtime::{
    comp::{CompContext, ComponentImpl},
    metrics::Metric,
};
use ouro_core::InternalResult;
use ouro_core::{error::InternalErrorKind, runtime::metrics::provider::MetricsProvider};
use ouro_daemon::runtime::context::DaemonResourceContextImpl;
use ouro_state::daemon::{DaemonTaskSpec, MetricsStore};
use std::sync::Arc;
use tokio::task::{JoinHandle, JoinSet};
use tracing::instrument;

/// Health check endpoint.
///
/// # Returns
/// * `StatusCode` - HTTP 200 OK
async fn health_check() -> impl IntoResponse {
    StatusCode::OK
}

/// Get all metrics endpoint.
///
/// # Arguments
/// * `state` - Server state containing the metrics provider
///
/// # Returns
/// * `Json<Vec<Metric>>` - All available metrics
#[instrument(skip(state))]
async fn get_metrics<T>(State(state): State<Arc<ServerState<T>>>) -> impl IntoResponse
where
    T: MetricsProvider + Send + Sync + 'static,
{
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!("GET /metrics request received");
    }

    let metrics = state.provider.get_all_metrics(1000, None).await;

    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!("Returning {} metrics", metrics.len());
    }

    Json(metrics)
}

/// Get specific metric by name.
///
/// # Arguments
/// * `name` - Metric identifier from URL path
/// * `state` - Server state containing the metrics provider
///
/// # Returns
/// * `Result` - Json with metric data or 404 Not Found
#[instrument(skip(state))]
async fn get_metric_by_name<T>(
    axum::extract::Path(name): axum::extract::Path<String>,
    State(state): State<Arc<ServerState<T>>>,
) -> impl IntoResponse
where
    T: MetricsProvider + Send + Sync + 'static,
{
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!("GET /metrics/{} request received", name);
    }

    match state.provider.get_metric(&name, 1000).await {
        Some(points) => {
            let metric = Metric { name, points };
            Ok(Json(metric))
        }
        None => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!("Metric not found: {}", name);
            }
            Err(StatusCode::NOT_FOUND)
        }
    }
}

/// HTTP server state shared across request handlers
struct ServerState<T>
where
    T: MetricsProvider + Send + Sync,
{
    provider: Arc<T>,
}

/// Task for serving metrics over HTTP.
#[derive(Clone)]
pub struct JsonHttpMetricsServerTask {
    metrics_provider: Arc<BuiltMetricsProvider>,
}

impl JsonHttpMetricsServerTask {
    /// Creates a new metrics server task.
    ///
    /// # Arguments
    ///
    /// * `metrics_provider` - Provider that supplies metric data
    ///
    /// # Returns
    ///
    /// * `Self` - New metrics server task
    #[instrument(skip(metrics_provider))]
    pub fn new(metrics_provider: Arc<BuiltMetricsProvider>) -> Arc<Self> {
        Arc::new(Self { metrics_provider })
    }
}

#[async_trait]
impl ComponentImpl for JsonHttpMetricsServerTask {
    type StateImpl = DaemonTaskSpec;
    type ContextImpl = DaemonResourceContextImpl;

    #[instrument(skip(self, state, context))]
    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>> {
        let mut join_set: JoinSet<InternalResult<()>> = JoinSet::new();

        match state {
            DaemonTaskSpec::Metrics {
                enabled,
                store,
                with_server,
                ..
            } => {
                if !*enabled {
                    tracing::info!("JsonHttpMetricsServerTask is disabled; not starting server");
                    return Ok(None);
                }

                let _max_points_per_metric = match *store {
                    MetricsStore::InMemory {
                        max_points_per_metric,
                    } => max_points_per_metric.unwrap_or(1000usize),
                };

                let provider = self.metrics_provider.clone();
                let server = context.with_server(with_server).await?;

                match server.as_ref() {
                    BuiltServer::MultiProtocolServer(server) => {
                        let state = Arc::new(ServerState { provider });

                        let router = Router::new()
                            .route("/", get(health_check))
                            .route("/metrics", get(get_metrics))
                            .route("/metrics/{name}", get(get_metric_by_name))
                            .with_state(state);

                        for endpoint in server.endpoints.iter() {
                            match endpoint {
                                BuiltServerEndpoint::HttpServerEndpoint(http_endpoint) => {
                                    let router = router.clone();
                                    let http_endpoint = http_endpoint.clone();
                                    join_set.spawn(async move {
                                        http_endpoint.listen(&router).await?;
                                        Ok(())
                                    });
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                return Err((
                    "JsonHttpMetricsServerTask can only be started with DaemonTask::Metrics",
                    InternalErrorKind::Runtime,
                )
                    .into());
            }
        };

        let handle = tokio::spawn(async move {
            join_set.join_all().await;
        });

        Ok(Some(handle))
    }
}
