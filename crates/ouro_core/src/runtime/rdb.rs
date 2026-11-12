// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Database transaction management module.
//!
//! This module provides abstractions for managing database transactions
//! with SQLx, including transaction context and result chaining.
//!
//! # Examples
//!
//! ```rust,ignore
//! use crate::rdb::Rdb;
//! use sqlx::{Sqlite, SqlitePool};
//!
//! #[tokio::main]
//! async fn main() -> InternalResult<(), anyhow::Error> {
//!     let pool = SqlitePool::connect("sqlite::memory:").await?;
//!     let rdb = Rdb::<Sqlite>::new(pool, vec![]);
//!
//!     // Chain operations with different return types using pipe()
//!     let count: Option<i64> = rdb
//!         .begin()
//!         .await?
//!         .pipe(|tx, _input| async move {
//!             sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY)")
//!                 .execute(&mut **tx)
//!                 .await?;
//!             Ok(())
//!         })
//!         .await?
//!         .pipe(|tx, _input| async move {
//!             let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
//!                 .fetch_one(&mut **tx)
//!                 .await?;
//!             Ok(count)
//!         })
//!         .await?
//!         .commit()
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

use sqlx::{Executor, IntoArguments};
use tokio::sync::OnceCell;

use crate::{error::InternalErrorKind, InternalResult};

/// Database wrapper providing transaction management.
///
/// Wraps a SQLx connection pool and provides methods to begin transactions
/// with result chaining support.
///
/// # Type Parameters
/// * `T` - Database type (e.g., `Sqlite`, `Postgres`)
#[derive(Debug, Clone)]
pub struct Rdb<T>
where
    T: sqlx::Database,
{
    /// The underlying SQLx connection pool.
    pool: sqlx::pool::Pool<T>,
    /// SQL statements executed once on first use to initialize the schema.
    init_sql: Vec<String>,
    init_mark: OnceCell<()>,
}

impl<T> Rdb<T>
where
    T: sqlx::Database,
    for<'c> &'c mut T::Connection: Executor<'c, Database = T>,
    for<'a> <T as sqlx::Database>::Arguments<'a>: IntoArguments<'a, T>,
{
    /// Create a new Rdb instance with the given connection pool.
    ///
    /// # Arguments
    /// * `pool` - SQLx connection pool for database access
    /// * `init_sql` - SQL statements to execute once on first transaction
    ///
    /// # Returns
    /// A new `Rdb` instance wrapping the provided pool.
    pub(crate) fn new(pool: sqlx::pool::Pool<T>, init_sql: Vec<String>) -> Self {
        Self {
            pool,
            init_sql,
            init_mark: OnceCell::new(),
        }
    }

    /// Ensure initialization SQL has been executed exactly once.
    ///
    /// Runs all `init_sql` statements within a single transaction on the
    /// first call. Subsequent calls are no-ops.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if any initialization statement fails.
    async fn ensure_initialized(&self) -> InternalResult<()> {
        self.init_mark
            .get_or_try_init(|| async {
                let mut tx = self.pool.begin().await?;
                for init_sql in &self.init_sql {
                    sqlx::query::<T>(init_sql).execute(&mut *tx).await?;
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!("Executed init SQL: {}", init_sql);
                    }
                }
                tx.commit().await?;
                Ok::<(), sqlx::Error>(())
            })
            .await
            .map_err(|e| ("Failed to initialize database", e).into())?;
        Ok(())
    }

    /// Begin a new transaction.
    ///
    /// Creates a new transaction context with no initial input value.
    /// Ensures initialization SQL has been executed before starting.
    ///
    /// # Returns
    /// A `TxCtx` wrapping the new transaction.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if initialization or transaction creation fails.
    pub async fn begin(&self) -> InternalResult<TxCtx<T>> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("begin<entry>");
        }
        self.ensure_initialized().await?;
        let tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ("Failed to begin transaction", e).into())?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("begin<exit>");
        }
        Ok(TxCtx {
            tx: Some(Box::new(tx)),
            value: None,
        })
    }

    /// Execute a transaction with auto-commit.
    ///
    /// Convenience method that begins a transaction, executes the provided
    /// closure, and automatically commits on success or rolls back on error.
    ///
    /// # Arguments
    /// * `func` - Async closure that performs database operations within the transaction
    ///
    /// # Errors
    /// Returns an error if beginning, executing, or committing the transaction fails.
    pub async fn transaction<F, Fut>(&self, func: F) -> InternalResult<()>
    where
        F: FnOnce(&'static mut sqlx::Transaction<'static, T>) -> Fut,
        Fut: std::future::Future<Output = InternalResult<()>>,
    {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("transaction<entry>");
        }
        self.ensure_initialized().await?;
        self.begin()
            .await?
            .pipe(|tx, _| func(tx))
            .await?
            .commit()
            .await?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("transaction<exit>");
        }
        Ok(())
    }

    /// Returns a reference to the underlying connection pool.
    pub fn pool(&self) -> &sqlx::pool::Pool<T> {
        &self.pool
    }

    /// Close the database connection pool.
    ///
    /// Gracefully closes all connections in the pool. This should be called
    /// before shutting down the application to ensure proper cleanup.
    ///
    /// # Errors
    /// Returns an error if closing the pool fails.
    #[allow(dead_code)] // API
    pub async fn close(&self) -> InternalResult<()> {
        self.pool.close().await;
        Ok(())
    }
}

/// Transaction context with optional input value.
///
/// Holds an active database transaction and an optional input value that can
/// be passed between chained operations via [`pipe`](TxCtx::pipe).
///
/// # Type Parameters
/// * `T` - Database type (e.g., `Sqlite`, `Postgres`)
/// * `I` - Input type carried between chained operations (default: `()`)
pub struct TxCtx<T: sqlx::Database, I = ()> {
    /// The active transaction, wrapped in an `Option` so it can be taken on commit/rollback.
    pub(crate) tx: Option<Box<sqlx::Transaction<'static, T>>>,
    /// The value produced by the most recent pipe operation.
    pub(crate) value: Option<I>,
}

impl<T: sqlx::Database, I> TxCtx<T, I>
where
    I: Clone,
{
    /// Commit the transaction and return the stored value.
    ///
    /// Consumes the transaction context, commits the underlying database
    /// transaction, and returns the value accumulated during the pipe chain.
    ///
    /// # Returns
    /// The stored value wrapped in `Option`, or `None` if no pipe was called.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if:
    /// - The transaction has already been consumed
    /// - The database commit operation fails
    pub async fn commit(mut self) -> InternalResult<Option<I>> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("commit<entry>");
        }
        let tx = self
            .tx
            .take()
            .ok_or_else(|| ("Transaction already consumed", InternalErrorKind::Protocol).into())?;
        tx.commit()
            .await
            .map_err(|e| ("Failed to commit transaction", e).into())?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("commit<exit>");
        }
        Ok(self.value)
    }

    /// Rollback the transaction.
    ///
    /// Consumes the transaction context and discards all changes made within
    /// the transaction.
    ///
    /// # Errors
    /// Returns `sqlx::Error` if:
    /// - The transaction has already been consumed
    /// - The database rollback operation fails
    #[allow(dead_code)] // API
    pub async fn rollback(mut self) -> InternalResult<()> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("rollback<entry>");
        }
        let tx = self
            .tx
            .take()
            .ok_or_else(|| ("Transaction already consumed", InternalErrorKind::Protocol).into())?;
        tx.rollback()
            .await
            .map_err(|e| ("Failed to rollback transaction", e).into())?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("rollback<exit>");
        }
        Ok(())
    }

    /// Execute a chainable operation and transform the carried value.
    ///
    /// Passes the active transaction and the current value to `func`, replaces
    /// the stored value with the closure's return value, and returns a new
    /// `TxCtx` parameterized by the output type.
    ///
    /// # Arguments
    /// * `func` - Async closure receiving a mutable transaction reference and
    ///   the current optional value, returning the next value.
    ///
    /// # Returns
    /// A new `TxCtx<T, O>` carrying the output of `func`.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The transaction has already been consumed
    /// - The closure execution fails
    ///
    /// # Safety
    /// Uses unsafe code to extend the transaction lifetime to `'static`. The
    /// transaction reference remains valid because it is owned by the `TxCtx`
    /// struct throughout the operation and the lifetime extension is confined
    /// to the closure's scope.
    pub async fn pipe<F, Fut, O>(mut self, func: F) -> InternalResult<TxCtx<T, O>>
    where
        F: FnOnce(&'static mut sqlx::Transaction<'static, T>, Option<I>) -> Fut,
        Fut: std::future::Future<Output = InternalResult<O>>,
    {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("pipe<entry>");
        }
        let tx_ref = self
            .tx
            .as_deref_mut()
            .ok_or_else(|| ("Transaction already consumed", InternalErrorKind::Protocol).into())?;

        // SAFETY: The transaction is heap-allocated in `self.tx` (Box) and lives
        // for the duration of this method. The 'static reference does not escape
        // beyond `func`, which is awaited before `self.tx` is moved into the new
        // TxCtx, so the pointee is never invalidated while the reference is live.
        let tx_ptr = tx_ref as *mut sqlx::Transaction<'static, T>;
        let tx_static: &'static mut sqlx::Transaction<'static, T> = unsafe { &mut *tx_ptr };

        let output = func(tx_static, self.value).await?;

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("pipe<exit>");
        }
        Ok(TxCtx {
            tx: self.tx,
            value: Some(output),
        })
    }
}
