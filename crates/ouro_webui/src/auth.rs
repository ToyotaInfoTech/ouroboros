// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Request guard for the REST/WebSocket API.
//!
//! Rejects cross-origin requests (CSRF / cross-site WebSocket hijacking) and,
//! when a token is configured, requires `Authorization: Bearer <token>`.
//! Browsers cannot set headers on WebSocket handshakes, so the token may also
//! be offered as a `token.<token>` entry of `Sec-WebSocket-Protocol`.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// WebSocket subprotocol prefix that carries the access token.
const WS_TOKEN_PREFIX: &str = "token.";

/// Axum middleware enforcing the same-origin policy and the optional bearer token.
///
/// Args:
///     token: Expected token, or `None` to disable authentication
///     req: Incoming request
///     next: Remaining handler chain
///
/// Returns:
///     Response: `403` for cross-origin requests, `401` for a missing or wrong token
pub async fn guard(State(token): State<Option<Arc<str>>>, req: Request, next: Next) -> Response {
    if !origin_allowed(req.headers()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(expected) = token.as_deref() {
        let authorized = presented_token(req.headers())
            .is_some_and(|presented| constant_time_eq(presented.as_bytes(), expected.as_bytes()));
        if !authorized {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Allows requests without `Origin` and requests whose `Origin` authority equals `Host`.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let (Ok(origin), Some(Ok(host))) = (
        origin.to_str(),
        headers.get(header::HOST).map(|h| h.to_str()),
    ) else {
        return false;
    };
    origin
        .split_once("://")
        .is_some_and(|(_, authority)| authority.eq_ignore_ascii_case(host))
}

/// Extracts the presented token from the `Authorization` or `Sec-WebSocket-Protocol` header.
fn presented_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        return value.strip_prefix("Bearer ").map(|t| t.trim().to_string());
    }
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)?
        .to_str()
        .ok()?
        .split(',')
        .find_map(|p| p.trim().strip_prefix(WS_TOKEN_PREFIX).map(str::to_string))
}

/// Compares two byte strings without short-circuiting on the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request as HttpRequest, middleware, routing::get, Router};
    use tower::ServiceExt as _;

    fn app(token: Option<&str>) -> Router {
        let token = token.map(Arc::<str>::from);
        Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(token, guard))
    }

    async fn status(app: Router, req: HttpRequest<Body>) -> StatusCode {
        app.oneshot(req)
            .await
            .map(|r| r.status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    #[tokio::test]
    async fn no_token_configured_allows_requests() {
        let req = HttpRequest::get("/").body(Body::empty()).unwrap();
        assert_eq!(status(app(None), req).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn token_is_required_when_configured() {
        let missing = HttpRequest::get("/").body(Body::empty()).unwrap();
        assert_eq!(
            status(app(Some("s3cret")), missing).await,
            StatusCode::UNAUTHORIZED
        );

        let wrong = HttpRequest::get("/")
            .header(header::AUTHORIZATION, "Bearer nope")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status(app(Some("s3cret")), wrong).await,
            StatusCode::UNAUTHORIZED
        );

        let ok = HttpRequest::get("/")
            .header(header::AUTHORIZATION, "Bearer s3cret")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(app(Some("s3cret")), ok).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn websocket_subprotocol_token_is_accepted() {
        let req = HttpRequest::get("/")
            .header(header::SEC_WEBSOCKET_PROTOCOL, "ouro.v1, token.s3cret")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(app(Some("s3cret")), req).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn cross_origin_requests_are_rejected() {
        let cross = HttpRequest::get("/")
            .header(header::HOST, "localhost:3001")
            .header(header::ORIGIN, "http://evil.example")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(app(None), cross).await, StatusCode::FORBIDDEN);

        let same = HttpRequest::get("/")
            .header(header::HOST, "localhost:3001")
            .header(header::ORIGIN, "http://localhost:3001")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(app(None), same).await, StatusCode::OK);

        let null_origin = HttpRequest::get("/")
            .header(header::HOST, "localhost:3001")
            .header(header::ORIGIN, "null")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(app(None), null_origin).await, StatusCode::FORBIDDEN);
    }

    #[test]
    fn constant_time_eq_checks_length_and_content() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
