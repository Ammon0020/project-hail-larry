//! Per-workspace editor tab sets stored in `workspace-tabs.json`.
//!
//! Only tab *identity and order* live here — never file content. Unsaved
//! buffers stay on the device that typed them: they are drafts, not workspace
//! state, and syncing them between devices would silently overwrite one
//! device's edits with another's. That split is also what makes the
//! cross-device follow-up safe.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::interfaces::AppError;

/// Owner-only: open file paths reveal what the user is working on.
const TABS_FILE_MODE: u32 = 0o600;

/// Cap per workspace. The editor is unusable long before this, so the limit
/// exists to bound the file a paired device can cause the daemon to write.
pub const MAX_TABS_PER_WORKSPACE: usize = 200;

/// Cap on every stored string. Paths are echoed back to clients, never used
/// for filesystem access from this module.
const MAX_FIELD_CHARS: usize = 1024;

/// One restorable editor tab. Content and unsaved state are deliberately absent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTab {
    pub id: String,
    pub path: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// Frontend `Tab.kind`; opaque here beyond its length.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_preview: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub view_mode: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub staged: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commit_oid: String,
}

/// A workspace's restorable editor state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTabs {
    #[serde(default)]
    pub tabs: Vec<WorkspaceTab>,
    /// Tab to focus on restore. Cleared when it names no listed tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
}

impl WorkspaceTabs {
    /// Reject oversized input and drop an `activeTabId` that names no tab.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the tab count or any field exceeds its
    /// cap, so a client cannot grow the daemon's state file without bound.
    pub fn validated(mut self) -> Result<Self, AppError> {
        if self.tabs.len() > MAX_TABS_PER_WORKSPACE {
            return Err(AppError::validation(format!(
                "too many tabs: {} (max {MAX_TABS_PER_WORKSPACE})",
                self.tabs.len()
            )));
        }
        for tab in &self.tabs {
            for (label, value) in [
                ("id", &tab.id),
                ("path", &tab.path),
                ("name", &tab.name),
                ("language", &tab.language),
                ("kind", &tab.kind),
                ("viewMode", &tab.view_mode),
                ("commitOid", &tab.commit_oid),
            ] {
                if value.chars().count() > MAX_FIELD_CHARS {
                    return Err(AppError::validation(format!(
                        "tab {label} exceeds {MAX_FIELD_CHARS} characters"
                    )));
                }
            }
            if tab.id.is_empty() {
                return Err(AppError::validation("tab id is required"));
            }
        }
        // A dangling active id would focus nothing and confuse restore.
        if let Some(active) = &self.active_tab_id {
            if !self.tabs.iter().any(|tab| &tab.id == active) {
                self.active_tab_id = None;
            }
        }
        Ok(self)
    }
}

/// JSON-backed store mapping workspace id → its tab set.
///
/// `BTreeMap` keeps the file's key order stable so successive writes produce
/// minimal diffs rather than reshuffling on every save.
#[derive(Debug, Clone, Default)]
pub struct TabStore {
    path: Option<PathBuf>,
}

impl TabStore {
    /// Creates a store at `path`; `None` explicitly disables persistence.
    #[must_use]
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Returns the configured file path, if persistence is enabled.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Tab set for one workspace. A workspace never saved yields an empty set.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists but cannot be read or parsed,
    /// rather than silently discarding the user's editor layout.
    pub fn load(&self, workspace_id: &str) -> Result<WorkspaceTabs, AppError> {
        Ok(self.load_all()?.remove(workspace_id).unwrap_or_default())
    }

    /// Replaces one workspace's tab set, leaving every other workspace intact.
    ///
    /// # Errors
    ///
    /// Returns an error when the existing file cannot be read, the result
    /// cannot be serialized, or the atomic write fails.
    pub fn save(&self, workspace_id: &str, tabs: WorkspaceTabs) -> Result<(), AppError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut all = self.load_all()?;
        if tabs.tabs.is_empty() {
            // Don't accumulate empty entries for workspaces the user closed out.
            all.remove(workspace_id);
        } else {
            all.insert(workspace_id.to_string(), tabs);
        }
        let data = serde_json::to_vec_pretty(&all)
            .map_err(|error| AppError::internal(format!("serialize workspace tabs: {error}")))?;
        fsutil::atomic_write(path, &data, Some(TABS_FILE_MODE)).map_err(|error| {
            AppError::internal(format!(
                "persist workspace tabs {}: {error}",
                path.display()
            ))
        })
    }

    /// Drops a workspace's tabs — used when a workspace is unregistered.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or rewritten.
    pub fn remove(&self, workspace_id: &str) -> Result<(), AppError> {
        self.save(workspace_id, WorkspaceTabs::default())
    }

    fn load_all(&self) -> Result<BTreeMap<String, WorkspaceTabs>, AppError> {
        let Some(path) = &self.path else {
            return Ok(BTreeMap::new());
        };
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => {
                return Err(AppError::internal(format!(
                    "read workspace tabs {}: {error}",
                    path.display()
                )));
            }
        };
        serde_json::from_slice(&data).map_err(|error| {
            AppError::internal(format!("parse workspace tabs {}: {error}", path.display()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TabStore, WorkspaceTab, WorkspaceTabs, MAX_TABS_PER_WORKSPACE};

    fn tab(id: &str) -> WorkspaceTab {
        WorkspaceTab {
            id: id.to_string(),
            path: format!("src/{id}.rs"),
            name: format!("{id}.rs"),
            ..WorkspaceTab::default()
        }
    }

    fn store(dir: &tempfile::TempDir) -> TabStore {
        TabStore::new(Some(dir.path().join("workspace-tabs.json")))
    }

    #[test]
    fn unsaved_workspace_loads_empty_on_first_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tabs = store(&dir).load("ws-never-seen").expect("load");
        assert_eq!(tabs, WorkspaceTabs::default());
    }

    /// The whole point of the store: one workspace's editor layout must never
    /// clobber another's, since every save rewrites the single shared file.
    #[test]
    fn saving_one_workspace_leaves_the_others_intact() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = store(&dir);

        store
            .save(
                "ws-a",
                WorkspaceTabs {
                    tabs: vec![tab("alpha")],
                    active_tab_id: Some("alpha".to_string()),
                },
            )
            .expect("save a");
        store
            .save(
                "ws-b",
                WorkspaceTabs {
                    tabs: vec![tab("beta")],
                    active_tab_id: None,
                },
            )
            .expect("save b");
        // Rewriting A must not disturb B.
        store
            .save(
                "ws-a",
                WorkspaceTabs {
                    tabs: vec![tab("gamma")],
                    active_tab_id: None,
                },
            )
            .expect("resave a");

        let a = store.load("ws-a").expect("load a");
        let b = store.load("ws-b").expect("load b");
        assert_eq!(a.tabs.len(), 1);
        assert_eq!(a.tabs[0].id, "gamma");
        assert_eq!(b.tabs.len(), 1, "ws-b must survive writes to ws-a");
        assert_eq!(b.tabs[0].id, "beta");
    }

    /// Closing every tab should drop the workspace's entry rather than leaving
    /// empty records to accumulate for every workspace ever opened.
    #[test]
    fn saving_an_empty_set_removes_the_workspace_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = store(&dir);
        store
            .save(
                "ws-a",
                WorkspaceTabs {
                    tabs: vec![tab("alpha")],
                    active_tab_id: None,
                },
            )
            .expect("save");
        store.save("ws-a", WorkspaceTabs::default()).expect("clear");

        let raw = std::fs::read_to_string(store.path().expect("path")).expect("read file");
        assert!(
            !raw.contains("ws-a"),
            "emptied workspace should be dropped: {raw}"
        );
    }

    /// A paired device supplies this payload, so the caps are the only thing
    /// stopping it from growing the daemon's state file without bound.
    #[test]
    fn validation_rejects_oversized_payloads() {
        let too_many = WorkspaceTabs {
            tabs: (0..=MAX_TABS_PER_WORKSPACE)
                .map(|i| tab(&i.to_string()))
                .collect(),
            active_tab_id: None,
        };
        assert!(too_many.validated().is_err(), "tab count cap must hold");

        let long_field = WorkspaceTabs {
            tabs: vec![WorkspaceTab {
                path: "x".repeat(2000),
                ..tab("alpha")
            }],
            active_tab_id: None,
        };
        assert!(
            long_field.validated().is_err(),
            "field length cap must hold"
        );

        let no_id = WorkspaceTabs {
            tabs: vec![WorkspaceTab {
                id: String::new(),
                ..tab("alpha")
            }],
            active_tab_id: None,
        };
        assert!(
            no_id.validated().is_err(),
            "a tab without an id is unusable"
        );
    }

    /// An `activeTabId` naming a closed tab would focus nothing on restore.
    #[test]
    fn validation_drops_a_dangling_active_tab_id() {
        let validated = WorkspaceTabs {
            tabs: vec![tab("alpha")],
            active_tab_id: Some("closed-long-ago".to_string()),
        }
        .validated()
        .expect("valid apart from the stale id");
        assert_eq!(validated.active_tab_id, None);

        let kept = WorkspaceTabs {
            tabs: vec![tab("alpha")],
            active_tab_id: Some("alpha".to_string()),
        }
        .validated()
        .expect("valid");
        assert_eq!(kept.active_tab_id.as_deref(), Some("alpha"));
    }
}
