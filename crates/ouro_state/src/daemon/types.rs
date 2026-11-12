// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Trait implementations for daemon state types.
//!
//! Implements the `State` and `RootState` marker traits for all daemon
//! configuration types to enable their use in the component lifecycle.

use crate::core::types::{RootState, State};
use crate::daemon::{DaemonSpec, DaemonTaskSpec, MetricsStore, RepositorySpec, ShadowStoreSpec};

impl RootState for DaemonSpec {}
impl State for DaemonSpec {}
impl State for RepositorySpec {}
impl State for ShadowStoreSpec {}
impl State for DaemonTaskSpec {}
impl State for MetricsStore {}
