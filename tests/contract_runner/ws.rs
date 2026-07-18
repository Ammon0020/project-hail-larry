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
//! - **Live broadcast**: pair + revoke via REST → receive `DeviceRevocationPending`
//!   on an open `/ws` connection (API-driven; not the synthetic Go OnEvent frames).
//! - **`?after=` replay + live**: seed durable events, reconnect with a cursor,
//!   assert replay then a subsequent live frame.
//! - **Auth rejection**: dial a non-loopback local address without credentials
//!   → expect HTTP 401 (daemon binds `0.0.0.0` in the harness).
//!
//! **Slow-client recovery** is not exercised black-box: filling the 64-deep
//! send buffer and observing durable resync requires controlling the hub's
//! try_send path. Unit coverage lives in `src/sync/tests.rs`
//! (`lagged_resync_from_bus_on_full_buffer`).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::harness::{first_non_loopback_ipv4, BackendHarness};

/// Test that a cross-origin WebSocket connection is rejected with 403.
/// Mirrors `ws_origin_rejection` behavior (evil Origin on loopback).
pub async fn test_origin_rejection(harness: &BackendHarness) {
    let ws_url = format!("ws://127.0.0.1:{}/ws", harness.port);

    // Build a WS upgrade request with a cross-origin Origin header.
    // IntoClientRequest parses the ws:// URL into a proper HTTP request with
    // Host, Upgrade, Connection, Sec-WebSocket-Key, and Sec-WebSocket-Version
    // headers set automatically. We then override the Origin to a cross-origin
    // value to trigger the server's origin check.
    let mut req = ws_url
        .into_client_request()
        .expect("parse ws:// URL into client request");
    req.headers_mut().insert(
        "Origin",
        "http://evil.example.com"
            .parse()
            .expect("valid Origin header"),
    );
    req.headers_mut().insert(
        "Authorization",
        "Bearer dummy:dummy"
            .parse()
            .expect("valid Authorization header"),
    );

    // Attempt the connection. The server should reject the upgrade before the
    // WS handshake completes, returning an HTTP error (403) instead.
    let result = tokio_tungstenite::connect_async(req).await;

    match result {
        Ok(_) => {
            // If the connection succeeded, the server didn't reject the
            // cross-origin request — this is a security bug.
            panic!("WS origin rejection: expected rejection (403), but connection succeeded");
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
/// event broadcast is covered by [`test_live_broadcast`]).
pub async fn test_connection_success(harness: &BackendHarness) {
    let (mut write, mut read, status) = connect_loopback(harness, "").await;
    assert_eq!(
        status, 101,
        "WS connection success: expected 101 Switching Protocols, got {status}"
    );

    // Verify we can send and receive a ping/pong (connection is alive).
    let ping_payload = vec![1u8, 2, 3];
    write
        .send(Message::Ping(ping_payload.clone().into()))
        .await
        .expect("send ping");

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

/// Live broadcast: open `/ws`, then drive a durable event via pair+revoke REST
/// and assert the client receives a `DeviceRevocationPending` frame.
pub async fn test_live_broadcast(harness: &BackendHarness) {
    let (mut write, mut read, status) = connect_loopback(harness, "").await;
    assert_eq!(status, 101, "live broadcast: expected 101, got {status}");

    // Brief settle so the hub registers the client before we broadcast.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let device_id = pair_device(harness, "ws-broadcast-device").await;
    let revoke_status = revoke_device(harness, &device_id).await;
    assert_eq!(
        revoke_status, 202,
        "live broadcast: revoke should return 202 (grace pending), got {revoke_status}"
    );

    let event = recv_event_frame(&mut read, Duration::from_secs(5)).await;
    let event_type = event
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(
        event_type, "DeviceRevocationPending",
        "live broadcast: expected DeviceRevocationPending, got {event}"
    );
    assert!(
        event.get("id").and_then(|v| v.as_i64()).unwrap_or(0) > 0,
        "live broadcast: event must have a durable id > 0"
    );

    let _ = write.send(Message::Close(None)).await;
    eprintln!("[contract] PASS: ws_live_broadcast (DeviceRevocationPending via REST)");
}

/// `?after=` replay then live: seed two durable events, reconnect with a
/// cursor after the first, assert the second is replayed, then drive a third
/// event and assert live delivery. Mirrors `golden/ws/ws_after_replay.jsonl`.
pub async fn test_after_replay_and_live(harness: &BackendHarness) {
    // Seed event 1: pair + revoke.
    let device_a = pair_device(harness, "ws-replay-a").await;
    let status = revoke_device(harness, &device_a).await;
    assert_eq!(status, 202, "seed revoke A: expected 202, got {status}");

    let events_after_a = list_events(harness).await;
    let event_a = events_after_a
        .last()
        .cloned()
        .expect("expected at least one event after revoke A");
    let id_a = event_a["id"].as_i64().expect("event A id");

    // Seed event 2: cancel the pending revocation.
    let action_id = event_a["requestId"]
        .as_str()
        .expect("requestId on DeviceRevocationPending")
        .to_string();
    cancel_revocation(harness, &action_id).await;

    let events_after_cancel = list_events(harness).await;
    let event_b = events_after_cancel
        .iter()
        .find(|e| e["id"].as_i64().unwrap_or(0) > id_a)
        .cloned()
        .expect("expected cancel event after event A");
    let id_b = event_b["id"].as_i64().expect("event B id");
    assert_eq!(
        event_b["type"].as_str(),
        Some("DeviceRevocationCancelled"),
        "seed cancel: unexpected type {event_b}"
    );

    // Reconnect with cursor after A → expect B via replay only.
    let (mut write, mut read, status) = connect_loopback(harness, &format!("after={id_a}")).await;
    assert_eq!(status, 101, "after= connect: expected 101, got {status}");

    let replayed = recv_event_frame(&mut read, Duration::from_secs(5)).await;
    assert_eq!(
        replayed["id"].as_i64(),
        Some(id_b),
        "after= replay: expected event B id={id_b}, got {replayed}"
    );
    assert_eq!(
        replayed["type"].as_str(),
        Some("DeviceRevocationCancelled"),
        "after= replay: unexpected type"
    );

    // Live transition: pair + revoke another device while connected.
    let device_c = pair_device(harness, "ws-replay-c").await;
    let status = revoke_device(harness, &device_c).await;
    assert_eq!(status, 202, "live revoke C: expected 202, got {status}");

    let live = recv_event_frame(&mut read, Duration::from_secs(5)).await;
    assert_eq!(
        live["type"].as_str(),
        Some("DeviceRevocationPending"),
        "after= live: expected DeviceRevocationPending, got {live}"
    );
    let id_c = live["id"].as_i64().expect("live event id");
    assert!(id_c > id_b, "after= live: expected id > {id_b}, got {id_c}");

    let _ = write.send(Message::Close(None)).await;
    eprintln!("[contract] PASS: ws_after_replay (replay id={id_b} then live id={id_c})");
}

/// Auth rejection: dial via a non-loopback local IP without credentials → 401.
/// Requires the harness bind address `0.0.0.0` and a usable LAN/global IPv4.
pub async fn test_auth_rejection(harness: &BackendHarness) {
    let Some(lan_ip) = first_non_loopback_ipv4() else {
        eprintln!(
            "[contract] SKIP: ws_auth_rejection — no non-loopback IPv4 on this host \
             (cannot exercise LAN auth gate black-box)"
        );
        return;
    };

    let ws_url = format!("ws://{lan_ip}:{}/ws", harness.port);
    let host = format!("{lan_ip}:{}", harness.port);

    let mut req = ws_url
        .into_client_request()
        .expect("parse lan ws:// URL into client request");
    // Same-origin Origin so the failure is auth (401), not CSRF (403).
    req.headers_mut().insert(
        "Origin",
        format!("http://{host}")
            .parse()
            .expect("valid Origin header"),
    );
    // No Authorization / deviceId / secret — non-loopback peer must fail auth.

    let result = tokio_tungstenite::connect_async(req).await;
    match result {
        Ok(_) => panic!(
            "WS auth rejection: expected 401 for non-loopback dial to {lan_ip}, but upgrade succeeded"
        ),
        Err(e) => {
            let status = extract_http_status(&e);
            eprintln!("[contract] WS auth rejection via {lan_ip}: got status {status}");
            assert_eq!(
                status, 401,
                "WS auth rejection: expected 401, got {status}. Error: {e}"
            );
            eprintln!("[contract] PASS: ws_auth_rejection (401 via non-loopback)");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type WsRead = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;
type WsWrite = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

/// Same-origin loopback WS connect; `query` is appended after `/ws` (may be empty).
async fn connect_loopback(harness: &BackendHarness, query: &str) -> (WsWrite, WsRead, u16) {
    let path = if query.is_empty() {
        "/ws".to_string()
    } else {
        format!("/ws?{query}")
    };
    let ws_url = format!("ws://127.0.0.1:{}{}", harness.port, path);
    let host = format!("127.0.0.1:{}", harness.port);

    let mut req = ws_url
        .into_client_request()
        .expect("parse ws:// URL into client request");
    req.headers_mut().insert(
        "Origin",
        format!("http://{host}")
            .parse()
            .expect("valid Origin header"),
    );
    req.headers_mut().insert(
        "Authorization",
        "Bearer dummy:dummy"
            .parse()
            .expect("valid Authorization header"),
    );

    let (ws_stream, response) = match tokio_tungstenite::connect_async(req).await {
        Ok(conn) => conn,
        Err(e) => panic!("WS connect failed ({path}): {e}"),
    };
    let status = response.status().as_u16();
    let (write, read) = ws_stream.split();
    (write, read, status)
}

/// Pair a device via initiate + verify-passcode; returns the device id.
async fn pair_device(harness: &BackendHarness, device_name: &str) -> String {
    let client = reqwest::Client::new();
    let initiate_url = format!("{}/api/pair/initiate", harness.base_url);
    let body = serde_json::json!({
        "host": "localhost",
        "port": harness.port,
    });
    let session: Value = client
        .post(&initiate_url)
        .json(&body)
        .send()
        .await
        .expect("pair initiate")
        .error_for_status()
        .expect("pair initiate status")
        .json()
        .await
        .expect("pair initiate json");
    let passcode = session["passcode"]
        .as_str()
        .expect("passcode in initiate response")
        .to_string();

    let verify_url = format!("{}/api/pair/verify-passcode", harness.base_url);
    let verify_body = serde_json::json!({
        "passcode": passcode,
        "deviceName": device_name,
    });
    let cred: Value = client
        .post(&verify_url)
        .json(&verify_body)
        .send()
        .await
        .expect("pair verify")
        .error_for_status()
        .expect("pair verify status")
        .json()
        .await
        .expect("pair verify json");
    cred["id"]
        .as_str()
        .expect("device id in credential")
        .to_string()
}

/// `DELETE /api/devices/{id}` — returns the HTTP status (202 when grace pending).
async fn revoke_device(harness: &BackendHarness, device_id: &str) -> u16 {
    let client = reqwest::Client::new();
    let url = format!("{}/api/devices/{device_id}", harness.base_url);
    client
        .delete(&url)
        .send()
        .await
        .expect("revoke device")
        .status()
        .as_u16()
}

/// `POST /api/devices/cancel-revocation` with `{"actionId":...}`.
async fn cancel_revocation(harness: &BackendHarness, action_id: &str) {
    let client = reqwest::Client::new();
    let url = format!("{}/api/devices/cancel-revocation", harness.base_url);
    let body = serde_json::json!({ "actionId": action_id });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .expect("cancel revocation");
    assert!(
        resp.status().is_success(),
        "cancel revocation failed: {}",
        resp.status()
    );
}

async fn list_events(harness: &BackendHarness) -> Vec<Value> {
    let url = format!("{}/api/events", harness.base_url);
    let body: Value = reqwest::get(&url)
        .await
        .expect("GET /api/events")
        .error_for_status()
        .expect("events status")
        .json()
        .await
        .expect("events json");
    body.as_array()
        .cloned()
        .unwrap_or_else(|| panic!("/api/events did not return an array: {body}"))
}

/// Read the next text frame and parse it as a JSON event (skip ping/pong).
async fn recv_event_frame(read: &mut WsRead, timeout: Duration) -> Value {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("timeout waiting for WS event frame");
        }
        let frame = tokio::time::timeout(remaining, read.next())
            .await
            .expect("timeout waiting for WS frame")
            .expect("WS stream closed")
            .expect("WS frame error");
        match frame {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .unwrap_or_else(|e| panic!("parse WS event JSON: {e}; text={text}"));
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected WS frame while waiting for event: {other:?}"),
        }
    }
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
