//! Synchronous ACP session metadata registry.
//!
//! The registry owns live actor metadata and dormant durable records. Its API
//! returns owned snapshots and handles only, so callers cannot accidentally
//! retain a registry lock while awaiting workspace, actor, or event work.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use chrono::Utc;
use tokio::sync::mpsc;

use super::super::providers::SessionCaps;
use super::super::store::{ConversationStore, StoredSession};
use super::actor;
use super::diagnostics::StderrTail;
use crate::interfaces::{AppError, Session, SessionInfo};

/// Session status stored in the in-memory registry during the core port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Created,
    Running,
    Idle,
    Interrupted,
    Failed,
    Closed,
}

impl SessionState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

pub(super) struct SessionEntry {
    info: SessionInfo,
    state: SessionState,
    actor: actor::Handle,
    stderr_tail: Arc<Mutex<StderrTail>>,
    prompt_cancel: Arc<AtomicBool>,
    caps: SessionCaps,
    model_config_id: Option<String>,
    profile_config_id: Option<String>,
    acp_session_id: String,
}

impl SessionEntry {
    // Constructor assigns every field directly; a params struct would add indirection.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        info: SessionInfo,
        actor: actor::Handle,
        stderr_tail: Arc<Mutex<StderrTail>>,
        prompt_cancel: Arc<AtomicBool>,
        caps: SessionCaps,
        model_config_id: Option<String>,
        profile_config_id: Option<String>,
        acp_session_id: String,
    ) -> Self {
        Self {
            info,
            state: SessionState::Created,
            actor,
            stderr_tail,
            prompt_cancel,
            caps,
            model_config_id,
            profile_config_id,
            acp_session_id,
        }
    }

    fn apply_state(&mut self, state: SessionState) {
        self.state = state;
        self.info.status = state.as_str().to_string();
        self.info.updated_at = Utc::now();
    }

    pub(super) fn commands(&self) -> mpsc::Sender<actor::ActorCommand> {
        self.actor.commands()
    }
}

#[derive(Clone, Default)]
pub(super) struct SessionRegistry {
    live: Arc<RwLock<HashMap<String, SessionEntry>>>,
    dormant: Arc<RwLock<HashMap<String, StoredSession>>>,
}

impl SessionRegistry {
    pub(super) fn live_len(&self) -> Result<usize, AppError> {
        Ok(self.live_read()?.len())
    }

    pub(super) fn contains_live(&self, session_id: &str) -> Result<bool, AppError> {
        Ok(self.live_read()?.contains_key(session_id))
    }

    pub(super) fn contains_dormant(&self, session_id: &str) -> Result<bool, AppError> {
        Ok(self.dormant_read()?.contains_key(session_id))
    }

    pub(super) fn dormant(&self, session_id: &str) -> Result<StoredSession, AppError> {
        self.dormant_read()?
            .get(session_id)
            .cloned()
            .ok_or_else(|| AppError::not_found_id("session", session_id))
    }

    pub(super) fn load_dormant(&self, records: Vec<StoredSession>) -> Result<usize, AppError> {
        let live = self.live_read()?;
        let mut dormant = self.dormant_write()?;
        dormant.clear();
        for mut stored in records {
            if live.contains_key(&stored.info.id) {
                continue;
            }
            stored.info.status = SessionState::Idle.as_str().to_string();
            dormant.insert(stored.info.id.clone(), stored);
        }
        Ok(dormant.len())
    }

    pub(super) fn publish(&self, mut entry: SessionEntry) -> Result<SessionInfo, AppError> {
        entry.apply_state(SessionState::Idle);
        let info = entry.info.clone();
        self.live_write()?.insert(info.id.clone(), entry);
        Ok(info)
    }

    /// Remove a dormant twin only after the actor has been successfully
    /// published, so a failed restore never makes the conversation disappear.
    pub(super) fn finish_promotion(&self, session_id: &str) -> Result<(), AppError> {
        self.dormant_write()?.remove(session_id);
        Ok(())
    }

    pub(super) fn mark_failed_if_current(&self, session_id: &str, actor_id: u64) -> bool {
        self.live_write()
            .ok()
            .and_then(|mut entries| {
                entries.get_mut(session_id).and_then(|entry| {
                    (entry.actor.id() == actor_id).then(|| entry.apply_state(SessionState::Failed))
                })
            })
            .is_some()
    }

    pub(super) fn command(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<actor::ActorCommand>, Arc<AtomicBool>), AppError> {
        let entry = self.live(session_id)?;
        match entry.state {
            SessionState::Failed => Err(AppError::internal(
                "ACP session failed; close it and create a new session",
            )),
            SessionState::Closed => Err(AppError::internal("ACP session is closed")),
            _ => Ok((entry.actor.commands(), Arc::clone(&entry.prompt_cancel))),
        }
    }

    pub(super) fn provider_command(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<actor::ActorCommand>, SessionCaps), AppError> {
        let entry = self.live(session_id)?;
        Ok((entry.actor.commands(), entry.caps))
    }

    pub(super) fn model_command(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<actor::ActorCommand>, Option<String>), AppError> {
        let entry = self.live(session_id)?;
        Ok((entry.actor.commands(), entry.model_config_id.clone()))
    }

    pub(super) fn profile_command(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<actor::ActorCommand>, Option<String>), AppError> {
        let entry = self.live(session_id)?;
        Ok((entry.actor.commands(), entry.profile_config_id.clone()))
    }

    pub(super) fn begin_prompt(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<actor::ActorCommand>, SessionCaps, String, bool), AppError> {
        let mut live = self.live_write()?;
        let entry = live
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        match entry.state {
            SessionState::Idle | SessionState::Interrupted => {
                entry.prompt_cancel.store(false, Ordering::Release);
                entry.apply_state(SessionState::Running);
                Ok((
                    entry.actor.commands(),
                    entry.caps,
                    entry.info.workspace.clone(),
                    entry.profile_config_id.is_none(),
                ))
            }
            SessionState::Running => Err(AppError::validation(
                "ACP session already has an active prompt",
            )),
            SessionState::Failed => Err(AppError::internal(
                "ACP session failed; close it and create a new session",
            )),
            SessionState::Closed | SessionState::Created => {
                Err(AppError::internal("ACP session is not ready for prompts"))
            }
        }
    }

    pub(super) fn update_state(&self, session_id: &str, state: SessionState) {
        if let Ok(mut live) = self.live_write() {
            if let Some(entry) = live.get_mut(session_id) {
                entry.apply_state(state);
            }
        }
    }

    pub(super) fn update_state_if(
        &self,
        session_id: &str,
        expected: SessionState,
        state: SessionState,
    ) {
        if let Ok(mut live) = self.live_write() {
            if let Some(entry) = live.get_mut(session_id) {
                if entry.state == expected {
                    entry.apply_state(state);
                }
            }
        }
    }

    pub(super) fn stderr_tail(&self, session_id: &str) -> Result<String, AppError> {
        let tail = Arc::clone(&self.live(session_id)?.stderr_tail);
        tail.lock()
            .map_err(|_| AppError::internal("ACP stderr lock poisoned"))
            .map(|tail| tail.as_string())
    }

    pub(super) fn info(&self, session_id: &str) -> Result<SessionInfo, AppError> {
        let live = self.live_read()?;
        if let Some(entry) = live.get(session_id) {
            return Ok(entry.info.clone());
        }
        drop(live);
        self.dormant_read()?
            .get(session_id)
            .map(StoredSession::to_info)
            .ok_or_else(|| AppError::not_found_id("session", session_id))
    }

    pub(super) fn history_caps(
        &self,
        session_id: &str,
    ) -> Result<crate::interfaces::SessionHistoryCapabilities, AppError> {
        let live = self.live_read()?;
        if let Some(entry) = live.get(session_id) {
            return Ok(entry.caps.to_history_capabilities(true));
        }
        drop(live);
        if self.contains_dormant(session_id)? {
            return Ok(crate::interfaces::SessionHistoryCapabilities::unavailable());
        }
        Err(AppError::not_found_id("session", session_id))
    }

    pub(super) fn list(&self) -> Result<Vec<Session>, AppError> {
        let live = self.live_read()?;
        let dormant = self.dormant_read()?;
        let mut by_id = HashMap::with_capacity(live.len() + dormant.len());
        for stored in dormant.values() {
            by_id.insert(stored.info.id.clone(), stored.to_info());
        }
        for entry in live.values() {
            by_id.insert(entry.info.id.clone(), entry.info.clone());
        }
        let mut sessions: Vec<_> = by_id.into_values().collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(sessions)
    }

    pub(super) fn rename(&self, session_id: &str, name: &str) -> Result<(), AppError> {
        if let Some(entry) = self.live_write()?.get_mut(session_id) {
            entry.info.name = name.to_string();
            entry.info.updated_at = Utc::now();
            return Ok(());
        }
        let mut dormant = self.dormant_write()?;
        let entry = dormant
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        entry.info.name = name.to_string();
        entry.info.updated_at = Utc::now();
        Ok(())
    }

    pub(super) fn remove_live(&self, session_id: &str) -> Result<SessionEntry, AppError> {
        self.live_write()?
            .remove(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))
    }

    pub(super) fn remove_dormant(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredSession>, AppError> {
        Ok(self.dormant_write()?.remove(session_id))
    }

    pub(super) fn take_interrupted(&self, session_id: &str) -> Option<SessionEntry> {
        let mut live = self.live_write().ok()?;
        if live.get(session_id)?.state != SessionState::Interrupted {
            return None;
        }
        live.remove(session_id)
    }

    pub(super) fn rebind_start(
        &self,
        session_id: &str,
    ) -> Result<(String, String, mpsc::Sender<actor::ActorCommand>), AppError> {
        let mut live = self.live_write()?;
        let entry = live
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        if entry.state != SessionState::Idle {
            return Err(AppError::validation(
                "ACP session must be idle before it can be rebound",
            ));
        }
        entry.apply_state(SessionState::Created);
        Ok((
            entry.info.workspace.clone(),
            entry.info.agent_id.clone(),
            entry.actor.commands(),
        ))
    }

    // Rebind replaces every mutable field; a params struct would add indirection.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rebind_finish(
        &self,
        session_id: &str,
        actor: actor::Handle,
        stderr_tail: Arc<Mutex<StderrTail>>,
        prompt_cancel: Arc<AtomicBool>,
        caps: SessionCaps,
        model_config_id: Option<String>,
        profile_config_id: Option<String>,
        acp_session_id: String,
        agent_id: &str,
        model_id: &str,
    ) -> Result<SessionInfo, AppError> {
        let mut live = self.live_write()?;
        let entry = live
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        entry.actor = actor;
        entry.stderr_tail = stderr_tail;
        entry.prompt_cancel = prompt_cancel;
        entry.caps = caps;
        entry.model_config_id = model_config_id;
        entry.profile_config_id = profile_config_id;
        entry.acp_session_id = acp_session_id;
        entry.info.agent_id = agent_id.to_string();
        entry.info.model_id = model_id.to_string();
        entry.apply_state(SessionState::Idle);
        Ok(entry.info.clone())
    }

    pub(super) fn update_model(&self, session_id: &str, model_id: &str) -> Result<(), AppError> {
        let mut live = self.live_write()?;
        let entry = live
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        entry.info.model_id = model_id.to_string();
        entry.info.updated_at = Utc::now();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn replace_actor_for_test(
        &self,
        session_id: &str,
        actor: actor::Handle,
        profile_config_id: Option<String>,
    ) -> Result<(), AppError> {
        let mut live = self.live_write()?;
        let entry = live
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        entry.actor = actor;
        entry.profile_config_id = profile_config_id;
        Ok(())
    }

    pub(super) fn persist(&self, store: &ConversationStore) -> Result<(), AppError> {
        let live = self.live_read()?;
        let dormant = self.dormant_read()?;
        let mut by_id = HashMap::with_capacity(live.len() + dormant.len());
        for stored in dormant.values() {
            by_id.insert(stored.info.id.clone(), stored.clone());
        }
        for entry in live.values() {
            by_id.insert(
                entry.info.id.clone(),
                StoredSession::from_parts(entry.info.clone(), entry.acp_session_id.clone()),
            );
        }
        store.persist(&by_id.into_values().collect::<Vec<_>>())
    }

    fn live(&self, session_id: &str) -> Result<SessionEntrySnapshot, AppError> {
        let live = self.live_read()?;
        let entry = live
            .get(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        Ok(SessionEntrySnapshot::from(entry))
    }

    fn live_read(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, HashMap<String, SessionEntry>>, AppError> {
        self.live
            .read()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))
    }

    fn live_write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<String, SessionEntry>>, AppError> {
        self.live
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))
    }

    fn dormant_read(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, HashMap<String, StoredSession>>, AppError> {
        self.dormant
            .read()
            .map_err(|_| AppError::internal("ACP dormant session lock poisoned"))
    }

    fn dormant_write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<String, StoredSession>>, AppError> {
        self.dormant
            .write()
            .map_err(|_| AppError::internal("ACP dormant session lock poisoned"))
    }
}

struct SessionEntrySnapshot {
    state: SessionState,
    actor: actor::Handle,
    stderr_tail: Arc<Mutex<StderrTail>>,
    prompt_cancel: Arc<AtomicBool>,
    caps: SessionCaps,
    model_config_id: Option<String>,
    profile_config_id: Option<String>,
}

impl From<&SessionEntry> for SessionEntrySnapshot {
    fn from(entry: &SessionEntry) -> Self {
        Self {
            state: entry.state,
            actor: entry.actor.clone(),
            stderr_tail: Arc::clone(&entry.stderr_tail),
            prompt_cancel: Arc::clone(&entry.prompt_cancel),
            caps: entry.caps,
            model_config_id: entry.model_config_id.clone(),
            profile_config_id: entry.profile_config_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use chrono::Utc;

    use super::super::actor::Handle;
    use super::super::diagnostics::StderrTail;
    use super::{SessionEntry, SessionRegistry, SessionState};
    use crate::acp::providers::SessionCaps;
    use crate::acp::store::{ConversationStore, StoredSession};
    use crate::interfaces::SessionInfo;

    fn info(id: &str, status: &str) -> SessionInfo {
        let now = Utc::now();
        SessionInfo {
            id: id.to_string(),
            name: format!("session {id}"),
            status: status.to_string(),
            agent_id: "mock".to_string(),
            model_id: "mock-model".to_string(),
            workspace: "workspace".to_string(),
            created_at: now,
            updated_at: now,
        }
    }

    fn live_entry(id: &str) -> SessionEntry {
        SessionEntry::new(
            info(id, "created"),
            Handle::dead(),
            std::sync::Arc::new(std::sync::Mutex::new(StderrTail::default())),
            std::sync::Arc::new(AtomicBool::new(false)),
            SessionCaps::default(),
            None,
            None,
            "acp-live".to_string(),
        )
    }

    #[test]
    fn merged_listing_prefers_live_metadata() {
        let registry = SessionRegistry::default();
        registry
            .load_dormant(vec![StoredSession::from_parts(
                info("sess", "running"),
                "acp-old",
            )])
            .expect("load dormant");
        let live = registry.publish(live_entry("sess")).expect("publish live");

        let sessions = registry.list().expect("list sessions");
        assert_eq!(sessions, vec![live]);
        assert_eq!(sessions[0].status, SessionState::Idle.as_str());
    }

    #[test]
    fn persistence_merges_records_and_preserves_acp_session_id() {
        let directory = tempfile::tempdir().expect("temporary store");
        let store = ConversationStore::new(Some(directory.path().join("conversations.json")));
        let registry = SessionRegistry::default();
        registry
            .load_dormant(vec![StoredSession::from_parts(
                info("dormant", "idle"),
                "acp-dormant",
            )])
            .expect("load dormant");
        registry.publish(live_entry("live")).expect("publish live");

        registry.persist(&store).expect("persist registry");
        let stored = store.load().expect("load persisted registry");
        assert_eq!(stored.len(), 2);
        assert!(stored
            .iter()
            .any(|session| session.info.id == "live" && session.acp_session_id == "acp-live"));
        assert!(stored.iter().any(|session| {
            session.info.id == "dormant" && session.acp_session_id == "acp-dormant"
        }));
    }
}
