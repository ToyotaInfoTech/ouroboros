// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Dashboard event types for WebSocket broadcasting.

use serde::Serialize;

/// Events broadcast to connected WebSocket clients.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum DashboardEvent {
    /// An edge's reported state changed.
    EdgeStateChanged { edge_id: String, status: String },
    /// A shadow was created or updated.
    ShadowUpdated { edge_id: String },
    /// A shadow was deleted.
    ShadowDeleted { edge_id: String },
    /// A tag was created, updated, or deleted.
    TagChanged { id: String, action: String },
    /// A tag assignment changed.
    TagAssignmentChanged {
        edge_id: String,
        id: String,
        action: String,
    },
}
