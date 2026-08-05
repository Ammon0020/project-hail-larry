use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tempfile::TempDir;

/// Timeout for async state-transition polls in these tests. Generous enough
/// to absorb Windows CI's slower process spawn + stdio IPC (the mock agent
/// streams with a 20ms-per-word delay) while still failing fast on a real
/// hang. Keep well below CI's per-test job timeout.
const TEST_POLL_TIMEOUT: Duration = Duration::from_secs(15);

use super::super::actor::{Handle, TerminalOutcome};
use super::super::diagnostics::StderrTail;
use super::super::registry::SessionEntry;
use super::super::{Client, ClientDeps};
use crate::acp::providers::SessionCaps;
use crate::acp::store::StoredSession;
use crate::acp::{AgentRegistry, ConversationStore};
use crate::config::{AgentInfo, AgentModel};
use crate::events::{EventBus, Store};
use crate::interfaces::{
    ACPClient, AppError, Event, EventStore, EventType, PermissionDecision, PermissionManager,
    PermissionRequest, SessionInfo, WorkspaceManager,
};
use crate::workspace::Manager as WorkspaceRegistry;
use chrono::Utc;

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
    // Unit tests inside the lib: Cargo doesn't set CARGO_BIN_EXE_*, so
    // reconstruct the path from the manifest dir / target dir.
    let exe = if cfg!(windows) {
        "mockagent.exe"
    } else {
        "mockagent"
    };
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| format!("{}/target", env!("CARGO_MANIFEST_DIR")));
    format!("{target_dir}/debug/{exe}")
}

/// Records only session cleanup so the test can prove the local ID is used.
#[derive(Default)]
pub(crate) struct RecordingPermissions {
    pub(crate) cleared_sessions: Mutex<Vec<String>>,
}

#[async_trait]
impl PermissionManager for RecordingPermissions {
    async fn request(&self, _request: PermissionRequest) -> Result<PermissionDecision, AppError> {
        Ok(PermissionDecision::Deny)
    }

    async fn respond(
        &self,
        _request_id: &str,
        _decision: PermissionDecision,
    ) -> Result<(), AppError> {
        Ok(())
    }

    fn clear_session(&self, session_id: &str) {
        if let Ok(mut cleared) = self.cleared_sessions.lock() {
            cleared.push(session_id.to_string());
        }
    }

    fn get_pending(&self) -> Vec<PermissionRequest> {
        Vec::new()
    }
}

/// Create an isolated ACP client with a configurable conversation store and
/// no live sessions (used to simulate post-restart dormant metadata).
///
/// Registers both a default mock agent (mode/profile capability on) and a
/// `mock-nocap` agent that suppresses the mode option via
/// `MOCKAGENT_NO_MODE_CAP=1` for rebind/fallback coverage.
async fn mock_client_empty(
    conversation_store: ConversationStore,
) -> (Arc<Client>, Arc<RecordingPermissions>, TempDir, String) {
    let mockagent_bin = mockagent_bin();
    assert!(
            Path::new(&mockagent_bin).exists(),
            "mockagent binary missing at {mockagent_bin}; build it with `cargo build --bin mockagent` or set LOCAL_AGENT_MOCKAGENT_BIN"
        );
    let tempdir = TempDir::new().expect("temporary workspace");
    let workspaces = Arc::new(WorkspaceRegistry::new());
    let workspace = workspaces
        .register(tempdir.path().to_str().expect("UTF-8 temporary workspace"))
        .await
        .expect("register temporary workspace");
    let permissions = Arc::new(RecordingPermissions::default());
    let event_bus = Arc::new(EventBus::new(
        Store::open(tempdir.path().join("events.db")).expect("open test event store"),
    ));
    let mock_model = AgentModel::new("mock-model".to_string(), "Mock model".to_string());
    #[cfg(unix)]
    let (nocap_cmd, nocap_args): (String, Vec<String>) = (
        "env".to_string(),
        vec!["MOCKAGENT_NO_MODE_CAP=1".to_string(), mockagent_bin.clone()],
    );
    #[cfg(windows)]
    let (nocap_cmd, nocap_args): (String, Vec<String>) = (
        "cmd".to_string(),
        vec![
            "/C".to_string(),
            format!("set MOCKAGENT_NO_MODE_CAP=1&&{}", mockagent_bin),
        ],
    );
    let registry = Arc::new(AgentRegistry::from_agents([
        AgentInfo {
            id: "mock".to_string(),
            name: "Mock agent".to_string(),
            command: mockagent_bin.clone(),
            args: Vec::new(),
            models: vec![mock_model.clone()],
            warning: String::new(),
        },
        // `env` injects MOCKAGENT_NO_MODE_CAP without process-global set_var.
        AgentInfo {
            id: "mock-nocap".to_string(),
            name: "Mock agent without mode cap".to_string(),
            command: nocap_cmd,
            args: nocap_args,
            models: vec![mock_model],
            warning: String::new(),
        },
    ]));
    let client = Arc::new(Client::new(ClientDeps {
        registry,
        workspaces,
        permissions: permissions.clone(),
        event_bus,
        conversation_store,
        mcp_config_path: None,
    }));
    (client, permissions, tempdir, workspace.id)
}

/// Create an isolated local ACP client backed by the deterministic Go fixture.
pub(crate) async fn mock_client() -> (Arc<Client>, Arc<RecordingPermissions>, TempDir) {
    let (client, permissions, tempdir, workspace_id) =
        mock_client_empty(ConversationStore::new(None)).await;
    let session = client
        .create_session("mock", "mock-model", &workspace_id)
        .await
        .expect("create mock ACP session");
    assert_eq!(
        session.status, "idle",
        "successful startup must publish idle metadata"
    );
    assert_eq!(
        client
            .get_session_info(&session.id)
            .expect("session remains registered after startup")
            .status,
        "idle"
    );
    client
        .rename_session(&session.id, "Mock session")
        .expect("session remains registered after startup");
    (client, permissions, tempdir)
}

/// Wait until `send_prompt` has atomically reserved the session's turn.
pub(crate) async fn wait_until_running(client: &Client, session_id: &str) {
    tokio::time::timeout(TEST_POLL_TIMEOUT, async {
        loop {
            if client
                .get_session_info(session_id)
                .is_ok_and(|session| session.status == "running")
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("prompt did not reserve its session slot");
}

/// Startup failure (nonexistent agent command) must return an error and
/// leave no entry in the live session registry.
#[tokio::test]
async fn startup_failure_before_publication_is_not_registered() {
    let tempdir = TempDir::new().expect("temporary workspace");
    let workspaces = Arc::new(WorkspaceRegistry::new());
    let workspace = workspaces
        .register(tempdir.path().to_str().expect("UTF-8 temporary workspace"))
        .await
        .expect("register temporary workspace");
    let permissions = Arc::new(RecordingPermissions::default());
    let event_bus = Arc::new(EventBus::new(
        Store::open(tempdir.path().join("events.db")).expect("open test event store"),
    ));
    let registry = Arc::new(AgentRegistry::from_agents([AgentInfo {
        id: "broken".to_string(),
        name: "Broken agent".to_string(),
        command: "/nonexistent/binary/that/does/not/exist".to_string(),
        args: Vec::new(),
        models: vec![AgentModel::new("m".to_string(), "M".to_string())],
        warning: String::new(),
    }]));
    let client = Arc::new(Client::new(ClientDeps {
        registry,
        workspaces,
        permissions: permissions.clone(),
        event_bus,
        conversation_store: ConversationStore::new(None),
        mcp_config_path: None,
    }));

    let error = client
        .create_session("broken", "m", &workspace.id)
        .await
        .expect_err("startup with a nonexistent agent must fail");
    assert!(
        error.to_string().to_ascii_lowercase().contains("spawn"),
        "startup error must mention spawn failure: {error}"
    );
    assert!(
        client.list_sessions().is_empty(),
        "failed startup must not publish a live session"
    );
}

/// Unexpected post-startup exit (agent crashes after initialize + new)
/// must transition the session to Failed and append an `AgentExited` event.
#[tokio::test]
async fn unexpected_post_startup_exit_marks_session_failed() {
    let tempdir = TempDir::new().expect("temporary workspace");
    let workspaces = Arc::new(WorkspaceRegistry::new());
    let workspace = workspaces
        .register(tempdir.path().to_str().expect("UTF-8 temporary workspace"))
        .await
        .expect("register temporary workspace");
    let permissions = Arc::new(RecordingPermissions::default());
    let event_bus = Arc::new(EventBus::new(
        Store::open(tempdir.path().join("events.db")).expect("open test event store"),
    ));
    // The env wrapper injects MOCKAGENT_EXIT_AFTER_INIT so the mock exits
    // right after session/new, simulating an unexpected post-startup crash.
    let registry = Arc::new(AgentRegistry::from_agents([AgentInfo {
        id: "mock-exit".to_string(),
        name: "Mock agent that exits after init".to_string(),
        command: "env".to_string(),
        args: vec!["MOCKAGENT_EXIT_AFTER_INIT=1".to_string(), mockagent_bin()],
        models: vec![AgentModel::new(
            "mock-model".to_string(),
            "Mock model".to_string(),
        )],
        warning: String::new(),
    }]));
    let client = Arc::new(Client::new(ClientDeps {
        registry,
        workspaces,
        permissions: permissions.clone(),
        event_bus: event_bus.clone(),
        conversation_store: ConversationStore::new(None),
        mcp_config_path: None,
    }));

    // create_session may succeed (readiness fires before the crash) or
    // fail (if the SDK cancels the closure before registration). Either
    // way, the terminal watcher must converge the session to Failed.
    let session_id = match client
        .create_session("mock-exit", "mock-model", &workspace.id)
        .await
    {
        Ok(session) => session.id,
        Err(_) => {
            // Startup failure path: the session was never published, so
            // there is nothing to transition. This is also acceptable.
            return;
        }
    };

    // Wait for the terminal watcher to mark the session Failed.
    tokio::time::timeout(TEST_POLL_TIMEOUT, async {
        loop {
            if client
                .get_session_info(&session_id)
                .is_ok_and(|s| s.status == "failed")
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("session must transition to failed after unexpected actor exit");

    // An AgentExited event must be appended to the durable store.
    let events = event_bus
        .query(&session_id, 0, 1000)
        .await
        .expect("query session events");
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::AgentExited),
        "unexpected actor exit must append an AgentExited event"
    );
}

/// Session lifecycle changes must be published so other clients can reload.
#[tokio::test]
async fn session_creation_and_closure_append_lifecycle_events() {
    let (client, _permissions, _workspace, workspace_id) =
        mock_client_empty(ConversationStore::new(None)).await;
    let session = client
        .create_session("mock", "mock-model", &workspace_id)
        .await
        .expect("create mock ACP session");

    let created_events = client
        .deps
        .event_bus
        .query(&session.id, 0, 100)
        .await
        .expect("query creation events");
    assert!(
        created_events
            .iter()
            .any(|event| event.event_type == EventType::SessionCreated),
        "creating a session must append a SessionCreated event"
    );

    client
        .close_session(&session.id)
        .await
        .expect("close mock ACP session");

    let events = client
        .deps
        .event_bus
        .query(&session.id, 0, 100)
        .await
        .expect("query lifecycle events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == EventType::SessionClosed),
        "closing a session must append a SessionClosed event"
    );
}

/// Rebinding replaces only transport ownership: the stable local ID,
/// display metadata, and durable transcript must survive intact.
#[tokio::test]
async fn rebind_preserves_session_identity_and_event_history() {
    let (client, _permissions, _workspace) = mock_client().await;
    let session = client.list_sessions().pop().expect("one mock session");
    client
        .send_prompt(&session.id, "record this before rebind", &[])
        .await
        .expect("admit first prompt");
    // send_prompt returns after admission; wait until the turn published history.
    let before = tokio::time::timeout(TEST_POLL_TIMEOUT, async {
        loop {
            let events = client
                .deps
                .event_bus
                .query(&session.id, 0, 100)
                .await
                .expect("query event history");
            if !events.is_empty()
                && events
                    .iter()
                    .any(|event| event.event_type == EventType::StreamUpdate && !event.streaming)
            {
                return events;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for first prompt history");
    assert!(!before.is_empty(), "prompt should create durable history");

    let rebound = client
        .rebind_session(&session.id, "mock", "mock-model", 8 * 1024)
        .await
        .expect("rebind idle session");
    let after = client
        .deps
        .event_bus
        .query(&session.id, 0, 100)
        .await
        .expect("query event history after rebind");

    assert_eq!(rebound.id, session.id);
    assert_eq!(rebound.name, "Mock session");
    assert!(
        after.len() > before.len(),
        "rebind should preserve history and append a restart event"
    );
    client
        .close_session(&session.id)
        .await
        .expect("close rebound session");
}

/// Rebind must refresh `profile_config_id` from the replacement actor so
/// `session_for_profile_switch` reflects the new agent's mode capability.
#[tokio::test]
async fn rebind_refreshes_profile_config_id_from_replacement_agent() {
    let (client, _permissions, _workspace, workspace_id) =
        mock_client_empty(ConversationStore::new(None)).await;
    let session = client
        .create_session("mock-nocap", "mock-model", &workspace_id)
        .await
        .expect("create session without mode capability");

    let (_, before_cfg) = client
        .session_for_profile_switch(&session.id)
        .expect("live session lookup");
    assert_eq!(
        before_cfg, None,
        "mock-nocap must not advertise a mode/profile config option"
    );

    client
        .rebind_session(&session.id, "mock", "mock-model", 8 * 1024)
        .await
        .expect("rebind to agent with mode capability");

    let (_, after_cfg) = client
        .session_for_profile_switch(&session.id)
        .expect("live session lookup after rebind");
    assert_eq!(
        after_cfg.as_deref(),
        Some("profile"),
        "rebind must cache the replacement agent's profile config id"
    );

    client
        .close_session(&session.id)
        .await
        .expect("close rebound session");
}

/// When the agent does NOT advertise the `mode`-category `profile` config
/// option (`mock-nocap`), the capability gate must take the prompt-injection
/// fallback branch: `find_profile_config_id` returns `None`, no
/// `session/set_config_option` RPC is sent, and profile instructions are
/// injected into the prompt context instead. Verified end-to-end by asserting
/// the streamed reply contains no `[profile:` marker (the mock only emits that
/// prefix when it received the RPC) while `session_for_profile_switch` reports
/// a `None` config id, proving the fallback path was selected. This is the
/// integration counterpart to `profile_is_injected_when_the_agent_lacks_profile_configuration`.
#[tokio::test]
async fn prompt_injection_fallback_skips_set_config_option() {
    let (client, _permissions, _workspace, workspace_id) =
        mock_client_empty(ConversationStore::new(None)).await;
    let session = client
        .create_session("mock-nocap", "mock-model", &workspace_id)
        .await
        .expect("create session without mode capability");

    // The capability gate must have cached no profile config id for mock-nocap.
    let (_, profile_config_id) = client
        .session_for_profile_switch(&session.id)
        .expect("live session lookup");
    assert_eq!(
        profile_config_id, None,
        "mock-nocap must not advertise a mode/profile config option"
    );

    // Seed a local profile selection so the fallback has instructions to inject.
    client
        .pipeline
        .profiles
        .set_profile(&session.id, "code")
        .expect("seed local profile");

    client
        .send_prompt(&session.id, "hello", &[])
        .await
        .expect("admit prompt against mock-nocap");

    // Concatenate non-thought StreamUpdate chunks; the mock prefixes its first
    // streamed reply with `[profile: X]` only when it received the
    // `session/set_config_option` RPC, so absence proves the RPC was skipped.
    // send_prompt returns after admission — wait for the turn to finish streaming.
    let events = tokio::time::timeout(TEST_POLL_TIMEOUT, async {
        loop {
            let events = client
                .deps
                .event_bus
                .query(&session.id, 0, 1000)
                .await
                .expect("query session events");
            if events
                .iter()
                .any(|event| event.event_type == EventType::StreamUpdate && !event.streaming)
            {
                return events;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for mock-nocap prompt stream");
    let mut streamed = String::new();
    for event in &events {
        if event.event_type == EventType::StreamUpdate && !event.thought {
            streamed.push_str(&event.content);
        }
    }
    assert!(
        !streamed.contains("[profile:"),
        "fallback must not send set_config_option; reply contained `[profile:` marker: {streamed}"
    );

    // Re-confirm the capability gate stayed on the fallback path after the turn.
    let (_, profile_config_id_after) = client
        .session_for_profile_switch(&session.id)
        .expect("live session lookup after prompt");
    assert_eq!(
        profile_config_id_after, None,
        "fallback path must keep the cached profile config id None"
    );

    client
        .close_session(&session.id)
        .await
        .expect("close mock-nocap session");
}

/// When a live profile RPC cannot be delivered, local middleware must not
/// advance — client and agent stay consistent (commit-after-RPC order).
#[tokio::test]
async fn set_session_profile_leaves_local_state_on_rpc_failure() {
    let (client, _permissions, _workspace) = mock_client().await;
    let session = client.list_sessions().pop().expect("one mock session");

    // Ensure a known local selection before the failed switch.
    client
        .pipeline
        .profiles
        .set_profile(&session.id, "code")
        .expect("seed local profile");
    assert_eq!(
        client
            .pipeline
            .profiles
            .profile(&session.id)
            .expect("read seeded profile"),
        "code"
    );

    // Force the capability path, then break the actor command channel so
    // SetProfile cannot complete. Local state must stay at "code".
    client
        .sessions
        .replace_actor_for_test(
            &session.id,
            super::super::actor::Handle::dead(),
            Some("profile".to_string()),
        )
        .expect("session remains registered");

    let result = client.set_session_profile(&session.id, "ask").await;
    assert!(
        result.is_err(),
        "broken actor channel must surface as set_session_profile error"
    );
    assert_eq!(
        client
            .pipeline
            .profiles
            .profile(&session.id)
            .expect("read profile after failed switch"),
        "code",
        "local profile must not commit when the ACP update fails"
    );
}

/// After restart, `list_sessions` must surface durable metadata with no actors.
#[tokio::test]
async fn list_sessions_includes_stored_without_live_actors() {
    let store_dir = TempDir::new().expect("store dir");
    let store_path = store_dir.path().join("conversations.json");
    let (client, _permissions, _workspace, workspace_id) =
        mock_client_empty(ConversationStore::new(Some(store_path))).await;
    let now = Utc::now();
    client
        .deps
        .conversation_store
        .persist(&[StoredSession::from_parts(
            SessionInfo {
                id: "sess-persisted".to_string(),
                name: "Survived restart".to_string(),
                // Stale running bit from the previous daemon process.
                status: "running".to_string(),
                agent_id: "mock".to_string(),
                model_id: "mock-model".to_string(),
                workspace: workspace_id,
                created_at: now,
                updated_at: now,
            },
            "acp-prior-1",
        )])
        .expect("seed conversations.json");

    assert!(
        client.list_sessions().is_empty(),
        "store is not visible until load_conversations"
    );
    client
        .load_conversations()
        .expect("load durable conversations");

    let listed = client.list_sessions();
    assert_eq!(listed.len(), 1, "list must include stored session");
    assert_eq!(listed[0].id, "sess-persisted");
    assert_eq!(listed[0].name, "Survived restart");
    assert_eq!(
        listed[0].status, "idle",
        "loaded sessions must be idle until an actor is restored"
    );
    assert!(
        !client
            .has_live_session("sess-persisted")
            .expect("live lookup"),
        "load_conversations must not auto-start actors"
    );
}

/// Prompting a stored-only id starts an actor and must not wipe `EventBus` history.
#[tokio::test]
async fn prompt_on_stored_session_starts_actor_without_wiping_history() {
    let store_dir = TempDir::new().expect("store dir");
    let store_path = store_dir.path().join("conversations.json");
    let (client, _permissions, _workspace, workspace_id) =
        mock_client_empty(ConversationStore::new(Some(store_path))).await;
    let now = Utc::now();
    let session_id = "sess-restore-prompt";
    client
        .deps
        .conversation_store
        .persist(&[StoredSession::from_parts(
            SessionInfo {
                id: session_id.to_string(),
                name: "Prior chat".to_string(),
                status: "idle".to_string(),
                agent_id: "mock".to_string(),
                model_id: "mock-model".to_string(),
                workspace: workspace_id,
                created_at: now,
                updated_at: now,
            },
            "",
        )])
        .expect("seed conversations.json");
    client
        .load_conversations()
        .expect("load durable conversations");

    let mut prior = Event::new(0, EventType::PromptSubmitted, session_id, now);
    prior.role = "user".to_string();
    prior.content = "history from before restart".to_string();
    client
        .deps
        .event_bus
        .append_and_publish(prior)
        .await
        .expect("seed prior transcript");
    let before = client
        .deps
        .event_bus
        .query(session_id, 0, 100)
        .await
        .expect("query history before restore");
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].content, "history from before restart");

    client
        .send_prompt(session_id, "hello after restart", &[])
        .await
        .expect("prompt must lazily restore the actor");

    assert!(
        client.has_live_session(session_id).expect("live lookup"),
        "prompt should promote the dormant session to a live actor"
    );
    let info = client
        .get_session_info(session_id)
        .expect("restored session info");
    assert_eq!(info.name, "Prior chat");
    assert_eq!(info.id, session_id);

    // send_prompt returns after admission; wait for the restored turn to append.
    let after = tokio::time::timeout(TEST_POLL_TIMEOUT, async {
        loop {
            let events = client
                .deps
                .event_bus
                .query(session_id, 0, 100)
                .await
                .expect("query history after restore");
            if events.len() > before.len() {
                return events;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for restore prompt history");
    assert!(
        after.len() > before.len(),
        "restore prompt should append events without wiping prior history"
    );
    assert_eq!(
        after[0].content, "history from before restart",
        "EventBus history must survive actor restore"
    );

    client
        .close_session(session_id)
        .await
        .expect("close restored session");
}

/// Durable `acpSessionId` survives load→rename→persist without leaking into REST info.
#[tokio::test]
async fn persisted_acp_session_id_survives_rename_round_trip() {
    let store_dir = TempDir::new().expect("store dir");
    let store_path = store_dir.path().join("conversations.json");
    let (client, _permissions, _workspace, workspace_id) =
        mock_client_empty(ConversationStore::new(Some(store_path.clone()))).await;
    let now = Utc::now();
    client
        .deps
        .conversation_store
        .persist(&[StoredSession::from_parts(
            SessionInfo {
                id: "sess-acp-id".to_string(),
                name: "With ACP id".to_string(),
                status: "idle".to_string(),
                agent_id: "mock".to_string(),
                model_id: "mock-model".to_string(),
                workspace: workspace_id,
                created_at: now,
                updated_at: now,
            },
            "acp-durable-9",
        )])
        .expect("seed");
    client
        .load_conversations()
        .expect("load durable conversations");

    let info = client
        .get_session_info("sess-acp-id")
        .expect("get session info");
    let info_json = serde_json::to_value(&info).expect("serialize REST info");
    assert!(
        info_json.get("acpSessionId").is_none(),
        "get_session_info must not expose acpSessionId"
    );

    client
        .rename_session("sess-acp-id", "Renamed")
        .expect("rename dormant");
    let reloaded = client
        .deps
        .conversation_store
        .load()
        .expect("reload store after rename");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].info.name, "Renamed");
    assert_eq!(
        reloaded[0].acp_session_id, "acp-durable-9",
        "rename must preserve durable acpSessionId"
    );
}

/// An old actor's channel-based terminal report must not fail the actor
/// that replaced it during a rebind.
#[tokio::test]
async fn stale_actor_outcome_cannot_fail_replacement_session() {
    let (client, _permissions, _workspace) = mock_client().await;
    let session = client.list_sessions().pop().expect("one mock session");
    let stale = Handle::dead();
    client
        .sessions
        .replace_actor_for_test(&session.id, stale.clone(), None)
        .expect("install stale actor generation");
    client
        .sessions
        .replace_actor_for_test(&session.id, Handle::dead(), None)
        .expect("install replacement actor generation");
    let (outcome_tx, outcome_rx) = tokio::sync::oneshot::channel();
    client.watch_actor_terminal(outcome_rx, stale, session.id.clone());
    assert!(
        outcome_tx
            .send(TerminalOutcome::Failed(AppError::internal(
                "stale actor exited",
            )))
            .is_ok(),
        "watcher still receives stale outcome"
    );
    tokio::task::yield_now().await;
    assert_eq!(
        client
            .get_session_info(&session.id)
            .expect("session info")
            .status,
        "idle",
        "stale outcome must not alter the replacement session"
    );
}

/// Concurrent dormant restores serialize through `restore_lock`, so one
/// durable conversation publishes exactly one actor entry.
#[tokio::test]
async fn concurrent_restore_publishes_one_actor() {
    let directory = TempDir::new().expect("store dir");
    let store_path = directory.path().join("conversations.json");
    let (client, _permissions, _workspace, workspace_id) =
        mock_client_empty(ConversationStore::new(Some(store_path))).await;
    let now = Utc::now();
    client
        .deps
        .conversation_store
        .persist(&[StoredSession::from_parts(
            SessionInfo {
                id: "sess-concurrent-restore".to_string(),
                name: "Restore once".to_string(),
                status: "idle".to_string(),
                agent_id: "mock".to_string(),
                model_id: "mock-model".to_string(),
                workspace: workspace_id,
                created_at: now,
                updated_at: now,
            },
            String::new(),
        )])
        .expect("seed durable session");
    client.load_conversations().expect("load dormant metadata");
    let (left, right) = tokio::join!(
        client.ensure_live_session("sess-concurrent-restore"),
        client.ensure_live_session("sess-concurrent-restore"),
    );
    left.expect("first restore");
    right.expect("second restore observes published actor");
    assert!(client
        .has_live_session("sess-concurrent-restore")
        .expect("live lookup"));
    assert_eq!(client.sessions.live_len().expect("live count"), 1);
    client
        .close_session("sess-concurrent-restore")
        .await
        .expect("close restored actor");
}

/// The live-session cap is checked before agent resolution/spawn, keeping
/// an over-cap request from creating another child process.
#[tokio::test]
async fn max_sessions_rejects_before_new_actor_is_spawned() {
    let (client, _permissions, _workspace) = mock_client().await;
    let existing = client.list_sessions().pop().expect("one mock session");
    for index in 1..super::super::MAX_SESSIONS {
        let now = Utc::now();
        let info = SessionInfo {
            id: format!("sess-cap-{index}"),
            name: "Capacity placeholder".to_string(),
            status: "idle".to_string(),
            agent_id: existing.agent_id.clone(),
            model_id: existing.model_id.clone(),
            workspace: existing.workspace.clone(),
            created_at: now,
            updated_at: now,
        };
        client
            .sessions
            .publish(SessionEntry::new(
                info,
                Handle::dead(),
                Arc::new(Mutex::new(StderrTail::default())),
                Arc::new(AtomicBool::new(false)),
                SessionCaps::default(),
                None,
                None,
                String::new(),
            ))
            .expect("publish capacity placeholder");
    }
    let result = client
        .create_session(&existing.agent_id, &existing.model_id, &existing.workspace)
        .await;
    assert!(matches!(result, Err(AppError::RateLimited(_))));
    assert_eq!(
        client.sessions.live_len().expect("live count"),
        super::super::MAX_SESSIONS,
        "rejected create must not publish or spawn another actor"
    );
}
