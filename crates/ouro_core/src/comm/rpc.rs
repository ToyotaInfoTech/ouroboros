// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! RPC error handling and handler traits.
//!
//! Provides error types and traits for implementing RPC method handlers
//! and request processing.

use async_trait::async_trait;
use std::sync::Arc;

use crate::InternalResult;

/// Trait for dispatching RPC requests.
///
/// `RpcDispatcher` owns/maintains a registry of [`MethodHandler`]s and routes
/// incoming RPC requests to the appropriate handler based on the requested
/// method name.
///
/// Implementations are responsible for:
/// - Decoding the binary RPC request in [`RpcDispatcher::handle_request`]
/// - Dispatching to the registered [`MethodHandler`]
/// - Encoding the binary RPC response (or returning [`crate::InternalError`])
#[async_trait]
pub trait RpcDispatcher: Send + Sync {
    /// Handle a complete RPC request.
    ///
    /// # Arguments
    /// * `request` - Binary-encoded RPC request
    ///
    /// # Returns
    /// * `Result<Box<[u8]>, InternalError>` - Binary-encoded response or error
    async fn handle_request(&self, request: Box<[u8]>) -> InternalResult<Box<[u8]>>;

    /// Register a method handler.
    ///
    /// # Arguments
    /// * `method` - Method name to register
    /// * `handler` - Handler implementation
    fn set_method_handler(&mut self, method: &str, handler: &Arc<dyn MethodHandler + Send + Sync>);

    /// Unregister a method handler.
    ///
    /// # Arguments
    /// * `method` - Method name to remove
    fn remove_method_handler(&mut self, method: &str) -> bool;
}

/// Trait for handling individual RPC methods.
///
/// Implementations process method calls with binary parameters and return
/// binary results. Owned bytes are returned to avoid lifetime issues.
#[async_trait]
pub trait MethodHandler: Send + Sync {
    /// Handle an RPC method call.
    ///
    /// # Arguments
    /// * `method` - The method name to invoke
    /// * `params` - Binary-encoded parameters
    ///
    /// # Returns
    /// * `Result<Box<[u8]>, InternalError>` - Binary-encoded result or error
    async fn handle(&self, method: &str, params: Box<[u8]>) -> InternalResult<Box<[u8]>>;
}

pub mod client {
    use crate::{error::InternalErrorKind, InternalResult};
    use futures::{SinkExt, StreamExt};
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio_util::codec::{Framed, LengthDelimitedCodec};

    pub struct Client<S>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        framed: Framed<S, LengthDelimitedCodec>,
    }

    impl<S> Client<S>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        pub async fn new(stream: S) -> Self {
            let framed = Framed::new(stream, LengthDelimitedCodec::new());
            Client { framed }
        }

        pub async fn call<Q, P>(&mut self, req: Q) -> InternalResult<P>
        where
            Q: serde::Serialize,
            P: serde::de::DeserializeOwned,
        {
            let mut buf = Vec::new();
            ciborium::into_writer(&req, &mut buf)
                .map_err(|e| ("Failed to serialize request", e).into())?;

            self.framed
                .send(buf.into())
                .await
                .map_err(|e| ("Failed to send request", e).into())?;

            let result = self.framed.next().await;

            let bytes = match result {
                Some(Ok(bytes)) => bytes,
                Some(Err(e)) => {
                    return Err(("Failed to read response", e).into());
                }
                None => {
                    return Err(("Connection closed by server", InternalErrorKind::Network).into());
                }
            };

            ciborium::from_reader(bytes.as_ref())
                .map_err(|e| ("Failed to deserialize response", e).into())
        }
    }
}

pub mod framed {
    use std::sync::Arc;

    use tokio::sync::Mutex;
    use tokio_util::codec::{Framed, LengthDelimitedCodec};

    pub type StreamFramed<S> = Arc<Mutex<Framed<S, LengthDelimitedCodec>>>;
}
