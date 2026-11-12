// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use ouro_common::io::{FromCbor, ToCbor};
use serde::{Deserialize, Serialize};

use ouro_state::core::TagAssignment;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeStateSyncIntent {
    pub tag_assignment: TagAssignment,
}

impl ToCbor for EdgeStateSyncIntent {}
impl FromCbor for EdgeStateSyncIntent {}
