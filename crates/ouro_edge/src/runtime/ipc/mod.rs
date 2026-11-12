// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

pub mod ipc_uds;

use async_trait::async_trait;

use crate::gen::peer_generated::fb::peer as fbp;
use ouro_core::{comm::rpc::RpcDispatcher, InternalResult};
use ouro_schema::schema::traj::CameraId;

/// Trait for IPC peer communication.
///
/// Defines the interface for establishing and managing IPC connections
/// with RPC handlers.
#[async_trait]
#[allow(dead_code)] // API
pub trait Peer<T: RpcDispatcher + 'static> {
    /// Create a new peer with the specified RPC handler.
    ///
    /// # Arguments
    /// * `handler` - RPC request dispatcher
    ///
    /// # Returns
    /// * `Result<Self, IpcError>` - New peer instance or error
    #[allow(clippy::result_large_err)]
    fn new(handler: T) -> InternalResult<Self>
    where
        Self: Sized;

    /// Run the peer's main communication loop.
    ///
    /// # Returns
    /// * `Result<(), IpcError>` - Ok on graceful shutdown, error otherwise
    async fn run(&mut self) -> InternalResult<()>;

    /// Stop the peer and clean up resources.
    async fn stop(&mut self);
}

/// Converts a FlatBuffers CameraIdEnum to the application-level CameraId.
impl From<fbp::CameraIdEnum> for CameraId {
    fn from(v: fbp::CameraIdEnum) -> Self {
        match v {
            fbp::CameraIdEnum::Front => CameraId::Front,
            fbp::CameraIdEnum::FrontLeft => CameraId::FrontLeft,
            fbp::CameraIdEnum::FrontRight => CameraId::FrontRight,
            fbp::CameraIdEnum::RearLeft => CameraId::RearLeft,
            fbp::CameraIdEnum::RearRight => CameraId::RearRight,
            fbp::CameraIdEnum::Rear => CameraId::Rear,
            _ => CameraId::Front, // unknown values default to Front
        }
    }
}
