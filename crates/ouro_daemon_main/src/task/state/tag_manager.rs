// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Tag-based configuration management task for daemon.
//!
//! Publishes tag configurations and edge-tag assignments via Zenoh
//! so that edge devices can merge patches locally.

use async_trait::async_trait;
use ouro_common::io::ToCbor as _;
use ouro_common::topic;
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::client::Callable as _;
use ouro_core::runtime::client::RetryableClient;
use ouro_core::runtime::comp::ComponentImplExt;
use ouro_core::runtime::comp::{CompContext, ComponentImpl};
use ouro_core::runtime::context::ResourceContext as _;
use ouro_core::runtime::types::SharableAction;
use ouro_core::InternalResult;
use ouro_daemon::runtime::context::DaemonResourceContext as _;
use ouro_daemon::runtime::context::DaemonResourceContextImpl;
use ouro_schema::schema::shadow::sync::EdgeStateSyncIntent;
use ouro_state::core::StateEpoch;
use ouro_state::core::StateMetadata;
use ouro_state::core::Tag;
use ouro_state::core::TagAssignment;
use ouro_state::daemon::{DaemonTaskSpec, PredefinedTagSpec};
use ouro_state::edge::patch::EdgeSpecPatch;
use ouro_state::edge::EdgeSpec;
use std::path::Path;
use std::sync::Arc;
use tokio::task::JoinHandle;
use toml;
use tracing::instrument;

use crate::task::icc::register_icc_handler;
use crate::task::state::store::BuiltShadowStore;
use crate::task::state::store::ShadowStore as _;
use crate::task::state::tag_manager::icc::TagIccHandler;
use crate::task::state::tag_store::BuiltTagStore;
use crate::task::state::tag_store::TagStore as _;
use crate::types::BuiltRepository;

/// Tag management task for daemon.
///
/// Manages configuration tags and their assignments to edge devices.
/// Publishes tag configs to `edge/tag/{tag_id}/config` and
/// tag assignments to `edge/{edge_id}/tags` via Zenoh pub/sub.
#[derive(Clone)]
pub struct TagManagerTask {
    default_edge_state: EdgeSpec,
    predefined_tags: Vec<PredefinedTagSpec>,
}

impl TagManagerTask {
    /// Create a new tag manager task.
    ///
    /// # Arguments
    ///
    /// * `default_edge_state` - Base edge configuration used as merge foundation
    /// * `predefined_tags` - Tags to upsert into the store on startup
    ///
    /// # Returns
    ///
    /// A new `TagManagerTask` instance
    pub(crate) fn new(
        default_edge_state: EdgeSpec,
        predefined_tags: Vec<PredefinedTagSpec>,
    ) -> Arc<Self> {
        Arc::new(Self {
            default_edge_state,
            predefined_tags,
        })
    }

    /// Load an `EdgeSpecPatch` from a TOML file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML file containing an `EdgeSpecPatch`
    ///
    /// # Returns
    ///
    /// Parsed `EdgeSpecPatch` or an error
    #[instrument]
    #[allow(clippy::result_large_err)]
    fn load_edge_spec_patch(path: &Path) -> InternalResult<EdgeSpecPatch> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            (
                format!("Failed to read edge spec patch TOML: {:?}", path).as_str(),
                e,
            )
                .into()
        })?;
        let patch: EdgeSpecPatch = toml::from_str(&content).map_err(|e| {
            (
                format!("Failed to parse edge spec patch TOML: {:?}", path).as_str(),
                e,
            )
                .into()
        })?;
        Ok(patch)
    }

    /// Upsert all predefined tags into the tag store.
    ///
    /// Each tag is loaded from its configured TOML file and inserted/overwritten
    /// in the store before any Zenoh publish occurs.
    ///
    /// # Arguments
    ///
    /// * `predefined_tags` - List of predefined tag specs from config
    /// * `tag_store` - Destination tag store (upsert via `insert_tag`)
    #[instrument(skip(predefined_tags, tag_store))]
    async fn upsert_predefined_tags(
        predefined_tags: &[PredefinedTagSpec],
        tag_store: Arc<BuiltTagStore>,
    ) -> InternalResult<()> {
        for spec in predefined_tags {
            let patch = Some(Self::load_edge_spec_patch(&spec.path)?);
            let tag = Tag {
                id: spec.id.clone(),
                epoch: StateEpoch(0), // Placeholder; actual epoch is assigned by the DB.
                display_name: Some(spec.display_name.clone()),
                priority: Some(spec.priority),
                patch,
            };
            tag_store.insert_tag_if_not_exists(&tag).await?;
            tracing::info!(tag_id = %spec.id, priority = spec.priority, "Upserted predefined tag");
        }
        Ok(())
    }

    /// Publish a single tag to Zenoh via `edge/tag/{tag_id}/config`.
    ///
    /// Sends the full [`Tag`] (including patch) so that edges subscribed to this
    /// topic can update their local cache immediately.
    ///
    /// # Arguments
    ///
    /// * `tag` - The config tag to publish
    /// * `client` - Retryable client for Zenoh communication
    #[instrument(skip(tag, client))]
    pub(crate) async fn publish_tag_config(
        tag: &Tag,
        client: Arc<RetryableClient>,
    ) -> InternalResult<()> {
        let topic = topic::tag_config_key(&tag.id);
        let publisher = client
            .transport()?
            .into_messaging()?
            .publisher(&topic)
            .await?;

        let payload = tag
            .to_cbor()
            .map_err(|e| (format!("Failed to serialize tag '{}'", tag.id), e).into())?;

        tracing::debug!(
            tag_id = %tag.id,
            topic = %topic,
            "Publishing tag (length={})",
            payload.len(),
        );

        client
            .call(|_transport| async move {
                publisher.put(payload.as_slice()).await?;
                Ok(())
            })
            .await?;

        Ok(())
    }

    /// Publish tag assignments for a specific edge device to Zenoh.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Target edge device identifier
    /// * `tags` - Tag assignment (ordered by priority)
    /// * `client` - Retryable client for Zenoh communication
    #[instrument(skip(tags, client))]
    pub(crate) async fn publish_edge_tags(
        edge_id: &str,
        tags: &TagAssignment,
        client: Arc<RetryableClient>,
    ) -> InternalResult<()> {
        let topic = topic::edge_tags_key(edge_id);
        let publisher = client
            .transport()?
            .into_messaging()?
            .publisher(&topic)
            .await?;

        let intent = EdgeStateSyncIntent {
            tag_assignment: tags.clone(),
        };
        let payload = intent.to_cbor().map_err(|e| {
            (
                format!(
                    "Failed to serialize EdgeStateSyncIntent for edge '{}'",
                    edge_id
                )
                .as_str(),
                e,
            )
                .into()
        })?;

        tracing::debug!(
            edge_id = %edge_id,
            topic = %topic,
            tag_count = tags.len(),
            "Publishing tag assignments to edge",
        );

        client
            .call(|_transport| async move {
                publisher.put(payload.as_slice()).await?;
                Ok(())
            })
            .await?;

        Ok(())
    }

    /// Publish all tag configs and all edge assignments on startup.
    ///
    /// # Arguments
    ///
    /// * `tag_store` - Tag store for reading tags and assignments
    /// * `shadow_store` - Shadow store for listing known edges
    /// * `client` - Retryable client for Zenoh communication
    #[instrument(skip(tag_store, shadow_store, client))]
    async fn publish_all(
        tag_store: Arc<BuiltTagStore>,
        shadow_store: Arc<BuiltShadowStore>,
        client: Arc<RetryableClient>,
    ) -> InternalResult<()> {
        // Publish all tag configs.
        let tags = tag_store.list_tags().await?;
        for tag in &tags {
            Self::publish_tag_config(tag, client.clone()).await?;
        }

        // Publish tag assignments for every known edge.
        let edge_ids = shadow_store.list_shadow_ids().await?;
        for edge_id in &edge_ids {
            let edge_tags = tag_store.get_tags_for_edge(edge_id).await?;
            Self::publish_edge_tags(edge_id, &edge_tags, client.clone()).await?;
        }

        tracing::info!(
            tag_count = tags.len(),
            edge_count = edge_ids.len(),
            "Published all tag configs and edge assignments"
        );

        Ok(())
    }
}

#[async_trait]
impl ComponentImpl for TagManagerTask {
    type StateImpl = DaemonTaskSpec;
    type ContextImpl = DaemonResourceContextImpl;

    #[instrument(skip(self, state, context))]
    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>> {
        ComponentImplExt::on_active(self, state, context).await
    }
}

#[async_trait]
impl ComponentImplExt for TagManagerTask {
    #[instrument(skip(self, state, context))]
    async fn gather_actions(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>> {
        let mut actions = vec![];

        match state {
            DaemonTaskSpec::TagManagement {
                name,
                enabled,
                with_client,
                with_tag_repository,
                with_shadow_repository,
                ..
            } => {
                if !enabled {
                    return Ok(vec![]);
                }

                let client = context.with_client(with_client).await?;
                let client = Arc::<RetryableClient>::try_from(client.as_ref())?;

                // Resolve tag store.
                let tag_repository = context.with_repository(with_tag_repository).await?;
                let tag_repository = BuiltRepository::try_from_any_sharable(tag_repository)?;
                let tag_store = match tag_repository.as_ref() {
                    BuiltRepository::TagStore(store) => store.clone(),
                    _ => {
                        return Err((
                            "TagManagerTask requires a tag store repository",
                            InternalErrorKind::InvalidConfig,
                        )
                            .into());
                    }
                };

                // Resolve shadow store (for edge list).
                let shadow_repository = context.with_repository(with_shadow_repository).await?;
                let shadow_repository = BuiltRepository::try_from_any_sharable(shadow_repository)?;
                let shadow_store = match shadow_repository.as_ref() {
                    BuiltRepository::ShadowStore(store) => store.clone(),
                    _ => {
                        return Err((
                            "TagManagerTask requires a shadow store for edge list",
                            InternalErrorKind::InvalidConfig,
                        )
                            .into());
                    }
                };

                // Upsert predefined tags into the store before publishing.
                Self::upsert_predefined_tags(&self.predefined_tags, tag_store.clone()).await?;

                // Publish all tag configs and assignments on startup.
                Self::publish_all(tag_store.clone(), shadow_store.clone(), client.clone()).await?;

                // Register ICC handler for CLI operations.
                let icc_handler = Arc::new(TagIccHandler {
                    tag_store: tag_store.clone(),
                    shadow_store: shadow_store.clone(),
                    client: client.clone(),
                    default_edge_state: self.default_edge_state.clone(),
                });
                let action = register_icc_handler(&icc_handler, name, &context).await?;
                actions.push(action);
            }
            _ => {
                return Err((
                    "TagManagerTask requires a tag management configuration",
                    InternalErrorKind::InvalidConfig,
                )
                    .into());
            }
        }

        Ok(actions)
    }
}

/// Compare the edge's reported state against the expected tag assignment and send
/// an [`EdgeStateSyncIntent`] if they diverge.
///
/// The comparison checks whether the ordered sequence of `(id, epoch)` pairs in the
/// expected [`TagAssignment`] matches what the edge reports in its `current_state`
/// and `desired_state`.  When they differ, an intent is built where:
///
/// - Tags already known to the edge (present in reported current/desired by id+epoch)
///   have `priority` and `patch` set to `None` to save bandwidth.
/// - Unknown tags carry the full `priority` and `patch` from the daemon's store.
/// - `display_name` is always `None`.
///
/// Does nothing when:
/// - `reported_state.schema_version` is lower than `CURRENT_SCHEMA_VERSION`.
/// - All tag ids/epochs already match in the correct order.
///
/// # Arguments
///
/// * `edge_id` - Target edge device identifier
/// * `reported_state` - `current_state` from the edge's last state report
/// * `reported_desired` - `desired_state` from the edge's last state report (if any)
/// * `tag_store` - Tag store for reading expected tags
/// * `client` - Zenoh client for publishing
#[instrument(skip(reported_state, reported_desired, tag_store, client))]
pub(crate) async fn sync_state(
    edge_id: &str,
    reported_state: &StateMetadata,
    reported_desired: Option<&StateMetadata>,
    tag_store: Arc<BuiltTagStore>,
    client: Arc<RetryableClient>,
) -> ouro_core::InternalResult<()> {
    use ouro_common::io::ToCbor as _;
    use ouro_state::core::CURRENT_SCHEMA_VERSION;
    use std::collections::HashSet;

    if reported_state.schema_version < CURRENT_SCHEMA_VERSION {
        tracing::warn!(
            edge_id = %edge_id,
            reported = ?reported_state.schema_version,
            current = ?CURRENT_SCHEMA_VERSION,
            "Edge reports older schema version — skipping tag resync (future: trigger OTA)"
        );
        return Ok(());
    }

    let expected = tag_store.get_tags_for_edge(edge_id).await?;
    if expected.0.is_empty() && reported_state.epoch.0.is_empty() {
        // No tags assigned and edge reports none — nothing to sync.
        return Ok(());
    }

    // Build the ordered (id, epoch) sequence the daemon expects.
    let expected_seq: Vec<(&str, StateEpoch)> = expected
        .0
        .iter()
        .map(|t| (t.id.as_str(), t.epoch))
        .collect();

    // Build the ordered (id, epoch) sequence the edge currently has.
    let reported_seq: Vec<(&str, StateEpoch)> = reported_state
        .epoch
        .0
        .iter()
        .map(|t| (t.id.as_str(), t.epoch))
        .collect();

    if expected_seq == reported_seq {
        tracing::trace!(edge_id = %edge_id, "Edge tag state is in sync");
        return Ok(());
    }

    tracing::info!(
        edge_id = %edge_id,
        expected_tags = expected_seq.len(),
        reported_tags = reported_seq.len(),
        "Tag mismatch detected — sending EdgeStateSyncIntent"
    );

    // Collect tag (id, epoch) pairs the edge already knows about
    // (from both current_state and desired_state).
    let mut known: HashSet<(&str, StateEpoch)> = HashSet::new();
    for t in reported_state.epoch.iter() {
        known.insert((t.id.as_str(), t.epoch));
    }
    if let Some(desired) = reported_desired {
        for tag in desired.epoch.iter() {
            known.insert((tag.id.as_str(), tag.epoch));
        }
    }

    // Build the sync intent: strip known tags, include full data for unknown tags.
    let sync_tags: Vec<Tag> = expected
        .iter()
        .map(|tag| {
            if known.contains(&(tag.id.as_str(), tag.epoch)) {
                tag.stripped()
            } else {
                Tag {
                    id: tag.id.clone(),
                    epoch: tag.epoch,
                    display_name: None,
                    priority: tag.priority,
                    patch: tag.patch.clone(),
                }
            }
        })
        .collect();

    let intent = EdgeStateSyncIntent {
        tag_assignment: TagAssignment(sync_tags),
    };

    let topic = topic::edge_tags_key(edge_id);
    let publisher = client
        .transport()?
        .into_messaging()?
        .publisher(&topic)
        .await?;

    let payload = intent.to_cbor().map_err(|e| {
        (
            format!(
                "Failed to serialize EdgeStateSyncIntent for edge '{}'",
                edge_id
            )
            .as_str(),
            e,
        )
            .into()
    })?;

    client
        .call(|_transport| async move {
            publisher.put(payload.as_slice()).await?;
            Ok(())
        })
        .await?;

    Ok(())
}

pub(crate) mod icc {
    use std::sync::Arc;

    use async_trait::async_trait;
    use ouro_core::runtime::client::RetryableClient;
    use ouro_schema::schema::cli::ListResp;
    use ouro_schema::{
        operation_descriptions,
        schema::cli::{
            icc::IccHandler, require_input, unsupported_operation, DescribeResp, Description,
            OperateReq, OperateResp,
        },
        Void,
    };
    use ouro_state::core::Tag;
    use ouro_state::edge::merge;
    use ouro_state::edge::patch::EdgeSpecPatch;
    use ouro_state::edge::EdgeSpec;
    use tracing::instrument;

    use super::TagManagerTask;
    use crate::task::state::store::BuiltShadowStore;
    use crate::task::state::store::ShadowStore as _;
    use crate::task::state::tag_store::{BuiltTagStore, TagStore as _};
    use ouro_schema::schema::shadow::EdgeShadow;

    /// ICC handler for tag management operations.
    pub(crate) struct TagIccHandler {
        pub(crate) tag_store: Arc<BuiltTagStore>,
        pub(crate) shadow_store: Arc<BuiltShadowStore>,
        pub(crate) client: Arc<RetryableClient>,
        pub(crate) default_edge_state: EdgeSpec,
    }

    /// Canonical operation descriptions for TagIccHandler.
    ///
    /// This is the single source of truth for operation names, inputs, and
    /// documentation. Both `describe()` and `operate()` derive their behaviour
    /// from the list returned here.
    fn descriptions() -> Vec<Description> {
        operation_descriptions![
            /// Create a new configuration tag.
            "create_tag" {
                inputs {
                    /// Display name for the tag (must be unique)
                    "display_name",
                    /// Merge priority (lower = applied first)
                    "priority",
                    /// EdgeSpecPatch as JSON string, or file://path.toml / file://path.json
                    "patch"
                }
                output {
                    /// Void on success
                }
            },
            /// Update an existing configuration tag.
            "update_tag" {
                inputs {
                    /// Tag identifier
                    "tag_id",
                    /// Display name for the tag (optional)
                    "display_name",
                    /// Merge priority
                    "priority",
                    /// EdgeSpecPatch as JSON string, or file://path.toml / file://path.json
                    "patch"
                }
                output {
                    /// Void on success
                }
            },
            /// Delete a configuration tag.
            "delete_tag" {
                inputs {
                    /// Tag identifier to delete
                    "tag_id"
                }
                output {
                    /// Void on success
                }
            },
            /// List all configuration tags.
            "list_tags" {
                inputs {}
                output {
                    /// List of all config tags
                }
            },
            /// Get a specific configuration tag.
            "get_tag" {
                inputs {
                    /// Tag identifier
                    "tag_id"
                }
                output {
                    /// The config tag, if found
                }
            },
            /// Assign a tag to an edge device.
            "assign_tag" {
                inputs {
                    /// Edge device identifier
                    "edge_id",
                    /// Tag identifier to assign
                    "tag_id"
                }
                output {
                    /// Void on success
                }
            },
            /// Remove a tag assignment from an edge device.
            "unassign_tag" {
                inputs {
                    /// Edge device identifier
                    "edge_id",
                    /// Tag identifier to unassign
                    "tag_id"
                }
                output {
                    /// Void on success
                }
            },
            /// Get all tags assigned to a specific edge.
            "get_tags_for_edge" {
                inputs {
                    /// Edge device identifier
                    "edge_id"
                }
                output {
                    /// List of config tags assigned to the edge
                }
            },
            /// Preview the merged configuration for an edge.
            "preview_merged_config" {
                inputs {
                    /// Edge device identifier
                    "edge_id"
                }
                output {
                    /// Full EdgeSpec after merging all assigned tag patches
                }
            },
            /// List all edge-tag assignments grouped by edge ID.
            "list_all_assignments" {
                inputs {}
                output {
                    /// Map of edge_id to list of assigned tags
                }
            }
        ]
    }

    #[async_trait]
    impl IccHandler for TagIccHandler {
        #[instrument(skip(self, _req))]
        async fn describe(&self, _req: Void) -> anyhow::Result<DescribeResp> {
            Ok(DescribeResp {
                descriptions: descriptions(),
            })
        }

        #[instrument(skip(self))]
        async fn operate(&self, req: OperateReq) -> anyhow::Result<OperateResp> {
            let descs = descriptions();
            match req.operation_name.as_str() {
                "create_tag" => {
                    let display_name =
                        require_input(&req.inputs, "display_name", &req.operation_name, &descs)?
                            .to_string();
                    // Reject if a tag with the same display_name already exists.
                    if self
                        .tag_store
                        .get_tag_by_display_name(&display_name)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to check display_name: {:?}", e))?
                        .is_some()
                    {
                        return Err(anyhow::anyhow!(
                            "Tag with display_name '{}' already exists",
                            display_name
                        ));
                    }
                    let tag = parse_config_tag(&req, &descs)?;
                    self.tag_store
                        .insert_tag(&tag)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to create tag: {:?}", e))?;
                    // Re-fetch to get the DB-assigned epoch before publishing.
                    let tag = self
                        .tag_store
                        .get_tag(&tag.id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to fetch tag after insert: {:?}", e))?
                        .ok_or_else(|| anyhow::anyhow!("Tag not found after insert: {}", tag.id))?;
                    TagManagerTask::publish_tag_config(&tag, self.client.clone())
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to publish tag config: {:?}", e))?;
                    Ok(OperateResp {
                        output: serde_value::to_value(Void {})?,
                    })
                }
                "update_tag" => {
                    let tag_id = require_input(&req.inputs, "tag_id", &req.operation_name, &descs)?
                        .to_string();
                    let mut tag = parse_config_tag(&req, &descs)?;
                    tag.id = tag_id;
                    self.tag_store
                        .update_tag(&tag)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to update tag: {:?}", e))?;
                    // Re-fetch to get the DB-assigned epoch before publishing.
                    let tag = self
                        .tag_store
                        .get_tag(&tag.id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to fetch tag after update: {:?}", e))?
                        .ok_or_else(|| anyhow::anyhow!("Tag not found after update: {}", tag.id))?;
                    TagManagerTask::publish_tag_config(&tag, self.client.clone())
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to publish tag config: {:?}", e))?;
                    // Re-publish assignments for all edges with this tag.
                    let edge_ids = self
                        .tag_store
                        .get_edges_for_tag(&tag.id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to get edges for tag: {:?}", e))?;
                    for edge_id in &edge_ids {
                        let tags = self
                            .tag_store
                            .get_tags_for_edge(edge_id)
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to get tags for edge: {:?}", e))?;
                        TagManagerTask::publish_edge_tags(edge_id, &tags, self.client.clone())
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to publish edge tags: {:?}", e))?;
                    }
                    Ok(OperateResp {
                        output: serde_value::to_value(Void {})?,
                    })
                }
                "delete_tag" => {
                    let tag_id = require_input(&req.inputs, "tag_id", &req.operation_name, &descs)?;
                    // Get affected edges before deletion (cascade will remove assignments).
                    let edge_ids = self
                        .tag_store
                        .get_edges_for_tag(tag_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to get edges for tag: {:?}", e))?;
                    self.tag_store
                        .delete_tag(tag_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to delete tag: {:?}", e))?;
                    // Re-publish assignments for affected edges.
                    for edge_id in &edge_ids {
                        let tags = self
                            .tag_store
                            .get_tags_for_edge(edge_id)
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to get tags for edge: {:?}", e))?;
                        TagManagerTask::publish_edge_tags(edge_id, &tags, self.client.clone())
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to publish edge tags: {:?}", e))?;
                    }
                    Ok(OperateResp {
                        output: serde_value::to_value(Void {})?,
                    })
                }
                "list_tags" => {
                    let tags = self
                        .tag_store
                        .list_tags()
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to list tags: {:?}", e))?;
                    Ok(OperateResp {
                        output: serde_value::to_value(tags)?,
                    })
                }
                "get_tag" => {
                    let tag_id = require_input(&req.inputs, "tag_id", &req.operation_name, &descs)?;
                    let tag = self
                        .tag_store
                        .get_tag(tag_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to get tag: {:?}", e))?;
                    Ok(OperateResp {
                        output: serde_value::to_value(tag)?,
                    })
                }
                "assign_tag" => {
                    let edge_id =
                        require_input(&req.inputs, "edge_id", &req.operation_name, &descs)?;
                    let tag_id = require_input(&req.inputs, "tag_id", &req.operation_name, &descs)?;
                    // Auto-register edge in shadow store (persistent) before the FK-constrained
                    // edge_tag_assignments insert. Uses desired_state from default_edge_state.
                    if self
                        .shadow_store
                        .get_shadow(edge_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to check shadow: {:?}", e))?
                        .is_none()
                    {
                        let mut shadow = EdgeShadow::new(edge_id);
                        shadow.desired_state = Some(self.default_edge_state.clone());
                        self.shadow_store.insert_shadow(shadow).await.map_err(|e| {
                            anyhow::anyhow!("Failed to register edge shadow: {:?}", e)
                        })?;
                        tracing::info!(edge_id = %edge_id, "Auto-registered edge shadow for tag assignment");
                    }
                    self.tag_store
                        .assign_tag(edge_id, tag_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to assign tag: {:?}", e))?;
                    // Publish updated assignments for this edge.
                    let tags = self
                        .tag_store
                        .get_tags_for_edge(edge_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to get tags for edge: {:?}", e))?;
                    TagManagerTask::publish_edge_tags(edge_id, &tags, self.client.clone())
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to publish edge tags: {:?}", e))?;
                    Ok(OperateResp {
                        output: serde_value::to_value(Void {})?,
                    })
                }
                "unassign_tag" => {
                    let edge_id =
                        require_input(&req.inputs, "edge_id", &req.operation_name, &descs)?;
                    let tag_id = require_input(&req.inputs, "tag_id", &req.operation_name, &descs)?;
                    let output = match self.tag_store.unassign_tag(edge_id, tag_id).await {
                        Ok(()) => {
                            // Publish updated assignments for this edge.
                            let tags =
                                self.tag_store
                                    .get_tags_for_edge(edge_id)
                                    .await
                                    .map_err(|e| {
                                        anyhow::anyhow!("Failed to get tags for edge: {:?}", e)
                                    })?;
                            TagManagerTask::publish_edge_tags(edge_id, &tags, self.client.clone())
                                .await
                                .map_err(|e| {
                                    anyhow::anyhow!("Failed to publish edge tags: {:?}", e)
                                })?;
                            let mut map = std::collections::HashMap::new();
                            map.insert("result", "ok".to_string());
                            map.insert("edge_id", edge_id.to_string());
                            map.insert("tag_id", tag_id.to_string());
                            serde_value::to_value(map)
                                .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                ouro_core::error::InternalErrorKind::NoSuchKey
                            ) =>
                        {
                            let mut map = std::collections::HashMap::new();
                            map.insert("result", "error".to_string());
                            map.insert("edge_id", edge_id.to_string());
                            map.insert("tag_id", tag_id.to_string());
                            map.insert("reason", "tag assignment not found".to_string());
                            serde_value::to_value(map)
                                .map_err(|e| anyhow::anyhow!("Serialization error: {:?}", e))?
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("Failed to unassign tag: {:?}", e));
                        }
                    };
                    Ok(OperateResp { output })
                }
                "get_tags_for_edge" => {
                    let edge_id =
                        require_input(&req.inputs, "edge_id", &req.operation_name, &descs)?;
                    let tags = self
                        .tag_store
                        .get_tags_for_edge(edge_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to get tags for edge: {:?}", e))?;
                    Ok(OperateResp {
                        output: serde_value::to_value(tags)?,
                    })
                }
                "preview_merged_config" => {
                    let edge_id =
                        require_input(&req.inputs, "edge_id", &req.operation_name, &descs)?;
                    let tags = self
                        .tag_store
                        .get_tags_for_edge(edge_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to get tags for edge: {:?}", e))?;
                    let patches: Vec<(merge::Priority, EdgeSpecPatch)> = tags
                        .0
                        .iter()
                        .filter_map(|t| Some((t.priority.unwrap_or(0), t.patch.clone()?)))
                        .collect();
                    let merged = merge::merge_edge_spec(&self.default_edge_state, &patches)
                        .map_err(|e| anyhow::anyhow!("Merge failed: {}", e))?;
                    Ok(OperateResp {
                        output: serde_value::to_value(merged)?,
                    })
                }
                "list_all_assignments" => {
                    let assignments =
                        self.tag_store.list_all_assignments().await.map_err(|e| {
                            anyhow::anyhow!("Failed to list all assignments: {:?}", e)
                        })?;
                    Ok(OperateResp {
                        output: serde_value::to_value(assignments)?,
                    })
                }
                _ => Err(unsupported_operation(&req.operation_name, &descs)),
            }
        }

        #[instrument(skip(self, _req))]
        async fn list(&self, _req: Void) -> anyhow::Result<ListResp> {
            Err(anyhow::anyhow!(
                "List operation not supported for '{}'",
                stringify!(TagIccHandler)
            ))
        }
    }

    /// Parse a [`Tag`] from ICC request inputs.
    ///
    /// The `id` field is always generated by `#[serde(default = "uuid_gen")]` during
    /// deserialization. Callers that need a specific ID (e.g. `update_tag`) must override
    /// `tag.id` after calling this function.
    #[instrument(skip(req, descs))]
    fn parse_config_tag(req: &OperateReq, descs: &[Description]) -> anyhow::Result<Tag> {
        let priority: u32 = require_input(&req.inputs, "priority", &req.operation_name, descs)?
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid priority: must be a u32 integer"))?;
        let patch_str = require_input(&req.inputs, "patch", &req.operation_name, descs)?;
        let display_name = req
            .inputs
            .get("display_name")
            .map(|v| v.as_str().to_string());
        let patch: EdgeSpecPatch = serde_json::from_str(patch_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse patch JSON: {:?}", e))?;
        // Omit `id` from the JSON so that `#[serde(default = "uuid_gen")]` on `Tag::id`
        // generates a new UUIDv7 automatically.
        let tag_json = serde_json::json!({
            "display_name": display_name,
            "priority": priority,
            "patch": serde_json::to_value(&patch)
                .map_err(|e| anyhow::anyhow!("Failed to serialize patch: {:?}", e))?,
        });
        tracing::debug!(priority = priority, "Parsing Tag from ICC inputs");
        let tag: Tag = serde_json::from_value(tag_json)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize Tag: {:?}", e))?;
        Ok(tag)
    }
}
