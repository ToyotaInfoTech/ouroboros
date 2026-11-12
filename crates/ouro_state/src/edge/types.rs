// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Trait implementations for edge state types.
//!
//! Implements the `State` and `RootState` marker traits for edge
//! configuration types to enable their use in the component lifecycle.

use crate::core::types::{RootState, State};
use crate::edge::{EdgeSpec, EdgeTaskSpec};

impl RootState for EdgeSpec {}
impl State for EdgeSpec {}
impl State for EdgeTaskSpec {}
