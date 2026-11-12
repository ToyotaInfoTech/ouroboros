// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! WebUI dashboard server task.
//!
//! Launches an axum HTTP server that serves the dashboard SPA and
//! exposes REST/WebSocket endpoints routed through ICC channels.

use async_trait::async_trait;
use ouro_core::runtime::comp::{CompContext, ComponentImpl};
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::server::ServerEndpoint as _;
use ouro_core::runtime::types::{BuiltServer, BuiltServerEndpoint};
use ouro_core::{error::InternalErrorKind, InternalResult};
use ouro_daemon::runtime::context::DaemonResourceContextImpl;
use ouro_schema::schema::cli::icc::{IccRequest, IccRequestWrapper, IccResponse};
use ouro_schema::Void;
use ouro_state::daemon::DaemonTaskSpec;
use ouro_webui::api::AppState;
use ouro_webui::event::DashboardEvent;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tracing::instrument;

/// Dashboard WebUI server task.
#[derive(Clone)]
pub struct WebUiTask {
    event_tx: broadcast::Sender<DashboardEvent>,
}

impl WebUiTask {
    /// Creates a new WebUI task with a shared event sender.
    ///
    /// # Arguments
    /// * `event_tx` - Shared broadcast sender for dashboard events
    ///
    /// # Returns
    ///     Arc<Self>: New task instance
    pub fn new(event_tx: broadcast::Sender<DashboardEvent>) -> Arc<Self> {
        Arc::new(Self { event_tx })
    }
}

#[async_trait]
impl ComponentImpl for WebUiTask {
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
            DaemonTaskSpec::WebUi {
                enabled,
                with_server,
                with_data_source,
                auth_token,
                ..
            } => {
                if !*enabled {
                    tracing::info!("WebUiTask is disabled; not starting server");
                    return Ok(None);
                }

                if auth_token.as_deref().is_none_or(str::is_empty) {
                    tracing::warn!(
                        "WebUI authentication is disabled; expose it only on trusted networks"
                    );
                }

                let server = context.with_server(with_server).await?;

                // Resolve optional database pool for metadata queries
                let db_pool = if let Some(ref ds) = with_data_source {
                    let data_source = context.with_data_source(ds).await?;
                    let rdb = data_source.as_postgres()?;
                    Some(rdb.pool().clone())
                } else {
                    None
                };

                let task_name_lookup_by_type = {
                    let context = context.clone();
                    let task_name_lookup_by_type = move |task_type: &str| -> Option<String> {
                        let task_type = task_type.to_string();
                        let context = context.clone();
                        // Use a blocking approach since the lookup is fast (just a HashMap get)
                        let rt = tokio::runtime::Handle::current();
                        std::thread::spawn(move || {
                            rt.block_on(async {
                                let task_name = context
                                    .current_state()
                                    .lock()
                                    .await
                                    .runtime
                                    .daemon_id
                                    .clone();
                                let tx = context.lookup_icc_channel(&task_name).await.ok()?;
                                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                                tx.send(IccRequestWrapper {
                                    request: IccRequest::List(Void),
                                    response_tx,
                                })
                                .await
                                .ok()?;
                                match response_rx.await.ok()? {
                                    IccResponse::List(resp) => {
                                        resp.tasks.into_iter().find_map(|task_info| {
                                            if task_info.r#type == task_type {
                                                Some(task_info.name)
                                            } else {
                                                None
                                            }
                                        })
                                    }
                                    _ => None,
                                }
                            })
                        })
                        .join()
                        .ok()?
                    };
                    task_name_lookup_by_type
                };

                let icc_lookup_by_name = {
                    // Build ICC lookup closure that captures the context
                    let context = context.clone();
                    let icc_lookup_by_name =
                        move |name: &str| -> Option<mpsc::Sender<IccRequestWrapper>> {
                            let ctx = context.clone();
                            let name = name.to_string();
                            // Use a blocking approach since the lookup is fast (just a HashMap get)
                            let rt = tokio::runtime::Handle::current();
                            std::thread::spawn(move || {
                                rt.block_on(async { ctx.lookup_icc_channel(&name).await.ok() })
                            })
                            .join()
                            .ok()
                            .flatten()
                        };
                    icc_lookup_by_name
                };

                let (event_tx, _event_rx) = ouro_webui::event_channel();
                let _ = (_event_rx, event_tx);
                let event_tx = self.event_tx.clone();
                let app_state = Arc::new(AppState::new(
                    task_name_lookup_by_type,
                    icc_lookup_by_name,
                    event_tx,
                    db_pool,
                ));
                let router = ouro_webui::build_router(app_state, auth_token.clone());

                match server.as_ref() {
                    BuiltServer::MultiProtocolServer(server) => {
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
                    "WebUiTask can only be started with DaemonTask::WebUi",
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
