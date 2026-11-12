// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

/// Zenoh key-space constants and key-generation helpers for Device Shadow operations.
///
/// Key hierarchy:
/// - `edge/{edge_id}/config` — desired configuration (daemon → edge)
/// - `edge/{edge_id}/status` — reported status (edge → daemon)
/// - `edge/{edge_id}/status/liveness` — heartbeat (edge → daemon)
/// - `edge/{edge_id}/command` — commands (daemon → edge)
/// - `edge/{edge_id}/log/metadata` — metadata log (edge → daemon)
/// - `edge/{edge_id}/tags` — tag assignments (daemon → edge)
/// - `edge/tag/{tag_id}/config` — tag configuration patch (daemon → edge fleet)
use crate::types::{Topic, TopicPattern};

/// Wildcard pattern matching all edge metadata log keys.
pub const PATTERN_METADATA_LOG: TopicPattern = TopicPattern("edge/*/log/metadata");
/// Generate the Zenoh key for an edge's metadata log.
///
/// # Arguments
/// * `edge_id` - Unique identifier for the edge device
///
/// # Returns
/// A [`Topic`] of the form `edge/{edge_id}/log/metadata`.
#[inline]
pub fn metadata_log_key(edge_id: &str) -> Topic {
    Topic(format!("edge/{}/log/metadata", edge_id))
}

/// Wildcard pattern matching all edge liveness status keys.
pub const PATTERN_LIVENESS_STATUS: TopicPattern = TopicPattern("edge/*/status/liveness");
/// Generate the Zenoh key for an edge's liveness heartbeat.
///
/// # Arguments
/// * `edge_id` - Unique identifier for the edge device
///
/// # Returns
/// A [`Topic`] of the form `edge/{edge_id}/status/liveness`.
#[inline]
pub fn liveness_status_key(edge_id: &str) -> Topic {
    Topic(format!("edge/{}/status/liveness", edge_id))
}

/// Wildcard pattern matching all edge status keys.
pub const PATTERN_STATUS: TopicPattern = TopicPattern("edge/*/status");

/// Generate the Zenoh key for an edge's reported status.
///
/// # Arguments
/// * `edge_id` - Unique identifier for the edge device
///
/// # Returns
/// A [`Topic`] of the form `edge/{edge_id}/status`.
#[inline]
pub fn status_key(edge_id: &str) -> Topic {
    Topic(format!("edge/{}/status", edge_id))
}

/// Generate the Zenoh key for sending a command to an edge.
///
/// # Arguments
/// * `edge_id` - Unique identifier for the edge device
///
/// # Returns
/// A [`Topic`] of the form `edge/{edge_id}/command`.
#[inline]
pub fn command_key(edge_id: &str) -> Topic {
    Topic(format!("edge/{}/command", edge_id))
}

/// Wildcard pattern matching all edge tag assignment keys.
pub const PATTERN_EDGE_TAGS: TopicPattern = TopicPattern("edge/*/tags");

/// Generate the Zenoh key for an edge's tag assignments.
///
/// # Arguments
/// * `edge_id` - Unique identifier for the edge device
///
/// # Returns
/// A [`Topic`] of the form `edge/{edge_id}/tags`.
#[inline]
pub fn edge_tags_key(edge_id: &str) -> Topic {
    Topic(format!("edge/{}/tags", edge_id))
}

/// Wildcard pattern matching all tag configuration keys.
pub const PATTERN_TAG_CONFIG: TopicPattern = TopicPattern("edge/tag/*/config");

/// Generate the Zenoh key for a tag's configuration patch.
///
/// # Arguments
/// * `tag_id` - Configuration tag identifier
///
/// # Returns
/// A [`Topic`] of the form `edge/tag/{tag_id}/config`.
#[inline]
pub fn tag_config_key(tag_id: &str) -> Topic {
    Topic(format!("edge/tag/{}/config", tag_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        assert_eq!(
            command_key("edge-001"),
            Topic("edge/edge-001/command".to_string())
        );
        assert_eq!(
            status_key("edge-001"),
            Topic("edge/edge-001/status".to_string())
        );
        //assert_eq!(command_key("edge-001"), "edge/edge-001/command");
        assert_eq!(
            metadata_log_key("edge-001"),
            Topic("edge/edge-001/log/metadata".to_string())
        );
        assert_eq!(
            liveness_status_key("edge-001"),
            Topic("edge/edge-001/status/liveness".to_string())
        );
    }
}
