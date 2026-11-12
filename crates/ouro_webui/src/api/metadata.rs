// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Metadata REST API handlers.
//!
//! Provides read-only access to telemetry metadata records and camera images
//! stored in TimescaleDB via direct SQL queries.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::AppState;

/// Build metadata API routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/metadata", get(list_metadata))
        .route("/metadata/{vin}/{start_time}", get(get_metadata_detail))
        .route(
            "/metadata/{vin}/{start_time}/images/{camera_id}/{ts}",
            get(get_camera_image),
        )
}

// ── Types ──────────────────────────────────────────────────────────────

/// Summary item returned in the metadata list.
#[derive(Debug, Serialize)]
struct MetadataListItem {
    vin: String,
    start_time: String,
    end_time: String,
    model: String,
    trajectory_length: usize,
    image_set_count: i64,
    bbox: Option<BoundingBox>,
    first_image_url: Option<String>,
    /// Full trajectory reusing the same LocationFix type as the detail endpoint.
    trajectory: Vec<LocationFix>,
    /// Behavior activation spans observed during this metadata window.
    active_behaviors: Vec<ActiveBehaviorItem>,
}

/// A behavior activation span with RFC 3339 timestamps.
#[derive(Debug, Serialize)]
struct ActiveBehaviorItem {
    name: String,
    start_time: String,
    end_time: String,
}

/// Geographic bounding box for a trajectory.
#[derive(Debug, Serialize)]
struct BoundingBox {
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
}

/// Full metadata detail including trajectory and camera frame refs.
#[derive(Debug, Serialize)]
struct MetadataDetail {
    vin: String,
    start_time: String,
    end_time: String,
    model: String,
    trajectory: Vec<LocationFix>,
    camera_frames: Vec<CameraFrameRef>,
    /// Behavior activation spans observed during this metadata window.
    active_behaviors: Vec<ActiveBehaviorItem>,
}

/// Single location fix in a trajectory.
#[derive(Debug, Clone, Serialize)]
struct LocationFix {
    timestamp: String,
    latitude: f64,
    longitude: f64,
}

/// Reference to a camera frame (without image data).
#[derive(Debug, Serialize)]
struct CameraFrameRef {
    camera_id: String,
    ts: String,
    image_url: String,
}

/// Query parameters for metadata list endpoint.
#[derive(Debug, Deserialize)]
struct ListParams {
    limit: Option<i64>,
    offset: Option<i64>,
    /// Wildcard pattern for VIN (* → %, ? → _).
    vin_pattern: Option<String>,
    /// Wildcard pattern for vehicle model (* → %, ? → _).
    model: Option<String>,
    /// Start of time range (RFC 3339).
    time_start: Option<String>,
    /// End of time range (RFC 3339).
    time_end: Option<String>,
    /// Bounding box for first trajectory point.
    min_lat: Option<f64>,
    max_lat: Option<f64>,
    min_lng: Option<f64>,
    max_lng: Option<f64>,
}

// ── DB row types ───────────────────────────────────────────────────────

/// Row from telemetry table.
#[derive(Debug, sqlx::FromRow)]
struct TelemetryRow {
    vin: String,
    start_time: time::OffsetDateTime,
    end_time: time::OffsetDateTime,
    model: String,
    metadata: Option<serde_json::Value>,
}

/// Row from camera_image count query.
#[derive(Debug, sqlx::FromRow)]
struct ImageCountRow {
    vin: String,
    start_time: time::OffsetDateTime,
    image_set_count: Option<i64>,
}

/// Row for camera frame reference.
#[derive(Debug, sqlx::FromRow)]
struct CameraFrameRow {
    camera_id: String,
    ts: time::OffsetDateTime,
}

// ── Handlers ───────────────────────────────────────────────────────────

/// List telemetry metadata records with summary info.
///
/// Supports filtering by VIN pattern, model, time range, and geo bounds.
///
/// Returns:
///     JSON array of MetadataListItem
async fn list_metadata(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let limit = params.limit.unwrap_or(20).min(1000);
    let offset = params.offset.unwrap_or(0);

    // Build dynamic WHERE clauses
    let mut conditions: Vec<String> = Vec::new();
    let mut param_idx = 2u32; // $1 = limit, $2 = offset

    if let Some(ref pattern) = params.vin_pattern {
        // Escape literal SQL wildcards first, then convert user wildcards
        let like_pattern = pattern
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('*', "%")
            .replace('?', "_");
        param_idx += 1;
        conditions.push(format!("vin LIKE ${} ESCAPE '\\'", param_idx));
        let _ = like_pattern; // used below
    }
    if let Some(ref model) = params.model {
        // Escape literal SQL wildcards first, then convert user wildcards
        let like_model = model
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('*', "%")
            .replace('?', "_");
        param_idx += 1;
        conditions.push(format!("model LIKE ${} ESCAPE '\\'", param_idx));
        let _ = like_model; // used below
    }
    if params.time_start.is_some() {
        param_idx += 1;
        conditions.push(format!("start_time >= ${}", param_idx));
    }
    if params.time_end.is_some() {
        param_idx += 1;
        conditions.push(format!("start_time < ${}", param_idx));
    }
    if params.min_lat.is_some()
        && params.max_lat.is_some()
        && params.min_lng.is_some()
        && params.max_lng.is_some()
    {
        param_idx += 1;
        let lat_col = "(metadata->'trajectory'->'location_fixes'->0->>'latitude')::float8";
        let lng_col = "(metadata->'trajectory'->'location_fixes'->0->>'longitude')::float8";
        conditions.push(format!(
            "{lat_col} >= ${} AND {lat_col} <= ${} AND {lng_col} >= ${} AND {lng_col} <= ${}",
            param_idx,
            param_idx + 1,
            param_idx + 2,
            param_idx + 3
        ));
        param_idx += 3;
    }
    let _ = param_idx;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT vin, start_time, end_time, model, metadata \
         FROM telemetry {} \
         ORDER BY start_time DESC \
         LIMIT $1 OFFSET $2",
        where_clause
    );

    // Build and bind query dynamically
    let mut query = sqlx::query_as::<_, TelemetryRow>(&sql)
        .bind(limit)
        .bind(offset);

    if let Some(ref pattern) = params.vin_pattern {
        let like_pattern = pattern
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('*', "%")
            .replace('?', "_");
        query = query.bind(like_pattern);
    }
    if let Some(ref model) = params.model {
        let like_model = model
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('*', "%")
            .replace('?', "_");
        query = query.bind(like_model);
    }
    if let Some(ref ts) = params.time_start {
        let parsed =
            time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339)
                .map_err(|_| StatusCode::BAD_REQUEST)?;
        query = query.bind(parsed);
    }
    if let Some(ref ts) = params.time_end {
        let parsed =
            time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339)
                .map_err(|_| StatusCode::BAD_REQUEST)?;
        query = query.bind(parsed);
    }
    if let (Some(min_lat), Some(max_lat), Some(min_lng), Some(max_lng)) = (
        params.min_lat,
        params.max_lat,
        params.min_lng,
        params.max_lng,
    ) {
        query = query
            .bind(min_lat)
            .bind(max_lat)
            .bind(min_lng)
            .bind(max_lng);
    }

    let rows: Vec<TelemetryRow> = query.fetch_all(pool).await.map_err(|e| {
        tracing::error!("Failed to query telemetry: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Short-circuit: no telemetry rows matched the filter.
    if rows.is_empty() {
        return Ok(Json(Vec::new()));
    }

    // Collect (vin, start_time) pairs from the current page (≤ limit rows) so
    // the camera_image query only scans chunks relevant to this page rather
    // than performing a full table scan.
    let page_vins: Vec<String> = rows.iter().map(|r| r.vin.clone()).collect();
    let page_start_times: Vec<time::OffsetDateTime> = rows.iter().map(|r| r.start_time).collect();

    // Fetch camera image set counts only for the telemetry rows on this page.
    let image_counts: Vec<ImageCountRow> = sqlx::query_as(
        r#"
        SELECT c.vin, c.start_time, COUNT(DISTINCT c.ts) AS image_set_count
        FROM camera_image c
        JOIN (
            SELECT UNNEST($1::text[]) AS vin,
                   UNNEST($2::timestamptz[]) AS start_time
        ) AS f ON c.vin = f.vin AND c.start_time = f.start_time
        GROUP BY c.vin, c.start_time
        "#,
    )
    .bind(&page_vins)
    .bind(&page_start_times)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query camera_image counts: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Build a lookup for image counts
    let count_map: std::collections::HashMap<(String, String), i64> = image_counts
        .into_iter()
        .map(|r| {
            let key = (r.vin, format_ts(r.start_time));
            (key, r.image_set_count.unwrap_or(0))
        })
        .collect();

    let items: Vec<MetadataListItem> = rows
        .into_iter()
        .map(|row| {
            let trajectory = extract_trajectory(&row.metadata);
            let bbox = compute_bbox(&trajectory);
            let st = format_ts(row.start_time);
            let image_set_count = count_map
                .get(&(row.vin.clone(), st.clone()))
                .copied()
                .unwrap_or(0);
            let first_image_url = if image_set_count > 0 {
                Some(format!(
                    "/api/v1/metadata/{}/{}/images/front/first",
                    row.vin, st
                ))
            } else {
                None
            };
            let traj_len = trajectory.len();
            let active_behaviors = extract_active_behaviors(&row.metadata);
            MetadataListItem {
                vin: row.vin,
                start_time: st,
                end_time: format_ts(row.end_time),
                model: row.model,
                trajectory_length: traj_len,
                image_set_count,
                bbox,
                first_image_url,
                trajectory,
                active_behaviors,
            }
        })
        .collect();

    Ok(Json(items))
}

/// Get detailed metadata for a specific telemetry record.
///
/// Args:
///     vin: Vehicle identification number
///     start_time: Start time (RFC 3339)
///
/// Returns:
///     JSON MetadataDetail with full trajectory and camera frame references
async fn get_metadata_detail(
    State(state): State<Arc<AppState>>,
    Path((vin, start_time)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let start_time_parsed: time::OffsetDateTime =
        time::OffsetDateTime::parse(&start_time, &time::format_description::well_known::Rfc3339)
            .map_err(|_| StatusCode::BAD_REQUEST)?;

    let row: TelemetryRow = sqlx::query_as(
        r#"
        SELECT vin, start_time, end_time, model, metadata
        FROM telemetry
        WHERE vin = $1 AND start_time = $2
        "#,
    )
    .bind(&vin)
    .bind(start_time_parsed)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query telemetry detail: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let trajectory = extract_trajectory(&row.metadata);
    let active_behaviors = extract_active_behaviors(&row.metadata);

    // Fetch camera frame timestamps
    let frames: Vec<CameraFrameRow> = sqlx::query_as(
        r#"
        SELECT camera_id, ts
        FROM camera_image
        WHERE vin = $1 AND start_time = $2
        ORDER BY ts, camera_id
        "#,
    )
    .bind(&vin)
    .bind(start_time_parsed)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query camera frames: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let camera_frames: Vec<CameraFrameRef> = frames
        .into_iter()
        .map(|f| {
            let ts_str = format_ts(f.ts);
            CameraFrameRef {
                image_url: format!(
                    "/api/v1/metadata/{}/{}/images/{}/{}",
                    vin, start_time, f.camera_id, ts_str
                ),
                camera_id: f.camera_id,
                ts: ts_str,
            }
        })
        .collect();

    Ok(Json(MetadataDetail {
        vin: row.vin,
        start_time: format_ts(row.start_time),
        end_time: format_ts(row.end_time),
        model: row.model,
        trajectory,
        camera_frames,
        active_behaviors,
    }))
}

/// Serve a single camera image as JPEG.
///
/// Args:
///     vin: Vehicle identification number
///     start_time: Start time (RFC 3339)
///     camera_id: Camera direction (front, rear, front_left, etc.)
///     ts: Frame timestamp (RFC 3339) or "first" for earliest frame
///
/// Returns:
///     JPEG image bytes
async fn get_camera_image(
    State(state): State<Arc<AppState>>,
    Path((vin, start_time, camera_id, ts)): Path<(String, String, String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let start_time_parsed: time::OffsetDateTime =
        time::OffsetDateTime::parse(&start_time, &time::format_description::well_known::Rfc3339)
            .map_err(|_| StatusCode::BAD_REQUEST)?;

    let image_data: Option<Vec<u8>> = if ts == "first" {
        // Get the earliest image for this camera
        sqlx::query_scalar(
            r#"
            SELECT image
            FROM camera_image
            WHERE vin = $1 AND start_time = $2 AND camera_id = $3
            ORDER BY ts ASC
            LIMIT 1
            "#,
        )
        .bind(&vin)
        .bind(start_time_parsed)
        .bind(&camera_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch first camera image: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
    } else {
        let ts_parsed: time::OffsetDateTime =
            time::OffsetDateTime::parse(&ts, &time::format_description::well_known::Rfc3339)
                .map_err(|_| StatusCode::BAD_REQUEST)?;

        sqlx::query_scalar(
            r#"
            SELECT image
            FROM camera_image
            WHERE vin = $1 AND start_time = $2 AND camera_id = $3 AND ts = $4
            "#,
        )
        .bind(&vin)
        .bind(start_time_parsed)
        .bind(&camera_id)
        .bind(ts_parsed)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch camera image: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
    };

    let data = image_data.ok_or(StatusCode::NOT_FOUND)?;

    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "image/jpeg")], data))
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Extract trajectory location fixes from the JSONB metadata.
///
/// The JSONB `timestamp` field is stored as an array:
/// `[year, ordinal_day, hour, minute, second, 0, 0, 0, 0]`
/// which corresponds to `time` crate's serialization format.
fn extract_trajectory(metadata: &Option<serde_json::Value>) -> Vec<LocationFix> {
    let fixes = metadata
        .as_ref()
        .and_then(|m| m.get("trajectory"))
        .and_then(|t| t.get("location_fixes"))
        .and_then(|lf| lf.as_array());

    let Some(fixes) = fixes else {
        return Vec::new();
    };

    fixes
        .iter()
        .filter_map(|fix| {
            let lat = fix.get("latitude")?.as_f64()?;
            let lng = fix.get("longitude")?.as_f64()?;
            let ts = parse_time_array(fix.get("timestamp")?)?;
            Some(LocationFix {
                timestamp: ts,
                latitude: lat,
                longitude: lng,
            })
        })
        .collect()
}

/// Extract behavior activation spans from the JSONB metadata.
fn extract_active_behaviors(metadata: &Option<serde_json::Value>) -> Vec<ActiveBehaviorItem> {
    let items = metadata
        .as_ref()
        .and_then(|m| m.get("active_behaviors"))
        .and_then(|v| v.as_array());

    let Some(items) = items else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?.to_string();
            let start_time = parse_time_array(item.get("start_time")?)?;
            let end_time = parse_time_array(item.get("end_time")?)?;
            Some(ActiveBehaviorItem {
                name,
                start_time,
                end_time,
            })
        })
        .collect()
}

/// Parse `time` crate's serialized array
/// `[year, ordinal_day, h, m, s, nanosecond, offset_h, offset_m, offset_s]`
/// into an RFC 3339 string.
///
/// Elements beyond index 4 are optional for backward compatibility:
/// - `[5]` nanosecond (defaults to 0)
/// - `[6..8]` UTC offset hours/minutes/seconds (defaults to UTC)
fn parse_time_array(val: &serde_json::Value) -> Option<String> {
    // If it's already a string, return as-is
    if let Some(s) = val.as_str() {
        return Some(s.to_string());
    }
    let arr = val.as_array()?;
    if arr.len() < 5 {
        return None;
    }
    let year = arr[0].as_i64()? as i32;
    let ordinal = arr[1].as_i64()? as u16;
    let hour = arr[2].as_i64()? as u8;
    let minute = arr[3].as_i64()? as u8;
    let second = arr[4].as_i64()? as u8;
    let nanosecond = arr.get(5).and_then(|v| v.as_i64()).unwrap_or(0) as u32;
    let off_h = arr.get(6).and_then(|v| v.as_i64()).unwrap_or(0) as i8;
    let off_m = arr.get(7).and_then(|v| v.as_i64()).unwrap_or(0) as i8;
    let off_s = arr.get(8).and_then(|v| v.as_i64()).unwrap_or(0) as i8;

    let date = time::Date::from_ordinal_date(year, ordinal).ok()?;
    let t = time::Time::from_hms_nano(hour, minute, second, nanosecond).ok()?;
    let offset = time::UtcOffset::from_hms(off_h, off_m, off_s).ok()?;
    let dt = time::OffsetDateTime::new_in_offset(date, t, offset);
    Some(format_ts(dt))
}

/// Compute the bounding box of a trajectory with 20% padding.
fn compute_bbox(trajectory: &[LocationFix]) -> Option<BoundingBox> {
    if trajectory.is_empty() {
        return None;
    }

    let mut min_lat = f64::MAX;
    let mut max_lat = f64::MIN;
    let mut min_lng = f64::MAX;
    let mut max_lng = f64::MIN;

    for fix in trajectory {
        min_lat = min_lat.min(fix.latitude);
        max_lat = max_lat.max(fix.latitude);
        min_lng = min_lng.min(fix.longitude);
        max_lng = max_lng.max(fix.longitude);
    }

    // Add 20% padding
    let lat_padding = (max_lat - min_lat) * 0.2;
    let lng_padding = (max_lng - min_lng) * 0.2;

    // Ensure minimum padding for single-point trajectories
    let lat_padding = if lat_padding < 0.001 {
        0.001
    } else {
        lat_padding
    };
    let lng_padding = if lng_padding < 0.001 {
        0.001
    } else {
        lng_padding
    };

    Some(BoundingBox {
        min_lat: min_lat - lat_padding,
        max_lat: max_lat + lat_padding,
        min_lng: min_lng - lng_padding,
        max_lng: max_lng + lng_padding,
    })
}

/// Format a time::OffsetDateTime as RFC 3339 string.
fn format_ts(ts: time::OffsetDateTime) -> String {
    ts.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| ts.to_string())
}
