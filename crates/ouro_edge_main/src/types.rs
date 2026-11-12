// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use ouro_core::{
    impl_try_from_any_sharable,
    runtime::comp::{CompGuard, CompStatus},
    InternalResult,
};
use ouro_state::edge::{EdgeSpec, EdgeTaskSpec};

use crate::task::{
    heartbeat::HeartbeatTask, metadata::MetadataSendTask, state::report::StateReportTask,
    tag_watch::TagWatchTask, watch::CommandWatchTask,
};

pub enum BuiltEdgeTask {
    Heartbeat(Arc<CompGuard<HeartbeatTask>>),
    Metadata(Arc<CompGuard<MetadataSendTask>>),
    StateReport(Arc<CompGuard<StateReportTask>>),
    CommandWatch(Arc<CompGuard<CommandWatchTask>>),
    TagWatch(Arc<CompGuard<TagWatchTask>>),
}

impl_try_from_any_sharable!(BuiltEdgeTask);

impl BuiltEdgeTask {
    pub async fn state(&self) -> InternalResult<EdgeTaskSpec> {
        match self {
            BuiltEdgeTask::Heartbeat(task) => task.state().await,
            BuiltEdgeTask::Metadata(task) => task.state().await,
            BuiltEdgeTask::StateReport(task) => task.state().await,
            BuiltEdgeTask::CommandWatch(task) => task.state().await,
            BuiltEdgeTask::TagWatch(task) => task.state().await,
        }
    }

    pub async fn activate(&self) -> InternalResult<()> {
        match self {
            BuiltEdgeTask::Heartbeat(task) => task.activate().await?,
            BuiltEdgeTask::Metadata(task) => task.activate().await?,
            BuiltEdgeTask::StateReport(task) => task.activate().await?,
            BuiltEdgeTask::CommandWatch(task) => task.activate().await?,
            BuiltEdgeTask::TagWatch(task) => task.activate().await?,
        }
        Ok(())
    }

    pub async fn deactivate(&self) -> InternalResult<()> {
        match self {
            BuiltEdgeTask::Heartbeat(task) => task.deactivate().await?,
            BuiltEdgeTask::Metadata(task) => task.deactivate().await?,
            BuiltEdgeTask::StateReport(task) => task.deactivate().await?,
            BuiltEdgeTask::CommandWatch(task) => task.deactivate().await?,
            BuiltEdgeTask::TagWatch(task) => task.deactivate().await?,
        }
        Ok(())
    }

    pub async fn reconfigure(&self, new_state: EdgeTaskSpec) -> InternalResult<()> {
        match self {
            BuiltEdgeTask::Heartbeat(task) => task.reconfigure(new_state).await?,
            BuiltEdgeTask::Metadata(task) => task.reconfigure(new_state).await?,
            BuiltEdgeTask::StateReport(task) => task.reconfigure(new_state).await?,
            BuiltEdgeTask::CommandWatch(task) => task.reconfigure(new_state).await?,
            BuiltEdgeTask::TagWatch(task) => task.reconfigure(new_state).await?,
        }
        Ok(())
    }

    pub async fn discard(&self) -> InternalResult<()> {
        match self {
            BuiltEdgeTask::Heartbeat(task) => task.discard().await?,
            BuiltEdgeTask::Metadata(task) => task.discard().await?,
            BuiltEdgeTask::StateReport(task) => task.discard().await?,
            BuiltEdgeTask::CommandWatch(task) => task.discard().await?,
            BuiltEdgeTask::TagWatch(task) => task.discard().await?,
        }
        Ok(())
    }

    pub async fn comp_status(&self) -> CompStatus<EdgeTaskSpec> {
        match self {
            BuiltEdgeTask::Heartbeat(task) => task.comp_status().await,
            BuiltEdgeTask::Metadata(task) => task.comp_status().await,
            BuiltEdgeTask::StateReport(task) => task.comp_status().await,
            BuiltEdgeTask::CommandWatch(task) => task.comp_status().await,
            BuiltEdgeTask::TagWatch(task) => task.comp_status().await,
        }
    }
}

pub trait StateManager {
    fn set_desired_state(&mut self, state: EdgeSpec);

    fn get_current_state(&self) -> EdgeSpec;
}
