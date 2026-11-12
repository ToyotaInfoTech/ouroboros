// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use ouro_common::types::UnixTimestamp;
/// Metrics probing module for collecting and reporting performance metrics.
///
/// This module provides abstractions for tracking various performance metrics in an asynchronous context.
/// It includes probes for throughput (operations per second, error rates) and latency measurements.
///
/// # Architecture
///
/// The module follows a trait-based design where:
/// - `Metrics<T>` trait defines the common interface for reporting metrics of type `T`
/// - Concrete probe implementations (`Throughput`, `Latency`) provide specific metric collection strategies
/// - Metrics are reported to an `InMemoryMetricsProvider` for aggregation and retrieval
///
/// # Components
///
/// - **`Metrics<T>`**: Core trait for reporting metrics of a generic type `T`
/// - **`Throughput`**: Tracks operation success/failure rates and calculates throughput over sampling intervals
/// - **`Latency`**: Records operation latency measurements in milliseconds
///
/// # Usage
///
/// ```rust,ignore
/// use std::sync::Arc;
/// use crate::metrics::provider::InMemoryMetricsProvider;
/// use crate::metrics::probe::{Throughput, Latency, Metrics};
///
/// let provider = Arc::new(InMemoryMetricsProvider::new(100));
///
/// // Track throughput
/// let mut throughput = Throughput::new("api_", time::Duration::seconds(5), provider.clone());
/// throughput.start();
/// throughput.report_one(true).await; // Report success
///
/// // Track latency
/// let latency = Latency::new("api_", provider.clone());
/// latency.report_one(time::Duration::milliseconds(150)).await;
/// ```
///
/// # Thread Safety
///
/// All probe implementations are designed to work safely across async tasks:
/// - Atomic counters are used for concurrent increment operations
/// - Mutex guards are held only for minimal critical sections
/// - No locks are held across `.await` points
///
/// # Metrics Emitted
///
/// **Throughput**:
/// - `{name}ok_per_sec`: Successful operations per second
/// - `{name}total_per_sec`: Total operations per second
/// - `{name}error_ratio`: Ratio of failed operations (0.0 to 1.0)
///
/// **Latency**:
/// - `{name}latency_ms`: Operation latency in milliseconds
use tokio::sync::Mutex;

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use crate::runtime::types::BuiltMetricsProvider;
use provider::MetricsProvider;
use serde::{Deserialize, Serialize};

/// A single metric data point with time and value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    /// Unix time in milliseconds.
    pub time: UnixTimestamp,
    /// Metric value (can be any numeric type).
    pub value: f64,
}

/// Metric data with name and time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    /// Metric identifier (e.g., "throughput", "vehicle_count").
    pub name: String,
    /// Time series data points.
    pub points: Vec<MetricPoint>,
}

/// Core trait for reporting metrics of a generic type.
///
/// Implementations define how to collect and report specific metric types
/// (e.g., boolean success/failure, latency durations) to a metrics provider.
#[async_trait::async_trait]
pub trait Metrics<T> {
    /// Report a single metric value.
    ///
    /// # Arguments
    /// * `value` - The metric value to report
    async fn report_one(&self, value: T);
}

/// Throughput probe for tracking operation throuput, and so on.
///
/// Collects operation results (success/failure) and periodically calculates:
/// - Operations per second (total and successful)
/// - Error ratio
///
/// Metrics are flushed to the provider when the sampling interval elapses.
pub struct Throughput<T>
where
    T: store::MetricsStore + Send + Sync,
{
    /// Metric name prefix.
    name: String,
    /// Count of successful operations.
    ok_count: Arc<AtomicU64>,
    /// Count of failed operations.
    err_count: Arc<AtomicU64>,
    /// Interval between metric flushes.
    sampling_interval: time::Duration,
    /// Start time of current sampling period.
    start_time: Mutex<std::time::Instant>,
    /// Metrics store for storing results.
    metrics_store: Arc<T>,
}

impl<T> Throughput<T>
where
    T: store::MetricsStore + Send + Sync,
{
    /// Create a new throughput probe.
    ///
    /// # Arguments
    /// * `name` - Metric name prefix (e.g., "api_" produces "api_ok_per_sec")
    /// * `sampling_interval` - Duration between metric flushes
    /// * `metrics_store` - Provider to store calculated metrics
    ///
    /// # Returns
    /// * `Throughput` - New throughput probe (call `start()` before use)
    pub fn new(name: &str, sampling_interval: time::Duration, metrics_store: Arc<T>) -> Self {
        Self {
            name: name.to_string(),
            ok_count: Arc::new(AtomicU64::new(0)),
            err_count: Arc::new(AtomicU64::new(0)),
            sampling_interval,
            start_time: Mutex::new(std::time::Instant::now()),
            metrics_store,
        }
    }

    /// Start the throughput probe.
    ///
    /// Initializes the sampling period start time to the current time.
    /// Must be called before reporting metrics.
    pub fn start(&mut self) {
        *self.start_time.get_mut() = std::time::Instant::now();
    }
}

#[async_trait::async_trait]
impl<T> Metrics<bool> for Throughput<T>
where
    T: store::MetricsStore + Send + Sync,
{
    async fn report_one(&self, is_ok: bool) {
        if is_ok {
            self.ok_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.err_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        let now = std::time::Instant::now();

        // Lock only to read/update start_time; do not hold the lock across .await.
        let (should_flush, elapsed_secs) = {
            let mut start = self.start_time.lock().await;
            let elapsed = now - *start;
            let secs = elapsed.as_secs_f64();

            if secs >= self.sampling_interval.as_seconds_f64() {
                *start = now;
                (true, secs)
            } else {
                (false, secs)
            }
        };

        if !should_flush {
            return;
        }

        let ok = self.ok_count.swap(0, Ordering::Relaxed);
        let err = self.err_count.swap(0, Ordering::Relaxed);
        let total = ok + err;

        let ok_count_per_sec = ok as f64 / elapsed_secs;
        let count_per_sec = total as f64 / elapsed_secs;
        let err_ratio = if total > 0 {
            err as f64 / total as f64
        } else {
            0.0
        };

        self.metrics_store
            .add_point(&format!("{}ok_per_sec", self.name), ok_count_per_sec)
            .await;
        self.metrics_store
            .add_point(&format!("{}total_per_sec", self.name), count_per_sec)
            .await;
        self.metrics_store
            .add_point(&format!("{}error_ratio", self.name), err_ratio)
            .await;
    }
}

/// Latency probe for recording operation duration measurements.
///
/// Collects operation latencies and reports them as millisecond values
/// to the metrics provider.
pub struct Latency<T>
where
    T: store::MetricsStore,
{
    /// Metric name prefix.
    name: String,
    /// Metrics store for storing measurements.
    metrics_store: Arc<T>,
}

impl<T> Latency<T>
where
    T: store::MetricsStore,
{
    /// Create a new latency probe.
    ///
    /// # Arguments
    /// * `name` - Metric name prefix (e.g., "api_" produces "api_latency_ms")
    /// * `metrics_store` - Provider to store latency measurements
    ///
    /// # Returns
    /// * `Latency` - New latency probe ready for use
    pub fn new(name: &str, metrics_store: Arc<T>) -> Self {
        Self {
            name: name.to_string(),
            metrics_store,
        }
    }
}

#[async_trait::async_trait]
impl<T> Metrics<Duration> for Latency<T>
where
    T: store::MetricsStore + Send + Sync,
{
    async fn report_one(&self, latency: Duration) {
        self.metrics_store
            .add_point(
                &format!("{}latency_ms", self.name),
                latency.as_secs_f64() * 1000.0,
            )
            .await;
    }
}

#[async_trait]
impl MetricsProvider for BuiltMetricsProvider {
    async fn get_metric_names(&self) -> Vec<String> {
        match self {
            BuiltMetricsProvider::Transient(provider) => provider.get_metric_names().await,
        }
    }

    async fn get_metric(&self, name: &str, max_points: usize) -> Option<Vec<MetricPoint>> {
        match self {
            BuiltMetricsProvider::Transient(provider) => {
                provider.get_metric(name, max_points).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_throughput_probe() {
        let store = Arc::new(transient::TransientMetricsStore::new(Some(100)));
        let provider = transient::TransientMetricsProvider::new(store.clone());
        let mut throughput = Throughput::new(
            "test_throughput_",
            time::Duration::seconds(1),
            store.clone(),
        );
        throughput.start();

        // Report some successful and failed operations
        for _ in 0..10 {
            throughput.report_one(true).await;
        }
        for _ in 0..5 {
            throughput.report_one(false).await;
        }

        // Wait to exceed the sampling interval
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Report more operations to trigger flush
        for _ in 0..20 {
            throughput.report_one(true).await;
        }
        for _ in 0..10 {
            throughput.report_one(false).await;
        }

        // Allow some time for async metric updates
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify metrics have been recorded
        let ok_count_points = provider
            .get_metric("test_throughput_ok_per_sec", 10)
            .await
            .expect("test_throughput_ok_per_sec metric should exist");
        let count_points = provider
            .get_metric("test_throughput_total_per_sec", 10)
            .await
            .expect("test_throughput_total_per_sec metric should exist");
        let err_ratio_points = provider
            .get_metric("test_throughput_error_ratio", 10)
            .await
            .expect("test_throughput_error_ratio metric should exist");

        assert!(!ok_count_points.is_empty());
        assert!(!count_points.is_empty());
        assert!(!err_ratio_points.is_empty());
    }

    #[tokio::test]
    async fn test_latency_probe() {
        let store = Arc::new(transient::TransientMetricsStore::new(Some(100)));
        let provider = transient::TransientMetricsProvider::new(store.clone());
        let latency = Latency::new("test_latency_", store.clone());

        // Report some latency measurements
        latency.report_one(Duration::from_millis(150)).await;
        latency.report_one(Duration::from_millis(200)).await;

        // Allow some time for async metric updates
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify metrics have been recorded
        let latency_points = provider
            .get_metric("test_latency_latency_ms", 10)
            .await
            .expect("test_latency_latency_ms metric should exist");

        assert_eq!(latency_points.len(), 2);
        assert_eq!(latency_points[0].value, 150.0);
        assert_eq!(latency_points[1].value, 200.0);
    }
}

pub mod provider {
    use super::*;

    /// Trait for implementing custom metrics providers
    ///
    /// Implement this trait to provide your own metrics sources.
    /// The provider can fetch metrics from any source: local state,
    /// databases, external APIs, etc.
    ///
    /// # Example
    /// ```ignore
    /// use ouro_core::runtime::metrics::provider::MetricsProvider;
    /// use ouro_core::runtime::metrics::provider::{Metric, MetricPoint};
    /// use ouro_core::runtime::metrics::server::JsonHttpMetricsServer;
    /// use std::sync::Arc;
    ///
    /// // Define your custom metrics provider
    /// struct MyMetricsProvider;
    ///
    /// #[async_trait::async_trait]
    /// impl MetricsProvider for MyMetricsProvider {
    ///     async fn get_metric_names(&self) -> Vec<String> {
    ///         vec!["my_metric".to_string()]
    ///     }
    ///     async fn get_metric(&self, name: &str, max_points: usize) -> Vec<MetricPoint>> {
    ///         Some(vec![])
    ///     }
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() -> InternalResult<(), Box<dyn std::error::Error>> {
    ///     let provider = Arc::new(MyMetricsProvider);
    ///     let server = JsonHttpMetricsServer::new(provider, "0.0.0.0:8080");
    ///     server.run().await?;
    ///     Ok(())
    /// }
    /// ```
    #[async_trait::async_trait]
    pub trait MetricsProvider {
        /// Get all available metric names.
        ///
        /// # Returns
        /// * `Vec<String>` - List of metric identifiers
        async fn get_metric_names(&self) -> Vec<String>;

        /// Get time series data for a specific metric.
        ///
        /// # Arguments
        /// * `name` - Metric identifier
        /// * `max_points` - Maximum number of data points to return
        ///
        /// # Returns
        /// * `Vec<MetricPoint>>` - Metric data or None if not found
        async fn get_metric(&self, name: &str, max_points: usize) -> Option<Vec<MetricPoint>>;

        /// Get latest values for all metrics.
        ///
        /// # Returns
        /// * `Vec<Metric>` - All metrics with their time series
        async fn get_all_metrics(&self, max_points: usize, prefix: Option<&str>) -> Vec<Metric> {
            let mut metrics = Vec::new();
            for name in self.get_metric_names().await {
                if let Some(prefix) = prefix {
                    if !name.starts_with(prefix) {
                        continue;
                    }
                }
                let points = self.get_metric(&name, max_points).await;
                if let Some(points) = points {
                    metrics.push(Metric { name, points });
                }
            }
            metrics
        }
    }
}

pub mod store {
    use super::*;

    #[async_trait::async_trait]
    pub trait MetricsStore {
        async fn add_point(&self, name: &str, value: f64);

        fn new_provider(&self) -> Arc<BuiltMetricsProvider>;
    }
}

pub mod transient {
    use super::*;
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::RwLock;
    /// Example in-memory metrics provider for testing
    ///
    /// This stores metrics in memory with a circular buffer.
    #[derive(Debug, Clone)]
    pub struct TransientMetricsStore {
        metrics: Arc<RwLock<HashMap<String, Vec<MetricPoint>>>>,
        max_points_per_metric: usize,
    }

    impl TransientMetricsStore {
        /// Create a new in-memory provider.
        ///
        /// # Arguments
        /// * `max_points_per_metric` - Maximum points to store per metric
        ///
        /// # Returns
        /// * `TransientMetricsContainer - New provider instance
        pub fn new(max_points_per_metric: Option<usize>) -> Self {
            let max_points_per_metric = max_points_per_metric.unwrap_or(1000);
            Self {
                metrics: Arc::new(RwLock::new(std::collections::HashMap::new())),
                max_points_per_metric,
            }
        }
    }

    #[async_trait::async_trait]
    impl store::MetricsStore for TransientMetricsStore {
        /// Add a data point to a metric.
        ///
        /// # Arguments
        /// * `name` - Metric identifier
        /// * `value` - Metric value
        async fn add_point(&self, name: &str, value: f64) {
            let name = name.into();
            let point = MetricPoint {
                time: UnixTimestamp::default(),
                value,
            };

            let mut metrics = self.metrics.write().await;
            let points = metrics.entry(name).or_insert_with(Vec::new);
            points.push(point);

            // Keep only the most recent points
            if points.len() > self.max_points_per_metric {
                points.drain(0..points.len() - self.max_points_per_metric);
            }
        }

        fn new_provider(&self) -> Arc<BuiltMetricsProvider> {
            Arc::new(BuiltMetricsProvider::Transient(Arc::new(
                TransientMetricsProvider::new(Arc::new(self.clone())),
            )))
        }
    }

    #[derive(Debug, Clone)]
    pub struct TransientMetricsProvider {
        store: Arc<TransientMetricsStore>,
    }

    impl TransientMetricsProvider {
        /// Create a new in-memory metrics provider.
        ///
        /// # Arguments
        /// * `max_points_per_metric` - Maximum points to store per metric
        ///
        /// # Returns
        /// * `TransientMetricsProvider - New provider instance
        pub fn new(store: Arc<TransientMetricsStore>) -> Self {
            Self { store }
        }
    }

    #[async_trait]
    impl MetricsProvider for TransientMetricsProvider {
        async fn get_metric_names(&self) -> Vec<String> {
            let metrics = self.store.metrics.read().await;
            metrics.keys().cloned().collect()
        }

        async fn get_metric(&self, name: &str, max_points: usize) -> Option<Vec<MetricPoint>> {
            let metrics = self.store.metrics.read().await;
            metrics.get(name).map(|points| {
                let start = if points.len() > max_points {
                    points.len() - max_points
                } else {
                    0
                };
                points[start..].to_vec()
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_in_memory_provider() {
            let store = Arc::new(TransientMetricsStore::new(Some(100)));

            // Add some test data
            use store::MetricsStore;
            store.add_point("test_metric", 42.0).await;
            store.add_point("test_metric", 43.0).await;

            let provider = TransientMetricsProvider::new(store);

            // Verify we can retrieve it
            let points = provider.get_metric("test_metric", 10).await;
            assert!(points.is_some());
            assert_eq!(points.unwrap().len(), 2);

            // Verify metric names
            let names = provider.get_metric_names().await;
            assert_eq!(names.len(), 1);
            assert!(names.contains(&"test_metric".to_string()));
        }
    }
}
