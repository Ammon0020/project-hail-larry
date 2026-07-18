//! Durable conversation metadata stored in `conversations.json`.
//!
//! Event history remains in SQLite; this store owns only the lightweight
//! session list needed to restore names, selected harnesses, timestamps, and
//! the agent-side ACP session id used for `session/load` after restart.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::interfaces::{AppError, SessionInfo};

/// Owner-only permissions because session names and workspace identifiers are private.
const CONVERSATIONS_FILE_MODE: u32 = 0o600;

/// Durable conversation record: public [`SessionInfo`] plus the private ACP
/// session id used to resume the agent after daemon restart.
///
/// `acpSessionId` is persisted for Go/Rust parity but never projected into the
/// REST [`SessionInfo`] shape (see [`StoredSession::to_info`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredSession {
    #[serde(flatten)]
    pub info: SessionInfo,
    /// Agent-side ACP session identifier for `session/load` resume.
    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "acpSessionId"
    )]
    pub acp_session_id: String,
}

impl StoredSession {
    /// Builds a stored record from public metadata and an optional ACP id.
    #[must_use]
    pub fn from_parts(info: SessionInfo, acp_session_id: impl Into<String>) -> Self {
        Self {
            info,
            acp_session_id: acp_session_id.into(),
        }
    }

    /// Public session projection (strips `acpSessionId` for REST/UI).
    #[must_use]
    pub fn to_info(&self) -> SessionInfo {
        self.info.clone()
    }
}

/// JSON-backed store for durable conversation metadata.
#[derive(Debug, Clone, Default)]
pub struct ConversationStore {
    path: Option<PathBuf>,
}

impl ConversationStore {
    /// Creates a store at `path`; `None` explicitly disables persistence.
    #[must_use]
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Returns the configured JSON file path, if conversation persistence is enabled.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Loads stored metadata. A missing file is a valid first-run state.
    ///
    /// # Errors
    ///
    /// Returns a descriptive error for unreadable or invalid state rather than
    /// silently discarding users' conversation metadata.
    pub fn load(&self) -> Result<Vec<StoredSession>, AppError> {
        let Some(path) = &self.path else {
            return Ok(Vec::new());
        };
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(AppError::internal(format!(
                    "read conversation metadata {}: {error}",
                    path.display()
                )));
            }
        };
        serde_json::from_slice(&data).map_err(|error| {
            AppError::internal(format!(
                "parse conversation metadata {}: {error}",
                path.display()
            ))
        })
    }

    /// Atomically replaces all durable session metadata.
    ///
    /// # Errors
    ///
    /// Serialization and filesystem failures are returned to callers so they
    /// can report that a rename/create/close did not become durable.
    pub fn persist(&self, sessions: &[StoredSession]) -> Result<(), AppError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let data = serde_json::to_vec_pretty(sessions).map_err(|error| {
            AppError::internal(format!("serialize conversation metadata: {error}"))
        })?;
        fsutil::atomic_write(path, &data, Some(CONVERSATIONS_FILE_MODE)).map_err(|error| {
            AppError::internal(format!(
                "persist conversation metadata {}: {error}",
                path.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{ConversationStore, StoredSession};
    use crate::interfaces::SessionInfo;

    fn sample_info(now: chrono::DateTime<Utc>) -> SessionInfo {
        SessionInfo {
            id: "sess-one".to_string(),
            name: "Investigate cache".to_string(),
            status: "idle".to_string(),
            agent_id: "codex".to_string(),
            model_id: "gpt-5".to_string(),
            workspace: "workspace-1".to_string(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn round_trips_durable_conversation_metadata() {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let store = ConversationStore::new(Some(directory.path().join("conversations.json")));
        let now = Utc::now();
        let expected = vec![StoredSession::from_parts(sample_info(now), "")];

        store.persist(&expected).expect("persist metadata");

        assert_eq!(store.load().expect("load metadata"), expected);
    }

    #[test]
    fn round_trips_acp_session_id() {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let store = ConversationStore::new(Some(directory.path().join("conversations.json")));
        let now = Utc::now();
        let expected = vec![StoredSession::from_parts(sample_info(now), "acp-persist-1")];

        store.persist(&expected).expect("persist with acpSessionId");
        let loaded = store.load().expect("load metadata");
        assert_eq!(loaded, expected);
        assert_eq!(loaded[0].acp_session_id, "acp-persist-1");
        // REST projection must not carry the ACP id.
        let info = loaded[0].to_info();
        let info_json = serde_json::to_value(&info).expect("serialize SessionInfo");
        assert!(
            info_json.get("acpSessionId").is_none(),
            "SessionInfo must not expose acpSessionId"
        );
    }

    #[test]
    fn preserves_go_shaped_json_with_acp_session_id() {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let path = directory.path().join("conversations.json");
        // Go writes camelCase Session fields including acpSessionId.
        std::fs::write(
            &path,
            r#"[
  {
    "id": "sess-go",
    "name": "From Go",
    "status": "idle",
    "agentId": "codex",
    "modelId": "gpt-5",
    "workspace": "ws-1",
    "createdAt": "2026-07-17T12:00:00Z",
    "updatedAt": "2026-07-17T12:30:00Z",
    "acpSessionId": "acp-from-go"
  }
]"#,
        )
        .expect("write Go-shaped conversations.json");

        let store = ConversationStore::new(Some(path));
        let loaded = store.load().expect("parse Go conversations.json");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].info.id, "sess-go");
        assert_eq!(loaded[0].info.name, "From Go");
        assert_eq!(loaded[0].acp_session_id, "acp-from-go");

        store.persist(&loaded).expect("re-persist after load");
        let again = store.load().expect("reload after persist");
        assert_eq!(again[0].acp_session_id, "acp-from-go");
    }

    #[test]
    fn omits_empty_acp_session_id_on_serialize() {
        let stored = StoredSession::from_parts(sample_info(Utc::now()), "");
        let value = serde_json::to_value(&stored).expect("serialize");
        assert!(
            value.get("acpSessionId").is_none(),
            "empty acpSessionId must be omitted like Go omitempty"
        );
    }

    #[test]
    fn absent_store_is_an_empty_conversation_list() {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let store = ConversationStore::new(Some(directory.path().join("conversations.json")));

        assert!(store.load().expect("load missing store").is_empty());
    }
}
