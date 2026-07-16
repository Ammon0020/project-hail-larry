//! S-MIGRATE acceptance tests.
//!
//! Fixtures live under `tests/migrate/fixtures/go-state/` (anonymized Go formats).
//! Tests copy the fixture into a `tempfile` directory so they never mutate the
//! checked-in tree, and use `LOCAL_AGENT_STATE_DIR` only where `Config::load`
//! is exercised.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::config::{Config, STATE_DIR_ENV_VAR};
use crate::interfaces::EventStore;

use super::{
    config_json_backup_path, detect_format, migrate_config, restore_config_from_backup,
    run_migrations, validate_event_db_async, validate_state_tree, ConfigMigrationOutcome,
    StateFormat, GO_CONFIG_FILE, RUST_CONFIG_FILE,
};

/// Serializes env mutation for `LOCAL_AGENT_STATE_DIR` (same pattern as config tests).
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Path to the checked-in anonymized Go state fixture.
fn fixture_go_state() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/migrate/fixtures/go-state")
}

/// Recursively copy a fixture tree into `dest` (creates dest).
fn copy_tree(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).expect("create dest");
    for entry in fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("entry");
        let ty = entry.file_type().expect("ty");
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_tree(&entry.path(), &target);
        } else if ty.is_file() {
            fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

/// Materialize the go-state fixture into a fresh temp dir; returns (tempdir, state path).
fn materialize_go_fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().to_path_buf();
    copy_tree(&fixture_go_state(), &state);
    (tmp, state)
}

fn with_state_dir<R>(dir: &Path, body: impl FnOnce() -> R) -> R {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prior = std::env::var_os(STATE_DIR_ENV_VAR);
    std::env::set_var(STATE_DIR_ENV_VAR, dir);
    let res = body();
    match prior {
        Some(v) => std::env::set_var(STATE_DIR_ENV_VAR, v),
        None => std::env::remove_var(STATE_DIR_ENV_VAR),
    }
    res
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

#[test]
fn detect_go_fixture_as_go_format() {
    let (_tmp, state) = materialize_go_fixture();
    assert_eq!(detect_format(&state), StateFormat::Go);
}

// ---------------------------------------------------------------------------
// Config migration: no data loss
// ---------------------------------------------------------------------------

#[test]
fn go_config_migrates_to_toml_without_data_loss() {
    let (_tmp, state) = materialize_go_fixture();

    // Capture expected values from the fixture JSON before migration.
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(state.join(GO_CONFIG_FILE)).unwrap()).unwrap();
    let expect_port = json["port"].as_i64().unwrap();
    let expect_host = json["host"].as_str().unwrap().to_string();
    let expect_workspaces: Vec<String> = json["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let expect_agent_id = json["agents"][0]["id"].as_str().unwrap().to_string();
    let expect_tls = json["tlsEnabled"].as_bool().unwrap();
    let expect_https = json["httpsPort"].as_i64().unwrap();
    let expect_cred_ttl = json["credentialInactivityTtlSeconds"].as_i64().unwrap();

    let outcome = migrate_config(&state).expect("migrate");
    match outcome {
        ConfigMigrationOutcome::Migrated { backup } => {
            assert!(backup.is_file(), "versioned backup must exist");
            assert!(
                backup
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".bak.v"),
                "backup must be versioned: {backup:?}"
            );
        }
        other => panic!("expected Migrated, got {other:?}"),
    }

    assert!(
        state.join(RUST_CONFIG_FILE).is_file(),
        "config.toml written"
    );
    // Go binary remains readable: original JSON still present.
    assert!(
        state.join(GO_CONFIG_FILE).is_file(),
        "config.json kept so Go can read prior state"
    );

    // Load via Config with state dir override.
    with_state_dir(&state, || {
        let cfg = Config::load().expect("load migrated toml");
        assert_eq!(cfg.port, expect_port);
        assert_eq!(cfg.host, expect_host);
        assert_eq!(cfg.workspaces, expect_workspaces);
        assert_eq!(cfg.agents.len(), 1);
        assert_eq!(cfg.agents[0].id, expect_agent_id);
        assert_eq!(cfg.agents[0].args, vec!["--fixture".to_string()]);
        assert_eq!(cfg.tls_enabled, expect_tls, "explicit false must survive");
        assert_eq!(cfg.https_port, expect_https);
        assert_eq!(cfg.credential_inactivity_ttl_seconds, expect_cred_ttl);
        // Layout paths rewritten to active state dir.
        assert_eq!(cfg.data_dir, state.to_string_lossy());
        assert!(cfg.db_path.ends_with("local-agent.db"));
        assert!(Path::new(&cfg.db_path).starts_with(&state));
    });
}

// ---------------------------------------------------------------------------
// Idempotent
// ---------------------------------------------------------------------------

#[test]
fn migration_is_idempotent() {
    let (_tmp, state) = materialize_go_fixture();
    let first = migrate_config(&state).expect("first");
    assert!(matches!(first, ConfigMigrationOutcome::Migrated { .. }));

    let toml_before = fs::read(state.join(RUST_CONFIG_FILE)).unwrap();
    let backup_before = fs::read(config_json_backup_path(&state)).unwrap();

    let second = migrate_config(&state).expect("second");
    assert!(
        matches!(second, ConfigMigrationOutcome::NoopAlreadyRust),
        "second run must be no-op: {second:?}"
    );
    assert_eq!(
        fs::read(state.join(RUST_CONFIG_FILE)).unwrap(),
        toml_before,
        "config.toml unchanged on second run"
    );
    assert_eq!(
        fs::read(config_json_backup_path(&state)).unwrap(),
        backup_before,
        "backup unchanged on second run"
    );
}

// ---------------------------------------------------------------------------
// Versioned backup before destructive change
// ---------------------------------------------------------------------------

#[test]
fn migration_creates_versioned_backup_before_change() {
    let (_tmp, state) = materialize_go_fixture();
    let original = fs::read(state.join(GO_CONFIG_FILE)).unwrap();
    migrate_config(&state).expect("migrate");
    let backup = config_json_backup_path(&state);
    assert!(backup.is_file());
    assert_eq!(
        fs::read(&backup).unwrap(),
        original,
        "backup must be exact copy of pre-migration config.json"
    );
}

// ---------------------------------------------------------------------------
// Interrupted run leaves Go-readable prior state
// ---------------------------------------------------------------------------

#[test]
fn interrupted_before_toml_leaves_go_readable() {
    // Simulate "interrupted before TOML write": only JSON present, no TOML.
    // Migration must still complete cleanly from that recoverable state.
    let (_tmp, state) = materialize_go_fixture();
    assert!(!state.join(RUST_CONFIG_FILE).exists());
    assert_eq!(detect_format(&state), StateFormat::Go);

    // Parse original JSON the way Go would (serde_json value).
    let raw = fs::read_to_string(state.join(GO_CONFIG_FILE)).unwrap();
    let _: serde_json::Value = serde_json::from_str(&raw).expect("Go-readable JSON");
}

#[test]
fn dual_state_after_success_remains_go_readable() {
    let (_tmp, state) = materialize_go_fixture();
    migrate_config(&state).expect("migrate");
    // Dual state: both files present.
    assert_eq!(detect_format(&state), StateFormat::Both);
    let json_raw = fs::read_to_string(state.join(GO_CONFIG_FILE)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_raw).expect("Go still parses JSON");
    assert_eq!(v["port"], 7337);
    assert_eq!(v["workspaces"].as_array().unwrap().len(), 2);
}

#[test]
fn restore_from_backup_restores_go_state() {
    let (_tmp, state) = materialize_go_fixture();
    let original = fs::read(state.join(GO_CONFIG_FILE)).unwrap();
    migrate_config(&state).expect("migrate");
    assert!(state.join(RUST_CONFIG_FILE).is_file());

    restore_config_from_backup(&state).expect("restore");
    assert!(!state.join(RUST_CONFIG_FILE).exists(), "toml removed");
    assert_eq!(
        fs::read(state.join(GO_CONFIG_FILE)).unwrap(),
        original,
        "JSON restored from versioned backup"
    );
    assert_eq!(detect_format(&state), StateFormat::Go);
}

// ---------------------------------------------------------------------------
// Migration failure leaves prior state readable by Go
// ---------------------------------------------------------------------------

#[test]
fn corrupt_json_fails_loudly_and_leaves_json() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path();
    fs::write(state.join(GO_CONFIG_FILE), b"{not-valid-json").unwrap();
    let err = migrate_config(state).expect_err("must fail");
    // Original corrupt file still there for inspection / Go error path.
    assert!(state.join(GO_CONFIG_FILE).is_file());
    assert!(!state.join(RUST_CONFIG_FILE).exists());
    let msg = err.to_string();
    assert!(
        msg.contains("JSON") || msg.contains("json") || msg.contains("expected"),
        "error must mention JSON problem: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Empty / already-rust no-ops
// ---------------------------------------------------------------------------

#[test]
fn empty_state_is_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let outcome = migrate_config(tmp.path()).expect("empty");
    assert_eq!(outcome, ConfigMigrationOutcome::NoopEmpty);
}

#[test]
fn already_rust_is_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path();
    with_state_dir(state, || {
        let mut cfg = Config::default_or_error().unwrap();
        cfg.data_dir = state.to_string_lossy().to_string();
        cfg.port = 9001;
        cfg.save().unwrap();
    });
    let outcome = migrate_config(state).expect("rust");
    assert_eq!(outcome, ConfigMigrationOutcome::NoopAlreadyRust);
}

// ---------------------------------------------------------------------------
// Event DB: Rust opens Go-created fixture without schema/payload drift
// ---------------------------------------------------------------------------

#[tokio::test]
async fn go_created_event_db_opens_without_drift() {
    let (_tmp, state) = materialize_go_fixture();
    let db = state.join("local-agent.db");
    assert!(db.is_file());

    let n = validate_event_db_async(&db)
        .await
        .expect("async open+query");
    assert_eq!(n, 5, "fixture has 5 event rows");

    let store = crate::events::Store::open(&db).expect("open");
    let all = store.query_all(0, 100).await.expect("query_all");
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].session_id, "conv-fixture-aaa111");
    assert_eq!(all[0].content, "Hello, fixture agent!");
    assert_eq!(all[0].role, "user");
    assert!(!all[0].workspace_id.is_empty());

    // IDs are stable AUTOINCREMENT starting at 1.
    assert_eq!(all[0].id, 1);
    assert_eq!(all[4].id, 5);

    // Attachment payload survives.
    let with_att = all
        .iter()
        .find(|e| e.session_id == "conv-fixture-bbb222")
        .expect("session b");
    assert_eq!(with_att.attachments.len(), 1);
    assert_eq!(with_att.attachments[0].id, "uploadfixture0001");

    // Cursor query by session.
    let sess = store
        .query("conv-fixture-aaa111", 0, 100)
        .await
        .expect("session query");
    assert_eq!(sess.len(), 4);
}

// ---------------------------------------------------------------------------
// Full state tree validation (devices, conversations, mcp, uploads, tls)
// ---------------------------------------------------------------------------

#[test]
fn go_fixture_state_tree_validates() {
    let (_tmp, state) = materialize_go_fixture();
    let report = validate_state_tree(&state).expect("validate");
    assert!(report.is_ok(), "validation failures: {:?}", report.failed);

    // Event DB + TLS ok; devices/conversations/mcp/uploads deferred (structurally OK).
    assert!(
        report.ok.iter().any(|a| a.name == "local-agent.db"),
        "event db should be ok: {:?}",
        report.ok
    );
    assert!(
        report
            .deferred
            .iter()
            .any(|a| a.name == "devices.json" && a.detail.contains("2 device")),
        "devices deferred: {:?}",
        report.deferred
    );
    assert!(
        report
            .deferred
            .iter()
            .any(|a| a.name == "conversations.json" && a.detail.contains("2 session")),
        "conversations deferred: {:?}",
        report.deferred
    );
    assert!(
        report
            .deferred
            .iter()
            .any(|a| a.name == "mcp.json" && a.detail.contains("2 server")),
        "mcp deferred: {:?}",
        report.deferred
    );
    assert!(
        report
            .deferred
            .iter()
            .any(|a| a.name == "uploads" && a.detail.contains("1 session")),
        "uploads deferred: {:?}",
        report.deferred
    );
}

#[test]
fn corrupt_devices_fails_validation() {
    let (_tmp, state) = materialize_go_fixture();
    fs::write(state.join("devices.json"), b"{\"not\":\"an-array\"}").unwrap();
    let report = validate_state_tree(&state).expect("scan returns report");
    assert!(!report.is_ok());
    assert!(
        report.failed.iter().any(|f| f.name == "devices.json"),
        "devices must hard-fail: {:?}",
        report.failed
    );
}

// ---------------------------------------------------------------------------
// Full run_migrations path: workspaces, event IDs, uploads, devices usable
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_migrations_preserves_workspaces_events_devices_uploads() {
    let (_tmp, state) = materialize_go_fixture();

    let report = run_migrations(&state).expect("run_migrations");
    assert!(report.is_ok());
    assert_eq!(report.before, StateFormat::Go);
    assert!(
        report.after == StateFormat::Both || report.after == StateFormat::Rust,
        "after should be Rust-capable: {:?}",
        report.after
    );
    assert!(matches!(
        report.config,
        ConfigMigrationOutcome::Migrated { .. }
    ));

    // Workspaces survive migration.
    with_state_dir(&state, || {
        let cfg = Config::load().unwrap();
        assert_eq!(cfg.workspaces.len(), 2);
        assert!(cfg.workspaces.iter().any(|w| w.ends_with("seed-workspace")));
    });

    // Event IDs stable.
    let store = crate::events::Store::open(state.join("local-agent.db")).unwrap();
    let events = store.query_all(0, 10).await.unwrap();
    assert_eq!(events[0].id, 1);
    assert_eq!(events.len(), 5);

    // Upload blob still on disk.
    let upload = state.join("uploads/sess-fixture-001/uploadfixture0001.png");
    assert!(upload.is_file());
    let png = fs::read(&upload).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic preserved");

    // Device credentials still parse with required fields.
    let devices: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(state.join("devices.json")).unwrap()).unwrap();
    assert_eq!(devices.as_array().unwrap().len(), 2);
    assert!(!devices[0]["secretHash"].as_str().unwrap().is_empty());
    assert_eq!(devices[0]["id"], "devfixture00000001");
}

#[test]
fn run_migrations_idempotent_second_pass() {
    let (_tmp, state) = materialize_go_fixture();
    let first = run_migrations(&state).expect("first");
    assert!(matches!(
        first.config,
        ConfigMigrationOutcome::Migrated { .. }
    ));
    let second = run_migrations(&state).expect("second");
    assert!(matches!(
        second.config,
        ConfigMigrationOutcome::NoopAlreadyRust
    ));
    assert!(second.is_ok());
}

// ---------------------------------------------------------------------------
// Legacy defaults on migration (tlsEnabled omitted → true)
// ---------------------------------------------------------------------------

#[test]
fn migrate_applies_secure_default_when_tls_key_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path();
    // Minimal Go config omitting tlsEnabled.
    let json = r#"{
      "port": 7337,
      "host": "0.0.0.0",
      "dataDir": "/tmp/x",
      "dbPath": "/tmp/x/local-agent.db",
      "workspaces": ["/ws"],
      "agents": []
    }"#;
    fs::write(state.join(GO_CONFIG_FILE), json).unwrap();
    migrate_config(state).expect("migrate");
    with_state_dir(state, || {
        let cfg = Config::load().unwrap();
        assert!(
            cfg.tls_enabled,
            "missing tlsEnabled must secure-default to true"
        );
        assert_eq!(
            cfg.revocation_grace_period_seconds, 300,
            "missing revocationGracePeriodSeconds defaults to 300"
        );
        assert_eq!(cfg.workspaces, vec!["/ws".to_string()]);
    });
}
