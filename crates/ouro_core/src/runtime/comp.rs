// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Component lifecycle management framework.
//!
//! Provides the core abstractions for managing component lifecycles including
//! start, update, disable, enable, and discard transitions. Components are
//! protected by async read-write locks and communicate lifecycle changes
//! through message passing.

use crate::error::InternalErrorKind;
use crate::runtime::types::{LiveAction, SelectableAction, SharableAction};
use crate::InternalResult;
use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt as _;
use ouro_common::constant::core::SIZE_OF_USIZE;
use ouro_common::types::UnixTimestamp;
use ouro_common::{ansi_color, constant, AnsiColor};
use ouro_schema::schema::shadow::report::TaskStatus;
use ouro_state::core::types::{CompState, State};
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::instrument;

/// Represents the status of a component during its lifecycle.
///
/// # Type Parameters
/// * `S` - The state type implementing the `State` trait.
#[derive(Clone, PartialEq)]
pub enum CompStatus<S>
where
    Self: Send + Sync,
    S: State,
{
    /// The component has just been created and initialization is in progress.
    Created(S),
    /// The component is active.
    /// The timestamp indicates when it last time actioned.
    Active(S, UnixTimestamp),
    /// The component is inactive (disabled but not discarded).
    /// The timestamp indicates when it was deactivated.
    Inactive(S, UnixTimestamp),
    /// The component has been discarded and is no longer usable.
    Discarded,
}

impl<S> From<CompStatus<S>> for Option<S>
where
    S: State,
{
    fn from(status: CompStatus<S>) -> Self {
        match status {
            CompStatus::Created(s) | CompStatus::Active(s, _) | CompStatus::Inactive(s, _) => {
                Some(s)
            }
            CompStatus::Discarded => None,
        }
    }
}

impl<S> std::fmt::Display for CompStatus<S>
where
    S: State,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompStatus::Created(_) => write!(f, "{}", ansi_color(AnsiColor::Green, "Created")),
            CompStatus::Active(..) => write!(f, "{}", ansi_color(AnsiColor::Blue, "Active")),
            CompStatus::Inactive(..) => {
                write!(f, "{}", ansi_color(AnsiColor::BrightBlack, "Inactive"))
            }
            CompStatus::Discarded => write!(f, "{}", ansi_color(AnsiColor::Red, "Discarded")),
        }
    }
}

impl<S> std::fmt::Debug for CompStatus<S>
where
    S: State,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompStatus::Created(state) => {
                write!(
                    f,
                    "{}:\n{:#?}",
                    ansi_color(AnsiColor::Green, "Created"),
                    state
                )
            }
            CompStatus::Active(state, timestamp) => {
                write!(
                    f,
                    "{} ({}):\n{:#?}",
                    ansi_color(AnsiColor::Blue, "Active"),
                    timestamp,
                    state
                )
            }
            CompStatus::Inactive(state, timestamp) => {
                write!(
                    f,
                    "{} ({}):\n{:#?}",
                    ansi_color(AnsiColor::BrightBlack, "Inactive"),
                    timestamp,
                    state
                )
            }
            CompStatus::Discarded => {
                write!(f, "{}", ansi_color(AnsiColor::Red, "Discarded"),)
            }
        }
    }
}

impl<S> From<CompStatus<S>> for TaskStatus
where
    S: State,
{
    fn from(status: CompStatus<S>) -> Self {
        match status {
            CompStatus::Created(_) => TaskStatus::Created,
            CompStatus::Active(_, timestamp) => TaskStatus::Active(timestamp),
            CompStatus::Inactive(_, timestamp) => TaskStatus::Inactive(timestamp),
            CompStatus::Discarded => TaskStatus::Discarded,
        }
    }
}

/// Trait for the inner logic of a component, defining lifecycle methods.
///
/// Implementations provide the actual start/stop/update behavior while the
/// framework handles synchronization, state tracking, and message passing.
#[async_trait]
pub trait ComponentImpl
where
    Self: Send + Sync + Sized + 'static,
{
    type StateImpl: CompState + std::fmt::Debug;
    type ContextImpl: Send + Sync;

    fn name(&self) -> String {
        #[cfg(debug_assertions)]
        {
            let name = std::any::type_name::<Self>()
                .split("::")
                .collect::<Vec<_>>();
            let name = name.last().unwrap_or(&"Unknown");
            format!(
                "{} @ {:0width$x}",
                name,
                ouro_common::addr(self),
                width = SIZE_OF_USIZE * 2
            )
        }
        #[cfg(not(debug_assertions))]
        {
            format!("struct: {}", std::any::type_name::<Self>())
        }
    }

    async fn handle_status_change(
        self: Arc<Self>,
        status: &CompStatus<Self::StateImpl>,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<JoinHandle<()>>> {
        let mut handles: Vec<JoinHandle<()>> = vec![];
        match status {
            CompStatus::Created(state) => {
                let handle = self.on_created(state, context).await?;
                if let Some(handle) = handle {
                    handles.push(handle);
                }
            }
            CompStatus::Active(state, _) => {
                let handle = self.on_active(state, context).await?;
                if let Some(handle) = handle {
                    handles.push(handle);
                }
            }
            CompStatus::Inactive(state, _) => {
                let handle = self.on_inactive(state, context).await?;
                if let Some(handle) = handle {
                    handles.push(handle);
                }
            }
            CompStatus::Discarded => {
                let handle = self.on_discarded(context).await?;
                if let Some(handle) = handle {
                    handles.push(handle);
                }
            }
        };

        Ok(handles)
    }

    #[instrument(skip(self, state, context))]
    async fn on_created(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>> {
        self.on_inactive(state, context).await
    }

    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>>;

    #[instrument(skip(self, _context))]
    async fn on_discarded(
        self: Arc<Self>,
        _context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>> {
        Ok(None)
    }

    #[instrument(skip(self, _state, context))]
    async fn on_inactive(
        self: Arc<Self>,
        _state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>> {
        let context = context.clone();
        let handle = tokio::spawn(async move {
            loop {
                match context.recv_status_change().await {
                    Ok(event) => {
                        if let Err(e) = context.handle_status_change(&event).await {
                            tracing::error!(
                                "Error handling status change in task '{}': {:?}",
                                self.name(),
                                e
                            );
                        }
                        return;
                    }
                    Err(e) => {
                        tracing::error!(
                            "Error receiving status change in task '{}': {:?}",
                            self.name(),
                            e
                        );
                    }
                }
            }
        });
        Ok(Some(handle))
    }
}

#[async_trait]
pub trait ComponentImplExt: ComponentImpl {
    #[instrument(skip(self, state, context))]
    async fn on_active(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Option<JoinHandle<()>>> {
        let this = self.clone();

        let actions: Vec<LiveAction> = self
            .gather_actions(state, context.clone())
            .await?
            .into_iter()
            .map(LiveAction::new)
            .collect();

        let handle = tokio::spawn(async move {
            loop {
                let mut stream: FuturesUnordered<SelectableAction> =
                    actions.clone().into_iter().map(Into::into).collect();

                tracing::debug!("{}: Waiting for actions to be ready...", this.name());
                tokio::select! {
                    biased;
                    Ok(event) = context.recv_status_change() => {
                        if let Err(e) = context.handle_status_change(&event).await {
                            tracing::error!("Failed to handle status change in task '{}': {:?}", this.name(), e);
                        }
                        return;
                    }
                    Some(action) = stream.next() => {
                        tracing::debug!(name = %this.name(), "Executing action...");
                        context.mutex.lock().await.touch().await; // update last actioned timestamp
                        if let Err(e) = action.await {
                            tracing::error!(name = %this.name(), "Action execution failed: {:?}", e);
                        } else {
                            tracing::debug!(name = %this.name(), "Action executed successfully");
                        }
                    }
                }
            }
        });
        Ok(Some(handle))
    }

    async fn gather_actions(
        self: Arc<Self>,
        state: &Self::StateImpl,
        context: Arc<CompContext<Self>>,
    ) -> InternalResult<Vec<SharableAction>>
    where
        Self: Sized;
}

/// Context wrapper that holds a component with its surrounding resource context.
///
/// Provides async read/write access to the inner component implementation
/// and processes pending lifecycle messages before each access.
pub struct CompContext<Impl>
where
    Self: Send + Sync,
    Impl: ComponentImpl + Sized,
{
    name: String,
    inner: Arc<Impl::ContextImpl>,
    status_tx: Sender<CompStatus<Impl::StateImpl>>,
    status_rx: Mutex<Receiver<CompStatus<Impl::StateImpl>>>,
    pub mutex: Mutex<Comp<Impl>>,
}

impl<Impl> Deref for CompContext<Impl>
where
    Self: Send + Sync,
    Impl: ComponentImpl + Sized,
{
    type Target = Arc<Impl::ContextImpl>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<Impl> CompContext<Impl>
where
    Impl: ComponentImpl,
{
    fn new(global_context: &Arc<Impl::ContextImpl>, comp: Comp<Impl>) -> Arc<Self> {
        let name = comp.name();
        let mutex = Mutex::new(comp);
        let (status_tx, status_rx) =
            channel::<CompStatus<Impl::StateImpl>>(constant::core::MPSC_BUFSIZ);

        Arc::new(Self {
            name,
            inner: global_context.clone(),
            status_tx,
            status_rx: Mutex::new(status_rx),
            mutex,
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub async fn recv_status_change(&self) -> InternalResult<CompStatus<Impl::StateImpl>> {
        let new_status = self
            .status_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or(("Failed to receive state change", InternalErrorKind::Runtime).into())?;

        tracing::debug!(name = %self.name(), status = %new_status, "received");

        let mut _guard = self.mutex.lock().await; // TODO fix this for long-term by processing messages in a separate task and updating the status in the context
        _guard.comp_status = new_status.clone();

        Ok(new_status)
    }

    pub async fn handle_status_change(
        self: &Arc<Self>,
        new_status: &CompStatus<Impl::StateImpl>,
    ) -> InternalResult<()> {
        let mut _guard = self.mutex.lock().await;

        _guard.comp_status = new_status.clone();

        let old_handles = _guard
            .handle_status_change(new_status, self.clone())
            .await?;

        // forget and fire
        tokio::spawn(async move {
            for old_handle in old_handles {
                old_handle.abort();
            }
        });

        Ok(())
    }

    #[instrument(skip(self, new_status))]
    async fn _notify_status_change(
        &self,
        new_status: &CompStatus<Impl::StateImpl>,
    ) -> InternalResult<()> {
        tracing::debug!(name = %self.name(), "notified");
        self.status_tx.send(new_status.clone()).await.map_err(|e| {
            (
                format!(
                    "failed to send status change: {}: due to {:?}",
                    new_status, e
                )
                .as_str(),
                InternalErrorKind::Runtime,
            )
                .into()
        })
    }
}

/// Guard wrapper providing safe access to a component through its context.
///
/// `CompGuard` is the primary public API for interacting with components.
/// It encapsulates a `CompContext` and exposes lifecycle operations and
/// read/write access to the inner implementation.
pub struct CompGuard<Impl>
where
    Self: Send + Sync,
    Impl: ComponentImpl,
{
    context: Arc<CompContext<Impl>>,
}

impl<Impl> CompGuard<Impl>
where
    Impl: ComponentImpl,
{
    /// Creates a new `CompGuard` wrapping a component, its initial state, and context.
    ///
    /// # Arguments
    /// * `inner` - Component implementation
    /// * `state` - Initial state for the component
    /// * `context` - Resource context for dependency resolution
    ///
    /// # Returns
    /// An `Arc`-wrapped `CompGuard` ready for lifecycle operations
    #[instrument(skip(inner, state, context))]
    pub async fn new(
        name: &str,
        inner: Arc<Impl>,
        state: Impl::StateImpl,
        context: &Arc<Impl::ContextImpl>,
    ) -> InternalResult<Arc<Self>> {
        let s1 = state.clone();
        let s2 = state.clone();
        let this = Arc::new(Self {
            context: CompContext::new(context, Comp::new(name, inner, state)),
        });

        {
            let this = this.clone();
            let mut _guard = this.context.mutex.lock().await;
            _guard
                .handle_status_change(&CompStatus::Created(s1), this.context.clone())
                .await?;

            if s2.is_enabled() {
                this.notify_status_change(&CompStatus::Active(s2, UnixTimestamp::now()))
                    .await?;
            } else {
                this.notify_status_change(&CompStatus::Inactive(s2, UnixTimestamp::now()))
                    .await?;
            }
        }

        Ok(this)
    }

    pub fn name(&self) -> &str {
        self.context.name()
    }

    #[instrument(skip(self, new_status))]
    pub async fn notify_status_change(
        &self,
        new_status: &CompStatus<Impl::StateImpl>,
    ) -> InternalResult<()> {
        self.context._notify_status_change(new_status).await
    }

    #[instrument(skip(self))]
    pub async fn activate(self: &Arc<Self>) -> InternalResult<()> {
        let _guard = self.context.mutex.lock().await;
        match &_guard.comp_status {
            CompStatus::Discarded => Err((
                "Cannot activate a discarded component",
                InternalErrorKind::InvalidStateTransition,
            )
                .into()),
            CompStatus::Created(state)
            | CompStatus::Inactive(state, _)
            | CompStatus::Active(state, _) => {
                self.notify_status_change(&CompStatus::Active(state.clone(), UnixTimestamp::now()))
                    .await
            }
        }
    }

    #[instrument(skip(self))]
    pub async fn deactivate(self: &Arc<Self>) -> InternalResult<()> {
        let _guard = self.context.mutex.lock().await;
        match &_guard.comp_status {
            CompStatus::Discarded => Err((
                "Cannot deactivate a discarded component",
                InternalErrorKind::InvalidStateTransition,
            )
                .into()),
            CompStatus::Created(state)
            | CompStatus::Active(state, _)
            | CompStatus::Inactive(state, _) => {
                self.notify_status_change(&CompStatus::Inactive(
                    state.clone(),
                    UnixTimestamp::now(),
                ))
                .await
            }
        }
    }

    #[instrument(skip(self))]
    pub async fn reconfigure(self: &Arc<Self>, new_state: Impl::StateImpl) -> InternalResult<()> {
        let _guard = self.context.mutex.lock().await;
        match &_guard.comp_status {
            CompStatus::Created(_) | CompStatus::Inactive(..) => {
                self.notify_status_change(&CompStatus::Inactive(
                    new_state.clone(),
                    UnixTimestamp::now(),
                ))
                .await
            }
            CompStatus::Active(..) => {
                self.notify_status_change(&CompStatus::Active(
                    new_state.clone(),
                    UnixTimestamp::now(),
                ))
                .await
            }
            CompStatus::Discarded => Err((
                "Cannot reconfigure a discarded component",
                InternalErrorKind::InvalidStateTransition,
            )
                .into()),
        }
    }

    #[instrument(skip(self))]
    pub async fn discard(self: &Arc<Self>) -> InternalResult<()> {
        if let Err(e) = self.notify_status_change(&CompStatus::Discarded).await {
            tracing::error!("Failed to notify discard status change: {:?}", e);
        }
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn state(&self) -> InternalResult<Impl::StateImpl> {
        match self.comp_status().await {
            CompStatus::Created(state)
            | CompStatus::Active(state, _)
            | CompStatus::Inactive(state, _) => Ok(state.clone()),
            CompStatus::Discarded => Err((
                "Cannot retrieve state of a discarded component",
                InternalErrorKind::InvalidStateTransition,
            )
                .into()),
        }
    }

    #[instrument(skip(self))]
    pub async fn comp_status(&self) -> CompStatus<Impl::StateImpl> {
        let _guard = self.context.mutex.lock().await;
        _guard.comp_status.clone()
    }
}

/// Internal component state holder.
///
/// Wraps the component implementation, its current state, and lifecycle
/// message channels. Not exposed to users; accessed via `CompGuard`.
pub struct Comp<Impl>
where
    Self: Send + Sync,
    Impl: ComponentImpl + Sized,
{
    name: String,
    comp_status: CompStatus<Impl::StateImpl>,
    comp_impl: Arc<Impl>,
    handles: Vec<JoinHandle<()>>,
}

impl<Impl> std::fmt::Display for Comp<Impl>
where
    Self: Send + Sync,
    Impl: ComponentImpl + Sized,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{} ({})]", self.name, self.comp_status)?;
        Ok(())
    }
}

impl<Impl> std::fmt::Debug for Comp<Impl>
where
    Self: Send + Sync,
    Impl: ComponentImpl + Sized,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]: {:#?}", self.name, self.comp_status)?;
        Ok(())
    }
}

impl<Impl> Comp<Impl>
where
    Impl: ComponentImpl,
{
    fn new(name: &str, comp_impl: Arc<Impl>, state: Impl::StateImpl) -> Self {
        Self {
            name: name.to_string(),
            comp_status: CompStatus::Created(state),
            comp_impl,
            handles: Vec::new(),
        }
    }

    pub fn name(&self) -> String {
        format!("{:>8.8}: {}", self.name, self.comp_impl.name())
    }

    pub fn comp_impl(&self) -> &Impl {
        &self.comp_impl
    }

    pub fn current_status(&self) -> CompStatus<Impl::StateImpl> {
        self.comp_status.clone()
    }

    async fn handle_status_change(
        &mut self,
        status: &CompStatus<Impl::StateImpl>,
        context: Arc<CompContext<Impl>>,
    ) -> InternalResult<Vec<JoinHandle<()>>> {
        let old_handles = std::mem::take(&mut self.handles);
        // TODO abort old handles
        self.handles = self
            .comp_impl
            .clone()
            .handle_status_change(status, context)
            .await?;

        tracing::info!(name = %self.name(), status = %status, "done");

        Ok(old_handles)
    }

    #[instrument(skip(self))]
    pub async fn touch(&mut self) {
        match &self.comp_status {
            CompStatus::Active(state, _) => {
                self.comp_status = CompStatus::Active(state.clone(), UnixTimestamp::now());
            }
            _ => {
                tracing::warn!(
                    "Touching component '{}' that is not active (current status: {})",
                    self.name(),
                    self.comp_status
                );
            }
        };
    }
}
