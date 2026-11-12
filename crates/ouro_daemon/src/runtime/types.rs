// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use ouro_core::runtime::{
    metrics::{store, transient::TransientMetricsStore},
    types::BuiltMetricsProvider,
};
use std::sync::Arc;

/// Built metrics store, unified as an enum over concrete store backends.
///
/// Currently only the in-memory transient backend is supported.
#[derive(Debug, Clone)]
pub enum BuiltMetricsStore {
    Transient(Arc<TransientMetricsStore>),
}

#[async_trait]
impl store::MetricsStore for BuiltMetricsStore {
    async fn add_point(&self, name: &str, value: f64) {
        match self {
            BuiltMetricsStore::Transient(provider) => {
                provider.add_point(name, value).await;
            }
        }
    }

    fn new_provider(&self) -> Arc<BuiltMetricsProvider> {
        match self {
            BuiltMetricsStore::Transient(provider) => provider.new_provider(),
        }
    }
}
