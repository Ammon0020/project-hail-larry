//! Validate non-config state artifacts created by the Go daemon.
//!
//! Rust modules for pairing/uploads/MCP/ACP are not fully ported yet. S-MIGRATE
//! validates structural readability (JSON parse / SQLite open / directory layout)
//! so we know those files survive the binary switch. Semantic load of each
//! artifact lands in the respective port stories.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::info;

use crate::events::Store as EventStore;
use crate::interfaces::EventStore as _;

use super::error::MigrateError;

/// Well-known Go state artifacts under the state directory.
pub const EVENT_DB_FILE: &str = "local-agent.db";
pub const DEVICES_FILE: &str = "devices.json";
pub const CONVERSATIONS_FILE: &str = "conversations.json";
pub const MCP_FILE: &str = "mcp.json";
pub const UPLOADS_DIR: &str = "uploads";
pub const TLS_DIR: &str = "tls";

/// Result of validating the full Go-created state tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateValidation {
    /// Absolute/display path of the state directory that was scanned.
    pub state_dir: PathBuf,
    /// Config-format notes (filled by the caller or migrate report).
    pub notes: Vec<String>,
    /// Artifact checks that succeeded.
    pub ok: Vec<ArtifactStatus>,
    /// Artifact checks that failed hard (migration should fail loudly).
    pub failed: Vec<ArtifactStatus>,
    /// Artifacts present but only structure-validated (semantic load deferred).
    pub deferred: Vec<ArtifactStatus>,
    /// Artifacts not present (optional / never written by Go for this install).
    pub missing: Vec<String>,
}

/// Status of one on-disk artifact after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactStatus {
    /// File or directory name (relative to state dir).
    pub name: String,
    /// Human-readable outcome detail.
    pub detail: String,
}

impl StateValidation {
    /// True when nothing failed hard.
    pub fn is_ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Go `devices.json` record (hash-only credentials).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDevice {
    id: String,
    #[allow(dead_code)]
    name: String,
    secret_hash: String,
    // pairedAt / lastSeen accepted but not required for structural validation.
}

/// Minimal conversation/session metadata record (Go `acp.Session` export fields).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationRecord {
    id: String,
    #[allow(dead_code)]
    name: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

/// MCP envelope (Go `mcp.File`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpFile {
    #[serde(default)]
    version: i64,
    #[serde(default, rename = "mcpServers")]
    mcp_servers: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Open and validate every known Go state artifact under `state_dir`.
///
/// Missing optional files are reported in `missing`, not as failures. Corrupt
/// existing files land in `failed` so the caller can fail loudly.
///
/// # Errors
///
/// Returns [`MigrateError::EventStore`] only when the event DB exists but cannot
/// be opened/queried (schema/payload drift). Other artifacts accumulate into the
/// returned [`StateValidation`] without aborting the whole scan — the caller
/// decides whether to surface `failed` as an error.
pub fn validate_state_tree(state_dir: &Path) -> Result<StateValidation, MigrateError> {
    let mut report = StateValidation {
        state_dir: state_dir.to_path_buf(),
        ..StateValidation::default()
    };

    validate_event_db(state_dir, &mut report)?;
    validate_devices(state_dir, &mut report);
    validate_conversations(state_dir, &mut report);
    validate_mcp(state_dir, &mut report);
    validate_uploads(state_dir, &mut report);
    validate_tls(state_dir, &mut report);

    if report.is_ok() {
        info!(
            state_dir = %state_dir.display(),
            ok = report.ok.len(),
            deferred = report.deferred.len(),
            missing = report.missing.len(),
            "state tree validation passed"
        );
    }

    Ok(report)
}

/// Open a Go-created SQLite event DB with the Rust event store and query a page.
fn validate_event_db(state_dir: &Path, report: &mut StateValidation) -> Result<(), MigrateError> {
    let db_path = state_dir.join(EVENT_DB_FILE);
    if !db_path.is_file() {
        report.missing.push(EVENT_DB_FILE.to_string());
        return Ok(());
    }

    // Open via the real Rust store — this is the core S-MIGRATE event check.
    let store = EventStore::open(&db_path)
        .map_err(|e| MigrateError::EventStore(format!("open {}: {e}", db_path.display())))?;

    // Blocking query through the async trait would need a runtime; use the
    // same open path and a synchronous internal query via try-with tokio if
    // available, else open a second connection for schema/row smoke checks.
    // Prefer a lightweight rusqlite re-open for the count so this stays sync.
    let count = count_events_sync(&db_path)
        .map_err(|e| MigrateError::EventStore(format!("count events: {e}")))?;

    // Drop the store handle (mutex + connection). Ignore close errors.
    drop(store);

    report.ok.push(ArtifactStatus {
        name: EVENT_DB_FILE.into(),
        detail: format!("opened with Rust event store; {count} event row(s)"),
    });
    Ok(())
}

/// Count rows in the events table with a short-lived connection (sync helper).
fn count_events_sync(db_path: &Path) -> Result<i64, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    // Verify expected columns exist (schema drift would fail here).
    let mut stmt = conn
        .prepare("SELECT id, type, session_id, timestamp, payload FROM events LIMIT 1")
        .map_err(|e| format!("schema probe: {e}"))?;
    let _ = stmt.exists([]).map_err(|e| format!("schema probe: {e}"))?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    Ok(count)
}

/// Structural parse of `devices.json` (SHA-256 hashes only, never raw secrets).
fn validate_devices(state_dir: &Path, report: &mut StateValidation) {
    let path = state_dir.join(DEVICES_FILE);
    if !path.is_file() {
        report.missing.push(DEVICES_FILE.to_string());
        return;
    }
    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<Vec<StoredDevice>>(&s) {
            Ok(devices) => {
                let bad = devices
                    .iter()
                    .find(|d| d.id.is_empty() || d.secret_hash.is_empty());
                if let Some(d) = bad {
                    report.failed.push(ArtifactStatus {
                        name: DEVICES_FILE.into(),
                        detail: format!("device entry missing id or secretHash (id={:?})", d.id),
                    });
                } else {
                    // Semantic load deferred until S-PAIRING ports the manager.
                    report.deferred.push(ArtifactStatus {
                        name: DEVICES_FILE.into(),
                        detail: format!(
                            "structurally valid JSON array ({} device(s)); full credential load deferred to pairing module",
                            devices.len()
                        ),
                    });
                }
            }
            Err(e) => report.failed.push(ArtifactStatus {
                name: DEVICES_FILE.into(),
                detail: format!("JSON parse failed: {e}"),
            }),
        },
        Err(e) => report.failed.push(ArtifactStatus {
            name: DEVICES_FILE.into(),
            detail: format!("read failed: {e}"),
        }),
    }
}

/// Structural parse of `conversations.json`.
fn validate_conversations(state_dir: &Path, report: &mut StateValidation) {
    let path = state_dir.join(CONVERSATIONS_FILE);
    if !path.is_file() {
        report.missing.push(CONVERSATIONS_FILE.to_string());
        return;
    }
    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<Vec<ConversationRecord>>(&s) {
            Ok(records) => {
                if records.iter().any(|r| r.id.is_empty()) {
                    report.failed.push(ArtifactStatus {
                        name: CONVERSATIONS_FILE.into(),
                        detail: "conversation entry with empty id".into(),
                    });
                } else {
                    report.deferred.push(ArtifactStatus {
                        name: CONVERSATIONS_FILE.into(),
                        detail: format!(
                            "structurally valid ({} session(s)); full ACP store load deferred",
                            records.len()
                        ),
                    });
                    // Silence unused-field warning path for agent_id presence in shape.
                    let _ = records
                        .iter()
                        .filter(|r| r.agent_id.as_ref().is_some_and(|a| !a.is_empty()))
                        .count();
                }
            }
            Err(e) => report.failed.push(ArtifactStatus {
                name: CONVERSATIONS_FILE.into(),
                detail: format!("JSON parse failed: {e}"),
            }),
        },
        Err(e) => report.failed.push(ArtifactStatus {
            name: CONVERSATIONS_FILE.into(),
            detail: format!("read failed: {e}"),
        }),
    }
}

/// Structural parse of `mcp.json` Claude-compatible envelope.
fn validate_mcp(state_dir: &Path, report: &mut StateValidation) {
    let path = state_dir.join(MCP_FILE);
    if !path.is_file() {
        report.missing.push(MCP_FILE.to_string());
        return;
    }
    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<McpFile>(&s) {
            Ok(file) => {
                let n = file.mcp_servers.as_ref().map_or(0, |m| m.len());
                report.deferred.push(ArtifactStatus {
                    name: MCP_FILE.into(),
                    detail: format!(
                        "structurally valid envelope (version={}, {} server(s)); full MCP load deferred",
                        file.version, n
                    ),
                });
            }
            Err(e) => report.failed.push(ArtifactStatus {
                name: MCP_FILE.into(),
                detail: format!("JSON parse failed: {e}"),
            }),
        },
        Err(e) => report.failed.push(ArtifactStatus {
            name: MCP_FILE.into(),
            detail: format!("read failed: {e}"),
        }),
    }
}

/// Verify uploads directory layout (`uploads/<sessionId>/<uploadId>.ext`).
fn validate_uploads(state_dir: &Path, report: &mut StateValidation) {
    let path = state_dir.join(UPLOADS_DIR);
    if !path.is_dir() {
        report.missing.push(UPLOADS_DIR.to_string());
        return;
    }
    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut sessions = 0usize;
            let mut files = 0usize;
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    sessions += 1;
                    if let Ok(inner) = fs::read_dir(entry.path()) {
                        files += inner
                            .filter(|e| e.as_ref().is_ok_and(|x| x.path().is_file()))
                            .count();
                    }
                }
            }
            report.deferred.push(ArtifactStatus {
                name: UPLOADS_DIR.into(),
                detail: format!(
                    "directory present ({sessions} session dir(s), {files} file(s)); full uploads manager deferred"
                ),
            });
        }
        Err(e) => report.failed.push(ArtifactStatus {
            name: UPLOADS_DIR.into(),
            detail: format!("read_dir failed: {e}"),
        }),
    }
}

/// Verify TLS cert directory exists (files optional — self-signed may be generated later).
fn validate_tls(state_dir: &Path, report: &mut StateValidation) {
    let path = state_dir.join(TLS_DIR);
    if !path.exists() {
        report.missing.push(TLS_DIR.to_string());
        return;
    }
    if !path.is_dir() {
        report.failed.push(ArtifactStatus {
            name: TLS_DIR.into(),
            detail: "tls path exists but is not a directory".into(),
        });
        return;
    }
    report.ok.push(ArtifactStatus {
        name: TLS_DIR.into(),
        detail: "TLS cert directory present (cert generation remains app/tls concern)".into(),
    });
}

/// Async helper: open the event DB with the Rust store and `query_all` a page.
///
/// Used by tests that already have a tokio runtime to prove payload round-trip
/// beyond the sync schema probe.
pub async fn validate_event_db_async(db_path: &Path) -> Result<usize, MigrateError> {
    let store =
        EventStore::open(db_path).map_err(|e| MigrateError::EventStore(format!("open: {e}")))?;
    let events = store
        .query_all(0, 1000)
        .await
        .map_err(|e| MigrateError::EventStore(format!("query_all: {e}")))?;
    Ok(events.len())
}
