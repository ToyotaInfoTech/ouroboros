// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Tag management REST API handlers.
//!
//! Provides CRUD access to configuration tags and tag-edge assignments
//! via ICC forwarding to the TagManager task.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tracing::instrument;

use super::AppState;
use crate::api::shadows::icc_operate;
use crate::event::DashboardEvent;

/// The ICC task type for the tag manager.
const TAG_TASK_TYPE: &str = "tag";

/// Build tag API routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tags", get(list_tags).post(create_tag))
        .route(
            "/tags/{tag_id}",
            get(get_tag).put(update_tag).delete(delete_tag),
        )
        .route("/tags/assign", post(assign_tag).delete(unassign_tag))
        .route("/edges/tags", get(list_all_tag_assignments))
        .route("/edges/{edge_id}/tags", get(get_tags_for_edge))
        .route("/edges/{edge_id}/preview", get(preview_merged_config))
}

/// Request body for tag creation/update.
#[derive(Debug, Deserialize)]
pub struct TagBody {
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub priority: i64,
    /// EdgeSpecPatch as a JSON object.
    pub patch: serde_json::Value,
}

/// Request body for tag assignment.
#[derive(Debug, Deserialize)]
pub struct TagAssignBody {
    pub edge_id: String,
    pub id: String,
}

/// GET /api/v1/tags
///
/// List all configuration tags.
#[instrument(skip(state))]
async fn list_tags(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match icc_operate(&state, TAG_TASK_TYPE, "list_tags", HashMap::new()).await {
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

/// GET /api/v1/tags/:tag_id
///
/// Get a specific tag.
#[instrument(skip(state))]
async fn get_tag(
    State(state): State<Arc<AppState>>,
    Path(tag_id): Path<String>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("tag_id".to_string(), tag_id);

    match icc_operate(&state, TAG_TASK_TYPE, "get_tag", inputs).await {
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

/// POST /api/v1/tags
///
/// Create a new configuration tag.
#[instrument(skip(state))]
async fn create_tag(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TagBody>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    if let Some(ref display_name) = body.display_name {
        inputs.insert("display_name".to_string(), display_name.clone());
    }
    inputs.insert("priority".to_string(), body.priority.to_string());
    inputs.insert("patch".to_string(), body.patch.to_string());

    match icc_operate(&state, TAG_TASK_TYPE, "create_tag", inputs).await {
        Ok(_) => {
            // The id is generated server-side; send an empty id since the frontend
            // only uses this event to invalidate the tags query cache.
            let _ = state.event_tx.send(DashboardEvent::TagChanged {
                id: String::new(),
                action: "created".to_string(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// PUT /api/v1/tags/:tag_id
///
/// Update an existing configuration tag.
#[instrument(skip(state))]
async fn update_tag(
    State(state): State<Arc<AppState>>,
    Path(tag_id): Path<String>,
    Json(body): Json<TagBody>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("tag_id".to_string(), tag_id.clone());
    if let Some(ref display_name) = body.display_name {
        inputs.insert("display_name".to_string(), display_name.clone());
    }
    inputs.insert("priority".to_string(), body.priority.to_string());
    inputs.insert("patch".to_string(), body.patch.to_string());

    match icc_operate(&state, TAG_TASK_TYPE, "update_tag", inputs).await {
        Ok(_) => {
            let _ = state.event_tx.send(DashboardEvent::TagChanged {
                id: tag_id,
                action: "updated".to_string(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// DELETE /api/v1/tags/:tag_id
///
/// Delete a configuration tag.
#[instrument(skip(state))]
async fn delete_tag(
    State(state): State<Arc<AppState>>,
    Path(tag_id): Path<String>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("tag_id".to_string(), tag_id.clone());

    match icc_operate(&state, TAG_TASK_TYPE, "delete_tag", inputs).await {
        Ok(_) => {
            let _ = state.event_tx.send(DashboardEvent::TagChanged {
                id: tag_id,
                action: "deleted".to_string(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// POST /api/v1/tags/assign
///
/// Assign a tag to an edge device.
#[instrument(skip(state))]
async fn assign_tag(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TagAssignBody>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("edge_id".to_string(), body.edge_id.clone());
    inputs.insert("tag_id".to_string(), body.id.clone());

    match icc_operate(&state, TAG_TASK_TYPE, "assign_tag", inputs).await {
        Ok(_) => {
            let _ = state.event_tx.send(DashboardEvent::TagAssignmentChanged {
                edge_id: body.edge_id,
                id: body.id,
                action: "assigned".to_string(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// DELETE /api/v1/tags/assign
///
/// Unassign a tag from an edge device.
#[instrument(skip(state))]
async fn unassign_tag(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TagAssignBody>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("edge_id".to_string(), body.edge_id.clone());
    inputs.insert("tag_id".to_string(), body.id.clone());

    match icc_operate(&state, TAG_TASK_TYPE, "unassign_tag", inputs).await {
        Ok(_) => {
            let _ = state.event_tx.send(DashboardEvent::TagAssignmentChanged {
                edge_id: body.edge_id,
                id: body.id,
                action: "unassigned".to_string(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// GET /api/v1/edges/:edge_id/tags
///
/// Get all tags assigned to an edge device.
#[instrument(skip(state))]
async fn get_tags_for_edge(
    State(state): State<Arc<AppState>>,
    Path(edge_id): Path<String>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("edge_id".to_string(), edge_id);

    match icc_operate(&state, TAG_TASK_TYPE, "get_tags_for_edge", inputs).await {
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

/// GET /api/v1/edges/:edge_id/preview
///
/// Preview the merged configuration for an edge after applying all tag patches.
#[instrument(skip(state))]
async fn preview_merged_config(
    State(state): State<Arc<AppState>>,
    Path(edge_id): Path<String>,
) -> impl IntoResponse {
    let mut inputs = HashMap::new();
    inputs.insert("edge_id".to_string(), edge_id);

    match icc_operate(&state, TAG_TASK_TYPE, "preview_merged_config", inputs).await {
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

/// GET /api/v1/edges/tags
///
/// List all edge-tag assignments grouped by edge ID.
#[instrument(skip(state))]
async fn list_all_tag_assignments(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match icc_operate(
        &state,
        TAG_TASK_TYPE,
        "list_all_assignments",
        HashMap::new(),
    )
    .await
    {
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
