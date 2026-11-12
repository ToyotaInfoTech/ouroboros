// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Runtime framework and component infrastructure.
//!
//! This module provides the core abstractions for building, managing, and
//! executing runtime components, including clients, servers, executors,
//! and database access.

pub mod builder;
pub mod client;
pub mod comp;
pub mod context;
#[cfg(feature = "daemon")]
pub mod executor;
#[cfg(feature = "daemon")]
pub mod metrics;
pub mod rdb;
#[cfg(feature = "daemon")]
pub mod server;
pub mod types;
