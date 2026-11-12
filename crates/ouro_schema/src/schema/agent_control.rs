// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use ouro_common::{ansi_color, AnsiColor};
use ouro_state::{daemon::DaemonSpec, edge::EdgeSpec};
use serde::{Deserialize, Serialize};

use crate::schema::Message;

pub type AgentCommand = Message<AgentCommandKind>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RootStateKind {
    Daemon(DaemonSpec),
    Edge(EdgeSpec),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentCommandKind {
    Start,
    Suspend,
    Reconfigure(Box<RootStateKind>),
    Shutdown,
    Restart,
}

impl std::fmt::Display for AgentCommandKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentCommandKind::Start => write!(f, "{}", ansi_color(AnsiColor::Blue, "START")),
            AgentCommandKind::Suspend => write!(f, "{}", ansi_color(AnsiColor::Yellow, "SUSPEND")),
            AgentCommandKind::Reconfigure(_) => {
                write!(f, "{}", ansi_color(AnsiColor::Yellow, "RECONFIGURE"))
            }
            AgentCommandKind::Shutdown => write!(f, "{}", ansi_color(AnsiColor::Red, "SHUTDOWN")),
            AgentCommandKind::Restart => write!(f, "{}", ansi_color(AnsiColor::Magenta, "RESTART")),
        }
    }
}
