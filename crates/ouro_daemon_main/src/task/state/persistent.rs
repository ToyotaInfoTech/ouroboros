// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Database-backed persistent storage for device shadows.
//!
//! Provides durable storage of device shadow state using PostgreSQL.
//! All shadow operations are persisted to the database, ensuring state
//! survives process restarts.

use async_trait::async_trait;
use derivative::Derivative;
use ouro_common::types::UnixTimestamp;
use ouro_core::error::InternalErrorKind;
use ouro_core::runtime::rdb;
use ouro_core::InternalResult;
use ouro_schema::schema::shadow::{EdgeShadow, EdgeStateReport};
use ouro_state::edge::EdgeSpec;
use sqlx::Row as _;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

use crate::task::state::store;

/// SQL query to insert or update a device shadow.
const INSERT_OR_UPDATE: &str = r#"
    INSERT INTO device_shadows (edge_id, desired_state, state_report, last_seen)
    VALUES ($1, $2, $3, $4)
    ON CONFLICT (edge_id) DO UPDATE SET
    desired_state = $2, state_report = $3, last_seen = $4
"#;
/// SQL query to retrieve a single device shadow by edge ID.
const GET: &str =
    "SELECT desired_state, state_report, last_seen FROM device_shadows WHERE edge_id = $1";
/// SQL query to retrieve all device shadows.
#[allow(dead_code)] // Reserved for bulk shadow retrieval
const GET_ALL: &str = "SELECT edge_id, desired_state, state_report, last_seen FROM device_shadows";
/// SQL query to update a device shadow.
#[allow(dead_code)] // Reserved for partial shadow update
const UPDATE: &str = r#"
    UPDATE device_shadows 
    SET desired_state = $1, state_report = $2, last_seen = $3 
    WHERE edge_id = $4
"#;
/// SQL query to delete a device shadow.
#[allow(dead_code)] // Reserved for shadow deletion
const DELETE: &str = "DELETE FROM device_shadows WHERE edge_id = $1";

/// Database-backed persistent shadow store.
///
/// Stores device shadows in PostgreSQL for durable state management.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct PersistentShadowStore
where
    Self: Send + Sync,
{
    pub name: String,
    enabled: bool,
    /// Database connection pool for PostgreSQL operations.
    #[derivative(Debug = "ignore")]
    rdb: Arc<rdb::Rdb<sqlx::postgres::Postgres>>,
}

impl PersistentShadowStore {
    /// Creates a new PersistentShadowStore instance.
    ///
    /// # Arguments
    ///
    /// * `rdb` - Arc-wrapped database connection pool for PostgreSQL
    ///
    /// # Returns
    ///
    /// A new PersistentShadowStore instance
    #[instrument(skip(name, rdb))]
    pub(crate) fn new(name: String, rdb: Arc<rdb::Rdb<sqlx::postgres::Postgres>>) -> Self {
        Self {
            name,
            enabled: false,
            rdb,
        }
    }
}

#[async_trait]
impl store::ShadowStore for PersistentShadowStore {
    /// Inserts or updates a device shadow in the database.
    ///
    /// If a shadow with the given edge_id already exists, it will be updated.
    /// Otherwise, a new shadow will be created.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    /// * `shadow` - Device shadow data to store
    ///
    /// # Returns
    ///
    /// * `Ok(())` on success
    /// * `Err` if database operation fails
    #[instrument(skip(self, shadow))]
    async fn insert_shadow(&self, shadow: EdgeShadow) -> InternalResult<()> {
        let rdb = self.rdb.as_ref();
        rdb.transaction(|tx| async move {
            sqlx::query(INSERT_OR_UPDATE)
                .bind(&shadow.edge_id)
                .bind(
                    serde_json::to_value(&shadow.desired_state)
                        .map_err(|e| ("Failed to serialize desired_state", e).into())?,
                )
                .bind(
                    serde_json::to_value(&shadow.last_report)
                        .map_err(|e| ("Failed to serialize last_report", e).into())?,
                )
                .bind(time::OffsetDateTime::now_utc())
                .execute(&mut **tx)
                .await
                .map_err(|e| ("Failed to insert/update device shadow", e).into())?;
            Ok(())
        })
        .await?;

        Ok(())
    }

    /// Retrieves a device shadow from the database.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device
    ///
    /// # Returns
    ///
    /// * `Ok(Some(DeviceShadow))` if the shadow exists
    /// * `Ok(None)` if the shadow does not exist
    /// * `Err` if database operation fails
    ///
    /// # Note
    ///
    /// The `last_seen` field in the returned shadow is set to the current time,
    /// ignoring the value stored in the database.
    #[instrument(skip(self, edge_id))]
    async fn get_shadow(&self, edge_id: &str) -> InternalResult<Option<EdgeShadow>> {
        // Fetch from DB
        let shadow = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let rows = sqlx::query(GET)
                    .bind(edge_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;

                if let Some(row) = rows {
                    let desired_state: Option<EdgeSpec> = serde_json::from_value(row.get(0)).ok();
                    let last_report: Option<EdgeStateReport> =
                        serde_json::from_value(row.get(1)).ok();

                    let shadow = EdgeShadow {
                        edge_id: edge_id.to_string(),
                        last_report,
                        desired_state,
                        last_seen: UnixTimestamp::now(), // ignore the last_seen from DB
                    };

                    Ok(Some(shadow))
                } else {
                    Ok(None)
                }
            })
            .await?
            .commit()
            .await?;

        if let Some(Some(shadow)) = shadow {
            Ok(Some(shadow))
        } else {
            Ok(None)
        }
    }

    #[instrument(skip(self))]
    async fn list_shadow_ids(&self) -> InternalResult<Vec<String>> {
        let shadow_ids = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let rows = sqlx::query("SELECT edge_id FROM device_shadows")
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;

                let ids = rows.into_iter().map(|row| row.get(0)).collect();
                Ok(ids)
            })
            .await?
            .commit()
            .await?;

        Ok(shadow_ids.unwrap_or_default())
    }

    /// Retrieves all device shadows from the database with optional filtering.
    ///
    /// # Arguments
    ///
    /// * `filters` - Array of filter conditions to apply:
    ///   - `Filter::Offline(duration)` - Selects devices that haven't been seen for the specified duration
    ///   - `Filter::OutOfSync` - Selects devices where desired config version doesn't match reported version
    ///
    /// # Returns
    ///
    /// * `Ok(Some(HashMap))` containing all matching shadows keyed by edge_id
    /// * `Ok(None)` if no shadows match the filters
    /// * `Err` if database operation fails
    ///
    /// # Note
    ///
    /// The `last_seen` field in returned shadows is set to the current time,
    /// ignoring values stored in the database.
    #[instrument(skip(self, filters))]
    async fn get_shadow_all(
        &self,
        filters: &[store::Filter],
    ) -> InternalResult<HashMap<String, EdgeShadow>> {
        let shadows = self
            .rdb
            .begin()
            .await?
            .pipe(|tx, _| async move {
                let mut sql = GET_ALL.to_string();

                for (index, filter) in filters.iter().enumerate() {
                    if index == 0 {
                        sql.push_str(" WHERE "); // Use WHERE for the first separater
                    } else {
                        sql.push_str(" AND "); // Use AND for the subsequent separater
                    }
                    match filter {
                        store::Filter::Offline(_) => {
                            sql.push_str(&format!(
                                "last_seen < NOW() - ${} * INTERVAL '1 second'",
                                index + 1
                            ));
                        }
                        store::Filter::OutOfSync => {
                            sql.push_str(
                                "desired_state->'metadata'->'version' != state_report->'metadata'->'config_version'",
                            );
                        }
                    }
                }

                let mut query = sqlx::query(&sql);

                for filter in filters {
                    match filter {
                        store::Filter::Offline(offline_timeout) => {
                            query = query.bind(offline_timeout.as_secs() as i64);
                        }
                        store::Filter::OutOfSync => ()
                    }
                }

                let rows = query
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(|e| ("DB query error", e).into())?;
                let mut shadows = HashMap::new();
                for row in rows {
                    let edge_id: String = row.get(0);
                    let desired_state = row.get(1);
                    let state_report = row.get(2);
                    let desired_state: Option<EdgeSpec> = serde_json::from_value(desired_state).ok();
                    let last_report: Option<EdgeStateReport> = serde_json::from_value(state_report).ok();

                    let shadow = EdgeShadow {
                        edge_id: edge_id.clone(),
                        desired_state,
                        last_report,
                        last_seen: UnixTimestamp::now(), // ignore the last_seen from DB
                    };

                    shadows.insert(edge_id.clone(), shadow.clone());
                }
                Ok(shadows)
            })
            .await?
            .commit()
            .await?;

        Ok(shadows.unwrap_or_default())
    }

    /// Updates an existing device shadow in the database.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device to update
    /// * `updated_shadow` - New shadow data to replace the existing shadow
    ///
    /// # Returns
    ///
    /// * `Ok(())` on success
    /// * `Err` if the shadow does not exist or database operation fails
    #[instrument(skip(self, updated_shadow))]
    async fn update_shadow(&self, updated_shadow: EdgeShadow) -> InternalResult<()> {
        self.rdb
            .transaction(|tx| async move {
                let result = sqlx::query(UPDATE)
                    .bind(
                        serde_json::to_value(&updated_shadow.desired_state)
                            .map_err(|e| ("Failed to serialize desired_state", e).into())?,
                    )
                    .bind(
                        serde_json::to_value(&updated_shadow.last_report)
                            .map_err(|e| ("Failed to serialize last_report", e).into())?,
                    )
                    .bind(time::OffsetDateTime::now_utc())
                    .bind(&updated_shadow.edge_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to update device shadow", e).into())?;
                if result.rows_affected() == 0 {
                    return Err((
                        "No device shadow found with edge_id",
                        InternalErrorKind::Runtime,
                    )
                        .into());
                }
                Ok(())
            })
            .await?;

        Ok(())
    }

    /// Deletes a device shadow from the database.
    ///
    /// # Arguments
    ///
    /// * `edge_id` - Unique identifier for the edge device to delete
    ///
    /// # Returns
    ///
    /// * `Ok(())` on success
    /// * `Err(InternalErrorKind::NoKey)` if the shadow did not exist
    /// * `Err` if database operation fails
    #[instrument(skip(self, edge_id))]
    async fn delete_shadow(&self, edge_id: &str) -> InternalResult<()> {
        let edge_id_owned = edge_id.to_string();
        self.rdb
            .transaction(|tx| async move {
                let result = sqlx::query(DELETE)
                    .bind(&edge_id_owned)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ("Failed to delete device shadow", e).into())?;
                if result.rows_affected() == 0 {
                    return Err((
                        format!("Shadow not found: '{}'", edge_id_owned),
                        InternalErrorKind::NoSuchKey,
                    )
                        .into());
                }
                Ok(())
            })
            .await?;
        Ok(())
    }
}
