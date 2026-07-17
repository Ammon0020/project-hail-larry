//! Tests for the config module — ports `internal/config/config_test.go` and
//! adds the S-CONFIG acceptance criteria (atomic write, state-dir override,
//! golden DTO match, unknown-field preservation, `0600` permissions).
//!
//! Tests that touch the `LOCAL_AGENT_STATE_DIR` env var are serialized via
//! `ENV_LOCK` because `std::env::set_var` mutates process-global state and
//! would race with parallel test threads.

use std::fs;
use std::path::Path;
use std::sync::Mutex;

use super::{AgentInfo, AgentModel, Config, ConfigError, ConfigStore, STATE_DIR_ENV_VAR};

/// Serializes tests that mutate `LOCAL_AGENT_STATE_DIR`.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Golden DTO fixture captured from the Go daemon (S-CONTRACT). The default
/// config projected to JSON must reproduce this shape (paths redacted).
const GOLDEN_CONFIG_DTO: &str = include_str!("../../tests/contract/golden/dto/config_default.json");

/// Helper: run `body` with `LOCAL_AGENT_STATE_DIR` set to `dir`, restoring the
/// prior value (or unsetting it) afterwards. Holds `ENV_LOCK` for the duration
/// so concurrent env-touching tests cannot race.
fn with_state_dir<R>(dir: &Path, body: impl FnOnce() -> R) -> R {
    // Recover from a poisoned lock (a prior test panicked while holding it)
    // so one failure does not cascade to every env-touching test.
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
/// `TestDefaultConfig` (Go): default config has sensible values.
#[test]
fn default_config_has_sensible_values() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let cfg = Config::default_or_error().expect("default");
        assert_eq!(cfg.port, 7337, "expected port 7337");
        assert_eq!(cfg.host, "0.0.0.0", "expected host 0.0.0.0");
        assert!(!cfg.data_dir.is_empty(), "expected non-empty data dir");
        assert!(!cfg.db_path.is_empty(), "expected non-empty db path");
        assert!(cfg.tls_enabled, "expected tls enabled by default");
        assert_eq!(cfg.pairing_ttl_seconds, 300, "expected pairing ttl 300");
        assert_eq!(
            cfg.credential_inactivity_ttl_seconds, 2_592_000,
            "expected 30-day credential inactivity ttl"
        );
    });
}
/// Saving always targets the active state directory, not the persisted
/// `data_dir`, which may refer to a previous installation location.
#[test]
fn save_uses_active_state_dir_when_data_dir_differs() {
    let state_dir = tempfile::tempdir().expect("state tempdir");
    let stored_data_dir = tempfile::tempdir().expect("stored data tempdir");
    with_state_dir(state_dir.path(), || {
        let mut cfg = Config::default_or_error().expect("default");
        cfg.data_dir = stored_data_dir.path().to_string_lossy().to_string();

        cfg.save().expect("save");

        assert!(state_dir.path().join("config.toml").is_file());
        assert!(!stored_data_dir.path().join("config.toml").exists());
    });
}

/// `TestDefaultRevocationGracePeriod` (Go): default grace period is 300s and
/// remote workspace registration is off.
#[test]
fn default_revocation_grace_period_and_remote_registration() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let cfg = Config::default_or_error().expect("default");
        assert_eq!(
            cfg.revocation_grace_period_seconds, 300,
            "expected default revocation grace period 300"
        );
        assert!(
            !cfg.allow_remote_workspace_registration,
            "expected AllowRemoteWorkspaceRegistration to default to false"
        );
    });
}

/// `TestSaveAndLoad` (Go): config round-trips through TOML without data loss.
/// Also covers the S-CONFIG "Config round-trips through TOML without data loss"
/// acceptance criterion.
#[test]
fn save_and_load_roundtrips_without_data_loss() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let mut cfg = Config::default_or_error().expect("default");
        cfg.port = 8443;
        cfg.host = "127.0.0.1".to_string();
        cfg.data_dir = tmp.path().to_string_lossy().to_string();
        cfg.db_path = tmp.path().join("test.db").to_string_lossy().to_string();
        cfg.workspaces = vec!["/tmp/test-workspace".to_string()];
        cfg.tls_enabled = false;
        cfg.https_port = 9443;
        cfg.credential_inactivity_ttl_seconds = 0; // explicit disable (survives: load does not default this field)
        cfg.allow_remote_workspace_registration = true;
        // Use a non-zero grace period so it survives `omitempty` on save.
        // An explicit 0 is intentionally NOT round-trip-stable: `omitempty`
        // drops it on save and `load` defaults the missing key to 300 — this
        // mirrors Go exactly and is covered by
        // `legacy_revocation_grace_period_defaulting`.
        cfg.revocation_grace_period_seconds = 600;
        cfg.upsert_agent(AgentInfo {
            id: "claude".into(),
            name: "Claude Code".into(),
            command: "claude".into(),
            args: vec!["--acp".into()],
            models: vec![AgentModel {
                id: "sonnet".into(),
                name: "Sonnet".into(),
            }],
            warning: "Executable not found in PATH".into(),
        })
        .expect("save");

        // Verify the file exists at the expected TOML path.
        let config_path = tmp.path().join("config.toml");
        assert!(config_path.exists(), "config file not created");

        // Load it back via the public Load path (reads from state dir).
        let loaded = Config::load().expect("load");
        assert_eq!(loaded, cfg, "round-trip should preserve all fields");
    });
}

/// S-CONFIG acceptance: atomic write (temp + rename) leaves no temp files and
/// a stale temp file in the directory does not corrupt load.
#[test]
fn atomic_write_leaves_no_temp_and_ignores_stale_temps() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let mut cfg = Config::default_or_error().expect("default");
        cfg.data_dir = tmp.path().to_string_lossy().to_string();
        cfg.db_path = tmp
            .path()
            .join("local-agent.db")
            .to_string_lossy()
            .to_string();
        cfg.tls_cert_dir = tmp.path().join("tls").to_string_lossy().to_string();
        cfg.save().expect("save");

        // No leftover temp files after a successful save.
        let temps: Vec<_> = fs::read_dir(tmp.path())
            .expect("read dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(temps.is_empty(), "leftover temp files: {temps:?}");

        // Drop a garbage temp file in the dir; load must still read the good
        // config (load only reads config.toml, never temps).
        let garbage = tmp.path().join(".config.toml.stale.tmp");
        fs::write(&garbage, "this is not valid toml at all {{{").expect("write garbage");
        let loaded = Config::load().expect("load ignores stale temps");
        assert_eq!(loaded.port, cfg.port);
    });
}

/// S-CONFIG acceptance: `LOCAL_AGENT_STATE_DIR` overrides the default
/// `~/.local-agent` location for both load and save.
#[test]
fn state_dir_env_override() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        // No file yet → load returns defaults pointing at the override dir.
        let cfg = Config::load().expect("load default");
        assert_eq!(
            Path::new(&cfg.data_dir),
            tmp.path(),
            "default data_dir should follow LOCAL_AGENT_STATE_DIR"
        );

        // Save writes into the override dir.
        let mut cfg = cfg;
        cfg.port = 9999;
        cfg.save().expect("save");
        assert!(
            tmp.path().join("config.toml").exists(),
            "file in override dir"
        );
    });

    // Outside the override, the file is not visible at the default path.
    // (We cannot assert the default ~/.local-agent is empty without touching
    // the user's real state, so we only assert the override-scoped behavior
    // above.)
}

/// S-CONFIG acceptance: default config projected to JSON matches the S-CONTRACT
/// golden DTO fixture (`config_default.json`), proving the Rust port reproduces
/// the Go daemon's external config shape byte-for-byte (modulo redacted paths).
#[test]
fn default_config_matches_golden_dto() {
    // Build a config with the redacted placeholder values the golden fixture
    // uses, plus the fixture workspace/agent, so the comparison is exact.
    let cfg = Config {
        port: 7337,
        host: "0.0.0.0".to_string(),
        data_dir: "<REDACTED_PATH>".to_string(),
        db_path: "<REDACTED_PATH>/local-agent.db".to_string(),
        workspaces: vec!["<REDACTED_PATH>/seed-workspace".to_string()],
        agents: vec![AgentInfo {
            id: "fixture-agent".into(),
            name: "Fixture Agent".into(),
            command: "fixture-agent-binary".into(),
            // The config_default.json fixture agent omits args/warning.
            args: Vec::new(),
            models: vec![AgentModel {
                id: "fixture-model".into(),
                name: "Fixture Model".into(),
            }],
            warning: String::new(),
        }],
        tls_enabled: true,
        tls_cert_dir: "<REDACTED_PATH>/tls".to_string(),
        https_port: 0, // omitted (omitempty)
        pairing_ttl_seconds: 300,
        credential_inactivity_ttl_seconds: 2_592_000,
        allow_remote_workspace_registration: false, // omitted (omitempty)
        revocation_grace_period_seconds: 300,
        extra: toml::Table::new(),
    };

    let json = serde_json::to_string_pretty(&cfg).expect("serialize");
    let got: serde_json::Value = serde_json::from_str(&json).expect("parse got");
    let want: serde_json::Value = serde_json::from_str(GOLDEN_CONFIG_DTO).expect("parse golden");
    assert_eq!(got, want, "default config DTO must match golden fixture");
}

/// S-CONFIG acceptance: unknown (forward-compatible) TOML fields are preserved
/// across a load → save round-trip, not silently dropped.
#[test]
fn unknown_fields_preserved_across_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let toml_src = "\
port = 8443
host = \"127.0.0.1\"
dataDir = \"/tmp/x\"
dbPath = \"/tmp/x/local-agent.db\"
tlsEnabled = true
futureField = \"hello-from-future-daemon\"
futureInt = 42
";
        fs::write(tmp.path().join("config.toml"), toml_src).expect("write");

        let cfg = Config::load().expect("load");
        assert_eq!(
            cfg.extra.get("futureField").and_then(|v| v.as_str()),
            Some("hello-from-future-daemon"),
            "unknown string field must be captured"
        );
        assert_eq!(
            cfg.extra.get("futureInt").and_then(|v| v.as_integer()),
            Some(42),
            "unknown int field must be captured"
        );

        // Round-trip: save then reload, unknowns must survive.
        cfg.save().expect("save");
        let reloaded = Config::load().expect("reload");
        assert_eq!(
            reloaded.extra.get("futureField").and_then(|v| v.as_str()),
            Some("hello-from-future-daemon"),
            "unknown string field must survive round-trip"
        );
        assert_eq!(
            reloaded.extra.get("futureInt").and_then(|v| v.as_integer()),
            Some(42),
            "unknown int field must survive round-trip"
        );
    });
}

/// S-CONFIG acceptance: config file is written with mode `0600` (Unix only —
/// the file may contain secrets).
#[cfg(unix)]
#[test]
fn config_file_permissions_are_0600() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let mut cfg = Config::default_or_error().expect("default");
        cfg.data_dir = tmp.path().to_string_lossy().to_string();
        cfg.db_path = tmp
            .path()
            .join("local-agent.db")
            .to_string_lossy()
            .to_string();
        cfg.tls_cert_dir = tmp.path().join("tls").to_string_lossy().to_string();
        cfg.save().expect("save");

        let meta = fs::metadata(tmp.path().join("config.toml")).expect("metadata");
        assert_eq!(
            meta.permissions().mode() & 0o777,
            0o600,
            "config file must be 0600"
        );
    });
}

/// `TestLoadLegacyRevocationGracePeriodDefault` (Go): a config omitting
/// `revocationGracePeriodSeconds` loads with the 300s default; an explicit `0`
/// is respected as an opt-out.
#[test]
fn legacy_revocation_grace_period_defaulting() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        // Case 1: legacy config without the key → default to 300.
        fs::write(
            tmp.path().join("config.toml"),
            "port = 7333\nhost = \"0.0.0.0\"\ntlsEnabled = true\n",
        )
        .expect("write legacy");
        let cfg = Config::load().expect("load legacy");
        assert_eq!(
            cfg.revocation_grace_period_seconds, 300,
            "legacy: expected grace period 300"
        );

        // Case 2: explicit 0 → respected as opt-out (no defaulting).
        fs::write(
            tmp.path().join("config.toml"),
            "port = 7333\nrevocationGracePeriodSeconds = 0\ntlsEnabled = true\n",
        )
        .expect("write explicit-zero");
        let cfg = Config::load().expect("load explicit-zero");
        assert_eq!(
            cfg.revocation_grace_period_seconds, 0,
            "explicit 0: expected grace period 0"
        );
    });
}

/// TLS secure-by-default upgrade: a config omitting `tlsEnabled` loads with
/// TLS on; an explicit `false` is respected as an opt-out.
#[test]
fn tls_enabled_secure_by_default_upgrade() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        // Legacy config without tlsEnabled → forced on.
        fs::write(
            tmp.path().join("config.toml"),
            "port = 7333\nhost = \"0.0.0.0\"\n",
        )
        .expect("write legacy");
        let cfg = Config::load().expect("load legacy");
        assert!(cfg.tls_enabled, "legacy: expected tls enabled forced on");

        // Explicit false → respected.
        fs::write(
            tmp.path().join("config.toml"),
            "port = 7333\ntlsEnabled = false\n",
        )
        .expect("write explicit-false");
        let cfg = Config::load().expect("load explicit-false");
        assert!(!cfg.tls_enabled, "explicit false: expected tls disabled");
    });
}

/// Workspace add/remove/list methods mutate and persist correctly.
#[test]
fn workspace_add_remove_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let mut cfg = Config::default_or_error().expect("default");
        cfg.data_dir = tmp.path().to_string_lossy().to_string();
        cfg.db_path = tmp
            .path()
            .join("local-agent.db")
            .to_string_lossy()
            .to_string();
        cfg.tls_cert_dir = tmp.path().join("tls").to_string_lossy().to_string();

        cfg.add_workspace("/tmp/ws-a").expect("add a");
        cfg.add_workspace("/tmp/ws-b").expect("add b");
        // Duplicate add is a no-op (kept unique).
        cfg.add_workspace("/tmp/ws-a").expect("add a dup");
        assert_eq!(
            cfg.list_workspaces(),
            vec!["/tmp/ws-a".to_string(), "/tmp/ws-b".to_string()]
        );

        cfg.remove_workspace("/tmp/ws-a").expect("remove a");
        assert_eq!(cfg.list_workspaces(), vec!["/tmp/ws-b".to_string()]);

        // Removing an unregistered path errors.
        let err = cfg
            .remove_workspace("/tmp/not-registered")
            .expect_err("remove unregistered should error");
        assert!(
            matches!(err, ConfigError::WorkspaceNotRegistered(ref p) if p == "/tmp/not-registered"),
            "expected WorkspaceNotRegistered, got {err:?}"
        );

        // Empty path is rejected.
        let err = cfg.add_workspace("").expect_err("empty add should error");
        assert!(
            matches!(err, ConfigError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );

        // Persistence: reload and verify the surviving workspace.
        let reloaded = Config::load().expect("reload");
        assert_eq!(reloaded.list_workspaces(), vec!["/tmp/ws-b".to_string()]);
    });
}

/// Agent upsert/delete methods mutate and persist correctly.
#[test]
fn agent_upsert_delete() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let mut cfg = Config::default_or_error().expect("default");
        cfg.data_dir = tmp.path().to_string_lossy().to_string();
        cfg.db_path = tmp
            .path()
            .join("local-agent.db")
            .to_string_lossy()
            .to_string();
        cfg.tls_cert_dir = tmp.path().join("tls").to_string_lossy().to_string();

        let a1 = AgentInfo {
            id: "claude".into(),
            name: "Claude".into(),
            command: "claude".into(),
            args: Vec::new(),
            models: Vec::new(),
            warning: String::new(),
        };
        cfg.upsert_agent(a1.clone()).expect("upsert a1");
        assert_eq!(cfg.agents.len(), 1);

        // Upsert with same ID replaces, not appends.
        let a1_updated = AgentInfo {
            name: "Claude Code".into(),
            ..a1
        };
        cfg.upsert_agent(a1_updated).expect("upsert replace");
        assert_eq!(cfg.agents.len(), 1, "upsert should replace not append");
        assert_eq!(cfg.agents[0].name, "Claude Code");

        // Delete by ID; deleting a missing ID is a no-op (no error).
        cfg.delete_agent("claude").expect("delete");
        assert!(cfg.agents.is_empty());
        cfg.delete_agent("nonexistent")
            .expect("delete missing is no-op");

        // Persistence.
        let reloaded = Config::load().expect("reload");
        assert!(reloaded.agents.is_empty());
    });
}

/// `ConfigStore` provides thread-safe read/write access via `RwLock`.
#[test]
fn config_store_thread_safe_access() {
    let tmp = tempfile::tempdir().expect("tempdir");
    with_state_dir(tmp.path(), || {
        let mut cfg = Config::default_or_error().expect("default");
        cfg.data_dir = tmp.path().to_string_lossy().to_string();
        cfg.db_path = tmp
            .path()
            .join("local-agent.db")
            .to_string_lossy()
            .to_string();
        cfg.tls_cert_dir = tmp.path().join("tls").to_string_lossy().to_string();
        let store = ConfigStore::new(cfg);

        // Concurrent readers + a writer via scoped threads.
        let store_clone = store.clone();
        let handle = std::thread::spawn(move || {
            let g = store_clone.read();
            g.port
        });
        {
            let mut w = store.write();
            w.port = 5555;
        }
        let observed = handle.join().expect("reader thread panicked");
        // The reader may have observed either the old or new port depending on
        // scheduling; assert it is one of the valid values.
        assert!(
            observed == 7337 || observed == 5555,
            "unexpected port {observed}"
        );

        // Persist via the store.
        store.save().expect("store save");
        let reloaded = Config::load().expect("reload");
        assert_eq!(reloaded.port, 5555);
    });
}
