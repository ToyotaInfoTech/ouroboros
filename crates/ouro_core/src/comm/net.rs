// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use crate::InternalResult;
use ouro_common::constant::{CLI_TCP_SOCKET_ADDR_DEFAULT, CLI_UNIX_DOMAIN_SOCKET_PATH_DEFAULT};
use ouro_state::daemon::CliEndpoint;
use std::io::ErrorKind;
use tokio::{
    fs::remove_file,
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, UnixListener},
};

/// Trait alias for a stream that supports async read/write and can be sent across threads.
pub trait TransportStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T> TransportStream for T where T: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

/// A transport-agnostic listener for CLI endpoint connections.
///
/// Wraps either a Unix domain socket or TCP listener, depending on
/// the daemon's [`CliEndpoint`] configuration.
#[derive(Debug)]
pub enum Listener {
    Unix(UnixListener),
    Tcp(TcpListener),
}

impl Listener {
    pub async fn try_from(cli_endpoint: Option<CliEndpoint>) -> InternalResult<Option<Self>> {
        match cli_endpoint {
            Some(CliEndpoint::UnixDomainSocket { path }) => {
                let path = path.unwrap_or_else(|| CLI_UNIX_DOMAIN_SOCKET_PATH_DEFAULT.to_string());
                let path = path.as_str();
                if let Err(e) = remove_file(path).await {
                    if e.kind() != ErrorKind::NotFound {
                        return Err(
                            (format!("Failed to remove Unix Domain Socket '{}'", path), e).into(),
                        );
                    }
                }
                let listener = UnixListener::bind(path).map_err(|e| {
                    (
                        format!("Failed to bind Unix domain socket at '{}'", path),
                        e,
                    )
                        .into()
                })?;
                Ok(Some(Listener::Unix(listener)))
            }
            Some(CliEndpoint::TcpSocket { addr }) => {
                let addr = addr.unwrap_or_else(|| CLI_TCP_SOCKET_ADDR_DEFAULT.to_string());
                let listener = TcpListener::bind(&addr)
                    .await
                    .map_err(|e| (format!("Failed to bind TCP socket at '{}'", addr), e).into())?;
                Ok(Some(Listener::Tcp(listener)))
            }
            None => Ok(None),
        }
    }

    pub async fn accept(&self) -> InternalResult<Box<dyn TransportStream>> {
        match self {
            Listener::Unix(listener) => {
                let (stream, _) = listener
                    .accept()
                    .await
                    .map_err(|e| ("Failed to accept connection on Unix domain socket", e).into())?;
                Ok(Box::new(stream) as Box<dyn TransportStream>)
            }
            Listener::Tcp(listener) => {
                let (stream, _) = listener
                    .accept()
                    .await
                    .map_err(|e| ("Failed to accept connection on TCP socket", e).into())?;
                Ok(Box::new(stream) as Box<dyn TransportStream>)
            }
        }
    }
}
