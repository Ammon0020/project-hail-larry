//! Permission manager: request/response, policies, stale sweep, audit log.
//!
//! Port of Go `internal/permissions/permissions.go`. Receives permission
//! prompts from agents (via the ACP `session/request_permission` flow), blocks
//! the caller until a paired device responds, and enforces durable
//! `allow_always` / `allow_session` / `reject_always` policies so repeated
//! identical prompts auto-resolve without re-prompting.
//!
//! ## Concurrency model
//!
//! All shared state lives behind a single `std::sync::Mutex<Inner>` held inside
//! an `Arc`. Critical sections are short and never cross `.await` boundaries, so
//! a sync mutex is correct and cheaper than a tokio mutex. Awaiting a decision
//! happens on a `oneshot::Receiver` *outside* the lock, so a prompt that never
//! gets a response does not block other requests or the stale sweeper.
//!
//! First-response-wins is enforced by removing the `oneshot::Sender` from the
//! pending map under the lock before sending: only one `respond` / sweeper /
//! `clear_session` call can observe the entry, so exactly one decision reaches
//! the waiting `request` future.
//!
//! ## Cancellation
//!
//! The [`PermissionManager::request`] trait method has no cancellation token
//! parameter; cancellation is future-drop. A [`PendingCleanup`] guard removes
//! the pending entry when the `request` future is dropped (e.g. the agent's RPC
//! deadline elapses), so the prompt does not linger in the map. The stale
//! sweeper is the backstop for any prompt whose future is *not* dropped but
//! whose device went away (Wi-Fi drop mid-session).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tracing::warn;

use super::sink::PermissionSink;
use crate::interfaces::types::{PermissionDecision, PermissionRequest};
use crate::interfaces::{AppError, PermissionManager};

/// How long a prompt may remain unanswered before the stale sweeper auto-denies
/// it. A device that drops Wi-Fi mid-session may never respond; without a bound
/// the agent goroutine blocked in `request` would hang until the agent's own
/// deadline (or forever). Mirrors Go `pendingRequestTimeout`.
pub const DEFAULT_STALE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Stale-sweeper tick interval. Mirrors Go's 60s sweep cadence.
pub const DEFAULT_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Bound on the in-memory audit log so a long-running daemon does not grow it
/// without limit. Only the most recent entries are retained. Mirrors Go
/// `maxAuditEntries`.
pub const MAX_AUDIT_ENTRIES: usize = 10_000;

/// Policy cache key. Mirrors Go `policyKey`.
///
/// `tool_kind` is keyed on the request's `tool` title (the human-readable tool
/// name) rather than the ACP "tool kind", to keep the policy granular enough to
/// distinguish `edit_file` from `execute` while remaining stable across
/// requests.
///
/// `target` is the affected path for file-oriented tools, or empty for
/// shell/execute tools. `command` is the raw command text and is only used as
/// the discriminator when `target` is empty — without it, a single
/// `allow_always` for one shell command would auto-approve every subsequent
/// shell command in the session regardless of content (a permission bypass).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PolicyKey {
    session_id: String,
    tool_kind: String,
    target: String,
    command: String,
}

/// Build the cache key for a permission request. Mirrors Go `policyKeyFor`.
///
/// For file-oriented tools (`target` non-empty) the target path is the
/// discriminator, so `allow_always`/`allow_session` auto-approve repeated
/// operations on the same file. For shell/execute tools (`target` empty) the
/// command text is the discriminator, so an `allow_always` for `go test` does
/// not auto-approve `rm -rf /`.
fn policy_key_for(req: &PermissionRequest) -> PolicyKey {
    let mut key = PolicyKey {
        session_id: req.session_id.clone(),
        tool_kind: req.tool.clone(),
        target: req.target.clone(),
        command: String::new(),
    };
    if req.target.is_empty() {
        key.command.clone_from(&req.command);
    }
    key
}

/// Audit log entry recording a permission decision. Mirrors Go `AuditEntry`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub request_id: String,
    pub session_id: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    pub decision: PermissionDecision,
    pub timestamp: DateTime<Utc>,
}

/// Outstanding permission prompt tracked inside the manager.
struct Pending {
    request: PermissionRequest,
    tx: oneshot::Sender<PermissionDecision>,
    created_at: Instant,
}

/// Locked manager state. All fields are accessed only under `Manager::inner`.
struct Inner {
    pending: HashMap<String, Pending>,
    policy: HashMap<PolicyKey, PermissionDecision>,
    denied: HashSet<PolicyKey>,
    audit: VecDeque<AuditEntry>,
}

/// RAII guard that removes a pending entry on drop.
///
/// The `request` future stores one of these so that cancellation (future drop)
/// cleans the pending map even when no decision arrived. Removal is idempotent:
/// if `respond` / sweeper / `clear_session` already removed the entry, the
/// guard's drop is a no-op.
struct PendingCleanup {
    inner: Arc<Mutex<Inner>>,
    id: String,
}

impl Drop for PendingCleanup {
    fn drop(&mut self) {
        // Idempotent: no-op if the entry was already removed by respond/sweeper.
        // Recover from poison (see Manager::lock) — a panicked holder does not
        // make the pending map unusable.
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .remove(&self.id);
    }
}

/// Permission manager implementing [`PermissionManager`].
///
/// Construct with [`Manager::new`] (default 5-minute stale timeout) or
/// [`Manager::with_timeout`] (tests / custom policy). The optional
/// [`PermissionSink`] publishes new prompts; pass `None` for unit tests that
/// only exercise the policy / pending machinery.
pub struct Manager {
    inner: Arc<Mutex<Inner>>,
    sink: Option<Arc<dyn PermissionSink>>,
    stale_timeout: Duration,
    sweep_interval: Duration,
}

impl Manager {
    /// Create a manager with the default 5-minute stale timeout and 60s sweep
    /// interval. Returns an `Arc` so the stale sweeper task can hold a weak
    /// reference.
    #[must_use]
    pub fn new(sink: Option<Arc<dyn PermissionSink>>) -> Arc<Self> {
        Self::with_timeout(sink, DEFAULT_STALE_TIMEOUT, DEFAULT_SWEEP_INTERVAL)
    }

    /// Create a manager with a custom stale timeout and sweep interval (used by
    /// tests to avoid waiting 5 minutes for a prompt to go stale).
    #[must_use]
    pub fn with_timeout(
        sink: Option<Arc<dyn PermissionSink>>,
        stale_timeout: Duration,
        sweep_interval: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Mutex::new(Inner {
                pending: HashMap::new(),
                policy: HashMap::new(),
                denied: HashSet::new(),
                audit: VecDeque::new(),
            })),
            sink,
            stale_timeout,
            sweep_interval,
        })
    }

    /// Spawn the stale-prompt sweeper background task. The task holds a
    /// `Weak<Manager>` and exits when the manager is dropped. Dropping the
    /// returned `JoinHandle` detaches (does not cancel) the sweeper.
    ///
    /// The sweeper is the backstop for prompts whose `request` future is *not*
    /// dropped but whose responding device went away (Wi-Fi drop). Prompts whose
    /// `request` future is cancelled are cleaned up immediately by the
    /// [`PendingCleanup`] guard.
    #[must_use]
    pub fn start_sweeper(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let weak = Arc::downgrade(self);
        let interval = self.sweep_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // Skip missed ticks rather than bursting after a stall (e.g. GC).
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                match weak.upgrade() {
                    Some(m) => m.cleanup_stale(),
                    None => break, // Manager dropped — exit cleanly.
                }
            }
        })
    }

    /// Helper to deny and remove pending prompts older than the stale timeout.
    fn cleanup_stale_locked(inner: &mut Inner, stale_timeout: Duration) {
        let now = Instant::now();
        let stale_ids: Vec<String> = inner
            .pending
            .iter()
            .filter(|(_, p)| now.duration_since(p.created_at) >= stale_timeout)
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale_ids {
            if let Some(p) = inner.pending.remove(&id) {
                let _ = p.tx.send(PermissionDecision::Deny);
            }
        }
    }

    /// Deny and remove pending prompts older than the stale timeout. The deny
    /// is sent to the waiting `request` future via the oneshot channel; a send
    /// error means the receiver was already dropped (request cancelled), in
    /// which case the entry is simply discarded. Mirrors Go `CleanupStale`.
    pub fn cleanup_stale(&self) {
        Self::cleanup_stale_locked(&mut self.lock(), self.stale_timeout);
    }

    /// Currently pending prompts (for re-presentation on reconnect). Prunes
    /// stale prompts first so a reconnecting client never receives a list
    /// containing prompts whose context already died. Mirrors Go `GetPending`.
    #[must_use]
    pub fn get_pending(&self) -> Vec<PermissionRequest> {
        let mut inner = self.lock();
        Self::cleanup_stale_locked(&mut inner, self.stale_timeout);
        inner.pending.values().map(|p| p.request.clone()).collect()
    }

    /// Snapshot of the audit log (newest decisions appended at the end).
    /// Mirrors Go `GetAuditLog`.
    #[must_use]
    pub fn get_audit_log(&self) -> Vec<AuditEntry> {
        self.lock().audit.iter().cloned().collect()
    }

    /// Drop all cached permission policies for `session_id` and deny any
    /// pending prompts for it. Called when a session closes so
    /// `allow_always`/`allow_session` decisions do not leak across session
    /// lifetimes and in-flight `request` calls return promptly. Mirrors Go
    /// `ClearSession`.
    pub fn clear_session(&self, session_id: &str) {
        let mut inner = self.lock();
        inner.policy.retain(|k, _| k.session_id != session_id);
        inner.denied.retain(|k| k.session_id != session_id);
        // Deny pending prompts for this session so the agent's request RPC does
        // not hang with no response.
        let to_deny: Vec<String> = inner
            .pending
            .iter()
            .filter(|(_, p)| p.request.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in to_deny {
            if let Some(p) = inner.pending.remove(&id) {
                let _ = p.tx.send(PermissionDecision::Deny);
            }
        }
    }

    /// Push an audit entry onto the log under the existing lock. Caller must
    /// hold `inner`.
    fn push_audit_locked(inner: &mut Inner, req: &PermissionRequest, decision: PermissionDecision) {
        inner.audit.push_back(AuditEntry {
            request_id: req.id.clone(),
            session_id: req.session_id.clone(),
            tool: req.tool.clone(),
            command: req.command.clone(),
            decision,
            timestamp: Utc::now(),
        });
        // Bound the in-memory log to the last MAX_AUDIT_ENTRIES entries.
        while inner.audit.len() > MAX_AUDIT_ENTRIES {
            inner.audit.pop_front();
        }
    }

    /// Generate a fresh opaque request ID. Uses UUID v4 (Go used 16 random
    /// bytes hex-encoded); both yield unique-enough opaque identifiers.
    fn generate_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Lock the inner state, recovering the guard even if a previous holder
    /// panicked (mutex poison). Poison is treated as non-fatal: the manager's
    /// invariants are simple enough that a poisoned lock still leaves the maps
    /// in a usable state, and failing here would cascade into every request.
    /// Mirrors the `unwrap_or_else(|p| p.into_inner())` pattern used in
    /// `events::store`.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl PermissionManager for Manager {
    /// Broadcast a permission prompt and block until a decision is received.
    /// The first response wins; a cached `allow_always` / `allow_session` /
    /// `reject_always` policy auto-resolves without broadcasting.
    ///
    /// Cancellation is future-drop: if the caller drops the returned future
    /// (e.g. via `tokio::time::timeout`), the pending entry is removed by the
    /// [`PendingCleanup`] guard and an error is surfaced to the caller through
    /// the dropped future. On a successful decision the audit log is appended
    /// and durable policies (`allow_always` / `allow_session` / `reject_always`)
    /// are cached.
    async fn request(&self, mut req: PermissionRequest) -> Result<PermissionDecision, AppError> {
        // Assign an ID if the caller did not provide one.
        if req.id.is_empty() {
            req.id = Self::generate_id();
        }
        // Default options mirror Go: allow_once / allow_session / allow_always / deny.
        if req.options.is_empty() {
            req.options = vec![
                PermissionDecision::AllowOnce,
                PermissionDecision::AllowSession,
                PermissionDecision::AllowAlways,
                PermissionDecision::Deny,
            ];
        }
        let key = policy_key_for(&req);
        let mut global_key = key.clone();
        global_key.session_id = String::new();
        // Tool-kind-scoped key: blanks session_id, target, and command so it
        // matches any request with the same `tool` title regardless of target
        // or command. This is the broadest allow tier — "always allow this tool
        // kind" (e.g. all `move` operations). SECURITY TRADE-OFF: this is
        // intentionally coarser than `allow_always` (which pins to one exact
        // target/command). The ACP handler only offers this option for a
        // conservative allowlist of tool kinds (`move`, `edit`, `read`,
        // `search` — never `execute`), and the frontend requires an explicit
        // confirm step with warning language before recording it.
        let tool_kind_key = PolicyKey {
            session_id: String::new(),
            tool_kind: req.tool.clone(),
            target: String::new(),
            command: String::new(),
        };

        // Check caches + register pending under one lock acquisition. A cached
        // durable decision auto-resolves immediately (no broadcast, no block).
        let rx = {
            let mut inner = self.lock();
            // 1. Exact-key (session-scoped) then global-key (allow_always).
            if let Some(&d) = inner
                .policy
                .get(&key)
                .or_else(|| inner.policy.get(&global_key))
            {
                if d == PermissionDecision::AllowAlways || d == PermissionDecision::AllowSession {
                    Self::push_audit_locked(&mut inner, &req, d);
                    return Ok(d);
                }
            }
            // 2. Tool-kind-scoped fallback (allow_tool_kind). Checked after the
            // exact/global keys so a narrower grant still wins.
            if let Some(&PermissionDecision::AllowToolKind) = inner.policy.get(&tool_kind_key) {
                Self::push_audit_locked(&mut inner, &req, PermissionDecision::AllowToolKind);
                return Ok(PermissionDecision::AllowToolKind);
            }
            if inner.denied.contains(&key) || inner.denied.contains(&global_key) {
                Self::push_audit_locked(&mut inner, &req, PermissionDecision::Deny);
                return Ok(PermissionDecision::Deny);
            }
            let (tx, rx) = oneshot::channel();
            inner.pending.insert(
                req.id.clone(),
                Pending {
                    request: req.clone(),
                    tx,
                    created_at: Instant::now(),
                },
            );
            rx
        };

        // RAII cleanup: removes the pending entry if this future is dropped
        // before a decision arrives (cancellation via deadline / agent exit).
        let cleanup = PendingCleanup {
            inner: self.inner.clone(),
            id: req.id.clone(),
        };

        // Publish the prompt outside the lock so a slow sink never blocks
        // other requests. Best-effort: the sink logs failures internally.
        if let Some(sink) = &self.sink {
            sink.broadcast_request(&req).await;
        }

        // Await the decision. The sender is removed from `pending` before
        // sending (see `respond` / sweeper / `clear_session`), so exactly one
        // decision reaches this receiver.
        // Sender dropped without sending. Every removal path sends
        // before dropping, so this is unexpected — treat as an internal
        // error and let the cleanup guard remove the entry.
        let Ok(decision) = rx.await else {
            warn!(request_id = %req.id, "permission request sender dropped without decision");
            return Err(AppError::internal(
                "permission request cancelled (sender dropped without decision)",
            ));
        };

        // Record the decision and persist durable policies. `allow_once` and
        // bare `deny` are one-shot and do not seed the cache.
        {
            let mut inner = self.lock();
            Self::push_audit_locked(&mut inner, &req, decision);
            match decision {
                PermissionDecision::AllowSession => {
                    inner.policy.insert(key, decision);
                }
                PermissionDecision::AllowAlways => {
                    inner.policy.insert(global_key, decision);
                }
                // Tool-kind-scoped allow: stored under the global tool_kind_key
                // (session_id, target, command all blanked) so it matches any
                // future request with the same `tool` title regardless of
                // target/command. Like `allow_always`, this is global (survives
                // `clear_session`).
                PermissionDecision::AllowToolKind => {
                    inner.policy.insert(tool_kind_key, decision);
                }
                PermissionDecision::RejectAlways => {
                    inner.denied.insert(global_key);
                }
                PermissionDecision::AllowOnce | PermissionDecision::Deny => {}
            }
        }

        // Explicit drop for clarity; no-op if respond/sweeper already removed.
        drop(cleanup);
        Ok(decision)
    }

    /// Record a decision from a device. First response wins: the pending entry
    /// is removed under the lock, so a concurrent or duplicate respond observes
    /// "not found". An invalid decision (not in the request's offered options)
    /// leaves the entry pending so a valid respond can still succeed.
    async fn respond(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), AppError> {
        // Peek to validate without removing: an invalid decision must leave the
        // prompt pending (matches Go semantics — a device that sends a bad
        // option should not prevent another device from responding correctly).
        let tx = {
            let mut inner = self.lock();
            let valid = inner
                .pending
                .get(request_id)
                .map(|p| p.request.options.contains(&decision));
            match valid {
                None => {
                    return Err(AppError::not_found(format!(
                        "permission request not found or already resolved: {request_id}"
                    )));
                }
                Some(false) => {
                    return Err(AppError::validation(format!(
                        "invalid decision {} for request {request_id}",
                        decision.as_str()
                    )));
                }
                Some(true) => {}
            }
            // Valid: remove and take the sender. The entry cannot vanish
            // between peek and remove because we hold the lock.
            match inner.pending.remove(request_id) {
                Some(p) => p.tx,
                // Should be unreachable under the lock — fail loudly.
                None => {
                    return Err(AppError::internal(
                        "pending entry vanished between peek and remove under lock",
                    ));
                }
            }
        };

        // Send outside the lock. A send error means the receiver was dropped
        // (the request future was cancelled) — the entry is already removed.
        match tx.send(decision) {
            Ok(()) => Ok(()),
            Err(_) => Err(AppError::internal(format!(
                "request already resolved (receiver dropped): {request_id}"
            ))),
        }
    }

    fn clear_session(&self, session_id: &str) {
        Manager::clear_session(self, session_id);
    }

    fn get_pending(&self) -> Vec<PermissionRequest> {
        Manager::get_pending(self)
    }
}
