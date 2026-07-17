//! Durable conversation metadata stored in `conversations.json`.
//!
//! Event history remains in SQLite; this store owns only the lightweight
//! session list needed to restore names, selected harnesses, and timestamps.

use std::path::{Path, PathBuf};

use crate::fsutil;
use crate::interfaces::{AppError, SessionInfo};

/// Owner-only permissions because session names and workspace identifiers are private.
const CONVERSATIONS_FILE_MODE: u32 = 0o600;

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
    pub fn load(&self) -> Result<Vec<SessionInfo>, AppError> {
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
    pub fn persist(&self, sessions: &[SessionInfo]) -> Result<(), AppError> {
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

    use super::ConversationStore;
    use crate::interfaces::SessionInfo;

    #[test]
    fn round_trips_durable_conversation_metadata() {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let store = ConversationStore::new(Some(directory.path().join("conversations.json")));
        let now = Utc::now();
        let expected = vec![SessionInfo {
            id: "sess-one".to_string(),
            name: "Investigate cache".to_string(),
            status: "idle".to_string(),
            agent_id: "codex".to_string(),
            model_id: "gpt-5".to_string(),
            workspace: "workspace-1".to_string(),
            created_at: now,
            updated_at: now,
        }];

        store.persist(&expected).expect("persist metadata");

        assert_eq!(store.load().expect("load metadata"), expected);
    }

    #[test]
    fn absent_store_is_an_empty_conversation_list() {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let store = ConversationStore::new(Some(directory.path().join("conversations.json")));

        assert!(store.load().expect("load missing store").is_empty());
    }
}
