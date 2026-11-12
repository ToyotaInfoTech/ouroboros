// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Core runtime framework for the Ouroboros system.
//!
//! Provides the foundational abstractions for building distributed components,
//! including RPC dispatch, runtime lifecycle management, task supervision,
//! and database transaction handling.

pub mod comm;
pub mod error;
pub mod runtime;
#[cfg(feature = "daemon")]
pub mod supervisor;
use std::sync::{LazyLock, Mutex};

use ouro_schema::schema::shadow::report::SystemInfo;
use sqlx::Row as _;
use sysinfo::System;
use tracing::instrument;

use crate::error::{InternalError, InternalErrorKind};

pub type InternalResult<T> = anyhow::Result<T, crate::error::InternalError>;

/// Debug utility: lists all SQLite tables via the provided `Rdb` instance.
///
/// Only executes when debug-level logging is enabled.
///
/// # Arguments
/// * `rdb` - SQLite database connection wrapper
///
/// # Returns
/// * `Ok(())` on success, error if the query fails
#[allow(dead_code)]
#[instrument(skip(rdb))]
pub(crate) async fn show_sqlite_tables(
    rdb: &runtime::rdb::Rdb<sqlx::Sqlite>,
) -> InternalResult<()> {
    if tracing::enabled!(tracing::Level::DEBUG) {
        // Print all created tables for debugging
        let table_names = rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let tables: Vec<String> =
                    sqlx::query("SELECT name FROM sqlite_master WHERE type='table';")
                        .fetch_all(&mut **tx)
                        .await
                        .map_err(|e| ("Failed to fetch SQLite tables", e).into())?
                        .iter()
                        .map(|row| {
                            let table_name: String = row.get(0);
                            table_name
                        })
                        .collect();
                Ok::<Vec<String>, InternalError>(tables)
            })
            .await?
            .commit()
            .await?;

        if let Some(table_names) = table_names {
            tracing::debug!("Existing SQLite tables: {:?}", table_names);
        } else {
            tracing::debug!("No SQLite table exists");
        }
    }
    Ok(())
}

#[instrument]
#[allow(clippy::result_large_err)]
pub fn unix_time_secs() -> InternalResult<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| ("Failed to get UNIX time", e).into())?
        .as_secs())
}

/// Implements `try_from` for converting [`ouro_common::types::AnySharable`] into an `Arc<$type>`.
///
/// # Arguments
/// * `$type` - The target enum type that wraps built components.
///
/// # Example
/// ```ignore
/// impl_try_from_any_sharable!(BuiltDaemonTask);
/// ```
#[macro_export]
macro_rules! impl_try_from_any_sharable {
    ($type:ty) => {
        impl $type {
            /// Attempts to convert an [`ouro_core::AnySharable`] into an `Arc<Self>`.
            ///
            /// # Arguments
            /// * `value` - The [`ouro_core::AnySharable`] to convert.
            ///
            /// # Returns
            /// `Ok(Arc<Self>)` on success, or an [`ouro_core::InternalError`] on failure.
            pub fn try_from_any_sharable(
                value: ouro_common::types::AnySharable,
            ) -> $crate::InternalResult<Arc<Self>> {
                value.downcast::<Self>().map_err(|e| {
                    (
                        format!(
                            "Failed to convert AnySharable to {}: {:?}",
                            stringify!($type),
                            e
                        ),
                        $crate::error::InternalErrorKind::TypeConversion,
                    )
                        .into()
                })
            }
        }
    };
}

#[instrument]
pub fn setup_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let _ = tracing_subscriber::fmt()
        //.with_timer(tracing_subscriber::fmt::time::SystemTime)
        .with_env_filter(filter)
        .with_target(true)
        .with_ansi(true)
        .with_target(true)
        .with_line_number(true)
        .try_init();
}

static SYSTEM: LazyLock<Mutex<System>> = LazyLock::new(|| Mutex::new(System::new_all()));

#[allow(clippy::result_large_err)]
pub fn get_system_info() -> InternalResult<SystemInfo> {
    let mut guard = SYSTEM.lock().map_err(|e| {
        (
            format!("Failed to acquire lock on system info: {e:?}"),
            InternalErrorKind::Lock,
        )
            .into()
    })?;
    guard.refresh_all();
    let system_info = SystemInfo::from(&*guard);
    Ok(system_info)
}
