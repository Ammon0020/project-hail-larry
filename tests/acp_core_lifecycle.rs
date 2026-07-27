//! Lifecycle coverage for the production ACP session actor.
//!
//! These tests use the same Go mockagent fixture as `spike_acp`. They verify
//! the Rust client's registry and actor boundaries rather than SDK wire types.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use local_agent::acp::{AgentRegistry, Client, ClientDeps};
use local_agent::config::{AgentInfo, AgentModel};
use local_agent::events::{EventBus, Store};
use local_agent::interfaces::{ACPClient, EventStore, EventType, Session, WorkspaceManager};
use local_agent::permissions::{null_sink, Manager as PermissionManager};
use local_agent::workspace::Manager as WorkspaceManagerImpl;
use tempfile::TempDir;

/// Resolve the mockagent binary path.
///
/// Resolution order:
/// 1. `LOCAL_AGENT_MOCKAGENT_BIN` — explicit override (CI layouts that copy
///    the binary elsewhere).
/// 2. `CARGO_BIN_EXE_mockagent` — set by Cargo at runtime for tests in the
///    same package as the `mockagent` bin target (includes `.exe` on Windows).
/// 3. `${CARGO_TARGET_DIR:-${CARGO_MANIFEST_DIR}/target}/debug/mockagent[.exe]`
///    — fallback for unit tests where Cargo doesn't set `CARGO_BIN_EXE_*`.
fn mockagent_bin() -> String {
    if let Ok(path) = std::env::var("LOCAL_AGENT_MOCKAGENT_BIN") {
        return path;
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_mockagent") {
        return path;
    }
    let exe = if cfg!(windows) {
        "mockagent.exe"
    } else {
        "mockagent"
    };
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| format!("{}/target", env!("CARGO_MANIFEST_DIR")));
    format!("{target_dir}/debug/{exe}")
}

const ACTOR_TIMEOUT: Duration = Duration::from_secs(15);

/// Build the production client with a registered scratch workspace.
///
/// Returns `(client, temp_dir, workspace_id, event_bus)` so tests can query the
/// event store for streamed reply text assertions.
async fn client_with_workspace() -> (Arc<Client>, TempDir, String, Arc<EventBus>) {
    let mockagent_bin = mockagent_bin();
    assert!(
        std::path::Path::new(&mockagent_bin).exists(),
        "mockagent binary missing at {mockagent_bin}; build it with `cargo build --bin mockagent` or set LOCAL_AGENT_MOCKAGENT_BIN"
    );

    let directory = tempfile::tempdir().expect("create scratch workspace");
    let workspaces = Arc::new(WorkspaceManagerImpl::new());
    let workspace = workspaces
        .register(&directory.path().to_string_lossy())
        .await
        .expect("register scratch workspace");
    let registry = Arc::new(AgentRegistry::from_agents([AgentInfo {
        id: "mockagent".into(),
        name: "Mock agent".into(),
        command: mockagent_bin.clone(),
        args: Vec::new(),
        models: vec![AgentModel::new("mock-model".into(), "Mock model".into())],
        warning: String::new(),
    }]));
    let permissions = PermissionManager::new(Some(null_sink()));
    let event_bus = Arc::new(EventBus::new(
        Store::open(directory.path().join("events.db")).expect("open test event store"),
    ));
    let client = Arc::new(Client::new(ClientDeps {
        registry,
        workspaces,
        permissions,
        event_bus: event_bus.clone(),
        conversation_store: local_agent::acp::ConversationStore::new(None),
        mcp_config_path: None,
    }));

    (client, directory, workspace.id, event_bus)
}

/// Create a mockagent session, bounded by `ACTOR_TIMEOUT` so a hung spawn
/// fails loudly instead of stalling the test. Replaces the inline timeout +
/// `create_session` + double-`expect` boilerplate repeated across the tests.
async fn create_mock_session(client: &Client, workspace_id: &str) -> Session {
    tokio::time::timeout(
        ACTOR_TIMEOUT,
        client.create_session("mockagent", "mock-model", workspace_id),
    )
    .await
    .expect("session creation timed out")
    .expect("create mockagent session")
}

/// A cancelled prompt remains interrupted until the session is explicitly closed.
///
/// `send_prompt` admits the turn and returns immediately; cancel is observed via
/// session status / events rather than the HTTP return value.
#[tokio::test]
async fn mockagent_session_prompt_cancel_close_lifecycle() {
    let (client, _directory, workspace_id, _event_bus) = client_with_workspace().await;
    let session = create_mock_session(&client, &workspace_id).await;
    assert_eq!(session.status, "idle");

    client
        .send_prompt(&session.id, "describe the workspace", &[])
        .await
        .expect("prompt must be admitted");
    tokio::time::sleep(Duration::from_millis(50)).await;
    client
        .cancel_session(&session.id)
        .await
        .expect("cancel mockagent session");

    assert_eq!(
        client
            .get_session_info(&session.id)
            .expect("session remains registered after cancellation")
            .status,
        "interrupted"
    );

    client
        .close_session(&session.id)
        .await
        .expect("close mockagent session");
    assert!(client.get_session_info(&session.id).is_err());
    assert!(client.list_sessions().is_empty());
}

/// Concurrent creation publishes independent sessions and independent closes remove them.
#[tokio::test]
async fn concurrent_sessions_do_not_overwrite_registry_entries() {
    let (client, _directory, workspace_id, _event_bus) = client_with_workspace().await;
    let (first, second) = tokio::join!(
        client.create_session("mockagent", "mock-model", &workspace_id),
        client.create_session("mockagent", "mock-model", &workspace_id),
    );
    let first = first.expect("create first mockagent session");
    let second = second.expect("create second mockagent session");
    assert_ne!(first.id, second.id);

    let ids: HashSet<_> = client
        .list_sessions()
        .into_iter()
        .map(|session| session.id)
        .collect();
    assert_eq!(ids, HashSet::from([first.id.clone(), second.id.clone()]));

    let (first_close, second_close) = tokio::join!(
        client.close_session(&first.id),
        client.close_session(&second.id),
    );
    first_close.expect("close first mockagent session");
    second_close.expect("close second mockagent session");
    assert!(client.list_sessions().is_empty());
}

/// Concatenate all `StreamUpdate` text chunks for a session from the event
/// store, in id order. Used to assert on the agent's streamed reply content.
///
/// Only agent message chunks are included; thought chunks (`thought == true`)
/// are excluded so the profile marker prefix is at the start of the result.
async fn stream_text(event_bus: &EventBus, session_id: &str) -> String {
    let events = event_bus
        .store()
        .query(session_id, 0, 10_000)
        .await
        .expect("query session events");
    let mut out = String::new();
    for event in events {
        if event.event_type == EventType::StreamUpdate && !event.thought {
            out.push_str(&event.content);
        }
    }
    out
}

/// When the mock agent advertises the mode/profile config option (default),
/// the client sends `session/set_config_option { profile, <id> }` on session
/// setup. Verified via the mock's `[profile: <value>]` reply prefix: send a
/// prompt after setup and assert the reply starts with `[profile: code]`
/// (the built-in default profile id).
#[tokio::test]
async fn mockagent_initial_profile_sent_over_acp_when_capability_advertised() {
    let (client, _directory, workspace_id, event_bus) = client_with_workspace().await;
    let session = create_mock_session(&client, &workspace_id).await;
    assert_eq!(session.status, "idle");

    // Send a prompt; the mock prefixes its first streamed chunk with the
    // active profile marker (`[profile: <id>]`) only when it received a
    // `session/set_config_option` for the `profile` config id.
    client
        .send_prompt(&session.id, "hello", &[])
        .await
        .expect("prompt admitted");
    // send_prompt returns after admission; wait for streamed agent text.
    let text = tokio::time::timeout(ACTOR_TIMEOUT, async {
        loop {
            let text = stream_text(&event_bus, &session.id).await;
            if text.starts_with("[profile: code]") {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for profile marker stream");
    assert!(
        text.starts_with("[profile: code]"),
        "expected reply to start with `[profile: code]`, got: {text}"
    );

    client
        .close_session(&session.id)
        .await
        .expect("close mockagent session");
}

/// `set_session_profile` switches the active profile over ACP when the agent
/// advertised the capability. Verified via the mock's `[profile: ask]` marker
/// on the next prompt reply.
#[tokio::test]
async fn mockagent_set_session_profile_switches_over_acp() {
    let (client, _directory, workspace_id, event_bus) = client_with_workspace().await;
    let session = create_mock_session(&client, &workspace_id).await;

    // Switch to the `ask` profile over ACP, then prompt and assert the marker.
    client
        .set_session_profile(&session.id, "ask")
        .await
        .expect("set profile to ask");
    client
        .send_prompt(&session.id, "hello", &[])
        .await
        .expect("prompt admitted");
    let text = tokio::time::timeout(ACTOR_TIMEOUT, async {
        loop {
            let text = stream_text(&event_bus, &session.id).await;
            if text.starts_with("[profile: ask]") {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for profile marker stream");
    assert!(
        text.starts_with("[profile: ask]"),
        "expected reply to start with `[profile: ask]`, got: {text}"
    );

    client
        .close_session(&session.id)
        .await
        .expect("close mockagent session");
}

/// `set_session_profile` rejects unknown profile ids with a validation error
/// (HTTP 400 at the API layer) rather than silently normalizing to the default.
#[tokio::test]
async fn set_session_profile_rejects_unknown_profile_id() {
    let (client, _directory, workspace_id, _event_bus) = client_with_workspace().await;
    let session = create_mock_session(&client, &workspace_id).await;

    let result = client
        .set_session_profile(&session.id, "no-such-profile")
        .await;
    assert!(
        result.is_err(),
        "unknown profile id must be rejected, got: {result:?}"
    );
    let err = result.expect_err("error");
    assert!(
        err.to_string().contains("unknown profile id"),
        "expected validation message, got: {err}"
    );

    client
        .close_session(&session.id)
        .await
        .expect("close mockagent session");
}

/// `set_session_profile` returns not-found for a missing session id.
#[tokio::test]
async fn set_session_profile_missing_session_is_not_found() {
    let (client, _directory, _workspace_id, _event_bus) = client_with_workspace().await;
    let result = client
        .set_session_profile("sess-does-not-exist", "code")
        .await;
    assert!(result.is_err(), "missing session must error");
    let err = result.expect_err("error");
    assert!(
        err.to_string().contains("not found"),
        "expected not-found message, got: {err}"
    );
}
