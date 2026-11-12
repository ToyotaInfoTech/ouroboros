// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use crate::gen::rpc_generated::fb::rpc as fbr;
use async_trait::async_trait;
use ouro_core::{comm::rpc, error::panic_if_env_set, InternalResult};

/// FlatBuffers-based RPC dispatcher for edge devices.
///
/// Routes incoming FlatBuffers-encoded [`Request`](fbr::Request) messages to
/// registered [`MethodHandler`](rpc::MethodHandler) instances by method name.
#[allow(dead_code)] // API
pub struct EdgeRpcHandler {
    method_handlers: std::collections::HashMap<String, Arc<dyn rpc::MethodHandler + Send + Sync>>,
}

#[allow(dead_code)] // API
#[allow(clippy::new_without_default)]
impl EdgeRpcHandler {
    pub fn new() -> Self {
        EdgeRpcHandler {
            method_handlers: std::collections::HashMap::new(),
        }
    }
}

#[async_trait]
impl rpc::RpcDispatcher for EdgeRpcHandler {
    async fn handle_request(&self, request: Box<[u8]>) -> InternalResult<Box<[u8]>> {
        let request = flatbuffers::root::<fbr::Request>(&request).map_err(|e| {
            Into::<ouro_core::error::InternalError>::into(("parse flatbuffers request", e))
        })?;

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "Handling RPC request: id={}, method='{}' (w/ params {} bytes)",
                request.id(),
                request.method().unwrap_or("<none>"),
                request.params().map_or(0, |p| p.len())
            );
        }

        let mut builder = flatbuffers::FlatBufferBuilder::new();
        let id = request.id();
        let mut result = None;
        let mut error = None;

        match request.method() {
            Some(method) => {
                if let Some(handler) = self.method_handlers.get(method) {
                    let params: Box<[u8]> = request
                        .params()
                        .map_or_else(Box::<[u8]>::default, |p| p.bytes().into());

                    match handler.handle(method, params).await {
                        Ok(result_bytes) => {
                            if tracing::enabled!(tracing::Level::DEBUG) {
                                tracing::debug!(
                                    "RPC method '{}' handled successfully, result_length={}",
                                    method,
                                    result_bytes.len()
                                );
                            }
                            result = Some(builder.create_vector(result_bytes.as_ref()));
                        }
                        Err(e) => {
                            tracing::error!("Error handling RPC method '{}': {:?}", method, e);
                            panic_if_env_set(&e);
                            error = Some(builder.create_string(&e.to_string()));
                        }
                    }
                } else {
                    tracing::warn!("No handler found for RPC method: '{}'", method);
                    error =
                        Some(builder.create_string(&format!(
                            "No handler found for RPC method: '{}'",
                            method
                        )));
                }
            }
            None => {
                tracing::warn!("RPC request has no method specified");
                error = Some(builder.create_string("RPC request has no method specified"));
            }
        }

        let response =
            fbr::Response::create(&mut builder, &fbr::ResponseArgs { id, result, error });
        builder.finish(response, None);

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "RPC response built successfully for request id={} with result_length={}",
                id,
                builder.finished_data().len()
            );
        }

        Ok(builder.finished_data().into())
    }

    fn set_method_handler(
        &mut self,
        method: &str,
        handler: &Arc<dyn rpc::MethodHandler + Send + Sync>,
    ) {
        self.method_handlers
            .insert(method.to_string(), handler.clone());
    }

    fn remove_method_handler(&mut self, method: &str) -> bool {
        self.method_handlers.remove(method).is_some()
    }
}
