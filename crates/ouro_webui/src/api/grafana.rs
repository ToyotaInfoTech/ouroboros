// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Grafana availability check endpoint.
//!
//! Provides a lightweight health-check that probes whether a Grafana
//! instance is reachable on `localhost:3000` via a TCP connection test.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::AppState;

/// Default Grafana address to probe.
const GRAFANA_ADDR: &str = "127.0.0.1:3000";

/// Timeout for the TCP connection probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Response body for the Grafana health-check endpoint.
#[derive(Serialize)]
pub struct GrafanaHealthResponse {
    /// Whether Grafana is reachable.
    pub available: bool,
}

/// Check whether Grafana is reachable by attempting a TCP connection.
///
/// Returns:
///     Json<GrafanaHealthResponse>: `{ "available": true }` if reachable
async fn grafana_health(State(_state): State<Arc<AppState>>) -> Json<GrafanaHealthResponse> {
    let available =
        tokio::time::timeout(PROBE_TIMEOUT, tokio::net::TcpStream::connect(GRAFANA_ADDR))
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false);

    Json(GrafanaHealthResponse { available })
}

/// Build routes for the Grafana sub-module.
///
/// Returns:
///     `Router<Arc<AppState>>`: Router with `/grafana/health` endpoint
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/grafana/health", get(grafana_health))
}
