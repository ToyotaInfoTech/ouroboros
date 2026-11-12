// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! WebSocket endpoint for real-time dashboard event streaming.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use tokio::sync::broadcast;
use tracing::instrument;

use super::AppState;
use crate::event::DashboardEvent;

/// Build WebSocket routes.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/ws", get(ws_handler))
}

/// GET /api/v1/ws
///
/// WebSocket upgrade endpoint for real-time event streaming.
#[instrument(skip(state))]
async fn ws_handler(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> impl IntoResponse {
    let rx = state.event_tx.subscribe();
    // Browsers require the server to select one of the offered subprotocols.
    ws.protocols(["ouro.v1"])
        .on_upgrade(move |socket| handle_socket(socket, rx))
}

/// Handle a connected WebSocket, forwarding broadcast events as JSON messages.
async fn handle_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<DashboardEvent>) {
    loop {
        tokio::select! {
            // Forward dashboard events to the client
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        match serde_json::to_string(&event) {
                            Ok(json) => {
                                if socket.send(Message::Text(json.into())).await.is_err() {
                                    // Client disconnected
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to serialize event: {}", e);
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("WebSocket client lagged, skipped {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            // Handle incoming messages from client (ping/pong, close)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {} // Ignore other messages
                    Some(Err(_)) => break,
                }
            }
        }
    }
}
