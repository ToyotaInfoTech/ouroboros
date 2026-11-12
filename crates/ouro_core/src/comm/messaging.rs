// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::sync::Arc;
use std::{fmt::Debug, pin::Pin};

use async_trait::async_trait;
use ouro_common::types::{Payload, SockAddr, Topic, TopicPattern};
use tracing::instrument;

use crate::{comm::messaging::zenoh_messaging::ZenohSession, InternalResult};

/// Trait for an established messaging session capable of creating publishers,
/// subscribers, and RPC endpoints on Zenoh-style topics.
#[async_trait]
pub trait Session: Debug + Send + Sync {
    async fn publisher(&self, topic: &Topic) -> InternalResult<Arc<dyn Publisher>>;

    async fn topic_pattern_subscriber(
        &self,
        topic: &TopicPattern,
    ) -> InternalResult<Arc<dyn Subscriber>>;

    async fn topic_subscriber(&self, topic: &Topic) -> InternalResult<Arc<dyn Subscriber>>;

    async fn rpc_client(&self, topic: &Topic) -> InternalResult<Arc<dyn RpcClient>>;

    async fn topic_pattern_server(
        &self,
        topic: &TopicPattern,
    ) -> InternalResult<Arc<dyn RpcServer>>;

    async fn topic_server(&self, topic: &Topic) -> InternalResult<Arc<dyn RpcServer>>;
}

/// Publisher trait for sending messages on a topic.
#[async_trait]
pub trait Publisher: Debug + Send + Sync {
    /// Publish `message` bytes to the bound topic.
    async fn put(&self, message: &[u8]) -> InternalResult<()>;
}

/// Subscriber trait for receiving messages from a topic.
#[async_trait]
pub trait Subscriber: Debug + Send + Sync {
    /// Wait for the next message and return it.
    async fn recv_async(&self) -> InternalResult<TopicMessage>;
}

/// RPC client trait for issuing requests over a topic.
#[async_trait]
pub trait RpcClient: Debug + Send + Sync {
    /// Send `payload` as a request and wait for the reply.
    async fn call(&self, payload: &[u8]) -> InternalResult<TopicMessage>;
}

/// RPC server trait for receiving and replying to requests.
#[async_trait]
pub trait RpcServer: Debug + Send + Sync {
    /// Wait for the next incoming request and return a handle for replying.
    async fn recv_async(&self) -> InternalResult<Arc<dyn RpcHandle>>;
}

/// Handle for an in-flight RPC request, used to read the request payload and send a reply.
#[async_trait]
pub trait RpcHandle: Debug + Send + Sync {
    #[allow(clippy::result_large_err)]
    fn request(&self) -> InternalResult<TopicMessage>;
    async fn response(&self, response: &Response) -> InternalResult<()>;
}

/// Supported transport backends.
#[derive(Debug, Clone)]
pub enum Transport {
    Zenoh(Vec<SockAddr>),
}

/// A received message consisting of a topic and its binary payload.
#[derive(Clone)]
pub struct TopicMessage(pub Topic, pub Payload);

type SessionFactory = dyn Fn(
        Option<ouro_state::core::RetryStrategySpec>,
    ) -> Pin<Box<dyn Future<Output = InternalResult<Arc<dyn Session>>> + Send>>
    + Send
    + Sync;

/// Lazily initializes and caches a messaging session.
///
/// The session is created on first use. If a [`RetryStrategySpec`] is provided,
/// connection attempts use exponential backoff until the session is established.
struct LazySession {
    factory: Box<SessionFactory>,
    retry: Option<ouro_state::core::RetryStrategySpec>,
    session: tokio::sync::OnceCell<Arc<dyn Session>>,
}

impl Debug for LazySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazySession")
            .field("session_initialized", &self.session.initialized())
            .finish()
    }
}

impl LazySession {
    fn new(
        factory: Box<SessionFactory>,
        retry: Option<ouro_state::core::RetryStrategySpec>,
    ) -> Self {
        Self {
            factory,
            retry,
            session: tokio::sync::OnceCell::new(),
        }
    }

    async fn get_or_init(&self) -> InternalResult<Arc<dyn Session>> {
        self.session
            .get_or_try_init(|| self.connect())
            .await
            .map(Arc::clone)
    }

    /// Attempt to open the underlying session, retrying with exponential backoff
    /// when a [`RetryStrategySpec`] is configured.
    async fn connect(&self) -> InternalResult<Arc<dyn Session>> {
        let Some(retry) = &self.retry else {
            return (self.factory)(None).await;
        };

        if retry.max_attempts == 0 {
            return (self.factory)(Some(retry.clone())).await;
        }

        let mut backoff_ms = retry.initial_backoff_ms;
        let max_backoff_ms = retry.max_backoff_ms.unwrap_or(u64::MAX);

        for attempt in 1..=retry.max_attempts {
            match (self.factory)(Some(retry.clone())).await {
                Ok(session) => return Ok(session),
                Err(e) => {
                    if attempt >= retry.max_attempts {
                        tracing::error!(
                            "Failed to open Zenoh session after {} attempt(s): {:?}",
                            retry.max_attempts,
                            e
                        );
                        return Err((
                            format!(
                                "Failed to open Zenoh session after {} attempt(s)",
                                retry.max_attempts
                            ),
                            crate::error::InternalErrorKind::RetryExhausted,
                        )
                            .into());
                    }
                    tracing::warn!(
                        "Failed to open Zenoh session (attempt {}/{}): {}, retrying in {} ms...",
                        attempt,
                        retry.max_attempts,
                        e,
                        backoff_ms
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    backoff_ms =
                        ((backoff_ms as f64 * retry.backoff_multiplier) as u64).min(max_backoff_ms);
                }
            }
        }

        unreachable!()
    }
}

#[async_trait]
impl Session for LazySession {
    #[instrument(skip(self))]
    async fn publisher(&self, topic: &Topic) -> InternalResult<Arc<dyn Publisher>> {
        tracing::debug!("Requesting publisher for topic: {}", topic);
        self.get_or_init().await?.publisher(topic).await
    }

    #[instrument(skip(self))]
    async fn topic_pattern_subscriber(
        &self,
        topic: &TopicPattern,
    ) -> InternalResult<Arc<dyn Subscriber>> {
        tracing::debug!("Requesting subscriber for topic pattern: {}", topic);
        self.get_or_init()
            .await?
            .topic_pattern_subscriber(topic)
            .await
    }

    #[instrument(skip(self))]
    async fn topic_subscriber(&self, topic: &Topic) -> InternalResult<Arc<dyn Subscriber>> {
        tracing::debug!("Requesting subscriber for topic: {}", topic);
        self.get_or_init().await?.topic_subscriber(topic).await
    }

    #[instrument(skip(self))]
    async fn rpc_client(&self, topic: &Topic) -> InternalResult<Arc<dyn RpcClient>> {
        tracing::debug!("Requesting RPC client for topic: {}", topic);
        self.get_or_init().await?.rpc_client(topic).await
    }

    #[instrument(skip(self))]
    async fn topic_pattern_server(
        &self,
        topic: &TopicPattern,
    ) -> InternalResult<Arc<dyn RpcServer>> {
        tracing::debug!("Requesting RPC server for topic pattern: {}", topic);
        self.get_or_init().await?.topic_pattern_server(topic).await
    }

    #[instrument(skip(self))]
    async fn topic_server(&self, topic: &Topic) -> InternalResult<Arc<dyn RpcServer>> {
        tracing::debug!("Requesting RPC server for topic: {}", topic);
        self.get_or_init().await?.topic_server(topic).await
    }
}

impl Transport {
    /// Open a lazily-initialized session for this transport.
    ///
    /// The session is not actually connected until first use. If `retry` is provided,
    /// the initial connection attempt uses exponential backoff according to the spec.
    #[allow(clippy::result_large_err, clippy::type_complexity)]
    pub async fn open(
        &self,
        retry: Option<ouro_state::core::RetryStrategySpec>,
    ) -> InternalResult<Arc<dyn Session>> {
        let factory: Box<SessionFactory> = match self {
            Transport::Zenoh(endpoints) => {
                let endpoints = endpoints.clone();
                Box::new(
                    move |retry| -> Pin<
                        Box<dyn Future<Output = InternalResult<Arc<dyn Session>>> + Send>,
                    > {
                        let endpoints = endpoints.clone();
                        Box::pin(async move {
                            let endpoints = endpoints
                                .iter()
                                .map(|e| format!("\"{}\"", e))
                                .collect::<Vec<_>>()
                                .join(", ");

                            let zenoh_config = format!(
                                r#"
                                {{
                                    mode: "client",
                                    connect: {{
                                        endpoints: [ {endpoints} ]
                                    }}
                                }}
                                "#
                            );

                            tracing::debug!("Opening zenoh session with endpoints: {}", endpoints);

                            let zenoh_config =
                                zenoh::Config::from_json5(&zenoh_config).map_err(|e| {
                                    (
                                        format!(
                                            "Failed to parse Zenoh config for endpoints: {}",
                                            &endpoints
                                        )
                                        .as_str(),
                                        e,
                                    )
                                        .into()
                                })?;

                            let session = zenoh::open(zenoh_config).await.map_err(|e| {
                                (
                                    format!(
                                        "Failed to open Zenoh session for endpoints: {}",
                                        &endpoints
                                    )
                                    .as_str(),
                                    e,
                                )
                                    .into()
                            })?;

                            Ok(Arc::new(ZenohSession { session, retry }) as Arc<dyn Session>)
                        })
                    },
                )
            }
        };
        let lazy_session = LazySession::new(factory, retry);
        Ok(Arc::new(lazy_session))
    }
}

/// An RPC reply that the server sends back to the client.
pub enum Response {
    Success(Topic, Payload),
    Error(Payload),
    TopicNotFound(Topic),
}

pub mod zenoh_messaging {
    use ouro_state::core::RetryStrategySpec;
    use zenoh::qos::CongestionControl;

    use crate::error::InternalErrorKind;

    use super::*;

    #[derive(Clone, Debug)]
    pub(crate) struct ZenohSession {
        pub(crate) session: zenoh::Session,
        pub(crate) retry: Option<ouro_state::core::RetryStrategySpec>,
    }

    #[async_trait]
    impl Session for ZenohSession {
        #[instrument(skip(self))]
        async fn publisher(&self, topic: &Topic) -> InternalResult<Arc<dyn Publisher>> {
            tracing::debug!("Requesting publisher for topic: {}", topic);
            let publisher = self
                .session
                .declare_publisher(topic.to_string())
                .await
                .map_err(|e| {
                    (
                        format!("Failed to declare Zenoh publisher for topic: {}", topic).as_str(),
                        e,
                    )
                        .into()
                })?;
            Ok(Arc::new(ZenohPublisher { publisher }))
        }

        #[instrument(skip(self))]
        async fn topic_pattern_subscriber(
            &self,
            topic: &TopicPattern,
        ) -> InternalResult<Arc<dyn Subscriber>> {
            tracing::debug!("Requesting subscriber for topic pattern: {}", topic);
            let subscriber: zenoh::pubsub::Subscriber<
                zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
            > = self
                .session
                .declare_subscriber(topic.to_string())
                .await
                .map_err(|e| {
                    (
                        format!("Failed to declare Zenoh subscriber for topic: {}", topic).as_str(),
                        e,
                    )
                        .into()
                })?;
            Ok(Arc::new(ZenohSubscriber { subscriber }))
        }

        #[instrument(skip(self))]
        async fn topic_subscriber(&self, topic: &Topic) -> InternalResult<Arc<dyn Subscriber>> {
            tracing::debug!("Requesting subscriber for topic: {}", topic);
            let subscriber: zenoh::pubsub::Subscriber<
                zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
            > = self
                .session
                .declare_subscriber(topic.to_string())
                .await
                .map_err(|e| {
                    (
                        format!("Failed to declare Zenoh subscriber for topic: {}", topic).as_str(),
                        e,
                    )
                        .into()
                })?;
            Ok(Arc::new(ZenohSubscriber { subscriber }))
        }

        #[instrument(skip(self))]
        async fn rpc_client(&self, topic: &Topic) -> InternalResult<Arc<dyn RpcClient>> {
            tracing::debug!("Requesting RPC client for topic: {}", topic);
            let builder = self
                .session
                .declare_querier(topic.to_string())
                .congestion_control(CongestionControl::Block);

            // If a retry strategy is configured, the RPC client will handle retries internally with exponential backoff.
            let builder = if let Some(retry) = &self.retry {
                if let Some(timeout_ms) = retry.retry_timeout_ms {
                    builder.timeout(std::time::Duration::from_millis(timeout_ms))
                } else {
                    builder
                }
            } else {
                builder
            };

            let client = builder.await.map_err(|e| {
                (
                    format!("Failed to declare Zenoh querier for topic: {}", topic).as_str(),
                    e,
                )
                    .into()
            })?;
            Ok(Arc::new(ZenohRpcClient {
                client,
                retry: self.retry.clone(),
            }))
        }

        #[instrument(skip(self))]
        async fn topic_pattern_server(
            &self,
            topic: &TopicPattern,
        ) -> InternalResult<Arc<dyn RpcServer>> {
            tracing::debug!("Requesting RPC server for topic pattern: {}", topic);
            let server = self
                .session
                .declare_queryable(topic.to_string())
                .await
                .map_err(|e| {
                    (
                        format!("Failed to declare Zenoh queryable for topic: {}", topic).as_str(),
                        e,
                    )
                        .into()
                })?;
            Ok(Arc::new(ZenohRpcServer { server }))
        }

        #[instrument(skip(self))]
        async fn topic_server(&self, topic: &Topic) -> InternalResult<Arc<dyn RpcServer>> {
            tracing::debug!("Requesting RPC server for topic: {}", topic);
            let server = self
                .session
                .declare_queryable(topic.to_string())
                .await
                .map_err(|e| {
                    (
                        format!("Failed to declare Zenoh queryable for topic: {}", topic).as_str(),
                        e,
                    )
                        .into()
                })?;
            Ok(Arc::new(ZenohRpcServer { server }))
        }
    }

    #[derive(Debug)]
    struct ZenohPublisher<'a>
    where
        Self: Sync + Send,
    {
        publisher: zenoh::pubsub::Publisher<'a>,
    }

    #[async_trait]
    impl<'a> Publisher for ZenohPublisher<'a> {
        #[instrument(skip(self, message))]
        async fn put(&self, message: &[u8]) -> InternalResult<()> {
            tracing::debug!(
                "Publishing message to Zenoh topic: {}, len={}",
                self.publisher.key_expr(),
                message.len()
            );
            self.publisher.put(message).await.map_err(|e| {
                (
                    format!(
                        "Failed to publish message to Zenoh topic: {}",
                        self.publisher.key_expr()
                    )
                    .as_str(),
                    e,
                )
                    .into()
            })?;
            Ok(())
        }
    }

    #[derive(Debug)]
    struct ZenohSubscriber
    where
        Self: Sync + Send,
    {
        subscriber:
            zenoh::pubsub::Subscriber<zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>>,
    }

    #[async_trait]
    impl Subscriber for ZenohSubscriber
    where
        Self: Sync + Send,
    {
        #[instrument(skip(self))]
        async fn recv_async(&self) -> InternalResult<TopicMessage> {
            tracing::debug!(
                "Receiving message from Zenoh topic: {}",
                self.subscriber.key_expr()
            );
            let sample = self.subscriber.recv_async().await.map_err(|e| {
                (
                    format!(
                        "Failed to receive Zenoh sample for topic: {}",
                        self.subscriber.key_expr()
                    )
                    .as_str(),
                    e,
                )
                    .into()
            })?;

            if sample.kind() == zenoh::sample::SampleKind::Delete {
                return Err((
                    format!(
                        "Received delete topic for key {}, topic deletion is not supported yet",
                        sample.key_expr()
                    )
                    .as_str(),
                    InternalErrorKind::Runtime,
                )
                    .into());
            }

            tracing::debug!(
                "Received message from Zenoh topic: {}, len={}",
                sample.key_expr(),
                sample.payload().to_bytes().len()
            );

            Ok(TopicMessage(
                Topic(sample.key_expr().to_string()),
                Payload(sample.payload().to_bytes().to_vec()),
            ))
        }
    }

    #[derive(Debug)]
    struct ZenohRpcClient<'a>
    where
        Self: Sync + Send,
    {
        client: zenoh::query::Querier<'a>,
        retry: Option<RetryStrategySpec>,
    }

    impl<'a> ZenohRpcClient<'a> {
        /// Send a single RPC request without retry.
        async fn call_once(&self, payload: &[u8]) -> InternalResult<TopicMessage> {
            tracing::debug!("Sending Zenoh RPC request");
            let channel = self
                .client
                .get()
                .payload(payload)
                .await
                .map_err(|e| ("Failed to send Zenoh RPC request", e).into())?;

            let reply = channel.iter().next().ok_or_else(|| {
                (
                    "No response received for Zenoh RPC request",
                    InternalErrorKind::Timeout,
                )
                    .into()
            })?;

            let sample = reply.result().map_err(|e| {
                (
                    format!("Failed to get result for Zenoh RPC request: {}", e),
                    InternalErrorKind::Protocol,
                )
                    .into()
            })?;

            Ok(TopicMessage(
                Topic(sample.key_expr().to_string()),
                Payload(sample.payload().to_bytes().to_vec()),
            ))
        }
    }

    #[async_trait]
    impl<'a> RpcClient for ZenohRpcClient<'a> {
        #[instrument(skip(self, payload))]
        async fn call(&self, payload: &[u8]) -> InternalResult<TopicMessage> {
            let Some(retry) = &self.retry else {
                return self.call_once(payload).await;
            };

            if retry.max_attempts == 0 {
                return self.call_once(payload).await;
            }

            let mut backoff_ms = retry.initial_backoff_ms;
            let max_backoff_ms = retry.max_backoff_ms.unwrap_or(u64::MAX);

            for attempt in 1..=retry.max_attempts {
                match self.call_once(payload).await {
                    Ok(msg) => return Ok(msg),
                    Err(e) => {
                        if attempt >= retry.max_attempts {
                            tracing::error!(
                                "Zenoh RPC call failed after {} attempt(s): {:?}",
                                retry.max_attempts,
                                e
                            );
                            return Err((
                                format!(
                                    "Zenoh RPC call failed after {} attempt(s)",
                                    retry.max_attempts
                                ),
                                InternalErrorKind::RetryExhausted,
                            )
                                .into());
                        }
                        tracing::warn!(
                            "Zenoh RPC call failed (attempt {}/{}): {}, retrying in {} ms...",
                            attempt,
                            retry.max_attempts,
                            e,
                            backoff_ms
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        backoff_ms = ((backoff_ms as f64 * retry.backoff_multiplier) as u64)
                            .min(max_backoff_ms);
                    }
                }
            }

            unreachable!()
        }
    }

    #[derive(Debug)]
    struct ZenohRpcServer
    where
        Self: Sync + Send,
    {
        server: zenoh::query::Queryable<zenoh::handlers::FifoChannelHandler<zenoh::query::Query>>,
    }

    #[async_trait]
    impl RpcServer for ZenohRpcServer {
        #[instrument(skip(self))]
        async fn recv_async(&self) -> InternalResult<Arc<dyn RpcHandle>> {
            tracing::debug!(
                "Receiving Zenoh RPC request from: {}",
                self.server.key_expr()
            );
            let query = self
                .server
                .recv_async()
                .await
                .map_err(|e| ("Failed to receive Zenoh query", e).into())?;

            Ok(Arc::new(ZenohRpcHandle { query }))
        }
    }

    #[derive(Debug)]
    struct ZenohRpcHandle {
        query: zenoh::query::Query,
    }

    #[async_trait]
    impl RpcHandle for ZenohRpcHandle {
        #[instrument(skip(self))]
        fn request(&self) -> InternalResult<TopicMessage> {
            tracing::debug!("Processing Zenoh RPC request");
            let payload = self
                .query
                .payload()
                .ok_or_else(|| {
                    (
                        "No payload contained in Zenoh RPC request",
                        InternalErrorKind::Runtime,
                    )
                        .into()
                })?
                .to_bytes()
                .to_vec();
            Ok(TopicMessage(
                Topic(self.query.key_expr().to_string()),
                Payload(payload),
            ))
        }

        #[instrument(skip(self, response))]
        async fn response(&self, response: &Response) -> InternalResult<()> {
            match response {
                Response::Success(topic, payload) => {
                    tracing::debug!("Sending Zenoh RPC response for topic: {}", topic);
                    self.query
                        .reply(topic as &str, payload as &[u8])
                        .await
                        .map_err(|e| ("Failed to send Zenoh RPC response", e).into())?;
                }
                Response::Error(message) => {
                    tracing::debug!("Sending Zenoh RPC error response: {}", message);
                    self.query
                        .reply_err(message as &[u8])
                        .await
                        .map_err(|e| ("Failed to send Zenoh RPC error response", e).into())?;
                }
                Response::TopicNotFound(topic) => {
                    tracing::debug!(
                        "Sending Zenoh RPC topic not found response for topic: {}",
                        topic
                    );
                    self.query.reply_del(topic as &str).await.map_err(|e| {
                        ("Failed to send Zenoh RPC topic not found response", e).into()
                    })?;
                }
            }
            Ok(())
        }
    }
}
