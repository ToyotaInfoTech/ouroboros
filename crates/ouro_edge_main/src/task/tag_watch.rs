// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Edge-side tag-based configuration watch task.
//!
//! Subscribes to tag assignments (`edge/{edge_id}/tags`) and per-tag config
//! updates (`edge/tag/{tag_id}/config`), merges them locally, and triggers
//! reconfiguration via the existing agent command flow.
//!
//! # Design
//!
//! The daemon sends an [`EdgeStateSyncIntent`] containing a [`TagAssignment`]
//! where tags already known to the edge have `priority` and `patch` set to
//! `None` to reduce bandwidth.  The edge fills in the missing fields from its
//! local `tag_cache` and performs the merge.
//!
//! Additionally, the edge subscribes to `edge/tag/{tag_id}/config` for each
//! tag in its current state.  When the daemon publishes a tag update on that
//! topic (a full [`Tag`] object), the edge updates its cache and re-merges.

use async_trait::async_trait;
use ouro_common::io::FromCbor as _;
use ouro_common::topic;
use ouro_core::comm::messaging::{Session, Subscriber, TopicMessage};
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::client::Callable as _;
use ouro_core::runtime::comp::{CompContext, ComponentImpl, ComponentImplExt};
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::types::SharableAction;
use ouro_core::{define_resource_trigger_action, InternalResult};
use ouro_edge::runtime::context::EdgeResourceContextImpl;
use ouro_schema::schema::agent_control::{AgentCommand, AgentCommandKind, RootStateKind};
use ouro_schema::schema::shadow::sync::EdgeStateSyncIntent;
use ouro_schema::schema::Message;
use ouro_state::core::{Tag, TagAssignment};
use ouro_state::edge::merge;
use ouro_state::edge::patch::EdgeSpecPatch;
use ouro_state::edge::EdgeTaskSpec;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::instrument;

/// Tag-based configuration watch task for edge devices.
///
/// Subscribes to:
/// - `edge/{edge_id}/tags` — [`EdgeStateSyncIntent`] from daemon
/// - `edge/tag/{tag_id}/config` — per-tag [`Tag`] updates from daemon
///
/// Maintains a local tag cache to avoid re-transmitting known patches.
#[derive(Debug)]
pub struct TagWatchTask {
    /// Local cache: tag_id → full Tag (with priority and patch).
    tag_cache: Arc<RwLock<HashMap<String, Tag>>>,

    /// Active per-tag config subscribers: tag_id → Zenoh subscriber.
    /// Updated dynamically in [`_recv_state_sync_intent`] when tag assignments change.
    /// Dropping a subscriber automatically unsubscribes from the Zenoh topic.
    per_tag_subscribers: Arc<RwLock<HashMap<String, Arc<dyn Subscriber>>>>,
}

impl TagWatchTask {
    /// Create a new tag watch task.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tag_cache: Arc::new(RwLock::new(HashMap::new())),
            per_tag_subscribers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Merge all cached tags and send a Reconfigure command.
    ///
    /// Collects all tags from `tag_cache` that are in the current assignment,
    /// validates that all have `priority` set, sorts via [`Tag`]'s `PartialOrd` impl,
    /// and merges onto the base state.
    ///
    /// Uses [`EdgeResourceContextImpl::base_state`] — the immutable startup
    /// configuration — as the merge foundation so that removing all tags always
    /// reverts to the original TOML values rather than to a previously-patched
    /// state stored in `root`.
    #[instrument(skip(tag_cache, assignment, context))]
    async fn merge_and_reconfigure(
        tag_cache: &HashMap<String, Tag>,
        assignment: &TagAssignment,
        context: &EdgeResourceContextImpl,
    ) -> InternalResult<()> {
        let base_state = (*context.base_state()).clone();

        // Collect tags in assignment order, filling from cache.
        let mut tags: Vec<Tag> = Vec::with_capacity(assignment.len());
        for assigned in assignment.iter() {
            if let Some(cached) = tag_cache.get(&assigned.id) {
                tags.push(cached.clone());
            } else {
                tracing::warn!(
                    tag_id = %assigned.id,
                    "Assigned tag not found in cache — skipping"
                );
            }
        }

        // Validate: all tags must have priority set.
        for tag in &tags {
            if tag.priority.is_none() {
                return Err((
                    format!(
                        "Tag '{}' has no priority after merge with cache — this is a bug",
                        tag.id
                    )
                    .as_str(),
                    InternalErrorKind::Runtime,
                )
                    .into());
            }
        }

        // Sort by Tag::PartialOrd (priority, id).
        tags.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Build patch list for merge.
        let ordered_patches: Vec<(merge::Priority, EdgeSpecPatch)> = tags
            .iter()
            .filter_map(|t| Some((t.priority.unwrap_or(0), t.patch.clone()?)))
            .collect();

        if ordered_patches.is_empty() {
            tracing::debug!("No tag patches to merge — resetting to base configuration");
            // Clear the epoch so the daemon sees no assigned tags
            // and send a Reconfigure to revert to the base state.
            let mut reverted = base_state.clone();
            reverted.metadata.epoch = assignment.stripped();
            let command: AgentCommand = Message::new(AgentCommandKind::Reconfigure(Box::new(
                RootStateKind::Edge(reverted),
            )));
            context.send_command(command).await?;
            return Ok(());
        }

        match merge::merge_edge_spec(&base_state, &ordered_patches) {
            Ok(mut merged) => {
                // Preserve edge_id and model from current runtime.
                merged.runtime.edge_id = base_state.runtime.edge_id.clone();
                merged.runtime.model = base_state.runtime.model.clone();

                // Override metadata.epoch with the actual tag assignment from
                // the daemon so that the edge reports the same (id, epoch)
                // pairs the daemon expects, allowing sync_state to converge.
                merged.metadata.epoch = assignment.stripped();

                let command: AgentCommand = Message::new(AgentCommandKind::Reconfigure(Box::new(
                    RootStateKind::Edge(merged),
                )));
                context.send_command(command).await?;

                tracing::info!(
                    tag_count = ordered_patches.len(),
                    "Successfully merged tag patches and sent Reconfigure"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Tag merge conflict — configuration NOT updated"
                );
                // TODO: report conflict via edge/{edge_id}/status
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ComponentImpl for TagWatchTask {
    type StateImpl = EdgeTaskSpec;
    type ContextImpl = EdgeResourceContextImpl;

    #[instrument(skip(self, state, context))]
    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<tokio::task::JoinHandle<()>>> {
        ComponentImplExt::on_active(self, state, context).await
    }
}

#[async_trait]
impl ComponentImplExt for TagWatchTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &EdgeTaskSpec,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = Vec::new();

        match state {
            EdgeTaskSpec::TagWatch { with_client, .. } => {
                let client = context.with_client(with_client).await?;
                let client = client.as_retryable()?;
                let edge_id = context.root().lock().await.runtime.edge_id.clone();

                // Action: Subscribe to tag assignment intent for this edge.
                let tags_topic = topic::edge_tags_key(edge_id.as_str());
                let session = client.transport()?.into_messaging()?;
                let tags_subscriber = session.topic_subscriber(&tags_topic).await?;

                let tag_cache = self.tag_cache.clone();
                let desired_state: Arc<RwLock<Option<TagAssignment>>> =
                    context.tag_desired_state.clone();
                let per_tag_subs = self.per_tag_subscribers.clone();

                // Pre-clone for the second action (first macro captures by move).
                let tag_cache_relay = tag_cache.clone();
                let desired_state_relay = desired_state.clone();
                let context_relay = context.clone();
                let per_tag_subs_relay = per_tag_subs.clone();

                // Action: handle tag assignment sync intents from daemon.
                let action = define_resource_trigger_action!(
                    with {
                        let tags_subscriber = tags_subscriber.clone();
                        let context = context.clone();
                        let tag_cache = tag_cache.clone();
                        let desired_state = desired_state.clone();
                        let session = session.clone();
                        let per_tag_subs = per_tag_subs.clone();
                    }
                    when: let result = tags_subscriber.recv_async().await,
                    then: _recv_state_sync_intent(result, tag_cache, desired_state, session, per_tag_subs, context)
                );
                actions.push(action);

                // Action: receive per-tag config updates via select_all over active subscribers.
                // The factory is called each loop iteration, picking up subscriber changes.
                let action = define_resource_trigger_action!(
                    with {
                        let per_tag_subs = per_tag_subs_relay.clone();
                        let tag_cache = tag_cache_relay.clone();
                        let desired_state = desired_state_relay.clone();
                        let context = context_relay.clone();
                    }
                    when: let result = {
                        let subs = per_tag_subs.read().await;
                        let arcs: Vec<_> = subs.values().cloned().collect();
                        drop(subs);
                        if arcs.is_empty() {
                            // No per-tag subscribers yet; yield to other actions.
                            std::future::pending::<InternalResult<TopicMessage>>().await
                        } else {
                            let futs: Vec<_> = arcs
                                .into_iter()
                                .map(|s| {
                                    Box::pin(async move { s.recv_async().await })
                                        as std::pin::Pin<Box<dyn std::future::Future<Output = InternalResult<TopicMessage>> + Send>>
                                })
                                .collect();
                            let (result, _, _) = futures::future::select_all(futs).await;
                            result
                        }
                    },
                    then: _recv_tag_config(result, tag_cache, desired_state, context)
                );
                actions.push(action);
            }
            _ => {
                return Err((
                    "Invalid task config for TagWatchTask",
                    InternalErrorKind::InvalidConfig,
                )
                    .into());
            }
        }

        Ok(actions)
    }
}

/// Handle a tag assignment update ([`EdgeStateSyncIntent`]) from daemon.
///
/// For each tag in the intent:
/// - If `patch` and `priority` are `Some`, update `tag_cache` (edge didn't know this tag).
/// - If `None`, use the existing entry from `tag_cache` (edge already knows it).
///
/// After updating the cache, re-merge and reconfigure.
/// Also updates per-tag subscriptions: subscribe to new tags, unsubscribe from removed.
#[instrument(skip(result, tag_cache, desired_state, session, per_tag_subs, context))]
async fn _recv_state_sync_intent(
    result: InternalResult<TopicMessage>,
    tag_cache: Arc<RwLock<HashMap<String, Tag>>>,
    desired_state: Arc<RwLock<Option<TagAssignment>>>,
    session: Arc<dyn Session>,
    per_tag_subs: Arc<RwLock<HashMap<String, Arc<dyn Subscriber>>>>,
    context: Arc<CompContext<TagWatchTask>>,
) -> InternalResult<()> {
    let TopicMessage(_topic, payload) = result?;

    let intent = EdgeStateSyncIntent::from_cbor(payload.as_slice())
        .map_err(|e| ("Failed to decode EdgeStateSyncIntent", e).into())?;

    let assignment = intent.tag_assignment;

    tracing::info!(tag_count = assignment.len(), "Received EdgeStateSyncIntent");

    // Update tag_cache: for tags with patch/priority, store them; for None, keep existing.
    {
        let mut cache = tag_cache.write().await;

        // Remove tags no longer in the assignment.
        let new_ids: HashSet<&str> = assignment.iter().map(|t| t.id.as_str()).collect();
        cache.retain(|id, _| new_ids.contains(id.as_str()));

        // Update/insert tags from intent.
        for tag in assignment.iter() {
            if tag.patch.is_some() || tag.priority.is_some() {
                // Daemon sent new data for this tag — update cache.
                if let Some(cached) = cache.get_mut(&tag.id) {
                    if let Some(ref p) = tag.priority {
                        cached.priority = Some(*p);
                    }
                    if let Some(ref patch) = tag.patch {
                        cached.patch = Some(patch.clone());
                    }
                    cached.epoch = tag.epoch;
                } else {
                    cache.insert(tag.id.clone(), tag.clone());
                }
            } else {
                // patch=None, priority=None → edge should already have this in cache.
                // Just update the epoch in case it changed.
                if let Some(cached) = cache.get_mut(&tag.id) {
                    cached.epoch = tag.epoch;
                } else {
                    tracing::warn!(
                        tag_id = %tag.id,
                        "Daemon sent stripped tag but edge has no cache entry — data loss?"
                    );
                }
            }
        }
    }

    // Store as desired_state.
    {
        let mut _guard = desired_state.write().await;
        *_guard = Some(assignment.clone());
    }

    // Update per-tag subscriptions: subscribe to new tags, unsubscribe from removed.
    // Changes are picked up by the per-tag action factory on the next loop iteration
    // via select_all over per_tag_subscribers values.
    {
        let mut subs = per_tag_subs.write().await;
        let new_ids: HashSet<String> = assignment.iter().map(|t| t.id.clone()).collect();
        let old_ids: HashSet<String> = subs.keys().cloned().collect();

        // Subscribe to newly assigned tags.
        for tag_id in new_ids.difference(&old_ids) {
            let config_topic = topic::tag_config_key(tag_id);
            match session.topic_subscriber(&config_topic).await {
                Ok(subscriber) => {
                    subs.insert(tag_id.clone(), subscriber);
                    tracing::info!(tag_id = %tag_id, "Subscribed to per-tag config topic");
                }
                Err(e) => {
                    tracing::error!(tag_id = %tag_id, error = %e, "Failed to subscribe to per-tag config topic");
                }
            }
        }

        // Unsubscribe from removed tags (dropping the subscriber unsubscribes automatically).
        for tag_id in old_ids.difference(&new_ids) {
            subs.remove(tag_id.as_str());
            tracing::info!(tag_id = %tag_id, "Unsubscribed from per-tag config topic");
        }
    }

    // Re-merge and reconfigure.
    let ctx: &EdgeResourceContextImpl = context.as_ref();
    let cache = tag_cache.read().await;
    TagWatchTask::merge_and_reconfigure(&cache, &assignment, ctx).await?;

    Ok(())
}

/// Handle a per-tag config update from daemon.
///
/// Receives a full [`Tag`] object from `edge/tag/{tag_id}/config`,
/// updates the local cache, and triggers re-merge & reconfigure.
#[instrument(skip(result, tag_cache, desired_state, context))]
async fn _recv_tag_config(
    result: InternalResult<TopicMessage>,
    tag_cache: Arc<RwLock<HashMap<String, Tag>>>,
    desired_state: Arc<RwLock<Option<TagAssignment>>>,
    context: Arc<CompContext<TagWatchTask>>,
) -> InternalResult<()> {
    let TopicMessage(topic_key, payload) = result?;

    // Extract tag_id from topic: "edge/tag/{tag_id}/config"
    let topic_str = topic_key.to_string();
    let tag_id = topic_str
        .strip_prefix("edge/tag/")
        .and_then(|s| s.strip_suffix("/config"))
        .ok_or_else(|| {
            Into::<ouro_core::error::InternalError>::into((
                format!("Unexpected tag config topic format: {}", topic_str).as_str(),
                InternalErrorKind::Protocol,
            ))
        })?
        .to_string();

    let tag = Tag::from_cbor(payload.as_slice())
        .map_err(|e| ("Failed to decode Tag from per-tag topic", e).into())?;

    tracing::debug!(tag_id = %tag_id, epoch = ?tag.epoch, "Received per-tag config update");

    // Update cache.
    {
        let mut cache = tag_cache.write().await;
        cache.insert(tag_id.clone(), tag);
    }

    // Re-merge if we have a current assignment.
    let ds = desired_state.read().await;
    if let Some(ref assignment) = *ds {
        // Validate all tags have priority.
        let cache = tag_cache.read().await;
        for assigned in assignment.iter() {
            if let Some(cached) = cache.get(&assigned.id) {
                if cached.priority.is_none() {
                    return Err((
                        format!(
                            "Tag '{}' has no priority after cache update — this is a bug",
                            assigned.id
                        )
                        .as_str(),
                        InternalErrorKind::Runtime,
                    )
                        .into());
                }
            }
        }

        let ctx: &EdgeResourceContextImpl = context.as_ref();
        TagWatchTask::merge_and_reconfigure(&cache, assignment, ctx).await?;
    }

    Ok(())
}
