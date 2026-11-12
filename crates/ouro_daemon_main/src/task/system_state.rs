// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! System state tracking for edge devices.
//!
//! Maintains last update timestamps for edge devices to track their
//! activity and connectivity status.

use ouro_core::InternalResult;
use sqlx::{postgres as pg, Transaction};
use tracing::instrument;

/// SQL queries for system state management.
pub(crate) mod sql {
    #[allow(dead_code)] // API
    pub(crate) const UPDATE_LAST_UPDATE: &str = r#"
        INSERT INTO system_state(vin, last_update)
        VALUES ($1, NOW())
        ON CONFLICT (vin) DO UPDATE SET last_update = EXCLUDED.last_update;
    "#;
}

/// Marks the last update timestamp for a given VIN.
///
/// # Arguments
///
/// * `tx` - Database transaction
/// * `vin` - Vehicle Identification Number
///
/// # Returns
///
/// * `Result<()>` - Ok on success, error otherwise
#[instrument(skip(tx, edge_id))]
pub(crate) async fn mark_update(
    tx: &mut Transaction<'_, pg::Postgres>,
    edge_id: &str,
) -> InternalResult<()> {
    sqlx::query(sql::UPDATE_LAST_UPDATE)
        .bind(edge_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| ("Failed to mark system state update", e).into())?;
    Ok(())
}
