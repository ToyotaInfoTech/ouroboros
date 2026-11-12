// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! REST API and WebSocket handlers for the dashboard.
//!
//! Routes ICC operations through typed REST endpoints, converting between
//! JSON request/response bodies and the daemon's string-keyed ICC protocol.

pub mod grafana;
pub mod metadata;
pub mod shadows;
pub mod system;
pub mod tags;
pub mod ws;

use std::sync::Arc;

use axum::Router;
use ouro_schema::schema::cli::icc::IccRequestWrapper;
use tokio::sync::{broadcast, mpsc};

use crate::event::DashboardEvent;

/// Shared application state for all API handlers.
#[allow(clippy::type_complexity)]
pub struct AppState {
    /// Mapping from task type (e.g. "state", "tag") to task name (e.g. "state", "tag").
    task_lookup_by_type: Box<dyn Fn(&str) -> Option<String> + Send + Sync + 'static>,
    /// Function to look up an ICC channel sender by task name.
    icc_lookup_by_name:
        Box<dyn Fn(&str) -> Option<mpsc::Sender<IccRequestWrapper>> + Send + Sync + 'static>,
    /// Broadcast sender for dashboard events.
    pub event_tx: broadcast::Sender<DashboardEvent>,
    /// Optional database connection pool for direct queries.
    pub db_pool: Option<sqlx::PgPool>,
}

impl AppState {
    /// Create a new AppState.
    ///
    /// Args:
    ///     icc_lookup: Closure that resolves task name to ICC sender
    ///     task_type_map: Mapping from task type to task name
    ///     event_tx: Broadcast sender for dashboard events
    ///
    /// Returns:
    ///     Self
    pub fn new(
        task_lookup_by_type: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
        icc_lookup_by_name: impl Fn(&str) -> Option<mpsc::Sender<IccRequestWrapper>>
            + Send
            + Sync
            + 'static,
        event_tx: broadcast::Sender<DashboardEvent>,
        db_pool: Option<sqlx::PgPool>,
    ) -> Self {
        Self {
            task_lookup_by_type: Box::new(task_lookup_by_type),
            icc_lookup_by_name: Box::new(icc_lookup_by_name),
            event_tx,
            db_pool,
        }
    }

    pub fn lookup_task_name_by_type(&self, task_type: &str) -> Option<String> {
        (self.task_lookup_by_type)(task_type)
    }

    /// Look up an ICC channel by task name.
    pub fn lookup_icc(&self, name: &str) -> Option<mpsc::Sender<IccRequestWrapper>> {
        (self.icc_lookup_by_name)(name)
    }

    /// Look up an ICC channel by task type, resolving the name automatically.
    ///
    /// Args:
    ///     task_type: The task type string (e.g. "state", "tag")
    ///
    /// Returns:
    ///     `Option<(String, mpsc::Sender<IccRequestWrapper>)>`: Task name and sender if found
    pub fn lookup_icc_by_type(
        &self,
        task_type: &str,
    ) -> Option<(String, mpsc::Sender<IccRequestWrapper>)> {
        let name = self.lookup_task_name_by_type(task_type)?;
        let sender = self.lookup_icc(&name)?;
        Some((name.clone(), sender))
    }
}

/// Build all API routes.
///
/// Args:
///     state: Shared application state
///
/// Returns:
///     Router: API router with all endpoints
pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(shadows::routes())
        .merge(tags::routes())
        .merge(system::routes())
        .merge(ws::routes())
        .merge(grafana::routes())
        .merge(metadata::routes())
        .with_state(state)
}
