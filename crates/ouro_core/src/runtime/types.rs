// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use crate::error::{InternalError, InternalErrorKind};
use crate::runtime::{client, rdb};
#[cfg(feature = "daemon")]
use crate::runtime::{metrics, server};
use crate::InternalResult;
use derivative::Derivative;
#[cfg(feature = "daemon")]
use ouro_schema::schema::cli::Description;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{interval_at, Instant, Interval, MissedTickBehavior};

#[derive(Derivative)]
#[derivative(Debug)]
pub enum BuiltDataSource {
    /// In-memory or persistent file-backed SQLite database.
    SqliteDb(#[derivative(Debug = "ignore")] Arc<rdb::Rdb<sqlx::Sqlite>>),
    /// Temporary file-backed SQLite database.
    ///
    /// The `Arc<tempfile::NamedTempFile>` keeps the temporary file alive for as
    /// long as this `BuiltDataSource` (or any clone of its wrapping `Arc`) is
    /// held. Once the last reference is dropped, the file is deleted by the OS.
    SqliteTempDb(
        #[derivative(Debug = "ignore")] Arc<rdb::Rdb<sqlx::Sqlite>>,
        #[derivative(Debug = "ignore")] Arc<tempfile::NamedTempFile>,
    ),
    #[cfg(feature = "daemon")]
    PostgresDb(#[derivative(Debug = "ignore")] Arc<rdb::Rdb<sqlx::Postgres>>),
    ZenohSession(#[derivative(Debug = "ignore")] Arc<client::CallableDataSource>),
}

/// Implements `TryFrom<&$built>` for `Arc<$target>` by matching one or more enum variants.
///
/// This macro provides ergonomic extraction of a concrete implementation from a
/// shared, enum-typed component while preserving shared ownership semantics.
/// The conversion is implemented for `&$built` (not `Arc<$built>`), so callers can
/// borrow the enum and attempt extraction without moving or cloning it.
/// When a matching variant is found, the inner `&Arc<$target>` is cloned and returned.
///
/// # Parameters
/// - `$built`: The enum type that holds the target `Arc` variant.
/// - `$variant`: A comma-separated list of one or more variant paths that each
///   contain the target `Arc`, enclosed in `[...]`.  When multiple variants are
///   listed the generated match arm uses `|` to accept any of them.
/// - `$target`: The inner target type wrapped by `Arc`.
/// - `$error`: The error type returned on variant mismatch.
/// - `$error_variant`: The `InternalErrorKind` variant used to construct the error.
/// - `$expected`: A human-readable description of the expected variant(s), included in the error.
///
/// # Returns
/// `Arc<$target>` — a cloned shared pointer to the underlying implementation when the variant matches.
#[macro_export]
macro_rules! impl_try_from_built_component {
    ($built:ty, [$($variant:path),+], $target:ty, $error:ty, $error_variant:path, $expected:expr) => {
        impl TryFrom<&$built> for Arc<$target> {
            type Error = $error;

            fn try_from(built: &$built) -> anyhow::Result<Self, Self::Error> {
                match built {
                    $($variant(arc_target, ..))|+ => Ok(arc_target.clone()),
                    #[allow(unreachable_patterns)]
                    _ => Err(($expected, $error_variant).into()),
                }
            }
        }
    };
}

impl_try_from_built_component!(
    BuiltDataSource,
    [BuiltDataSource::SqliteDb, BuiltDataSource::SqliteTempDb],
    rdb::Rdb<sqlx::Sqlite>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "SqliteDb or SqliteTempDb is expected"
);

#[cfg(feature = "daemon")]
impl_try_from_built_component!(
    BuiltDataSource,
    [BuiltDataSource::PostgresDb],
    rdb::Rdb<sqlx::Postgres>,
    InternalError,
    InternalErrorKind::TypeConversion,
    "PostgresDb is expected"
);

impl_try_from_built_component!(
    BuiltDataSource,
    [BuiltDataSource::ZenohSession],
    client::CallableDataSource,
    InternalError,
    InternalErrorKind::TypeConversion,
    "ZenohSession is expected"
);

impl BuiltDataSource {
    /// Returns the inner `Arc<Rdb<Sqlite>>` for `SqliteDb` and `SqliteTempDb` variants.
    ///
    /// # Errors
    /// Returns [`InternalErrorKind::TypeConversion`] if the variant is neither.
    #[allow(clippy::result_large_err)]
    pub fn as_sqlite(&self) -> InternalResult<Arc<rdb::Rdb<sqlx::Sqlite>>> {
        Arc::try_from(self)
    }

    /// Returns the inner `Arc<Rdb<Postgres>>` for the `PostgresDb` variant.
    ///
    /// # Errors
    /// Returns [`InternalErrorKind::TypeConversion`] if the variant is not `PostgresDb`.
    #[cfg(feature = "daemon")]
    #[allow(clippy::result_large_err)]
    pub fn as_postgres(&self) -> InternalResult<Arc<rdb::Rdb<sqlx::Postgres>>> {
        Arc::try_from(self)
    }
}

#[derive(Debug, Clone)]
pub enum BuiltClient {
    Basic(Arc<client::BasicClient>),
    Retryable(Arc<client::RetryableClient>),
}

impl_try_from_built_component!(
    BuiltClient,
    [BuiltClient::Basic],
    client::BasicClient,
    InternalError,
    InternalErrorKind::TypeConversion,
    "BasicClient is expected"
);

impl_try_from_built_component!(
    BuiltClient,
    [BuiltClient::Retryable],
    client::RetryableClient,
    InternalError,
    InternalErrorKind::TypeConversion,
    "RetryableClient is expected"
);

impl BuiltClient {
    /// Returns the inner `Arc<RetryableClient>` for the `Retryable` variant.
    ///
    /// # Errors
    /// Returns [`InternalErrorKind::TypeConversion`] if the variant is not `Retryable`.
    #[allow(clippy::result_large_err)]
    pub fn as_retryable(&self) -> InternalResult<Arc<client::RetryableClient>> {
        Arc::try_from(self)
    }

    /// Returns the inner `Arc<BasicClient>` for the `Basic` variant.
    ///
    /// # Errors
    /// Returns [`InternalErrorKind::TypeConversion`] if the variant is not `Basic`.
    #[allow(clippy::result_large_err)]
    pub fn as_basic(&self) -> InternalResult<Arc<client::BasicClient>> {
        Arc::try_from(self)
    }
}

/// A built server component, unified as an enum over concrete server implementations.
///
/// Each variant wraps its implementation in an `Arc` to enable shared ownership
/// across the runtime without cloning the underlying server.
#[cfg(feature = "daemon")]
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub enum BuiltServer {
    /// A multi-protocol server capable of handling multiple transport types.
    MultiProtocolServer(#[derivative(Debug = "ignore")] Arc<server::Server>),
}

#[cfg(feature = "daemon")]
impl_try_from_built_component!(
    BuiltServer,
    [BuiltServer::MultiProtocolServer],
    server::Server,
    InternalError,
    InternalErrorKind::TypeConversion,
    "MultiProtocolServer is expected"
);

#[cfg(feature = "daemon")]
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub enum BuiltServerEndpoint {
    HttpServerEndpoint(#[derivative(Debug = "ignore")] Arc<server::HttpServerEndpoint>),
}

#[cfg(feature = "daemon")]
impl_try_from_built_component!(
    BuiltServerEndpoint,
    [BuiltServerEndpoint::HttpServerEndpoint],
    server::HttpServerEndpoint,
    InternalError,
    InternalErrorKind::TypeConversion,
    "HttpServerEndpoint is expected"
);

/// A pinned, heap-allocated future representing a single executable task.
///
/// An `Action` is expected to close over any external values it needs and
/// produce an [`InternalResult<()>`] when awaited.
pub type Action = std::pin::Pin<Box<dyn Future<Output = InternalResult<()>> + Send>>;

/// A clonable factory that produces a fresh [`Action`] future on each call.
///
/// Wrapping the closure in [`Arc`] allows the factory to be shared and reused
/// across loop iterations without consuming or re-capturing the underlying resources.
pub type ActionFactory = Arc<dyn Fn() -> Action + Send + Sync>;

/// A clonable factory that produces a fresh [`SelectableAction`] future on each call.
///
/// Unlike [`ActionFactory`], the returned future first suspends at an async wait point
/// (e.g., waiting for an incoming request or a channel message) and only then
/// yields the [`Action`] to be executed. This enables the factory to be composed
/// inside `tokio::select!` alongside other event sources.
pub type SelectableActionFactory = Arc<dyn Fn() -> SelectableAction + Send + Sync>;

#[derive(Clone)]
pub enum SharableAction
where
    Self: Send,
{
    /// A task whose [`Action`] is available immediately, without any async wait.
    ///
    /// Use this when the action does not need to wait for an external event
    /// and should be dispatched as soon as it is polled.
    Instant(ActionFactory),
    /// An async event-driven task that suspends until an event fires, then produces an [`Action`].
    ///
    /// The factory creates a fresh [`SelectableAction`] on each loop iteration so that
    /// the async wait (e.g., `recv_async`, `accept`) can be polled inside `tokio::select!`
    /// concurrently with other event sources such as status-change notifications.
    Event(SelectableActionFactory),
    /// Syntactic sugar for a task that fires on a fixed time interval.
    ///
    /// This is a pure factory definition: it carries no runtime state.
    /// Runtime state (the live `tokio::time::Interval`) is allocated when a
    /// `SharableAction::Periodic` is wrapped in a [`LiveAction`].
    Periodic {
        interval_secs: u64,
        factory: ActionFactory,
    },
    /// A named, user-visible operation exposed through the CLI or RPC interface.
    ///
    /// Associates a human-readable [`Description`] with a [`SelectableActionFactory`]
    /// so that the operation can be listed, invoked, and dispatched by name.
    #[cfg(feature = "daemon")]
    Operation {
        description: Description,
        factory: SelectableActionFactory,
    },
}

/// Constructs a [`SharableAction::Event`] that captures resources, waits for a trigger, then runs an action.
///
/// This macro is the primary ergonomic interface for defining event-driven tasks.
/// It follows a three-part DSL syntax:
///
/// ```text
/// define_resource_trigger_action!(
///     with { <resource bindings> }
///     when: <trigger expression>,
///     then: <action expression>
/// )
/// ```
///
/// # Sections
///
/// - **`with { ... }`** — Zero or more Rust statements that bind resources (e.g., clones of
///   `Arc`s) into the factory closure. These bindings are captured by the outer `move` closure
///   and made available to the inner `async` block.
/// - **`when: <stmt>`** — A statement executed inside the `async` block to wait for the
///   triggering event (e.g., `let result = rx.recv().await`). The statement may produce a
///   value that the action expression can reference.
/// - **`then: <expr>`** — An expression that evaluates to an [`Action`] future. It is pinned
///   and returned from the selectable future after the trigger fires.
///
/// # Returns
///
/// A [`SharableAction::Event`] wrapping a [`SelectableActionFactory`]. Each call to the factory
/// produces a fresh [`SelectableAction`] that suspends at the `when` wait point and resolves
/// to the `then` action.
///
/// # Example
///
/// ```rust,ignore
/// let action = define_resource_trigger_action!(
///     with {
///         let rx = rx.clone();
///     }
///     when: let msg = rx.recv().await,
///     then: handle(msg)
/// );
/// ```
#[macro_export]
macro_rules! define_resource_trigger_action {
    (
        with { $( $resource:tt )* }
        when: $trigger:stmt,
        then: $action:expr
    ) => {
        SharableAction::Event(Arc::new(move || {
            // The factory captures any necessary resources and the trigger statement,
            // returning a future that waits for the trigger before executing the action.
            $(
                $resource // prepare the resources to be captured by the async block
            )*
            #[allow(clippy::async_yields_async)]
            Box::pin(async move {
                $trigger // wait for the trigger condition (e.g., an incoming request or a timer tick)
                Box::pin($action) as $crate::runtime::types::Action // return the action future to be executed after the trigger fires
            })
        }))
    }
}

pub type SelectableAction = Pin<Box<dyn Future<Output = Action> + Send>>;

/// Runtime instance of a [`SharableAction`].
///
/// `SharableAction` is a pure factory descriptor with no runtime state.
/// `LiveAction` wraps it and, for `Periodic` variants, allocates a
/// per-instance `Arc<Mutex<Option<Interval>>>` that persists across loop
/// iterations without requiring the factory itself to carry live state.
///
/// Create one `LiveAction` per action from the result of `gather_actions()`;
/// clone it each loop iteration and convert via `Into<SelectableAction>`.
#[derive(Clone)]
pub struct LiveAction {
    inner: SharableAction,
    /// Shared interval state for `Periodic` actions; `None` for all others.
    interval: Option<Arc<Mutex<Option<Interval>>>>,
}

impl LiveAction {
    /// Wrap a [`SharableAction`], allocating live timer state for `Periodic` variants.
    pub fn new(action: SharableAction) -> Self {
        let interval =
            matches!(action, SharableAction::Periodic { .. }).then(|| Arc::new(Mutex::new(None)));
        Self {
            inner: action,
            interval,
        }
    }
}

/// Converts non-`Periodic` [`SharableAction`] variants directly into a [`SelectableAction`].
///
/// `Periodic` must go through [`LiveAction`] so that timer state is properly attached.
impl From<SharableAction> for SelectableAction {
    fn from(action: SharableAction) -> SelectableAction {
        match action {
            SharableAction::Instant(factory) => {
                #[allow(clippy::async_yields_async)]
                let fut = Box::pin(async move { factory() }) as SelectableAction;
                fut
            }
            SharableAction::Event(factory) => factory(),
            SharableAction::Periodic { .. } => {
                panic!("Periodic actions must be converted via LiveAction, not SharableAction directly")
            }
            #[cfg(feature = "daemon")]
            SharableAction::Operation {
                description: _,
                factory,
            } => factory(),
        }
    }
}

/// Converts a [`LiveAction`] into a [`SelectableAction`].
///
/// For `Periodic` variants the shared `Arc<Mutex<Option<Interval>>>` held by the
/// `LiveAction` is captured into the generated future, so the same timer instance
/// survives across loop iterations.
impl From<LiveAction> for SelectableAction {
    fn from(live: LiveAction) -> SelectableAction {
        match live.inner {
            SharableAction::Periodic {
                interval_secs,
                factory,
            } => {
                let interval = live
                    .interval
                    .expect("invariant: Periodic LiveAction always has an interval");
                define_resource_trigger_action!(
                    with {
                        let factory = factory.clone();
                        let interval = interval.clone();
                    }
                    when: {
                        let duration = Duration::from_secs(interval_secs);
                        let mut _guard = interval.lock().await;
                        match _guard.as_ref() {
                            Some(ivl) => {
                                if ivl.period() != duration {
                                    tracing::debug!(interval_secs = %interval_secs, "changing interval duration");
                                    let mut new_ivl = interval_at(Instant::now(), duration);
                                    new_ivl.set_missed_tick_behavior(MissedTickBehavior::Skip);
                                    *_guard = Some(new_ivl);
                                }
                            }
                            None => {
                                tracing::debug!(interval_secs = %interval_secs, "creating new interval");
                                let mut new_ivl = interval_at(Instant::now(), duration);
                                new_ivl.set_missed_tick_behavior(MissedTickBehavior::Skip);
                                *_guard = Some(new_ivl);
                            }
                        };
                        let _ = _guard.as_mut().unwrap().tick().await;
                    },
                    then: factory()
                )
                .into()
            }
            other => other.into(),
        }
    }
}

#[cfg(feature = "daemon")]
#[derive(Debug, Clone)]
pub enum BuiltMetricsProvider {
    Transient(Arc<metrics::transient::TransientMetricsProvider>),
}
