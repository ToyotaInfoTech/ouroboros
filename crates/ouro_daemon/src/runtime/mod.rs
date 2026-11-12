// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Daemon runtime infrastructure.
//!
//! Provides daemon-specific resource context, types, and metrics store
//! integration for the Ouroboros cloud daemon.

pub mod context;
pub mod types;

use crate::runtime::types::BuiltMetricsStore;
use ouro_core::runtime::metrics;
use ouro_state::daemon::MetricsStore;
use std::sync::Arc;

/// Converts a [`MetricsStore`] configuration into the corresponding
/// [`BuiltMetricsStore`] runtime instance.
#[allow(clippy::from_over_into)]
impl Into<BuiltMetricsStore> for MetricsStore {
    fn into(self) -> BuiltMetricsStore {
        match self {
            MetricsStore::InMemory { .. } => BuiltMetricsStore::Transient(Arc::new(
                metrics::transient::TransientMetricsStore::new(Some(100)),
            )),
        }
    }
}
