// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Task supervision and resilient restart mechanisms.
//!
//! This module provides utilities for running tasks under supervision with automatic
//! restart capabilities. It ensures that long-running tasks continue to operate even
//! when they fail or panic, implementing a supervisor pattern with configurable backoff.
//!
//! # Features
//!
//! - **Automatic Restart**: Failed tasks are automatically restarted after a configurable delay
//! - **Concurrency Control**: Uses semaphores to limit concurrent task execution
//! - **Panic Recovery**: Gracefully handles panicked tasks and restarts them
//! - **Backoff Strategy**: Configurable delay between restart attempts to prevent resource exhaustion
//!
//! # Example
//!
//! ```ignore
//! use ouro_core::supervisor::spawn_resilient;
//! use std::sync::Arc;
//! use std::sync::atomic::{AtomicUsize, Ordering};
//!
//! # async fn example() {
//! let counter = Arc::new(AtomicUsize::new(0));
//!
//! // Spawn a task that will automatically restart on failure
//! spawn_resilient({
//!     let counter = counter.clone();
//!     move || {
//!         let counter = counter.clone();
//!         async move {
//!             // Task logic here
//!             counter.fetch_add(1, Ordering::SeqCst);
//!         }
//!     }
//! });
//! # }
//! ```

use std::future::Future;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

use crate::error::panic_if_env_set;
use crate::InternalResult;

/// Run a future factory once with semaphore-based concurrency control.
///
/// Acquires a permit from the semaphore before executing the future created by
/// the factory function. This ensures controlled concurrent execution when multiple
/// tasks share the same semaphore.
///
/// # Arguments
/// * `semaphore` - Semaphore controlling concurrent execution
/// * `make_future` - Factory function that creates the future to execute
///
/// # Returns
/// * `Result<()>` - Ok on successful execution, Err if semaphore acquisition fails
///
/// # Errors
/// Returns an error if the semaphore is closed or acquisition fails.
async fn run_once<FutureFactory, Fut, OkValue>(
    sem: Arc<Semaphore>,
    make_future: Arc<FutureFactory>,
) -> InternalResult<()>
where
    FutureFactory: Fn() -> InternalResult<Fut> + Send + Sync + 'static,
    Fut: Future<Output = InternalResult<OkValue>> + Send + 'static,
    OkValue: Send + Sync + 'static,
{
    let _permit = sem
        .acquire_owned()
        .await
        .map_err(|e| ("Failed to acquire semaphore", e).into())?;
    (make_future)()?.await?;
    Ok(())
}

/// Runs a task under a restart supervisor.
///
/// Note that a `Future` value is single-use: once awaited, it cannot be awaited again.
/// Therefore this function takes a factory `make_future` which is called on every restart.
///
/// # Arguments
/// * `make_future` - Factory function that creates a new Future on each restart
/// * `backoff_time` - Delay in seconds between restart attempts
#[allow(dead_code)] // API
pub async fn respawn_on_crash<FutureFactory, Fut, OkValue>(
    make_future: FutureFactory,
    backoff_time: u64,
) -> !
where
    FutureFactory: Fn() -> InternalResult<Fut> + Send + Sync + 'static,
    Fut: Future<Output = InternalResult<OkValue>> + Send + 'static,
    OkValue: Send + Sync + 'static,
{
    let make_future = Arc::new(make_future);
    let sem = Arc::new(Semaphore::new(1));

    loop {
        let sem = sem.clone();
        let make_future = make_future.clone();
        let handle = tokio::spawn(async move { run_once(sem, make_future).await });

        match handle.await {
            Ok(Ok(())) => {
                // Normal completion (break if you want to stop; otherwise continue to restart periodically)
            }
            Ok(Err(e)) => {
                tracing::error!("worker error: {e:?}");
                panic_if_env_set(&e);
            }
            Err(e) => {
                // Panic/cancellation/etc.
                tracing::error!("worker panicked/cancelled: {e:?}");
                panic_if_env_set(&e);
            }
        }

        // Simple backoff to avoid runaway restarts
        sleep(Duration::from_secs(backoff_time)).await;
    }
}

/// Spawns a resilient task that automatically restarts on failure with default 1-second backoff.
///
/// # Arguments
/// * `factory` - Factory function that creates a new Future on each restart
///
/// # Returns
/// * `JoinHandle<()>` - Handle to the spawned supervisor task
///
/// # Examples
///
/// ```ignore
/// use ouro_core::supervisor::spawn_resilient;
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicUsize, Ordering};
///
/// # async fn example() {
/// let counter = Arc::new(AtomicUsize::new(0));
///
/// spawn_resilient({
///     let counter = counter.clone();
///     move || {
///         let counter = counter.clone();
///         async move {
///             counter.fetch_add(1, Ordering::SeqCst);
///         }
///     }
/// });
/// # }
/// ```
#[allow(dead_code)] // API
pub fn spawn_resilient<FutureFactory, Fut, OkValue>(make_future: FutureFactory) -> JoinHandle<()>
where
    FutureFactory: Fn() -> InternalResult<Fut> + Send + Sync + 'static,
    Fut: Future<Output = InternalResult<OkValue>> + Send + 'static,
    OkValue: Send + Sync + 'static,
{
    spawn_resilient_with_backoff(make_future, 1)
}

/// Spawns a resilient task that automatically restarts on failure with custom backoff.
///
/// # Arguments
/// * `factory` - Factory function that creates a new Future on each restart
/// * `backoff_secs` - Number of seconds to wait before restarting a failed task
///
/// # Returns
/// * `JoinHandle<()>` - Handle to the spawned supervisor task
///
/// # Examples
///
/// ```ignore
/// use ouro_core::supervisor::spawn_resilient_with_backoff;
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicUsize, Ordering};
///
/// # async fn example() {
/// let counter = Arc::new(AtomicUsize::new(0));
///
/// spawn_resilient_with_backoff({
///     let counter = counter.clone();
///     move || {
///         let counter = counter.clone();
///         async move {
///             counter.fetch_add(1, Ordering::SeqCst);
///         }
///     }
/// }, 5);
/// # }
/// ```
pub fn spawn_resilient_with_backoff<FutureFactory, Fut, OkValue>(
    make_future: FutureFactory,
    backoff_secs: u64,
) -> JoinHandle<()>
where
    FutureFactory: Fn() -> InternalResult<Fut> + Send + Sync + 'static,
    Fut: Future<Output = InternalResult<OkValue>> + Send + 'static,
    OkValue: Send + Sync + 'static,
{
    // Intentionally detach the spawned supervisor task.
    tokio::spawn(async move {
        respawn_on_crash(make_future, backoff_secs).await;
    })
}

pub async fn get_locked<'a, M>(mutex: &'a Mutex<M>) -> MutexGuard<'a, M> {
    mutex.lock().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::result_large_err)]
    async fn semaphore_limits_concurrency_to_one() {
        let semaphore = Arc::new(Semaphore::new(1));
        let current = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));

        let make_future = {
            let current = current.clone();
            let max = max.clone();
            move || {
                let current = current.clone();
                let max = max.clone();
                Ok(async move {
                    let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(now, Ordering::SeqCst);

                    // Hold the critical section long enough that the second task would overlap
                    // if the semaphore were not enforced.
                    tokio::time::sleep(Duration::from_millis(25)).await;

                    current.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
            }
        };

        let make_future = Arc::new(make_future);

        let t1 = run_once(semaphore.clone(), make_future.clone());
        let t2 = run_once(semaphore.clone(), make_future.clone());
        let (r1, r2) = tokio::join!(t1, t2);

        r1.expect("t1 should succeed");
        r2.expect("t2 should succeed");

        assert_eq!(
            max.load(Ordering::SeqCst),
            1,
            "semaphore should prevent concurrent execution"
        );
    }

    #[tokio::test]
    #[allow(clippy::result_large_err)]
    async fn run_once_errors_when_semaphore_is_closed() {
        let semaphore = Arc::new(Semaphore::new(1));
        semaphore.close();

        let make_future = Arc::new(|| Ok(async { Ok(()) }));
        let err = run_once(semaphore, make_future)
            .await
            .expect_err("acquire should fail for a closed semaphore");

        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "error should have a non-empty display message"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::result_large_err)]
    async fn with_supervisor_restarts_task_multiple_times() {
        use tokio::time::timeout;

        let runs = Arc::new(AtomicUsize::new(0));

        // Intentionally detach the spawned supervisor task.
        let _handle = spawn_resilient_with_backoff(
            {
                let runs = runs.clone();
                move || {
                    let runs = runs.clone();
                    Ok(async move {
                        runs.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                }
            },
            0,
        );

        // Wait until the task has been started multiple times.
        timeout(Duration::from_millis(200), async {
            while runs.load(Ordering::SeqCst) < 3 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("supervisor should restart the task quickly");

        assert!(
            runs.load(Ordering::SeqCst) >= 3,
            "expected multiple restarts, got {}",
            runs.load(Ordering::SeqCst)
        );
    }
}
