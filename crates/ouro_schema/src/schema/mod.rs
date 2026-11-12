// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! RPC and data schema definitions for the Ouroboros system.
//!
//! Contains message types for CLI RPC, device shadows, trajectory telemetry,
//! and agent control commands.

use ouro_common::{types::UnixTimestamp, unix_epoch_now};
use serde::{Deserialize, Serialize};

pub mod agent_control;
#[cfg(feature = "daemon")]
pub mod cli;
pub mod shadow;
pub mod traj;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message<T> {
    pub timestamp: UnixTimestamp,
    pub payload: T,
}

impl<T> Message<T> {
    pub fn new(payload: T) -> Self {
        Self {
            timestamp: unix_epoch_now(),
            payload,
        }
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Message<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.payload, self.timestamp)
    }
}

#[macro_export]
macro_rules! rpc {
    (
        $prefix:ident,
        $(
            $name:ident ( $req:ty ) -> $resp:ty
        ),* $(,)?
    ) => {
        ::paste::paste! {
            #[derive(Debug, serde::Serialize, serde::Deserialize)]
            pub enum [<$prefix Request>] {
                $(
                    $name($req),
                )*
            }

            #[derive(Debug, serde::Serialize, serde::Deserialize)]
            pub enum [<$prefix Response>] {
                $(
                    $name($resp),
                )*
                Error(String),
            }

            #[async_trait::async_trait]
            pub trait [<$prefix Handler>]: Send + Sync + 'static {
                $(
                    async fn [<$name:snake>](&self, req: $req) -> anyhow::Result<$resp>;
                )*
            }

            pub async fn dispatch<H: [<$prefix Handler>]>(
                handler: &H,
                req: [<$prefix Request>]
            ) -> [<$prefix Response>] {
                match req {
                    $(
                        [<$prefix Request>]::$name(v) => {
                            let r = handler.[<$name:snake>](v).await;
                            match r {
                                Ok(resp) => [<$prefix Response>]::$name(resp),
                                Err(e) => [<$prefix Response>]::Error(format!("{}", e)),
                            }
                        }
                    )*
                }
            }
        }
    };
}

#[macro_export]
macro_rules! icc {
    (
        $prefix:ident,
        $(
            $name:ident ( $req:ty ) -> $resp:ty
        ),* $(,)?
    ) => {
        ::paste::paste! {

            #[derive(Debug)]
            pub struct [<$prefix RequestWrapper>] {
                pub request: [<$prefix Request>],
                pub response_tx: Sender<[<$prefix Response>]>,
            }

            #[derive(Debug)]
            pub enum [<$prefix Request>] {
                $(
                    $name($req),
                )*
            }

            #[derive(Debug)]
            pub enum [<$prefix Response>] {
                $(
                    $name($resp),
                )*
                Error(String),
            }

            #[async_trait::async_trait]
            pub trait [<$prefix Handler>]: Send + Sync + 'static {
                $(
                    async fn [<$name:snake>](&self, req: $req) -> anyhow::Result<$resp>;
                )*
            }

            pub async fn dispatch<H: [<$prefix Handler>]>(
                handler: &H,
                result: Option<[<$prefix RequestWrapper>]>
            ) -> anyhow::Result<()> {
                match result {
                    Some(wrapper) => {
                        let req = wrapper.request;
                        let response_tx = wrapper.response_tx;
                        match req {
                            $(
                                [<$prefix Request>]::$name(v) => {
                                    let r = handler.[<$name:snake>](v).await;
                                    match r {
                                        Ok(resp) => {
                                            if let Err(e) = response_tx.send([<$prefix Response>]::$name(resp)) {
                                                ::tracing::warn!(error = ?e, "Failed to send response");
                                            }
                                        },
                                        Err(e) => {
                                            if let Err(e) = response_tx.send([<$prefix Response>]::Error(format!("{}", e))) {
                                                ::tracing::warn!(error = ?e, "Failed to send error response");
                                            }
                                        },
                                    }
                                }
                            )*
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("No request received"));
                    },
                }
                Ok(())
            }
        }
    };
}
