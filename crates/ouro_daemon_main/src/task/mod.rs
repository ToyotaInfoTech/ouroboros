// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Daemon task implementations.
//!
//! Each sub-module defines a long-running task that implements
//! [`ouro_core::runtime::comp::ComponentImplExt`] and is registered in the daemon agent.

pub mod heartbeat;
pub mod metadata;
pub mod metrics;
pub mod state;
pub mod system_state;
pub mod webui;

pub mod icc {
    use std::sync::Arc;

    use ouro_common::constant::core::MPSC_BUFSIZ;
    use ouro_core::{
        define_resource_trigger_action, error::InternalErrorKind, runtime::types::SharableAction,
        InternalResult,
    };
    use ouro_daemon::runtime::context::DaemonResourceContextImpl;
    use ouro_schema::schema::cli::icc::{dispatch, IccHandler, IccRequestWrapper};
    use tokio::sync::{mpsc::channel, Mutex};

    pub async fn dispatch_handler<H: IccHandler>(
        handler: Arc<H>,
        result: Option<IccRequestWrapper>,
    ) -> InternalResult<()> {
        dispatch(handler.as_ref(), result).await.map_err(|e| {
            (
                format!("ICC dispatch error: {}", e),
                InternalErrorKind::Protocol,
            )
                .into()
        })
    }

    pub async fn register_icc_handler<H: IccHandler>(
        this: &Arc<H>,
        name: &str,
        context: &Arc<DaemonResourceContextImpl>,
    ) -> InternalResult<SharableAction> {
        let (tx, rx) = channel::<IccRequestWrapper>(MPSC_BUFSIZ);
        let rx = Arc::new(Mutex::new(rx));
        context.register_icc_channel(name, tx).await?;

        let this = this.clone();
        Ok(define_resource_trigger_action!(
            with {
                let rx = rx.clone();
                let this = this.clone();
            }
            when: let result = rx.lock().await.recv().await,
            then: dispatch_handler(this, result)
        ))
    }
}
