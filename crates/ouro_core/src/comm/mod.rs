// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Communication infrastructure for the Ouroboros system.
//!
//! Provides messaging (Zenoh pub/sub + RPC), network transport (UDS/TCP),
//! and RPC client/server abstractions used by daemon, edge, and CLI binaries.

pub mod messaging;
#[cfg(feature = "daemon")]
pub mod net;
pub mod rpc;
