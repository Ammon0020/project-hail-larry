//! Tests for the permission manager — port of `internal/permissions/permissions_test.go`.
//!
//! Concurrency patterns mirror the Go tests: a `request` call is spawned on a
//! tokio task (it blocks until responded), the test waits for it to register as
//! pending, then calls `respond` and awaits the spawned result. Timeouts use
//! `tokio::time::timeout` so a bug can never hang the suite.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::timeout;

use super::sink::PermissionSink;
use super::Manager;
use crate::interfaces::types::{PermissionDecision as D, PermissionRequest};
use crate::interfaces::PermissionManager;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Sink that records every broadcast request in order. Used by tests that need
/// to assert whether a prompt was published (auto-resolved requests must NOT
/// publish).
#[derive(Default)]
struct CapturingSink {
    calls: StdMutex<Vec<PermissionRequest>>,
}

impl CapturingSink {
    fn new() -> Self {
        Self::default()
    }

    fn requests(&self) -> Vec<PermissionRequest> {
        self.calls.lock().expect("sink lock").clone()
    }
}

#[async_trait]
impl PermissionSink for CapturingSink {
    async fn broadcast_request(&self, req: &PermissionRequest) {
        self.calls.lock().expect("sink lock").push(req.clone());
    }
}

/// Sink that increments an atomic counter — cheaper than `CapturingSink` when
/// only the call count matters.
struct CountingSink {
    n: Arc<AtomicUsize>,
}

impl CountingSink {
    fn new() -> (Self, Arc<AtomicUsize>) {
        let n = Arc::new(AtomicUsize::new(0));
        (Self { n: n.clone() }, n)
    }
}

#[async_trait]
impl PermissionSink for CountingSink {
    async fn broadcast_request(&self, _req: &PermissionRequest) {
        self.n.fetch_add(1, Ordering::SeqCst);
    }
}

/// Poll `manager.get_pending()` until it observes `want` pending prompts, or
/// fail the test after 2s. The policy auto-resolve path never creates a pending
/// entry, so this is how tests assert that a request actually blocked.
async fn wait_for_pending(m: &Manager, want: usize) -> Vec<PermissionRequest> {
    let deadline = Duration::from_secs(2);
    loop {
        let pending = m.get_pending();
        if pending.len() == want {
            return pending;
        }
        if timeout(deadline, tokio::time::sleep(Duration::from_millis(10)))
            .await
            .is_err()
        {
            panic!("expected {want} pending request(s), got {}", pending.len());
        }
    }
}

/// Spawn a `request` call on a background task, wait for it to register as
/// pending, respond with `decision`, and return the decision the `request` call
/// returns. Shared helper for seeding the policy map via the blocking path.
async fn resolve_first_request(
    m: &Arc<Manager>,
    req: PermissionRequest,
    decision: D,
) -> Result<D, crate::interfaces::AppError> {
    let m_clone = m.clone();
    let result = tokio::spawn(async move { m_clone.request(req).await });

    // Wait for the request to register as pending.
    let pending = wait_for_pending(m, 1).await;
    let id = pending[0].id.clone();
    m.respond(&id, decision).await.expect("respond");

    timeout(Duration::from_secs(2), result)
        .await
        .expect("timed out waiting for decision")
        .expect("task panicked")
}

/// Default decision set offered by the manager when a request omits options.
fn default_options() -> Vec<D> {
    vec![D::AllowOnce, D::AllowSession, D::AllowAlways, D::Deny]
}

// ---------------------------------------------------------------------------
// Basic request / respond flow
// ---------------------------------------------------------------------------

/// Verifies that `request` publishes the prompt via the sink so the UI is
/// notified. Mirrors Go `TestRequestInvokesCallback`.
#[tokio::test]
async fn request_invokes_sink() {
    let sink = Arc::new(CapturingSink::new());
    let m = Manager::new(Some(sink.clone()));

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        let _ = m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-1".into(),
                tool: "execute".into(),
                command: "go test ./...".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await;
    });

    // The sink should record exactly one broadcast with a generated ID.
    let req = timeout(Duration::from_secs(1), async {
        loop {
            let reqs = sink.requests();
            if let Some(r) = reqs.into_iter().next() {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("sink was not invoked");

    assert!(
        !req.id.is_empty(),
        "expected sink request to have a generated ID"
    );
    assert_eq!(req.tool, "execute");

    // Resolve so the spawned task exits cleanly.
    let pending = wait_for_pending(&m, 1).await;
    let _ = m.respond(&pending[0].id, D::AllowOnce).await;
    let _ = task.await;
}

/// Verifies the basic request-respond flow. Mirrors Go `TestRequestAndRespond`.
#[tokio::test]
async fn request_and_respond() {
    let m = Manager::new(None);

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "session-1".into(),
                tool: "shell".into(),
                command: "npm test".into(),
                ..Default::default()
            })
            .await
    });

    // Wait for the request to register.
    let pending = wait_for_pending(&m, 1).await;
    assert_eq!(pending[0].command, "npm test");

    m.respond(&pending[0].id, D::AllowOnce)
        .await
        .expect("respond");

    let decision = timeout(Duration::from_secs(1), task)
        .await
        .expect("timed out waiting for decision")
        .expect("task panicked")
        .expect("request error");
    assert_eq!(decision, D::AllowOnce);
}

/// Verifies that a request whose future is dropped (cancelled) surfaces a
/// timeout error to the caller. Mirrors Go `TestRequestTimeout` — the Go test
/// checks both an error and a `Deny` decision; Rust's `Result` type surfaces
/// the cancellation as an `Err`, and the caller is expected to deny on error.
#[tokio::test]
async fn request_timeout_via_drop() {
    let m = Manager::new(None);

    let req = PermissionRequest {
        id: String::new(),
        session_id: "session-1".into(),
        tool: "shell".into(),
        command: "rm -rf /".into(),
        ..Default::default()
    };

    // Drop the future after 100ms — simulates the agent's RPC deadline.
    let result = timeout(Duration::from_millis(100), m.request(req)).await;
    assert!(result.is_err(), "expected timeout error");

    // The dropped future's PendingCleanup guard should have removed the entry.
    // Give the runtime a tick to run the drop.
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        m.get_pending().is_empty(),
        "pending entry should be cleaned up on cancel"
    );
}

/// Verifies that responding to a nonexistent request fails. Mirrors Go
/// `TestRespondNotFound`.
#[tokio::test]
async fn respond_not_found() {
    let m = Manager::new(None);
    let err = m.respond("nonexistent", D::AllowOnce).await;
    assert!(err.is_err(), "expected error for nonexistent request");
}

/// Verifies that the first response is accepted and a second is rejected.
/// Mirrors Go `TestFirstResponseWins`.
#[tokio::test]
async fn first_response_wins() {
    let m = Manager::new(None);

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "session-1".into(),
                tool: "edit_file".into(),
                target: "server.js".into(),
                ..Default::default()
            })
            .await
    });

    let pending = wait_for_pending(&m, 1).await;
    let id = pending[0].id.clone();

    // First response succeeds.
    m.respond(&id, D::AllowSession)
        .await
        .expect("first respond");

    // Second response fails (entry already removed).
    let second = m.respond(&id, D::Deny).await;
    assert!(second.is_err(), "expected error for second response");

    let decision = timeout(Duration::from_secs(1), task)
        .await
        .expect("timed out")
        .expect("task panicked")
        .expect("request error");
    assert_eq!(decision, D::AllowSession);
}

/// Verifies that decisions are recorded in the audit log. Mirrors Go
/// `TestAuditLog`.
#[tokio::test]
async fn audit_log_records_decision() {
    let m = Manager::new(None);

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "session-1".into(),
                tool: "shell".into(),
                command: "npm run build".into(),
                ..Default::default()
            })
            .await
    });

    let pending = wait_for_pending(&m, 1).await;
    m.respond(&pending[0].id, D::AllowAlways)
        .await
        .expect("respond");
    let _ = task.await;

    // Give the request task a tick to record its audit entry after respond.
    tokio::time::sleep(Duration::from_millis(20)).await;

    let log = m.get_audit_log();
    assert_eq!(log.len(), 1, "expected 1 audit entry, got {}", log.len());
    assert_eq!(log[0].tool, "shell");
    assert_eq!(log[0].decision, D::AllowAlways);
}

/// Verifies that an invalid decision (not in the request's offered options) is
/// rejected and leaves the prompt pending. Mirrors Go `TestInvalidDecision`.
#[tokio::test]
async fn invalid_decision_rejected() {
    let m = Manager::new(None);

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        let _ = m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "session-1".into(),
                tool: "shell".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await;
    });

    let pending = wait_for_pending(&m, 1).await;

    // allow_always is not in the offered options.
    let err = m.respond(&pending[0].id, D::AllowAlways).await;
    assert!(err.is_err(), "expected error for invalid decision");

    // The prompt must still be pending (invalid respond does not remove it).
    assert_eq!(
        m.get_pending().len(),
        1,
        "invalid respond should leave prompt pending"
    );

    // Resolve so the spawned task exits cleanly.
    let _ = m.respond(&pending[0].id, D::Deny).await;
    let _ = task.await;
}

// ---------------------------------------------------------------------------
// Policy cache: allow_always / allow_session / allow_once
// ---------------------------------------------------------------------------

/// Verifies that an `allow_always` decision auto-resolves a subsequent identical
/// request without blocking or re-publishing. Mirrors Go
/// `TestPolicyAllowAlwaysAutoResolves`.
#[tokio::test]
async fn policy_allow_always_auto_resolves() {
    let (sink, counter) = CountingSink::new();
    let m = Manager::new(Some(Arc::new(sink)));

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-policy-always".into(),
        tool: "edit_file".into(),
        target: "main.go".into(),
        options: vec![D::AllowAlways, D::Deny],
        ..Default::default()
    };

    // First request blocks and is resolved with allow_always.
    let d = resolve_first_request(&m, req.clone(), D::AllowAlways)
        .await
        .expect("first request");
    assert_eq!(d, D::AllowAlways);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "sink invoked once for first request"
    );

    // Second identical request must auto-resolve immediately (no blocking, no
    // second broadcast).
    let second = timeout(Duration::from_secs(1), m.request(req.clone()))
        .await
        .expect("second request did not auto-resolve (blocked)")
        .expect("request error");
    assert_eq!(second, D::AllowAlways);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "sink not invoked for auto-resolve"
    );
}

/// Verifies that an `allow_session` decision auto-resolves subsequent
/// same-session requests. Mirrors Go `TestPolicyAllowSessionAutoResolves`.
#[tokio::test]
async fn policy_allow_session_auto_resolves() {
    let m = Manager::new(None);

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-policy-session".into(),
        tool: "execute".into(),
        command: "go test".into(),
        target: String::new(),
        options: vec![D::AllowSession, D::Deny],
        ..Default::default()
    };

    let d = resolve_first_request(&m, req.clone(), D::AllowSession)
        .await
        .expect("first request");
    assert_eq!(d, D::AllowSession);

    let second = timeout(Duration::from_secs(1), m.request(req))
        .await
        .expect("second request did not auto-resolve (blocked)")
        .expect("request error");
    assert_eq!(second, D::AllowSession);
}

/// Verifies that an `allow_once` decision does NOT seed the policy map — a
/// second identical request still blocks for user input. Mirrors Go
/// `TestPolicyAllowOnceDoesNotAutoResolve`.
#[tokio::test]
async fn policy_allow_once_does_not_auto_resolve() {
    let m = Manager::new(None);

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-policy-once".into(),
        tool: "shell".into(),
        command: "ls".into(),
        options: vec![D::AllowOnce, D::Deny],
        ..Default::default()
    };

    let d = resolve_first_request(&m, req.clone(), D::AllowOnce)
        .await
        .expect("first request");
    assert_eq!(d, D::AllowOnce);

    // Second request must block (no auto-resolve). Drop the future after 100ms.
    let start = std::time::Instant::now();
    let result = timeout(Duration::from_millis(100), m.request(req)).await;
    let elapsed = start.elapsed();
    assert!(
        result.is_err(),
        "expected second allow_once request to block and time out"
    );
    assert!(
        elapsed >= Duration::from_millis(80),
        "expected request to block ~100ms, returned after {elapsed:?}"
    );

    // Clean up the pending entry so no task lingers.
    let pending = m.get_pending();
    if let Some(p) = pending.first() {
        let _ = m.respond(&p.id, D::Deny).await;
    }
}

#[tokio::test]
async fn policy_session_scoped() {
    let decision = D::AllowSession;
    let m = Manager::new(None);

    let req_a = PermissionRequest {
        id: String::new(),
        session_id: "sess-A".into(),
        tool: "edit_file".into(),
        target: "a.go".into(),
        options: vec![decision, D::Deny],
        ..Default::default()
    };
    let req_b = PermissionRequest {
        id: String::new(),
        session_id: "sess-B".into(),
        tool: "edit_file".into(),
        target: "a.go".into(),
        options: vec![decision, D::Deny],
        ..Default::default()
    };

    // Seed the policy in session A.
    resolve_first_request(&m, req_a, decision)
        .await
        .expect("seed A");

    // Session B's request must still block — different session.
    let start = std::time::Instant::now();
    let result = timeout(Duration::from_millis(100), m.request(req_b)).await;
    let elapsed = start.elapsed();
    assert!(
        result.is_err(),
        "expected session B to block (policy is session-scoped), decision={decision:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(80),
        "expected session B to block ~100ms, returned after {elapsed:?}"
    );

    // Clean up.
    let pending = m.get_pending();
    if let Some(p) = pending.first() {
        let _ = m.respond(&p.id, D::Deny).await;
    }
}

/// Verifies that a policy decision globally scoped (`AllowAlways`) affects session B.
#[tokio::test]
async fn policy_global_scoped() {
    let decision = D::AllowAlways;
    let m = Manager::new(None);

    let req_a = PermissionRequest {
        id: String::new(),
        session_id: "sess-A".into(),
        tool: "edit_file".into(),
        target: "global.go".into(),
        options: vec![decision, D::Deny],
        ..Default::default()
    };
    let req_b = PermissionRequest {
        id: String::new(),
        session_id: "sess-B".into(),
        tool: "edit_file".into(),
        target: "global.go".into(),
        options: vec![decision, D::Deny],
        ..Default::default()
    };

    // Seed the policy in session A.
    resolve_first_request(&m, req_a, decision)
        .await
        .expect("seed A");

    // Session B's request must auto-resolve — policy is global.
    let d = timeout(Duration::from_secs(1), m.request(req_b))
        .await
        .expect("expected session B to auto-resolve")
        .expect("request error");
    assert_eq!(d, decision);
}

/// Verifies that `clear_session` drops cached policies so subsequent requests
/// block again. Mirrors Go `TestClearSessionRemovesPolicies`.
#[tokio::test]
async fn clear_session_removes_policies() {
    let m = Manager::new(None);

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-clear".into(),
        tool: "edit_file".into(),
        target: "main.go".into(),
        options: vec![D::AllowSession, D::Deny],
        ..Default::default()
    };

    // Seed the policy.
    resolve_first_request(&m, req.clone(), D::AllowSession)
        .await
        .expect("seed");

    // Confirm it auto-resolves.
    let d = timeout(Duration::from_secs(1), m.request(req.clone()))
        .await
        .expect("expected auto-resolve before clear")
        .expect("request error");
    assert_eq!(d, D::AllowSession);

    // Clear the session's policies.
    m.clear_session("sess-clear");

    // Now the request must block again.
    let start = std::time::Instant::now();
    let result = timeout(Duration::from_millis(100), m.request(req)).await;
    let elapsed = start.elapsed();
    assert!(
        result.is_err(),
        "expected request to block after clear_session, but it auto-resolved"
    );
    assert!(
        elapsed >= Duration::from_millis(80),
        "expected request to block ~100ms after clear, returned after {elapsed:?}"
    );

    // Clean up.
    let pending = m.get_pending();
    if let Some(p) = pending.first() {
        let _ = m.respond(&p.id, D::Deny).await;
    }
}

/// Verifies that auto-resolved decisions (served from the policy map) still
/// appear in the audit log. Mirrors Go
/// `TestAutoResolvedDecisionRecordedInAuditLog`.
#[tokio::test]
async fn auto_resolved_decision_recorded_in_audit_log() {
    let m = Manager::new(None);

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-audit".into(),
        tool: "edit_file".into(),
        target: "config.json".into(),
        options: vec![D::AllowAlways, D::Deny],
        ..Default::default()
    };

    // Seed via the blocking path (records one audit entry).
    resolve_first_request(&m, req.clone(), D::AllowAlways)
        .await
        .expect("seed");

    let before = m.get_audit_log().len();

    // Auto-resolve a second time — must also record an audit entry.
    m.request(req).await.expect("auto-resolved request");

    let log = m.get_audit_log();
    assert_eq!(
        log.len(),
        before + 1,
        "expected audit log to grow by 1 for auto-resolved decision, got {} (was {before})",
        log.len()
    );

    let entry = log.last().expect("log non-empty");
    assert_eq!(entry.decision, D::AllowAlways);
    assert_eq!(entry.session_id, "sess-audit");
    assert_eq!(entry.tool, "edit_file");
}

// ---------------------------------------------------------------------------
// Shell command scoping (Fix 2.1 in Go)
// ---------------------------------------------------------------------------

/// Verifies that an `allow_always` for one shell command does NOT auto-approve
/// a different shell command in the same session. Shell commands have no
/// location (target == ""), so the command text is the discriminator. Mirrors
/// Go `TestPolicyShellCommandBypass`.
#[tokio::test]
async fn policy_shell_command_bypass() {
    let m = Manager::new(None);

    let first = PermissionRequest {
        id: String::new(),
        session_id: "sess-shell-bypass".into(),
        tool: "execute".into(),
        command: "go test".into(),
        target: String::new(),
        options: vec![D::AllowAlways, D::Deny],
        ..Default::default()
    };
    let d = resolve_first_request(&m, first, D::AllowAlways)
        .await
        .expect("first");
    assert_eq!(d, D::AllowAlways);

    // A different command in the same session must NOT auto-resolve.
    let second = PermissionRequest {
        id: String::new(),
        session_id: "sess-shell-bypass".into(),
        tool: "execute".into(),
        command: "rm -rf /".into(),
        target: String::new(),
        options: vec![D::AllowAlways, D::Deny],
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let result = timeout(Duration::from_millis(100), m.request(second)).await;
    let elapsed = start.elapsed();
    assert!(
        result.is_err(),
        "expected different shell command to block (not auto-resolved)"
    );
    assert!(
        elapsed >= Duration::from_millis(80),
        "expected different shell command to block ~100ms, returned after {elapsed:?}"
    );

    let pending = m.get_pending();
    if let Some(p) = pending.first() {
        let _ = m.respond(&p.id, D::Deny).await;
    }
}

/// Verifies that the SAME shell command still auto-resolves after an
/// `allow_session` — command-text keying does not break the legitimate
/// auto-approve path. Mirrors Go `TestPolicySameShellCommandAutoResolves`.
#[tokio::test]
async fn policy_same_shell_command_auto_resolves() {
    let m = Manager::new(None);

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-shell-same".into(),
        tool: "execute".into(),
        command: "npm test".into(),
        target: String::new(),
        options: vec![D::AllowSession, D::Deny],
        ..Default::default()
    };
    let d = resolve_first_request(&m, req.clone(), D::AllowSession)
        .await
        .expect("first");
    assert_eq!(d, D::AllowSession);

    let second = timeout(Duration::from_secs(1), m.request(req))
        .await
        .expect("same shell command did not auto-resolve (blocked)")
        .expect("request error");
    assert_eq!(second, D::AllowSession);
}

// ---------------------------------------------------------------------------
// Stale sweeper
// ---------------------------------------------------------------------------

/// Verifies that `cleanup_stale` denies and removes a pending prompt older than
/// the stale timeout, unblocking the waiting `request` future with `Deny`.
/// Uses a short stale timeout so the test does not wait 5 minutes. Mirrors Go
/// `TestCleanupStaleDeniesExpiredRequest`.
#[tokio::test]
async fn cleanup_stale_denies_expired_request() {
    // 100ms stale timeout — short enough for a fast test, long enough that the
    // request registers before going stale.
    let m = Manager::with_timeout(
        None,
        Duration::from_millis(100),
        Duration::from_mins(1),
        Duration::from_mins(10),
    );

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-stale".into(),
                tool: "execute".into(),
                command: "rm -rf /tmp/x".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await
    });

    // Wait for the request to register.
    wait_for_pending(&m, 1).await;

    // Wait until the prompt is older than the stale timeout, then sweep.
    tokio::time::sleep(Duration::from_millis(150)).await;
    m.cleanup_stale();

    // The pending map should now be empty.
    assert!(
        m.get_pending().is_empty(),
        "expected 0 pending after cleanup_stale"
    );

    // The blocked request must unblock with Deny.
    let decision = timeout(Duration::from_secs(2), task)
        .await
        .expect("stale request was not unblocked by cleanup_stale (timed out)")
        .expect("task panicked")
        .expect("request error");
    assert_eq!(decision, D::Deny);
}

/// Verifies that `cleanup_stale` does NOT touch a fresh prompt — only stale
/// prompts are pruned. Mirrors Go `TestCleanupStaleKeepsFreshRequest`.
#[tokio::test]
async fn cleanup_stale_keeps_fresh_request() {
    // Long stale timeout so the prompt is always fresh during the test.
    let m = Manager::with_timeout(
        None,
        Duration::from_mins(1),
        Duration::from_mins(1),
        Duration::from_mins(10),
    );

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        let _ = m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-fresh".into(),
                tool: "execute".into(),
                command: "echo hi".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await;
    });

    let pending = wait_for_pending(&m, 1).await;

    // cleanup_stale on a fresh prompt should leave it intact.
    m.cleanup_stale();
    assert_eq!(
        m.get_pending().len(),
        1,
        "expected 1 pending after cleanup_stale on fresh"
    );

    // Resolve so the spawned task exits cleanly.
    let _ = m.respond(&pending[0].id, D::Deny).await;
    let _ = task.await;
}

/// Verifies the sweeper background task auto-denies a stale prompt without a
/// manual `cleanup_stale` call. Uses a short stale timeout + sweep interval.
#[tokio::test]
async fn sweeper_auto_denies_stale_prompt() {
    let m = Manager::with_timeout(
        None,
        Duration::from_millis(80),
        Duration::from_millis(20),
        Duration::from_mins(10),
    );
    let _handle = m.start_sweeper();

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-sweeper".into(),
                tool: "execute".into(),
                command: "long running".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await
    });

    // The sweeper should deny the prompt within a few sweep ticks.
    let decision = timeout(Duration::from_secs(2), task)
        .await
        .expect("sweeper did not unblock stale request (timed out)")
        .expect("task panicked")
        .expect("request error");
    assert_eq!(decision, D::Deny);
}

// ---------------------------------------------------------------------------
// reject_always deny cache
// ---------------------------------------------------------------------------

/// Verifies that a `reject_always` decision auto-denies a subsequent identical
/// request without blocking or re-publishing, and that the auto-deny is
/// recorded in the audit log. Mirrors Go `TestPolicyRejectAlwaysAutoDenies`.
#[tokio::test]
async fn policy_reject_always_auto_denies() {
    let (sink, counter) = CountingSink::new();
    let m = Manager::new(Some(Arc::new(sink)));

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-reject-always".into(),
        tool: "edit_file".into(),
        target: "main.go".into(),
        options: vec![D::RejectAlways, D::Deny],
        ..Default::default()
    };

    // First request blocks and is resolved with reject_always.
    let d = resolve_first_request(&m, req.clone(), D::RejectAlways)
        .await
        .expect("first request");
    assert_eq!(d, D::RejectAlways);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "sink invoked once for first request"
    );

    // Second identical request must auto-deny immediately (no blocking, no
    // second broadcast).
    let second = timeout(Duration::from_secs(1), m.request(req.clone()))
        .await
        .expect("second request did not auto-deny (blocked)")
        .expect("request error");
    assert_eq!(second, D::Deny, "auto-denied request should return Deny");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "sink not invoked for auto-deny"
    );

    // The auto-deny must be recorded in the audit log (seed + auto-deny = 2).
    let log = m.get_audit_log();
    assert_eq!(log.len(), 2, "expected 2 audit entries (seed + auto-deny)");
    assert_eq!(log[1].decision, D::Deny);
}

/// Verifies that `clear_session` does NOT drop cached `reject_always` decisions
/// since they are globally scoped.
#[tokio::test]
async fn clear_session_retains_global_deny_cache() {
    let m = Manager::new(None);

    let req = PermissionRequest {
        id: String::new(),
        session_id: "sess-clear-deny".into(),
        tool: "edit_file".into(),
        target: "main.go".into(),
        options: vec![D::RejectAlways, D::Deny],
        ..Default::default()
    };

    // Seed the deny cache.
    resolve_first_request(&m, req.clone(), D::RejectAlways)
        .await
        .expect("seed");

    // Confirm it auto-denies.
    let d = timeout(Duration::from_secs(1), m.request(req.clone()))
        .await
        .expect("expected auto-deny before clear")
        .expect("request error");
    assert_eq!(d, D::Deny);

    // Clear the session's cache.
    m.clear_session("sess-clear-deny");

    // Now the request must still auto-deny (since RejectAlways is global).
    let d2 = timeout(Duration::from_secs(1), m.request(req.clone()))
        .await
        .expect("expected auto-deny after clear")
        .expect("request error");
    assert_eq!(d2, D::Deny);
}

/// Verifies that a `reject_always` for one target does NOT auto-deny a request
/// for a different target in the same session. The deny cache is keyed by
/// (session, tool, target), matching the allow cache. Mirrors Go
/// `TestRejectAlwaysTargetScoped`.
#[tokio::test]
async fn reject_always_target_scoped() {
    let m = Manager::new(None);

    let first = PermissionRequest {
        id: String::new(),
        session_id: "sess-reject-scope".into(),
        tool: "edit_file".into(),
        target: "a.go".into(),
        options: vec![D::RejectAlways, D::Deny],
        ..Default::default()
    };
    let d = resolve_first_request(&m, first, D::RejectAlways)
        .await
        .expect("first");
    assert_eq!(d, D::RejectAlways);

    // A different target in the same session must NOT auto-deny.
    let second = PermissionRequest {
        id: String::new(),
        session_id: "sess-reject-scope".into(),
        tool: "edit_file".into(),
        target: "b.go".into(),
        options: vec![D::RejectAlways, D::Deny],
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let result = timeout(Duration::from_millis(100), m.request(second)).await;
    let elapsed = start.elapsed();
    assert!(
        result.is_err(),
        "expected different target to block (not auto-denied)"
    );
    assert!(
        elapsed >= Duration::from_millis(80),
        "expected different target to block ~100ms, returned after {elapsed:?}"
    );

    let pending = m.get_pending();
    if let Some(p) = pending.first() {
        let _ = m.respond(&p.id, D::Deny).await;
    }
}

// ---------------------------------------------------------------------------
// clear_session denies pending prompts
// ---------------------------------------------------------------------------

/// Verifies that `clear_session` denies pending prompts for the session so the
/// agent's request RPC does not hang. (Companion to the Go `ClearSession`
/// behavior — the deny path is exercised in production when a session closes
/// with an in-flight prompt.)
#[tokio::test]
async fn clear_session_denies_pending_prompts() {
    let m = Manager::new(None);

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-clear-pending".into(),
                tool: "shell".into(),
                command: "sleep 100".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await
    });

    wait_for_pending(&m, 1).await;

    // Clearing the session must deny the in-flight prompt.
    m.clear_session("sess-clear-pending");

    let decision = timeout(Duration::from_secs(1), task)
        .await
        .expect("clear_session did not unblock pending prompt")
        .expect("task panicked")
        .expect("request error");
    assert_eq!(decision, D::Deny);
}

// ---------------------------------------------------------------------------
// Default options
// ---------------------------------------------------------------------------

/// Verifies that a request with no options is offered the default decision set.
/// (Mirrors the Go default-options behavior exercised implicitly across tests.)
#[tokio::test]
async fn default_options_when_unspecified() {
    let m = Manager::new(None);

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        let _ = m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-defaults".into(),
                tool: "shell".into(),
                command: "ls".into(),
                ..Default::default() // no options
            })
            .await;
    });

    let pending = wait_for_pending(&m, 1).await;
    assert_eq!(
        pending[0].options,
        default_options(),
        "manager should fill in default options when none are provided"
    );

    let _ = m.respond(&pending[0].id, D::AllowOnce).await;
    let _ = task.await;
}

// ---------------------------------------------------------------------------
// EventBusPermissionSink integration (tempfile-backed EventBus)
// ---------------------------------------------------------------------------

/// Verifies the production sink persists + publishes a `PermissionRequested`
/// event through a real (tempfile-backed) `EventBus`. A subscriber should receive
/// the event with the request's fields populated.
#[tokio::test]
async fn event_bus_sink_publishes_permission_requested() {
    use crate::events::{EventBus, Store};
    use tempfile::TempDir;

    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("events.db");
    let store = Store::open(&path).expect("open store");
    let bus = Arc::new(EventBus::new(store));
    let sink = Arc::new(super::EventBusPermissionSink::new(bus.clone()));

    let m = Manager::new(Some(sink));

    let req = PermissionRequest {
        id: "req-1".into(),
        session_id: "sess-bus".into(),
        tool: "edit_file".into(),
        target: "main.rs".into(),
        options: vec![D::AllowOnce, D::Deny],
        ..Default::default()
    };

    // Spawn the request so it blocks on a decision.
    let m_clone = m.clone();
    let task = tokio::spawn(async move { m_clone.request(req).await });

    // Subscribe and wait for the PermissionRequested event.
    let mut sub = bus.subscribe(0).await.expect("subscribe");
    let event = timeout(Duration::from_secs(2), sub.recv())
        .await
        .expect("timed out waiting for published event")
        .expect("subscription closed");

    assert_eq!(
        event.event_type,
        crate::interfaces::types::EventType::PermissionRequested
    );
    assert_eq!(event.session_id, "sess-bus");
    assert_eq!(event.tool, "edit_file");
    assert_eq!(event.target, "main.rs");
    assert_eq!(event.request_id, "req-1");
    assert!(!event.options.is_empty());

    // Resolve so the spawned task exits cleanly.
    let pending = wait_for_pending(&m, 1).await;
    let _ = m.respond(&pending[0].id, D::AllowOnce).await;
    let _ = task.await;
}

// ---------------------------------------------------------------------------
// allow_tool_kind (tool-kind-scoped allow)
// ---------------------------------------------------------------------------

/// Verifies that an `allow_tool_kind` decision for `move` auto-resolves a
/// subsequent `move` request with a **different target**. The tool-kind key
/// blanks target/command, so any request with the same `tool` title matches
/// regardless of which file is being moved.
#[tokio::test]
async fn policy_allow_tool_kind_auto_resolves_different_target() {
    let (sink, counter) = CountingSink::new();
    let m = Manager::new(Some(Arc::new(sink)));

    let first = PermissionRequest {
        id: String::new(),
        session_id: "sess-tool-kind".into(),
        tool: "move".into(),
        target: "src/a.rs".into(),
        options: vec![D::AllowToolKind, D::AllowOnce, D::Deny],
        ..Default::default()
    };

    // First request blocks and is resolved with allow_tool_kind.
    let d = resolve_first_request(&m, first, D::AllowToolKind)
        .await
        .expect("first request");
    assert_eq!(d, D::AllowToolKind);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "sink invoked once for first request"
    );

    // Second request with a DIFFERENT target must auto-resolve immediately
    // (no blocking, no second broadcast) because the tool-kind key ignores
    // target/command.
    let second = PermissionRequest {
        id: String::new(),
        session_id: "sess-tool-kind".into(),
        tool: "move".into(),
        target: "src/b.rs".into(),
        options: vec![D::AllowToolKind, D::AllowOnce, D::Deny],
        ..Default::default()
    };
    let result = timeout(Duration::from_secs(1), m.request(second))
        .await
        .expect("second request did not auto-resolve (blocked)")
        .expect("request error");
    assert_eq!(result, D::AllowToolKind);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "sink not invoked for tool-kind auto-resolve"
    );

    // The auto-resolve must be recorded in the audit log (seed + auto = 2).
    let log = m.get_audit_log();
    assert_eq!(log.len(), 2, "expected 2 audit entries (seed + auto)");
    assert_eq!(log[1].decision, D::AllowToolKind);
}

/// Verifies that an `allow_tool_kind` decision for one tool does NOT
/// auto-resolve a request for a different tool kind. The tool-kind key
/// includes the `tool` title, so `move` and `edit` grants are independent.
#[tokio::test]
async fn allow_tool_kind_does_not_cross_tool_kinds() {
    let m = Manager::new(None);

    let first = PermissionRequest {
        id: String::new(),
        session_id: "sess-tool-kind-cross".into(),
        tool: "move".into(),
        target: "src/a.rs".into(),
        options: vec![D::AllowToolKind, D::Deny],
        ..Default::default()
    };
    resolve_first_request(&m, first, D::AllowToolKind)
        .await
        .expect("seed");

    // A different tool kind (`edit`) must NOT auto-resolve from the `move`
    // grant — it should block.
    let second = PermissionRequest {
        id: String::new(),
        session_id: "sess-tool-kind-cross".into(),
        tool: "edit".into(),
        target: "src/a.rs".into(),
        options: vec![D::AllowToolKind, D::Deny],
        ..Default::default()
    };
    let result = timeout(Duration::from_millis(100), m.request(second)).await;
    assert!(
        result.is_err(),
        "expected different tool kind to block (not auto-resolved)"
    );

    // Clean up the pending request so the test doesn't leak.
    let pending = m.get_pending();
    if let Some(p) = pending.first() {
        let _ = m.respond(&p.id, D::Deny).await;
    }
}

// ---------------------------------------------------------------------------
// Permission prompt timeout (auto-deny when no device responds)
// ---------------------------------------------------------------------------

/// Verifies that an unanswered permission prompt is auto-denied after the
/// configured `permission_timeout` elapses, so the agent does not hang
/// forever. The deny reaches the waiting `request` future via the normal
/// oneshot channel.
#[tokio::test]
async fn permission_timeout_auto_denies_unanswered_prompt() {
    // 50ms permission timeout — short enough for a fast test. The stale
    // timeout is set high so only the per-prompt timeout fires.
    let m = Manager::with_timeout(
        None,
        Duration::from_mins(10),
        Duration::from_mins(10),
        Duration::from_millis(50),
    );

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-timeout".into(),
                tool: "execute".into(),
                command: "echo timeout".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await
    });

    // Wait for the request to register as pending.
    let pending = wait_for_pending(&m, 1).await;
    assert_eq!(pending[0].command, "echo timeout");

    // Do NOT respond — let the per-prompt timeout fire.
    let decision = timeout(Duration::from_secs(2), task)
        .await
        .expect("timed out waiting for auto-deny")
        .expect("task panicked")
        .expect("request error");
    assert_eq!(
        decision,
        D::Deny,
        "expected auto-deny after permission timeout"
    );

    // The pending map should be empty after the timeout.
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        m.get_pending().is_empty(),
        "pending entry should be removed after timeout auto-deny"
    );
}

/// Verifies that a prompt answered before the permission timeout does NOT
/// trigger an auto-deny — first-response-wins ensures the device decision
/// reaches the agent.
#[tokio::test]
async fn permission_timeout_does_not_fire_if_responded() {
    // 100ms permission timeout — long enough to respond first.
    let m = Manager::with_timeout(
        None,
        Duration::from_mins(10),
        Duration::from_mins(10),
        Duration::from_millis(100),
    );

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-respond-first".into(),
                tool: "execute".into(),
                command: "echo respond".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await
    });

    // Wait for the request to register, then respond before the timeout.
    let pending = wait_for_pending(&m, 1).await;
    m.respond(&pending[0].id, D::AllowOnce)
        .await
        .expect("respond");

    let decision = timeout(Duration::from_secs(2), task)
        .await
        .expect("timed out waiting for decision")
        .expect("task panicked")
        .expect("request error");
    assert_eq!(
        decision,
        D::AllowOnce,
        "device response must win over the not-yet-fired timeout"
    );

    // Give the timeout task a chance to fire (it should be a no-op).
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        m.get_pending().is_empty(),
        "pending entry should be gone after respond"
    );
}

/// Verifies that the sink's `broadcast_timeout` is invoked when a permission
/// times out, so the frontend can surface a visible warning.
#[tokio::test]
async fn permission_timeout_invokes_sink_broadcast_timeout() {
    use std::sync::atomic::AtomicBool;

    struct TimeoutCapturingSink {
        timed_out: Arc<AtomicBool>,
    }

    #[async_trait]
    impl PermissionSink for TimeoutCapturingSink {
        async fn broadcast_request(&self, _req: &PermissionRequest) {}

        async fn broadcast_timeout(&self, _req: &PermissionRequest) {
            self.timed_out.store(true, Ordering::SeqCst);
        }
    }

    let timed_out = Arc::new(AtomicBool::new(false));
    let sink = Arc::new(TimeoutCapturingSink {
        timed_out: timed_out.clone(),
    });
    let m = Manager::with_timeout(
        Some(sink),
        Duration::from_mins(10),
        Duration::from_mins(10),
        Duration::from_millis(50),
    );

    let m_clone = m.clone();
    let task = tokio::spawn(async move {
        let _ = m_clone
            .request(PermissionRequest {
                id: String::new(),
                session_id: "sess-timeout-sink".into(),
                tool: "execute".into(),
                command: "echo sink".into(),
                options: vec![D::AllowOnce, D::Deny],
                ..Default::default()
            })
            .await;
    });

    // Wait for the timeout to fire and the request to resolve.
    let _ = timeout(Duration::from_secs(2), task)
        .await
        .expect("timed out waiting for auto-deny");

    // Give the broadcast_timeout call a tick to complete.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        timed_out.load(Ordering::SeqCst),
        "broadcast_timeout should be called on permission timeout"
    );
}
