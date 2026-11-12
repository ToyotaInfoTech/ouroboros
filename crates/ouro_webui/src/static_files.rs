// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Static file serving for the embedded SPA.
//!
//! In release builds, serves files from the embedded `webui/dist` directory.
//! Returns the SPA index.html for any unmatched route (client-side routing).

use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use rust_embed::Embed;
use tracing::instrument;

#[derive(Embed)]
#[folder = "../../webui/dist"]
struct Asset;

#[instrument]
pub fn dump_embedded_files() {
    tracing::debug!("Embedded static files:");
    for (i, file) in Asset::iter().enumerate() {
        tracing::debug!("  [{:>2}]: {}", i + 1, file.as_ref());
    }
}

/// Fallback handler that serves embedded static files or the SPA index.
///
/// Args:
///     uri: Requested URI path
///
/// Returns:
///     Response: Static file content or index.html for SPA routing
pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Try to serve the exact file
    if !path.is_empty() {
        if let Some(file) = Asset::get(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(axum::body::Body::from(file.data.to_vec()))
                .expect("building a valid HTTP response should not fail");
        }
    }

    // Fallback to index.html for SPA client-side routing
    match Asset::get("index.html") {
        Some(file) => Html(
            String::from_utf8(file.data.to_vec())
                .unwrap_or_else(|_| "<!DOCTYPE html><html><body>WebUI not built</body></html>".to_string()),
        )
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Html("<!DOCTYPE html><html><body><h1>WebUI not built</h1><p>Run <code>cd webui &amp;&amp; npm run build</code> to build the frontend.</p></body></html>".to_string()),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_embed_index() {
        assert!(Asset::get("index.html").is_some());
    }
}
