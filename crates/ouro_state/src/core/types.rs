// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Core type traits for state management.
//!
//! Defines marker traits that classify configuration types into
//! state, root state, and callable state categories.

use crate::core::{
    ClientSpec, DataSourceSpec, ExecutorSpec, NetworkEndpointSpec, RetryStrategySpec,
    ServerEndpointSpec, ServerSpec, SqlConnSpec, WithClient, WithDataSource, WithServer,
};

/// Marker trait for types that can be used as component state.
pub trait State: std::fmt::Debug + Clone + Send + Sync + PartialEq {}

pub trait CompState: State {
    fn is_enabled(&self) -> bool;
}

/// Marker trait for top-level root configuration types (e.g., EdgeSpec, DaemonSpec).
pub trait RootState: State {}

/// Marker trait for state types that support callable (RPC/network) operations.
pub trait CallableState: State {}

impl State for RetryStrategySpec {}
impl State for DataSourceSpec {}
impl State for SqlConnSpec {}
impl State for NetworkEndpointSpec {}
impl State for ServerEndpointSpec {}
impl State for ClientSpec {}
impl State for ServerSpec {}
impl State for ExecutorSpec {}
impl State for WithDataSource {}
impl State for WithClient {}
impl State for WithServer {}

impl CallableState for ClientSpec {}
impl CallableState for DataSourceSpec {}
