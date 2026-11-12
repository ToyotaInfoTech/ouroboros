// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

pub mod report;
/// Device Shadow module for configuration distribution and state synchronization.
///
/// This module implements an AWS IoT Device Shadow-like pattern using Zenoh pub/sub:
/// - daemon publishes desired configuration to edges
/// - edges report their current state back to daemon
/// - daemon maintains a shadow (desired + reported) for each edge
#[allow(clippy::module_inception)]
pub mod shadow;
pub mod sync;

pub use self::report::EdgeStateReport;
pub use self::shadow::EdgeShadow;
