// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::{any::Any, ops::Deref, sync::Arc};

use serde::{Deserialize, Serialize};

/// A type-erased, thread-safe, reference-counted value.
///
/// Used to store heterogeneous runtime components (tasks, repositories, etc.)
/// in shared maps. Downcast back to a concrete type via [`Arc::downcast`].
pub type AnySharable = Arc<dyn Any + Send + Sync>;

/// A socket address represented as a free-form string (e.g., `"tcp/127.0.0.1:7447"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SockAddr(pub String);
impl Deref for SockAddr {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<SockAddr> for String {
    fn from(val: SockAddr) -> String {
        val.0
    }
}
impl From<String> for SockAddr {
    fn from(value: String) -> Self {
        SockAddr(value)
    }
}
impl std::fmt::Display for SockAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An opaque binary payload exchanged over messaging or RPC channels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Payload(pub Vec<u8>);
impl Deref for Payload {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<Payload> for Vec<u8> {
    fn from(val: Payload) -> Vec<u8> {
        val.0
    }
}
impl From<Vec<u8>> for Payload {
    fn from(value: Vec<u8>) -> Self {
        Payload(value)
    }
}
impl std::fmt::Display for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// A concrete Zenoh topic key (e.g., `"edge/edge-001/status/liveness"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Topic(pub String);
impl Deref for Topic {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<Topic> for String {
    fn from(val: Topic) -> String {
        val.0
    }
}
impl From<String> for Topic {
    fn from(value: String) -> Self {
        Topic(value)
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A wildcard topic pattern for Zenoh subscriptions (e.g., `"edge/*/status/liveness"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopicPattern(pub &'static str);
impl Deref for TopicPattern {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}
impl From<TopicPattern> for &'static str {
    fn from(val: TopicPattern) -> &'static str {
        val.0
    }
}
impl From<&'static str> for TopicPattern {
    fn from(value: &'static str) -> Self {
        TopicPattern(value)
    }
}

impl std::fmt::Display for TopicPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Milliseconds since the Unix epoch.
///
/// Used as the canonical time representation across the Ouroboros system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd, Eq)]
pub struct UnixTimestamp(pub i64);

impl UnixTimestamp {
    pub fn now() -> UnixTimestamp {
        (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is before UNIX epoch")
            .as_millis() as i64)
            .into()
    }
}

impl std::fmt::Display for UnixTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dt = time::OffsetDateTime::from_unix_timestamp(self.0 / 1000)
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
        write!(f, "{}", dt)
    }
}

impl std::ops::Deref for UnixTimestamp {
    type Target = i64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<i64> for UnixTimestamp {
    fn from(value: i64) -> Self {
        UnixTimestamp(value)
    }
}

impl Default for UnixTimestamp {
    fn default() -> Self {
        UnixTimestamp::now()
    }
}
