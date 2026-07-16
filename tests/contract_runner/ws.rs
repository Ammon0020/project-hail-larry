//! WebSocket tests for the contract differential runner.
//!
//! The Go fixture harness captures WS fixtures using a real nhooyr.io/websocket
//! client over the httptest server. The black-box runner tests the same
//! behaviors over a real TCP connection to the running backend:
//!
//! - **Origin rejection**: connect with a cross-origin Origin header → expect
//!   the server to reject the upgrade (HTTP 403, matching the golden fixture).
//! - **Connection success**: connect with a same-origin Origin header → expect
//!   the WebSocket handshake to succeed and the connection to stay open.
//!
//! Auth rejection (401 from non-loopback) and event broadcast are skipped:
//! - Auth rejection requires a non-loopback TCP connection, which the runner
//!   can't simulate.
//! - Event broadcast requires driving in-process `server.OnEvent` calls, which
//!   is not possible black-box. The runner could trigger events via API calls
//!   (e.g., create a session), but the resulting events would differ from the
//!   synthetic fixture events. This is documented as a known limitation.

use std::time::Duration;

use anyhow::Context;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

use crate::harness::BackendHarness;

/// Test that a cross-origin WebSocket connection is rejected with 403.
/// Mirrors `ws_origin_rejection.json` golden fixture.
pub async fn test_origin_rejection(harness: &BackendHarness) {
    let ws_url = format!("ws://127.0.0.1:{}/ws", harness.port);

    // Build a WS upgrade request with a cross-origin Origin header.
    let req = Request::builder()
        .method("GET")
        .uri("/ws")
        .header("Host", format!("127.0.0.1:{}", harness.port))
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .header("Origin", "http://evil.example.com")
        .header("Sec-WebSocket-Protocol", "jsonrpc")
        .header("Authorization", "Bearer dummy:dummy")
        .body(())
        .expect("build WS request");

    // Attempt the connection. The server should reject the upgrade before the
    // WS handshake completes, returning an HTTP error (403) instead.
    let result = tokio_tungstenite::connect_async(req).await;

    match result {
        Ok(_) => {
            // If the connection succeeded, the server didn't reject the
            // cross-origin request — this is a security bug.
            panic!(
                "WS origin rejection: expected rejection (403), but connection succeeded"
            );
        }
        Err(e) => {
            // The error should indicate an HTTP rejection. Check that it's a
            // 403 status. tokio-tungstenite wraps the HTTP error response as
            // a tungstenite::Error::Http with a status code.
            let status = extract_http_status(&e);
            eprintln!("[contract] WS origin rejection: got status {status}");
            assert_eq!(
                status, 403,
                "WS origin rejection: expected 403, got {status}. Error: {e}"
            );
            eprintln!("[contract] PASS: ws_origin_rejection (403)");
        }
    }
}

/// Test that a same-origin WebSocket connection succeeds.
/// Mirrors `ws_auth_success.jsonl` golden fixture (connection success only;
/// event broadcast is not tested black-box).
pub async fn test_connection_success(harness: &BackendHarness) {
    let ws_url = format!("ws://127.0.0.1:{}/ws", harness.port);
    let host = format!("127.0.0.1:{}", harness.port);

    // Build a WS upgrade request with a same-origin Origin header (matching
    // the server's host). This mirrors what a same-origin browser WS handshake
    // would send.
    let req = Request::builder()
        .method("GET")
        .uri("/ws")
        .header("Host", &host)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .header("Origin", format!("http://{host}"))
        .header("Sec-WebSocket-Protocol", "jsonrpc")
        .header("Authorization", "Bearer dummy:dummy")
        .body(())
        .expect("build WS request");

    let (ws_stream, response) = match tokio_tungstenite::connect_async(req).await {
        Ok(conn) => conn,
        Err(e) => {
            panic!(
                "WS connection success: expected successful upgrade, got error: {e}"
            );
        }
    };

    // Verify the HTTP upgrade response is 101 Switching Protocols.
    let status = response.status().as_u16();
    assert_eq!(
        status, 101,
        "WS connection success: expected 101 Switching Protocols, got {status}"
    );

    // Verify we can send and receive a ping/pong (connection is alive).
    use futures_util::{SinkExt, StreamExt};
    let (mut write, mut read) = ws_stream.split();

    // Send a Ping frame.
    let ping_payload = vec![1u8, 2, 3];
    write.send(Message::Ping(ping_payload.clone())).await.expect("send ping");

    // Wait for a Pong response (or any frame) within a timeout.
    let frame = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .expect("timeout waiting for WS frame")
        .expect("WS stream closed");

    eprintln!("[contract] WS connection success: received frame: {frame:?}");

    // Close the connection cleanly.
    let _ = write.send(Message::Close(None)).await;

    eprintln!("[contract] PASS: ws_connection_success (101 + ping/pong)");
}

/// Extract the HTTP status code from a tungstenite error. When the server
/// rejects the WS upgrade, tungstenite returns an Error::Http variant that
/// contains the HTTP response status code.
fn extract_http_status(e: &tokio_tungstenite::tungstenite::Error) -> u16 {
    use tokio_tungstenite::tungstenite::error::Error;
    match e {
        Error::Http(resp) => resp.status().as_u16(),
        Error::ConnectionClosed => 0,
        _ => {
            // Some versions wrap the HTTP error differently. Check the error
            // chain for a status code.
            eprintln!("[contract] WS error (non-Http): {e:?}");
            0
        }
    }
}
