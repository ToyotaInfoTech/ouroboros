// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Device shadow and tag-based configuration management for edge devices.
//!
//! This module provides a device shadow implementation that maintains the state
//! of edge devices, including their desired configurations and reported states.
//! It supports multiple storage backends including transient (in-memory),
//! persistent (database), and write-behind caching strategies.
//!
//! Tag-based configuration management allows fleet-wide configuration via
//! priority-ordered patches that are merged on the edge side.

pub mod manager;
pub mod persistent;
pub mod store;
pub mod tag_manager;
pub mod tag_persistent;
pub mod tag_store;
pub mod transient;
pub mod write_behind;
