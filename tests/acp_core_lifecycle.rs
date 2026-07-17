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
use local_agent::interfaces::{ACPClient, WorkspaceManager};
use local_agent::permissions::{null_sink, Manager as PermissionManager};
use local_agent::workspace::Manager as WorkspaceManagerImpl;
use tempfile::TempDir;

const MOCKAGENT_BIN: &str = "/tmp/mockagent";
const ACTOR_TIMEOUT: Duration = Duration::from_secs(15);

/// Build the production client with a registered scratch workspace.
async fn client_with_workspace() -> (Arc<Client>, TempDir, String) {
    assert!(
        std::path::Path::new(MOCKAGENT_BIN).exists(),
        "mockagent binary missing at {MOCKAGENT_BIN}; build it with `go build -o /tmp/mockagent ./cmd/mockagent/`"
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
        command: MOCKAGENT_BIN.into(),
        args: Vec::new(),
        models: vec![AgentModel {
            id: "mock-model".into(),
            name: "Mock model".into(),
        }],
        warning: String::new(),
    }]));
    let permissions = PermissionManager::new(Some(null_sink()));
    let client = Arc::new(Client::new(ClientDeps {
        registry,
        workspaces,
        permissions,
    }));

    (client, directory, workspace.id)
}

/// A cancelled prompt remains interrupted until the session is explicitly closed.
#[tokio::test]
async fn mockagent_session_prompt_cancel_close_lifecycle() {
    let (client, _directory, workspace_id) = client_with_workspace().await;
    let session = tokio::time::timeout(
        ACTOR_TIMEOUT,
        client.create_session("mockagent", "mock-model", &workspace_id),
    )
    .await
    .expect("session creation timed out")
    .expect("create mockagent session");
    assert_eq!(session.status, "idle");

    let prompt_client = Arc::clone(&client);
    let prompt_session_id = session.id.clone();
    let prompt = tokio::spawn(async move {
        prompt_client
            .send_prompt(&prompt_session_id, "describe the workspace", &[])
            .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    client
        .cancel_session(&session.id)
        .await
        .expect("cancel mockagent session");
    tokio::time::timeout(ACTOR_TIMEOUT, prompt)
        .await
        .expect("prompt did not finish")
        .expect("prompt task panicked")
        .expect_err("cancelled prompt must return without waiting for agent completion");

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
    let (client, _directory, workspace_id) = client_with_workspace().await;
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
