// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use crate::runtime::ipc::Peer;
use async_trait::async_trait;
use ouro_core::error::{InternalError, InternalErrorKind};
use ouro_core::{comm::rpc, InternalResult};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::instrument;

#[allow(dead_code)] // API
pub struct UdsPeer<T: rpc::RpcDispatcher + 'static> {
    read_stream: Option<OwnedReadHalf>, // Use Option for later take()
    write_stream: Option<OwnedWriteHalf>, // Use Option for later take()
    handler: Arc<T>,
    write_back_response_task: Option<tokio::task::JoinHandle<()>>,
}

#[async_trait]
impl<T: rpc::RpcDispatcher> Peer<T> for UdsPeer<T> {
    #[instrument(skip(handler))]
    fn new(handler: T) -> InternalResult<Self> {
        // RPC_FD is required. Return PeerError if missing or not an integer.
        let fd = std::env::var("RPC_FD").map_err(|e| {
            (
                format!("RPC_FD env error: {e:?}").as_str(),
                InternalErrorKind::Protocol,
            )
                .into()
        })?;
        let fd: i32 = fd.parse().map_err(|e| {
            (
                format!("RPC_FD parse error: {e:?}").as_str(),
                InternalErrorKind::Protocol,
            )
                .into()
        })?;

        // Converts the received connected Unix stream FD to Tokio's UnixStream
        let std_stream =
            unsafe { <std::os::unix::net::UnixStream as std::os::fd::FromRawFd>::from_raw_fd(fd) };
        let stream = UnixStream::from_std(std_stream).map_err(|e| {
            (
                format!("Failed to create UnixStream from std: {e}").as_str(),
                e,
            )
                .into()
        })?; // Io -> PeerError::Io
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("RPC peer connected on FD {}", fd);
        }
        // stream from/to the parent proc
        let (read_stream, write_stream) = stream.into_split();

        Ok(Self {
            read_stream: Some(read_stream),
            write_stream: Some(write_stream),
            handler: Arc::new(handler),
            write_back_response_task: None,
        })
    }

    #[instrument(skip(self))]
    async fn run(&mut self) -> InternalResult<()> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("RPC peer started run loop");
        }

        // Channel for passing received requests to the main loop
        let (req_tx, mut req_rx) = mpsc::channel::<Box<[u8]>>(64);

        // stream from the parent proc
        let mut read_stream = self
            .read_stream
            .take()
            .ok_or_else(|| ("no read stream available", InternalErrorKind::Protocol).into())?;

        let mut _write_back_response =
            async move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                // Read length prefix
                let mut msg_length = [0u8; 4];
                read_stream.read_exact(&mut msg_length).await?;
                let message_length = u32::from_le_bytes(msg_length) as usize;
                tracing::debug!("RPC incoming message length: {} bytes", message_length);

                // Read payload
                let mut buffer = vec![0u8; message_length];

                read_stream.read_exact(&mut buffer).await?;

                let buf = buffer.into_boxed_slice();
                tracing::debug!("RPC received payload: {} bytes", buf.len());

                req_tx.send(buf).await?;
                Ok(())
            };

        self.write_back_response_task = Some(tokio::spawn(async move {
            // The loop receives responses from handler tasks and writes them to
            // the stream where the main thread is selecting available channels.
            loop {
                if let Err(e) = _write_back_response().await {
                    tracing::error!("Response write-back task error: {:?}", e);
                    break;
                }
            }
        }));

        let handler = self.handler.clone();
        let mut join_set: JoinSet<Result<Box<[u8]>, InternalError>> = JoinSet::new();

        // Avoid panic: convert missing stream to PeerError
        let mut write_stream = self
            .write_stream
            .take()
            .ok_or_else(|| ("no write stream available", InternalErrorKind::Protocol).into())?;

        let mut _handle_request_or_response =
            async move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                tracing::debug!("RPC peer waiting for requests or responses");
                select! {
                    // A handler task has completed
                    response = join_set.join_next(), if !join_set.is_empty() => {
                        let Some(response) = response else {
                            tracing::warn!("join_set returned None unexpectedly");
                            return Ok(());
                        };
                        match response {
                            Ok(Ok(response)) => {
                                tracing::debug!("RPC response ready, writing {} bytes", response.len());
                                write_stream.write_u32_le(response.len() as u32).await?;
                                write_stream.write_all(response.as_ref()).await?;
                                write_stream.flush().await?;
                                tracing::debug!("RPC response sent back to peer, {} bytes", response.len());
                            }
                            Ok(Err(e)) => {
                                tracing::error!("RPC handler error: {:?}", e);
                                // TODO send error response back to peer
                            }
                            Err(e) => {
                                tracing::warn!("RPC handler task aborted: {:?}", e);
                            }
                        }
                    }
                    // New request received
                    request = req_rx.recv() => {
                        match request {
                            Some(request) => {
                                tracing::debug!("RPC request received, spawning handler task: {} bytes", request.len());

                                let handler = handler.clone();
                                join_set.spawn(async move {
                                    handler
                                        .handle_request(request)
                                        .await
                                        .map_err(|e| {
                                            tracing::error!("RPC handler error: {:#}", e);
                                            e
                                        })
                                });
                                tracing::debug!("RPC request processing task spawned");
                            }
                            None => {
                                tracing::info!("RPC peer disconnected. Shutting down.");
                                std::process::exit(0);
                            }
                        }
                    }
                }
                Ok(())
            };

        // The main loop selects between incoming requests and the responses from completed handler tasks
        loop {
            if let Err(e) = _handle_request_or_response().await {
                tracing::error!("RPC peer send/receive error: {:?}", e);
                break;
            }
        }

        Ok(())
    }

    #[instrument(skip(self))]
    async fn stop(&mut self) {
        if let Some(write_stream) = &mut self.write_stream.take() {
            write_stream.shutdown().await.unwrap_or_else(|e| {
                tracing::error!("Error shutting down write stream: {}", e);
            });
        }
        if let Some(write_back_response_task) = self.write_back_response_task.take() {
            write_back_response_task.abort();
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!("Write back response task aborted");
            }
        }
        tracing::info!("UdsPeer stopped");
    }
}
