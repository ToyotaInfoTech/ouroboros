// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Unified error types for the Ouroboros system.
//!
//! Provides [`InternalError`] and [`InternalErrorKind`] as the canonical error
//! types. All fallible functions should return `InternalResult<T>` and wrap
//! errors with descriptive context via `("context", error).into()`.

use std::backtrace::Backtrace;

use ouro_common::{ansi_color, constant::core::ENV_PANIC_ON_ERROR, AnsiColor};
use sqlx;

/// Implements `Into<InternalError>` for `(&str, $error_type)` tuples.
///
/// Converts a `(&str, $error_type)` pair into an [`InternalError`],
/// wrapping the error in the specified [`InternalErrorKind`] variant.
///
/// # Parameters
/// - `$error_type`: The concrete error type to convert from.
/// - `$variant`: The [`InternalErrorKind`] variant to wrap the error in.
macro_rules! impl_error_into_internal_error_by_str {
    ($error_type:ty, $variant:ident) => {
        #[allow(clippy::from_over_into)]
        impl Into<InternalError> for (&str, $error_type) {
            fn into(self) -> InternalError {
                InternalError {
                    context: self.0.to_string(),
                    kind: InternalErrorKind::$variant(self.1),
                    backtrace: Backtrace::capture(),
                }
            }
        }
    };
}

/// Implements `Into<InternalError>` for `(String, $error_type)` tuples.
///
/// Same as [`impl_error_into_internal_error_by_str`] but accepts an owned `String`
/// context message, useful when the message is dynamically generated.
///
/// # Parameters
/// - `$error_type`: The concrete error type to convert from.
/// - `$variant`: The [`InternalErrorKind`] variant to wrap the error in.
macro_rules! impl_error_into_internal_error_by_string {
    ($error_type:ty, $variant:ident) => {
        #[allow(clippy::from_over_into)]
        impl Into<InternalError> for (String, $error_type) {
            fn into(self) -> InternalError {
                InternalError {
                    context: self.0,
                    kind: InternalErrorKind::$variant(self.1),
                    backtrace: Backtrace::capture(),
                }
            }
        }
    };
}

pub type Result<T> = std::result::Result<T, Error>;

/// Public-facing error type for the external API.
///
/// Maps internal error kinds into a smaller set of categories
/// suitable for consumers of the crate.
#[derive(Debug)]
pub enum Error {
    Initialization(String, Backtrace),

    InvalidConfig(String, Backtrace),

    Backend(String, Backtrace),

    Io(String, Backtrace),

    Internal(String, Backtrace),
}

impl From<InternalError> for Error {
    fn from(e: InternalError) -> Self {
        match e.kind {
            InternalErrorKind::Database(..) => Error::Backend(e.context, e.backtrace),
            InternalErrorKind::SerdeFlatbuf(..) => Error::Io(e.context, e.backtrace),
            InternalErrorKind::NotImplemented => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::TypeConversion => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::Network => Error::Io(e.context, e.backtrace),
            InternalErrorKind::Runtime => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::DeCbor(..) => Error::Io(e.context, e.backtrace),
            InternalErrorKind::SerCbor(..) => Error::Io(e.context, e.backtrace),
            InternalErrorKind::InvalidConfig => Error::InvalidConfig(e.context, e.backtrace),
            InternalErrorKind::Protocol => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::Sync(..) => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::Lock => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::SerdeJson(..) => Error::Io(e.context, e.backtrace),
            InternalErrorKind::Unknown(..) => Error::Backend(e.context, e.backtrace),
            InternalErrorKind::Parse => Error::Io(e.context, e.backtrace),
            InternalErrorKind::Io(..) => Error::Io(e.context, e.backtrace),
            InternalErrorKind::DeToml(..) => Error::Io(e.context, e.backtrace),
            InternalErrorKind::Bug => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::InvalidAddress(..) => Error::InvalidConfig(e.context, e.backtrace),
            InternalErrorKind::Time(..) => Error::Backend(e.context, e.backtrace),
            InternalErrorKind::InternalCommunication => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::InvalidStateTransition => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::SerToml(..) => Error::Io(e.context, e.backtrace),
            InternalErrorKind::IllegalArgument => Error::Internal(e.context, e.backtrace),
            InternalErrorKind::NoSuchKey => Error::Backend(e.context, e.backtrace),
            InternalErrorKind::Timeout => Error::Backend(e.context, e.backtrace),
            InternalErrorKind::RetryExhausted => Error::Backend(e.context, e.backtrace),
        }
    }
}

/// Internal error type carrying a context message, a typed error kind, and a captured backtrace.
#[derive(Debug)]
pub struct InternalError {
    context: String,
    kind: InternalErrorKind,
    backtrace: Backtrace,
}

impl InternalError {
    /// Returns the error kind.
    pub fn kind(&self) -> &InternalErrorKind {
        &self.kind
    }

    /// Returns the context message.
    pub fn context(&self) -> &str {
        &self.context
    }
}

/// Categorized internal error kinds.
///
/// Each variant either wraps a concrete upstream error or represents
/// a logical error condition without an underlying cause.
#[derive(Debug)]
pub enum InternalErrorKind {
    SerdeFlatbuf(flatbuffers::InvalidFlatbuffer),
    DeCbor(ciborium::de::Error<std::io::Error>),
    SerCbor(ciborium::ser::Error<std::io::Error>),
    SerdeJson(serde_json::Error),
    DeToml(toml::de::Error),
    SerToml(toml::ser::Error),
    InvalidAddress(std::net::AddrParseError),
    Time(std::time::SystemTimeError),
    Parse,
    Database(sqlx::Error),
    NotImplemented,
    TypeConversion,
    Network,
    Runtime,
    Timeout,
    RetryExhausted,
    InvalidConfig,
    Protocol,
    Sync(tokio::sync::AcquireError),
    Lock,
    InternalCommunication,
    Unknown(Box<dyn std::error::Error + Send + Sync>),
    Io(std::io::Error),
    Bug,
    InvalidStateTransition,
    IllegalArgument,
    NoSuchKey,
}

impl_error_into_internal_error_by_str!(flatbuffers::InvalidFlatbuffer, SerdeFlatbuf);
impl_error_into_internal_error_by_string!(flatbuffers::InvalidFlatbuffer, SerdeFlatbuf);
impl_error_into_internal_error_by_str!(ciborium::de::Error<std::io::Error>, DeCbor);
impl_error_into_internal_error_by_string!(ciborium::de::Error<std::io::Error>, DeCbor);
impl_error_into_internal_error_by_str!(ciborium::ser::Error<std::io::Error>, SerCbor);
impl_error_into_internal_error_by_string!(ciborium::ser::Error<std::io::Error>, SerCbor);
impl_error_into_internal_error_by_str!(serde_json::Error, SerdeJson);
impl_error_into_internal_error_by_string!(serde_json::Error, SerdeJson);
impl_error_into_internal_error_by_str!(toml::de::Error, DeToml);
impl_error_into_internal_error_by_string!(toml::de::Error, DeToml);
impl_error_into_internal_error_by_str!(toml::ser::Error, SerToml);
impl_error_into_internal_error_by_string!(toml::ser::Error, SerToml);
impl_error_into_internal_error_by_str!(sqlx::Error, Database);
impl_error_into_internal_error_by_string!(sqlx::Error, Database);
impl_error_into_internal_error_by_str!(tokio::sync::AcquireError, Sync);
impl_error_into_internal_error_by_string!(tokio::sync::AcquireError, Sync);
impl_error_into_internal_error_by_str!(Box<dyn std::error::Error + Send + Sync>, Unknown);
impl_error_into_internal_error_by_string!(Box<dyn std::error::Error + Send + Sync>, Unknown);
impl_error_into_internal_error_by_str!(std::io::Error, Io);
impl_error_into_internal_error_by_string!(std::io::Error, Io);
impl_error_into_internal_error_by_str!(std::net::AddrParseError, InvalidAddress);
impl_error_into_internal_error_by_string!(std::net::AddrParseError, InvalidAddress);
impl_error_into_internal_error_by_str!(std::time::SystemTimeError, Time);
impl_error_into_internal_error_by_string!(std::time::SystemTimeError, Time);

#[allow(clippy::from_over_into)]
impl Into<InternalError> for (&str, InternalErrorKind) {
    fn into(self) -> InternalError {
        InternalError {
            context: self.0.to_string(),
            kind: self.1,
            backtrace: Backtrace::capture(),
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<InternalError> for (String, InternalErrorKind) {
    /// Convert a `(String, InternalErrorKind)` tuple into an [`InternalError`].
    ///
    /// Useful when the context message is dynamically generated (e.g. via `format!`).
    fn into(self) -> InternalError {
        InternalError {
            context: self.0,
            kind: self.1,
            backtrace: Backtrace::capture(),
        }
    }
}

impl std::fmt::Display for InternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format the error with context and backtrace, using ANSI colors for emphasis.
        let context = {
            let mut lines = self.context.lines().collect::<Vec<_>>();
            let first_line = ansi_color(AnsiColor::BrightRed, lines[0]);
            lines[0] = first_line.as_str();
            lines.join("\n")
        };

        write!(
            f,
            "internal error: {}: {:?}:\nBacktrace:\n{}",
            context, self.kind, self.backtrace
        )
    }
}

/// Panics if the `OURO_PANIC_ON_ERROR` environment variable is set to a truthy value.
///
/// Useful during development to surface errors that would otherwise be
/// silently returned or logged.
pub fn panic_if_env_set<E: std::fmt::Display>(error: &E) {
    let panic_on_error = std::env::var(ENV_PANIC_ON_ERROR)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);

    if panic_on_error {
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            std::process::exit(1);
        });
        panic!("Fatal error: {error}");
    }

    eprintln!("Error: {error}");
}
