// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! WebUI dashboard server for the Ouroboros daemon.
//!
//! Provides an interactive web dashboard with REST API endpoints and
//! WebSocket-based real-time event streaming. All operations are routed
//! through ICC (Inter-Component Communication) channels to existing
//! daemon tasks.

pub mod api;
pub mod auth;
pub mod event;
pub mod static_files;

use std::sync::Arc;

use axum::{middleware, Router};
use tokio::sync::broadcast;

use crate::api::AppState;
use crate::event::DashboardEvent;

/// Build the complete axum Router for the WebUI dashboard.
///
/// The SPA is served from the same origin, so no CORS headers are emitted.
///
/// Args:
///     state: Shared application state containing ICC channel access
///     auth_token: Bearer token required for `/api/v1`; `None` or empty disables it
///
/// Returns:
///     Router: Configured axum router with all API, WebSocket, and static routes
pub fn build_router(state: Arc<AppState>, auth_token: Option<String>) -> Router {
    let token = auth_token.filter(|t| !t.is_empty()).map(Arc::<str>::from);

    let api_router =
        api::routes(state.clone()).layer(middleware::from_fn_with_state(token, auth::guard));

    static_files::dump_embedded_files();

    Router::new()
        .nest("/api/v1", api_router)
        .fallback(static_files::static_handler)
}

/// Configuration for the WebUI server.
#[derive(Debug, Clone)]
pub struct WebUiConfig {
    /// Bind addresses for the HTTP server.
    pub bind_addrs: Vec<String>,
}

/// Create a new broadcast channel for dashboard events.
///
/// Returns:
///     `(broadcast::Sender<DashboardEvent>, broadcast::Receiver<DashboardEvent>)`
pub fn event_channel() -> (
    broadcast::Sender<DashboardEvent>,
    broadcast::Receiver<DashboardEvent>,
) {
    broadcast::channel(256)
}
