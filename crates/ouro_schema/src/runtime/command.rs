// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use ouro_common::io::{FromCbor, ToCbor};

use crate::schema::agent_control::AgentCommand;

impl ToCbor for AgentCommand {}
impl FromCbor for AgentCommand {}
