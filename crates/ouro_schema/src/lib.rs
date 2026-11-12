// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Schema definitions for the Ouroboros system.
//!
//! This crate provides data models and serialization types used across
//! the system, including device shadow state, trajectory telemetry,
//! and event metadata.

use serde::{Deserialize, Serialize};

pub mod runtime;
pub mod schema;

/// A unit-like type for RPC endpoints that carry no request body.
#[derive(Debug, Serialize, Deserialize)]
pub struct Void;
