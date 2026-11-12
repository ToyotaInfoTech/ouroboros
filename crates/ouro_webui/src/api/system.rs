// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! System information REST API handlers.

use std::sync::Arc;

use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use tracing::instrument;

use super::AppState;

/// Build system API routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health_check))
}

/// GET /api/v1/health
///
/// Health check endpoint.
#[instrument]
async fn health_check() -> impl IntoResponse {
    StatusCode::OK
}
