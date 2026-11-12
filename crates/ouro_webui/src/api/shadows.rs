// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Shadow management REST API handlers.
//!
//! Provides CRUD access to edge device shadows via ICC forwarding
//! to the ShadowManager task.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use ouro_schema::schema::cli::icc::{IccRequest, IccRequestWrapper, IccResponse};
use ouro_schema::schema::cli::OperateReq;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::AppState;
use crate::event::DashboardEvent;

/// The ICC task type for the shadow manager.
const SHADOW_TASK_TYPE: &str = "state";

/// Build shadow API routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/shadows", get(list_shadows))
        .route(
            "/shadows/{shadow_id}",
            get(get_shadow).delete(delete_shadow),
        )
        .route("/edges/{edge_id}/command", post(send_command_to_edge))
        .route("/edges/batch/command", post(batch_command))
}

/// Forward an ICC operate request by task type and return the JSON output.
///
/// Resolves the task type to the configured task name at runtime,
/// then sends the ICC request through the corresponding channel.
///
/// Args:
///     state: Shared application state
///     task_type: The task type (e.g. "state", "tag"), resolved to name via task_type_map
///     operation_name: The ICC operation to invoke
///     inputs: Key-value inputs for the operation
///
/// Returns:
///     Result<serde_value::Value, (StatusCode, String)>: Operation output or HTTP error
#[instrument(skip(state))]
pub(crate) async fn icc_operate(
    state: &AppState,
    task_type: &str,
    operation_name: &str,
    inputs: HashMap<String, String>,
) -> Result<serde_value::Value, (StatusCode, String)> {
    let (task_name, tx) = state.lookup_icc_by_type(task_type).ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Task type '{}' not available", task_type),
        )
    })?;

    let req = OperateReq {
        name: task_name,
        operation_name: operation_name.to_string(),
        inputs,
    };

    tracing::debug!("Sending ICC request: {:?}", req);

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    tx.send(IccRequestWrapper {
        request: IccRequest::Operate(req),
        response_tx,
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to send ICC request: {}", e),
        )
    })?;

    let resp = response_rx.await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Task did not respond".to_string(),
        )
    })?;

    match resp {
        IccResponse::Operate(op) => Ok(op.output),
        IccResponse::Error(e) => Err((StatusCode::BAD_REQUEST, e)),
        _ => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Unexpected ICC response".to_string(),
        )),
    }
}

/// GET /api/v1/shadows
///
/// List all edge shadows.
#[instrument(skip(state))]
async fn list_shadows(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match icc_operate(&state, SHADOW_TASK_TYPE, "get_all_shadows", HashMap::new()).await {
        Ok(output) => match serde_json::to_value(&output) {
            Ok(json) => (StatusCode::OK, Json(json)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Serialization error: {}", e),
            )
                .into_response(),
        },
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// GET /api/v1/shadows/:shadow_id
///
/// Get a specific edge shadow by ID.
#[instrument(skip(state))]
async fn get_shadow(
    State(state): State<Arc<AppState>>,
    Path(shadow_id): Path<String>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("shadow_id".to_string(), shadow_id);

    match icc_operate(&state, SHADOW_TASK_TYPE, "get_shadow", inputs).await {
        Ok(output) => match serde_json::to_value(&output) {
            Ok(json) => (StatusCode::OK, Json(json)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Serialization error: {}", e),
            )
                .into_response(),
        },
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// DELETE /api/v1/shadows/:shadow_id
///
/// Delete a specific edge shadow.
#[instrument(skip(state))]
async fn delete_shadow(
    State(state): State<Arc<AppState>>,
    Path(shadow_id): Path<String>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("shadow_id".to_string(), shadow_id.clone());

    match icc_operate(&state, SHADOW_TASK_TYPE, "delete_shadow", inputs).await {
        Ok(_) => {
            let _ = state
                .event_tx
                .send(DashboardEvent::ShadowDeleted { edge_id: shadow_id });
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// Allowed lifecycle command kinds for edge devices.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CommandKind {
    Start,
    Shutdown,
    Restart,
    Suspend,
}

impl std::fmt::Display for CommandKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start => write!(f, "start"),
            Self::Shutdown => write!(f, "shutdown"),
            Self::Restart => write!(f, "restart"),
            Self::Suspend => write!(f, "suspend"),
        }
    }
}

/// Request body for sending a command to an edge device.
#[derive(Debug, Deserialize)]
struct CommandBody {
    /// Command kind: one of "start", "shutdown", "restart", "suspend"
    kind: CommandKind,
}

/// POST /api/v1/edges/:edge_id/command
///
/// Send a lifecycle command (start/shutdown/restart/suspend) to an edge device.
#[instrument(skip(state))]
async fn send_command_to_edge(
    State(state): State<Arc<AppState>>,
    Path(edge_id): Path<String>,
    Json(body): Json<CommandBody>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("edge_id".to_string(), edge_id);
    inputs.insert("command_kind".to_string(), body.kind.to_string());

    match icc_operate(&state, SHADOW_TASK_TYPE, "send_command", inputs).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// Maximum number of edge IDs allowed in a single batch command.
const BATCH_COMMAND_MAX_EDGES: usize = 100;

/// Request body for batch command.
#[derive(Debug, Deserialize)]
struct BatchCommandBody {
    /// List of target edge device identifiers.
    edge_ids: Vec<String>,
    /// Command kind: one of "start", "shutdown", "restart", "suspend"
    kind: CommandKind,
}

/// Result of a batch command operation.
#[derive(Debug, Serialize)]
struct BatchCommandResult {
    succeeded: Vec<String>,
    failed: Vec<BatchCommandError>,
}

/// Single failure within a batch command.
#[derive(Debug, Serialize)]
struct BatchCommandError {
    edge_id: String,
    error: String,
}

/// POST /api/v1/edges/batch/command
///
/// Send a lifecycle command to multiple edge devices at once.
#[instrument(skip(state))]
async fn batch_command(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchCommandBody>,
) -> impl IntoResponse {
    if body.edge_ids.len() > BATCH_COMMAND_MAX_EDGES {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Too many edge IDs: {} (max {})",
                body.edge_ids.len(),
                BATCH_COMMAND_MAX_EDGES
            ),
        )
            .into_response();
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for edge_id in &body.edge_ids {
        let mut inputs = HashMap::new();
        inputs.insert("edge_id".to_string(), edge_id.clone());
        inputs.insert("command_kind".to_string(), body.kind.to_string());

        match icc_operate(&state, SHADOW_TASK_TYPE, "send_command", inputs).await {
            Ok(_) => succeeded.push(edge_id.clone()),
            Err((_status, msg)) => failed.push(BatchCommandError {
                edge_id: edge_id.clone(),
                error: msg,
            }),
        }
    }

    let result = BatchCommandResult { succeeded, failed };
    (StatusCode::OK, Json(result)).into_response()
}
