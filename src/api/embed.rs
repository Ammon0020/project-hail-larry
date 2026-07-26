//! Embedded single-page application assets.
//!
//! Vite writes the production SPA to `web/dist/`. `build.rs` requires
//! `web/dist/index.html` before `cargo build` succeeds; run
//! `cd web && npm run build` or `./build.sh` first. Assets are baked in via
//! `rust-embed` at compile time.

use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::{EmbeddedFile, RustEmbed};

/// Build-time frontend bundle. Vite writes its production output to `web/dist`.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/web/dist/"]
struct Frontend;

/// Serve a static asset or fall back to the SPA entry point.
#[allow(clippy::unused_async)]
// kept async for caller await: `api::mod` and tests await this handler.
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
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
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

    /// `/` must return the embedded SPA when `web/dist/index.html` was present
    /// at compile time (required by `build.rs`). Asserts a 200 HTML response
    /// that identifies Local Agent — smoke that rust-embed baked the entry in.
    #[tokio::test]
    async fn spa_entry_serves_embedded_index() {
        let response = serve("/".to_string()).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "build.rs requires web/dist/index.html; expected embedded SPA, not fallback"
        );
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("embedded body");
        let text = std::str::from_utf8(&body).expect("UTF-8 SPA body");
        assert!(
            text.contains("Local Agent") || text.contains("<!"),
            "SPA response missing HTML marker"
        );
    }
}
