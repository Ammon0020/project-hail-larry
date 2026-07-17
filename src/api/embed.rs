//! Embedded single-page application assets.
//!
//! `web/dist` is present but normally empty, so `cargo test` and `cargo run
//! -- --serve` do not require Node. Running `cd web && npm run build` supplies
//! the production Vite build before compiling the Rust binary.

use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::{EmbeddedFile, RustEmbed};

/// Build-time frontend bundle. Vite writes its production output to `web/dist`.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/web/dist/"]
struct Frontend;

/// Serve a static asset or fall back to the SPA entry point.
pub async fn serve(path: String) -> Response {
    let requested = path.trim_start_matches('/');
    let asset = if requested.is_empty() {
        Frontend::get("index.html")
    } else {
        Frontend::get(requested).or_else(|| Frontend::get("index.html"))
    };

    match asset {
        Some(asset) => embedded_response(requested, asset),
        None => development_fallback(),
    }
}

fn development_fallback() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [("content-type", "text/html; charset=utf-8")],
        "<!doctype html><title>Local Agent</title><main><h1>Frontend not built</h1>\
         <p>Run <code>cd web && npm run build</code>, then rebuild the Rust binary.</p></main>",
    )
        .into_response()
}

fn embedded_response(requested: &str, asset: EmbeddedFile) -> Response {
    let mime = if requested.is_empty() || !requested.contains('.') {
        "text/html; charset=utf-8"
    } else {
        mime_for(requested)
    };
    let mut response = Response::new(Body::from(asset.data.into_owned()));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    response
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") | Some("map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn spa_entry_is_available_without_a_frontend_build() {
        let response = serve("/".to_string()).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("embedded body");
        assert!(std::str::from_utf8(&body)
            .expect("UTF-8 fallback")
            .contains("Local Agent"));
    }
}
