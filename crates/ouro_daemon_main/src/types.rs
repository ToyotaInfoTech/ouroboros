// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Built daemon task types.
//!
//! Defines the BuiltDaemonTask enum that wraps all concrete task implementations.

use std::sync::Arc;

use crate::task::{
    heartbeat, metadata, metrics,
    state::{
        manager::ShadowManagerTask, store::BuiltShadowStore, tag_manager::TagManagerTask,
        tag_store::BuiltTagStore,
    },
    webui,
};
use ouro_core::{
    error::{InternalError, InternalErrorKind},
    impl_try_from_any_sharable, impl_try_from_built_component,
    runtime::comp::CompGuard,
};

/// Enumeration of all built daemon task variants.
///
/// Each variant wraps a [`CompGuard`] around the concrete task to provide
/// lifecycle management (start, stop, update) through the component framework.
#[derive(strum_macros::Display)]
#[strum(serialize_all = "snake_case")]
pub enum BuiltDaemonTask {
    /// Heartbeat monitoring task.
    Heartbeat(Arc<CompGuard<heartbeat::HeartbeatTask>>),
    /// Device shadow state management task.
    State(Arc<CompGuard<ShadowManagerTask>>),
    /// Metrics HTTP server task.
    Metrics(Arc<CompGuard<metrics::JsonHttpMetricsServerTask>>),
    /// Telemetry trajectory ingestion task.
    Trajectory(Arc<CompGuard<metadata::TrajectoryTask>>),
    /// Tag-based configuration management task.
    Tag(Arc<CompGuard<TagManagerTask>>),
    /// WebUI dashboard server task.
    WebUi(Arc<CompGuard<webui::WebUiTask>>),
}

impl_try_from_built_component!(
    BuiltDaemonTask,
    [BuiltDaemonTask::Heartbeat],
    CompGuard<heartbeat::HeartbeatTask>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "HeartbeatTask is expected"
);

impl_try_from_built_component!(
    BuiltDaemonTask,
    [BuiltDaemonTask::State],
    CompGuard<ShadowManagerTask>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "ShadowManagerTask is expected"
);

impl_try_from_built_component!(
    BuiltDaemonTask,
    [BuiltDaemonTask::Metrics],
    CompGuard<metrics::JsonHttpMetricsServerTask>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "JsonHttpMetricsServerTask is expected"
);

impl_try_from_built_component!(
    BuiltDaemonTask,
    [BuiltDaemonTask::Trajectory],
    CompGuard<metadata::TrajectoryTask>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "TrajectoryTask is expected"
);

impl_try_from_built_component!(
    BuiltDaemonTask,
    [BuiltDaemonTask::Tag],
    CompGuard<TagManagerTask>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "TagManagerTask is expected"
);

impl_try_from_built_component!(
    BuiltDaemonTask,
    [BuiltDaemonTask::WebUi],
    CompGuard<webui::WebUiTask>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "WebUiTask is expected"
);

/// Enumeration of all built daemon repository variants.
pub enum BuiltRepository {
    ShadowStore(Arc<BuiltShadowStore>),
    TagStore(Arc<BuiltTagStore>),
}

impl_try_from_built_component!(
    BuiltRepository,
    [BuiltRepository::ShadowStore],
    BuiltShadowStore,
    InternalError,
    InternalErrorKind::TypeConversion,
    "ShadowStore is expected"
);

impl_try_from_built_component!(
    BuiltRepository,
    [BuiltRepository::TagStore],
    BuiltTagStore,
    InternalError,
    InternalErrorKind::TypeConversion,
    "TagStore is expected"
);

impl_try_from_any_sharable!(BuiltDaemonTask);
impl_try_from_any_sharable!(BuiltRepository);
