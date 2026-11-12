// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Ouroboros Edge binary.
//!
//! The edge component runs on individual devices, communicating with
//! the daemon and handling local operations.

pub mod gen {
    #[allow(
        unused_extern_crates,
        clippy::missing_safety_doc,
        clippy::unused_unit,
        clippy::extra_unused_lifetimes
    )]
    pub mod peer_generated;
    #[allow(
        unused_extern_crates,
        clippy::missing_safety_doc,
        clippy::unused_unit,
        clippy::extra_unused_lifetimes
    )]
    pub mod rpc_generated;
}
pub mod runtime;
