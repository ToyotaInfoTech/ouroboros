// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Topic-based task executor with dynamic worker thread management.
//!
//! This module provides a concurrent executor that processes tasks on a per-topic basis,
//! ensuring that tasks for the same topic are executed serially while tasks for different
//! topics can run in parallel. The executor dynamically manages worker threads based on
//! workload and evicts idle workers and queues to optimize resource usage.

use completion::{future::block_on, CompletionFuture};
use crossbeam::channel as crossbeam;
use num_cpus;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{self, Duration, Instant};
use tokio::sync::oneshot;

// A thread-local variable that stores the last time the thread was used.
thread_local! {
    static LAST_USED: RefCell<Instant> = RefCell::new(Instant::now());
}

/// Checks if the current thread has been idle for longer than the specified duration.
///
/// # Parameters
/// * `idle_worker_eviction_time` - The maximum allowed idle duration
///
/// # Returns
/// `true` if the thread has exceeded the idle threshold.
fn check_thread_idle_time_or_evict(idle_worker_eviction_time: Duration) -> bool {
    LAST_USED.with(|last_used| last_used.borrow().elapsed() > idle_worker_eviction_time)
}

fn thread_name() -> String {
    thread::current()
        .name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// A topic queue that holds tasks for a specific topic.
///
/// Tasks within the same topic are processed serially to maintain order,
/// while different topics can be processed in parallel.
struct TopicQueue<F>
where
    F: CompletionFuture + Send + 'static,
    F::Output: Send + 'static,
{
    /// The topic identifier for this queue
    _topic: String,
    /// Whether a task from this queue is currently being processed
    in_flight: bool,
    /// The queue of pending tasks and their result channels
    queue: VecDeque<(F, oneshot::Sender<F::Output>)>,
    /// The last time this queue was accessed or used
    last_used: time::Instant,
}

impl<F> TopicQueue<F>
where
    F: CompletionFuture + Send + 'static,
    F::Output: Send + 'static,
{
    /// Creates a new topic queue.
    ///
    /// # Parameters
    /// * `topic` - The topic identifier for this queue
    ///
    /// # Returns
    /// A new `TopicQueue` instance initialized with an empty queue
    fn new(topic: String) -> Self {
        TopicQueue {
            _topic: topic,
            in_flight: false,
            queue: VecDeque::new(),
            last_used: time::Instant::now(),
        }
    }

    /// Marks the topic queue as in-flight and updates the last used timestamp.
    ///
    /// This should be called when a task from this queue is dispatched to a worker.
    fn mark_as_used(&mut self) {
        self.in_flight = true;
        self.last_used = time::Instant::now();
    }
}

/// A builder for configuring and creating a topic queue executor.
///
/// Provides a fluent API for customizing executor parameters such as worker thread limits,
/// idle timeouts, and eviction policies.
pub struct TopicQueueExecutorBuilder<F>
where
    F: CompletionFuture + Send + 'static,
    F::Output: Send + 'static,
{
    /// Maximum number of worker threads
    max_workers: usize,
    /// Prefix for worker thread names
    thread_name_prefix: String,
    /// Interval for checking idle worker threads
    idle_worker_check_interval: Duration,
    /// Duration after which idle workers are evicted
    idle_worker_eviction_time: Duration,
    /// Interval for checking idle topic queues
    idle_queue_check_interval: Duration,
    /// Duration after which idle queues are evicted
    idle_queue_eviction_time: Duration,
    /// Phantom data to hold the future type
    phantom_data: PhantomData<F>,
}

#[allow(clippy::new_without_default)]
impl<F> TopicQueueExecutorBuilder<F>
where
    F: CompletionFuture + Send + 'static,
    F::Output: Send + 'static,
{
    /// Creates a new builder with default configuration values.
    ///
    /// # Default values
    /// * `max_workers`: min(32, number of CPUs)
    /// * `thread_name_prefix`: "worker_"
    /// * `idle_worker_check_interval`: 20 seconds
    /// * `idle_worker_eviction_time`: 60 seconds
    /// * `idle_queue_check_interval`: 20 seconds
    /// * `idle_queue_eviction_time`: 60 seconds
    ///
    /// # Returns
    /// A new `TopicQueueExecutorBuilder` instance with default settings
    pub fn new() -> Self {
        TopicQueueExecutorBuilder {
            max_workers: std::cmp::min(32, num_cpus::get()),
            thread_name_prefix: "worker_".to_string(),
            idle_worker_check_interval: Duration::from_secs(20),
            idle_worker_eviction_time: Duration::from_secs(60),
            idle_queue_check_interval: Duration::from_secs(20),
            idle_queue_eviction_time: Duration::from_secs(60),
            phantom_data: PhantomData,
        }
    }

    /// Sets the maximum number of worker threads.
    ///
    /// # Parameters
    /// * `max_workers` - Maximum number of concurrent worker threads
    ///
    /// # Returns
    /// The builder instance for method chaining
    pub fn max_workers(mut self, max_workers: usize) -> Self {
        self.max_workers = max_workers;
        self
    }

    /// Sets the thread name prefix for worker threads.
    ///
    /// # Parameters
    /// * `thread_name_prefix` - Prefix to use for naming worker threads
    ///
    /// # Returns
    /// The builder instance for method chaining
    pub fn thread_name_prefix(mut self, thread_name_prefix: String) -> Self {
        self.thread_name_prefix = thread_name_prefix;
        self
    }

    /// Sets the interval for checking idle worker threads.
    ///
    /// # Parameters
    /// * `idle_worker_check_interval` - Duration between idle worker checks
    ///
    /// # Returns
    /// The builder instance for method chaining
    pub fn idle_worker_check_interval(mut self, idle_worker_check_interval: Duration) -> Self {
        self.idle_worker_check_interval = idle_worker_check_interval;
        self
    }

    /// Sets the eviction time for idle worker threads.
    ///
    /// # Parameters
    /// * `idle_worker_eviction_time` - Duration after which idle workers are terminated
    ///
    /// # Returns
    /// The builder instance for method chaining
    pub fn idle_worker_eviction_time(mut self, idle_worker_eviction_time: Duration) -> Self {
        self.idle_worker_eviction_time = idle_worker_eviction_time;
        self
    }

    /// Sets the interval for checking idle topic queues.
    ///
    /// # Parameters
    /// * `idle_queue_check_interval` - Duration between idle queue checks
    ///
    /// # Returns
    /// The builder instance for method chaining
    pub fn idle_queue_check_interval(mut self, idle_queue_check_interval: Duration) -> Self {
        self.idle_queue_check_interval = idle_queue_check_interval;
        self
    }

    /// Sets the eviction time for idle topic queues.
    ///
    /// # Parameters
    /// * `idle_queue_eviction_time` - Duration after which idle queues are removed
    ///
    /// # Returns
    /// The builder instance for method chaining
    pub fn idle_queue_eviction_time(mut self, idle_queue_eviction_time: Duration) -> Self {
        self.idle_queue_eviction_time = idle_queue_eviction_time;
        self
    }

    /// Builds and initializes the topic queue executor.
    ///
    /// Creates the executor with the configured settings and starts the idle queue watcher thread.
    ///
    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime context.
    ///
    /// # Returns
    /// A fully initialized `TopicQueueExecutor` instance ready to accept tasks
    pub fn build(self) -> TopicQueueExecutor<F> {
        let (worker_tx, worker_rx) = crossbeam::unbounded();
        let topic_queue_executor = TopicQueueExecutor {
            max_workers: self.max_workers,
            thread_name_prefix: self.thread_name_prefix,
            idle_worker_check_interval: self.idle_worker_check_interval,
            idle_worker_eviction_time: self.idle_worker_eviction_time,
            idle_queues_check_interval: self.idle_queue_check_interval,
            idle_queue_eviction_time: self.idle_queue_eviction_time,
            queues: Arc::new(Mutex::new(HashMap::new())),
            worker_tx,
            worker_rx,
            num_workers: Arc::new(Mutex::new(0)),
            num_active_workers: Arc::new(Mutex::new(0)),
            tokio_handle: tokio::runtime::Handle::current(),
        };
        topic_queue_executor.spawn_idle_queue_watcher();
        topic_queue_executor
    }
}

/// An executor that processes tasks for different topics in parallel.
///
/// Tasks for the same topic are executed serially in FIFO order, while tasks for different
/// topics can run concurrently across multiple worker threads. The executor dynamically
/// adjusts the number of worker threads based on workload and evicts idle resources.
#[allow(clippy::type_complexity)]
pub struct TopicQueueExecutor<F>
where
    F: CompletionFuture + Send + 'static,
    F::Output: Send + 'static,
{
    /// Maximum number of concurrent worker threads
    max_workers: usize,
    /// Prefix for naming worker threads
    thread_name_prefix: String,
    /// Interval for checking idle worker threads
    idle_worker_check_interval: Duration,
    /// Duration after which idle workers are evicted
    idle_worker_eviction_time: Duration,
    /// Interval for checking idle topic queues
    idle_queues_check_interval: Duration,
    /// Duration after which idle queues are evicted
    idle_queue_eviction_time: Duration,
    /// Map of topic names to their corresponding task queues
    queues: Arc<Mutex<HashMap<String, Arc<Mutex<TopicQueue<F>>>>>>,
    /// Channel sender for dispatching queues to worker threads
    worker_tx: crossbeam::Sender<Arc<Mutex<TopicQueue<F>>>>,
    /// Channel receiver for worker threads to receive queues
    worker_rx: crossbeam::Receiver<Arc<Mutex<TopicQueue<F>>>>,
    /// Total number of worker threads (active and idle)
    num_workers: Arc<Mutex<usize>>,
    /// Number of worker threads currently processing tasks
    num_active_workers: Arc<Mutex<usize>>,
    /// Tokio runtime handle passed to worker threads so futures that
    /// require a Tokio context (e.g. sqlx pool operations) can run.
    tokio_handle: tokio::runtime::Handle,
}

impl<F> TopicQueueExecutor<F>
where
    F: CompletionFuture + Send + 'static,
    F::Output: Send + 'static,
{
    /// Submits a task to the executor for processing.
    ///
    /// The task will be queued for the specified topic and executed when a worker becomes available.
    /// Tasks for the same topic are processed serially in FIFO order.
    ///
    /// # Parameters
    /// * `topic` - The topic identifier for this task
    /// * `task` - The task to execute
    ///
    /// # Returns
    /// A oneshot receiver that will contain the task's result when it completes
    pub fn submit(
        &self,
        topic: &str,
        task: F,
    ) -> oneshot::Receiver<<F as CompletionFuture>::Output> {
        self.spawn_new_worker_if_necessary();
        self.submit_internal(topic.to_string(), task)
    }

    /// Spawns a new worker thread if necessary.
    ///
    /// A new worker is spawned when all existing workers are busy and the maximum
    /// worker count has not been reached.
    fn spawn_new_worker_if_necessary(&self) {
        let mut num_workers_locked = self.num_workers.lock().expect("num_workers mutex poisoned");
        let num_active_workers_locked = self
            .num_active_workers
            .lock()
            .expect("num_active_workers mutex poisoned");
        if *num_workers_locked == *num_active_workers_locked
            && *num_workers_locked < self.max_workers
        {
            // need more threads
            *num_workers_locked += 1;
            tracing::info!(
                "[{}]: Spawning a new worker thread (total number of workers = {})",
                thread::current().name().unwrap_or("unknown"),
                num_workers_locked
            );
            self.spawn_new_worker();
        };
    }

    /// Internal implementation of task submission.
    ///
    /// # Parameters
    /// * `topic` - The topic identifier for this task
    /// * `task` - The task to execute
    ///
    /// # Returns
    /// A oneshot receiver that will contain the task's result when it completes
    fn submit_internal(
        &self,
        topic: String,
        task: F,
    ) -> oneshot::Receiver<<F as CompletionFuture>::Output> {
        let (tx, rx) = oneshot::channel();

        let topic_queue = self.get_or_create_topic_queue(&topic);
        let mut topic_queue_locked = topic_queue.lock().expect("topic_queue mutex poisoned");
        topic_queue_locked.queue.push_back((task, tx));
        if !topic_queue_locked.in_flight {
            topic_queue_locked.mark_as_used();
            self.worker_tx
                .send(topic_queue.clone())
                .expect("worker channel closed");
        }

        rx
    }

    /// Returns the topic queue for the given topic, creating a new one if it does not exist.
    ///
    /// # Parameters
    /// * `topic` - The topic identifier
    ///
    /// # Returns
    /// An Arc-wrapped mutex-protected TopicQueue for the specified topic
    fn get_or_create_topic_queue(&self, topic: &str) -> Arc<Mutex<TopicQueue<F>>> {
        let mut queues_locked = self.queues.lock().expect("queues mutex poisoned");
        queues_locked
            .entry(topic.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(TopicQueue::<F>::new(topic.to_string()))))
            .clone()
    }

    /// Spawns a background thread that periodically checks and evicts idle topic queues.
    ///
    /// The watcher runs continuously and removes queues that have been idle for longer
    /// than the configured eviction time to free up resources.
    fn spawn_idle_queue_watcher(&self) {
        let queues = self.queues.clone();
        let idle_queues_check_interval = self.idle_queues_check_interval;
        let idle_queue_eviction_time = self.idle_queue_eviction_time;
        thread::Builder::new()
            .name("idle-queue-watcher".to_string())
            .spawn(move || {
                tracing::info!("\t\x1b[32mstarted: [{}]\x1b[0m", thread_name());
                loop {
                    thread::sleep(idle_queues_check_interval);
                    let mut queues_locked = queues.lock().expect("queues mutex poisoned");
                    let mut victim_queue_topics = vec![];
                    for (topic, topic_queue) in queues_locked.iter() {
                        let topic_queue_locked =
                            topic_queue.lock().expect("topic_queue mutex poisoned");
                        if !topic_queue_locked.in_flight
                            && topic_queue_locked.last_used.elapsed() > idle_queue_eviction_time
                        {
                            victim_queue_topics.push(topic.clone());
                        }
                    }
                    if !victim_queue_topics.is_empty() {
                        tracing::info!(
                            "[{}]: Idle queue watcher: evicted idle queues: {:?}",
                            thread_name(),
                            victim_queue_topics
                        );
                        for victim_queue_topic in victim_queue_topics {
                            queues_locked.remove(&victim_queue_topic);
                        }
                    }
                }
            })
            .expect("failed to spawn idle-queue-watcher thread");
    }

    /// Spawns a new worker thread to process tasks from topic queues.
    ///
    /// The worker thread runs in a loop, waiting for topic queues to be dispatched via
    /// the worker channel. It processes tasks from queues and automatically evicts itself
    /// after being idle for the configured duration.
    fn spawn_new_worker(&self) {
        let (worker_tx, worker_rx) = (self.worker_tx.clone(), self.worker_rx.clone());
        let num_workers = self.num_workers.clone();
        let num_active_workers = self.num_active_workers.clone();
        let idle_worker_check_interval = self.idle_worker_check_interval;
        let idle_worker_eviction_time = self.idle_worker_eviction_time;
        let tokio_handle = self.tokio_handle.clone();
        thread::Builder::new()
            .name(format!("{}{}", self.thread_name_prefix, time::SystemTime::now().duration_since(time::UNIX_EPOCH).expect("system clock before UNIX epoch").as_micros()))
            .spawn(move || {
                tracing::info!(
                    "\t\x1b[32mstarted: [{}]\x1b[0m",
                    thread_name()
                );
                loop {
                    // Dequeue the topic queue from the worker receiver
                    let topic_queue = {
                        loop {
                            match worker_rx.recv_timeout(idle_worker_check_interval) {
                                Ok(topic_queue) => break topic_queue,
                                Err(_) => { // timeout
                                    if check_thread_idle_time_or_evict(idle_worker_eviction_time) {
                                        let mut num_workers_locked = num_workers.lock().expect("num_workers mutex poisoned");
                                        *num_workers_locked -= 1;
                                        tracing::debug!("[{}]: A worker thread is evicted due to inactivity; (total number of workers = {})", thread_name(), num_workers_locked);
                                        return;
                                    }
                                }
                            }
                        }
                    };

                    // Increment the number of active workers
                    {
                        let mut num_active_workers_locked = num_active_workers.lock().expect("num_active_workers mutex poisoned");
                        *num_active_workers_locked += 1;
                        tracing::debug!("[{}]: Worker thread started working (total number of active workers = {})", thread_name(), *num_active_workers_locked);
                    }

                    // Dequeue the task from the topic queue
                    let (task, tx) = {
                        let mut topic_queue_locked = topic_queue.lock().expect("topic_queue mutex poisoned");
                        topic_queue_locked.queue.pop_front().expect("topic_queue must not be empty when dispatched")
                    };

                    // Execute the task within the Tokio runtime context so that
                    // futures requiring a Tokio context (e.g. sqlx pool ops) work correctly.
                    match tx.send(tokio_handle.block_on(async move { block_on(task) })) {
                        Ok(_) => {
                            tracing::debug!("[{}]: Sent task result for {}", thread_name(), topic_queue.lock().expect("topic_queue mutex poisoned")._topic);
                        }
                        Err(_) => {
                            tracing::debug!("[{}]: Sending task result failed", thread_name());
                        }
                    }

                    // Decrement the number of active workers
                    {
                        let mut num_active_workers_locked = num_active_workers.lock().expect("num_active_workers mutex poisoned");
                        *num_active_workers_locked -= 1;
                        tracing::debug!("[{}]: Worker thread stopped working (total number of active workers = {})", thread_name(), *num_active_workers_locked);
                    }

                    // Enqueue the topic queue if there are more tasks to be processed
                    {
                        let mut topic_queue_locked = topic_queue.lock().expect("topic_queue mutex poisoned");
                        if topic_queue_locked.queue.is_empty() {
                            topic_queue_locked.in_flight = false;
                        } else {
                            topic_queue_locked.mark_as_used();
                            tracing::debug!("[{}]: Enqueuing topic queue for further processing", thread_name());
                            worker_tx.send(topic_queue.clone()).expect("worker channel closed");
                        }
                    }
                }
            }).expect("failed to spawn executor worker thread");
    }
}

#[cfg(test)]
mod tests {

    /// Test the executor module
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_executor() {
        use crate::runtime::executor::TopicQueueExecutorBuilder;
        use completion::completion_async_move;
        use rand::prelude::*;
        use std::thread::sleep;
        use std::time::Duration;

        tracing::info!("Creating executor");
        let executor = TopicQueueExecutorBuilder::new()
            .thread_name_prefix("worker-".to_string())
            .idle_worker_check_interval(Duration::from_millis(500))
            .idle_worker_eviction_time(Duration::from_millis(1000))
            .idle_queue_check_interval(Duration::from_millis(500))
            .idle_queue_eviction_time(Duration::from_millis(1000))
            .build();

        let mut handles = vec![];
        for task_id in 0..3usize {
            for topic_id in 0..5usize {
                //let handle: JoinHandle<Result<(usize, usize, String), (usize, usize, String)>> =
                let handle = executor.submit(format!("topic-{}", topic_id).as_str(), completion_async_move! {
                    tracing::info!("[{}.{}]: task started", topic_id, task_id);
                    sleep(Duration::from_secs(rand::rng().random_range(0..3)));
                    //sleep(Duration::from_secs(rand::thread_rng().gen_range(1..=3)));
                    let result = if rand::rng().random_range(0..2) == 0 {
                        Ok((topic_id, task_id, format!("[{}.{}]: Hello, world!", topic_id, task_id).to_string()))
                    } else {
                        Err((topic_id, task_id, format!("[{}.{}]: Error!", topic_id, task_id).to_string())) // Convert &str to String
                    };
                    tracing::info!("[{}.{}]: task ended", topic_id, task_id);
                    result
                });
                handles.push(handle);
            }
        }

        for handle in handles {
            match handle.await {
                Ok(result) => {
                    tracing::info!("Result: {:?}", result);
                }
                Err(value) => {
                    tracing::error!("Error: {:?}", value);
                }
            }
        }
    }
}
