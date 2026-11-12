// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Ouroboros Daemon library.
//!
//! Provides infrastructure for the daemon: resource context, agent,
//! and repository management. Task implementations live in `ouro_daemon_task`,
//! which depends on this crate - not the other way around.

pub mod runtime;
