//! QR + mnemonic pairing and device credentials (Go `internal/pairing/`).
//!
//! This module deliberately keeps raw pairing material in memory only. Device
//! state contains SHA-256 hashes, is written atomically with mode `0600`, and
//! never implements `Debug` with secret fields.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, Luma};
use qrcode::QrCode;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::fsutil;
pub use crate::interfaces::{DeviceCredential, DeviceInfo, PairingSession, PendingActionInfo};
pub use crate::interfaces::{
    PENDING_ACTION_TYPE_REVOCATION, PENDING_ACTION_TYPE_WORKSPACE_REGISTRATION,
};

const DEVICES_FILE: &str = "devices.json";
const DEFAULT_SESSION_TTL: Duration = Duration::from_mins(5);
const MAX_VERIFY_ATTEMPTS: usize = 5;
const RATE_LIMIT_WINDOW: Duration = Duration::from_mins(5);
const BASE_LOCKOUT: Duration = Duration::from_secs(1);
const MAX_LOCKOUT: Duration = Duration::from_mins(5);
const PERSIST_THROTTLE: Duration = Duration::from_mins(1);

/// Dependency used for grace-delayed workspace registration. It is injected at
/// construction so pairing has no mutable post-construction callback slot.
pub trait WorkspaceRegistrar: Send + Sync {
    /// Register a workspace after its grace window has elapsed.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace cannot be registered.
    fn register_workspace(&self, path: &str) -> Result<(), PairingError>;
}

/// Dependency used to notify the sync hub when a device revocation executes.
/// Injected via [`Manager::set_revocation_listener`] so the hub can drop the
/// revoked device's active WebSocket connections immediately.
pub trait RevocationListener: Send + Sync {
    /// Called after a device's credentials have been removed.
    fn device_revoked(&self, device_id: &str);
}

/// Pairing manager failures intentionally contain no supplied credentials.
#[derive(Debug, Error)]
pub enum PairingError {
    /// Passcode verify failed (wrong/expired/used). Distinct from token so the
    /// REST layer can return Go's `"invalid or expired passcode"`.
    #[error("invalid or expired passcode")]
    InvalidPasscode,
    /// Token verify failed (wrong/expired/used).
    #[error("invalid or expired token")]
    InvalidToken,
    #[error("too many pairing attempts; try again later")]
    RateLimited,
    // The id is kept for server-side Debug introspection but intentionally
    // omitted from Display so the HTTP body cannot act as a device-id oracle.
    #[error("device not found")]
    DeviceNotFound(String),
    #[error("pending action not found")]
    PendingActionNotFound,
    #[error("pending action has a different type")]
    PendingActionTypeMismatch,
    #[error("an equivalent pending action already exists")]
    DuplicatePendingAction,
    #[error("pairing persistence failed")]
    Persistence(#[source] std::io::Error),
    #[error("device state is invalid")]
    State(#[source] serde_json::Error),
    #[error("QR generation failed")]
    Qr(#[source] qrcode::types::QrError),
    #[error("QR encoding failed")]
    QrEncoding(#[source] image::ImageError),
}

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Mutex<Inner>>,
}

impl fmt::Debug for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = lock(&self.inner);
        f.debug_struct("Manager")
            .field("session_count", &inner.sessions.len())
            .field("device_count", &inner.devices.len())
            .field("data_dir", &inner.data_dir)
            .finish()
    }
}

struct Inner {
    sessions: HashMap<String, PairingSession>,
    devices: HashMap<String, StoredDevice>,
    pending: HashMap<String, PendingAction>,
    data_dir: PathBuf,
    session_ttl: Duration,
    inactivity_ttl: Duration,
    last_persist: Option<DateTime<Utc>>,
    /// Per-IP lockout state so one attacker cannot lock out all pairing
    /// attempts. Key is the normalized peer IP (or `GLOBAL_LOCKOUT_KEY` for
    /// calls without a peer address, e.g. CLI).
    lockouts: HashMap<String, LockoutState>,
    workspace_registrar: Option<Arc<dyn WorkspaceRegistrar>>,
    revocation_listener: Option<Arc<dyn RevocationListener>>,
}

/// Per-IP brute-force lockout state, mirroring the former global fields.
struct LockoutState {
    failures: Vec<DateTime<Utc>>,
    lockout_until: Option<DateTime<Utc>>,
    lockout_count: u32,
}

impl LockoutState {
    fn new() -> Self {
        Self {
            failures: Vec::new(),
            lockout_until: None,
            lockout_count: 0,
        }
    }
}

impl Default for LockoutState {
    fn default() -> Self {
        Self::new()
    }
}

/// Fallback key for verify calls without a peer address (e.g. CLI).
const GLOBAL_LOCKOUT_KEY: &str = "_global";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDevice {
    id: String,
    name: String,
    secret_hash: String,
    paired_at: DateTime<Utc>,
    #[serde(default)]
    last_seen: Option<DateTime<Utc>>,
}

struct PendingAction {
    info: PendingActionInfo,
    cancellation: CancellationToken,
}

impl Manager {
    /// Load Go-compatible `devices.json`; corrupt existing state fails closed.
    ///
    /// # Errors
    ///
    /// Returns an error if `devices.json` cannot be loaded or parsed.
    pub fn new(
        data_dir: impl Into<PathBuf>,
        workspace_registrar: Option<Arc<dyn WorkspaceRegistrar>>,
    ) -> Result<Self, PairingError> {
        let data_dir = data_dir.into();
        let devices = load_devices(&data_dir)?;
        // Remove orphaned pairing QR PNGs from a prior crash/kill. In-memory
        // sessions no longer exist, so any leftover file contains a stale
        // (possibly still-valid within TTL) pairing token.
        cleanup_stale_qr_files(&data_dir);
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                sessions: HashMap::new(),
                devices,
                pending: HashMap::new(),
                data_dir,
                session_ttl: DEFAULT_SESSION_TTL,
                inactivity_ttl: Duration::ZERO,
                last_persist: None,
                lockouts: HashMap::new(),
                workspace_registrar,
                revocation_listener: None,
            })),
        })
    }

    pub fn set_session_ttl(&self, ttl: Duration) {
        lock(&self.inner).session_ttl = if ttl.is_zero() {
            DEFAULT_SESSION_TTL
        } else {
            ttl
        };
    }

    pub fn set_inactivity_ttl(&self, ttl: Duration) {
        lock(&self.inner).inactivity_ttl = ttl;
    }

    /// Install a listener invoked when a device revocation executes (immediate
    /// or grace-delayed) so the sync hub can drop active WebSocket connections.
    pub fn set_revocation_listener(&self, listener: Arc<dyn RevocationListener>) {
        lock(&self.inner).revocation_listener = Some(listener);
    }

    /// Create a single-use session. The URL intentionally remains `http` to
    /// match the checked-in Go contract fixture (`pairing_session.json`).
    ///
    /// # Errors
    ///
    /// Returns an error if random token generation or QR PNG writing fails.
    pub fn create_session(&self, host: &str, port: u16) -> Result<PairingSession, PairingError> {
        let mut inner = lock(&self.inner);
        cleanup_sessions(&mut inner);
        let token = random_hex(32);
        let passcode = generate_passcode();
        let id = random_hex(16);
        let url = format!("http://{host}:{port}?token={token}");
        let qr_path = inner.data_dir.join(format!("pairing-{id}.png"));
        write_qr(&url, &qr_path)?;
        let created_at = Utc::now();
        let session = PairingSession {
            id: id.clone(),
            token,
            passcode,
            url,
            qr_path: qr_path.display().to_string(),
            created_at,
            expires_at: created_at
                + chrono::Duration::from_std(inner.session_ttl)
                    .unwrap_or_else(|_| chrono::Duration::minutes(5)),
            used: false,
        };
        inner.sessions.insert(id, session.clone());
        Ok(session)
    }

    /// Verifies a passcode against active pairing sessions and issues device
    /// credentials on success.
    ///
    /// # Errors
    /// Returns an error if no session matches the passcode or credential issuance/persistence fails.
    pub fn verify_passcode(
        &self,
        passcode: &str,
        device_name: impl Into<String>,
        peer_key: Option<&str>,
    ) -> Result<DeviceCredential, PairingError> {
        self.verify(
            device_name.into(),
            |session| bool::from(session.passcode.as_bytes().ct_eq(passcode.as_bytes())),
            PairingError::InvalidPasscode,
            peer_key,
        )
    }

    /// Verifies a single-use pairing token and issues device credentials on
    /// success.
    ///
    /// # Errors
    /// Returns an error if no session matches the token or credential issuance/persistence fails.
    pub fn verify_token(
        &self,
        token: &str,
        device_name: impl Into<String>,
        peer_key: Option<&str>,
    ) -> Result<DeviceCredential, PairingError> {
        self.verify(
            device_name.into(),
            |session| bool::from(session.token.as_bytes().ct_eq(token.as_bytes())),
            PairingError::InvalidToken,
            peer_key,
        )
    }

    fn verify(
        &self,
        device_name: String,
        matches: impl Fn(&PairingSession) -> bool,
        miss: PairingError,
        peer_key: Option<&str>,
    ) -> Result<DeviceCredential, PairingError> {
        let key = peer_key.unwrap_or(GLOBAL_LOCKOUT_KEY);
        let mut inner = lock(&self.inner);
        cleanup_sessions(&mut inner);
        check_rate_limit(&mut inner, key)?;
        let now = Utc::now();
        let session_id = inner
            .sessions
            .values()
            .find(|s| !s.used && s.expires_at > now && matches(s))
            .map(|s| s.id.clone());
        let Some(session_id) = session_id else {
            record_failure(&mut inner, key);
            return Err(miss);
        };
        let credential = issue_credential(&mut inner, &session_id, device_name)?;
        // A verified pairing proves the user controls the valid credential, so
        // stale failed attempts must not escalate future lockouts indefinitely.
        if let Some(state) = inner.lockouts.get_mut(key) {
            state.failures.clear();
            state.lockout_count = 0;
            state.lockout_until = None;
        }
        Ok(credential)
    }

    /// Validate and renew a device's sliding activity window.
    #[must_use]
    pub fn validate_credential(&self, device_id: &str, secret: &str) -> bool {
        let mut inner = lock(&self.inner);
        let digest = hash_secret(secret);
        let inactivity_ttl = inner.inactivity_ttl;
        let Some(device) = inner.devices.get_mut(device_id) else {
            // Equal-cost hash + comparison keeps unknown-id and wrong-secret
            // paths from becoming a useful device-ID oracle.
            let _ = digest.as_bytes().ct_eq(digest.as_bytes());
            return false;
        };
        if !bool::from(digest.as_bytes().ct_eq(device.secret_hash.as_bytes())) {
            return false;
        }
        let now = Utc::now();
        if !inactivity_ttl.is_zero()
            && device
                .last_seen
                .is_some_and(|seen| now - seen > chrono_duration(inactivity_ttl))
        {
            return false;
        }
        device.last_seen = Some(now);
        let should_persist = inner
            .last_persist
            .is_none_or(|last| now - last > chrono_duration(PERSIST_THROTTLE));
        if should_persist && save_devices(&mut inner).is_ok() {
            inner.last_persist = Some(now);
        }
        true
    }

    #[must_use]
    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        let inner = lock(&self.inner);
        let mut devices: Vec<_> = inner
            .devices
            .values()
            .map(|device| DeviceInfo {
                id: device.id.clone(),
                name: device.name.clone(),
                paired_at: device.paired_at,
                last_seen: device.last_seen.unwrap_or(device.paired_at),
            })
            .collect();
        devices.sort_by(|a, b| a.id.cmp(&b.id));
        devices
    }

    /// Revokes a device immediately, removing its credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the device is unknown or persistence fails.
    pub fn revoke_device(&self, device_id: &str) -> Result<(), PairingError> {
        let mut inner = lock(&self.inner);
        let removed = inner
            .devices
            .remove(device_id)
            .ok_or_else(|| PairingError::DeviceNotFound(device_id.to_string()))?;
        if let Err(error) = save_devices(&mut inner) {
            inner.devices.insert(removed.id.clone(), removed);
            return Err(error);
        }
        Ok(())
    }

    /// Schedules a grace-delayed device revocation, returning the pending
    /// action info.
    ///
    /// # Errors
    /// Returns an error if the device is unknown or a duplicate revocation is already pending.
    pub fn request_revocation(
        &self,
        device_id: &str,
        requested_by: impl Into<String>,
        grace: Duration,
    ) -> Result<PendingActionInfo, PairingError> {
        // Device must exist before scheduling; duplicate check is type+device.
        self.request_pending(grace, |inner| {
            let device = inner
                .devices
                .get(device_id)
                .ok_or_else(|| PairingError::DeviceNotFound(device_id.to_string()))?;
            if inner.pending.values().any(|p| {
                p.info.action_type == PENDING_ACTION_TYPE_REVOCATION
                    && p.info.device_id == device_id
            }) {
                return Err(PairingError::DuplicatePendingAction);
            }
            Ok(pending_info(
                PENDING_ACTION_TYPE_REVOCATION,
                device_id,
                &device.name,
                "",
                requested_by.into(),
                grace,
            ))
        })
    }

    /// Cancels a pending device revocation by action id.
    ///
    /// # Errors
    ///
    /// Returns [`PairingError::PendingActionNotFound`] if no pending action has
    /// the given id, or [`PairingError::PendingActionTypeMismatch`] if the action
    /// is not a revocation.
    pub fn cancel_revocation(&self, action_id: &str) -> Result<(), PairingError> {
        cancel_pending(&self.inner, action_id, PENDING_ACTION_TYPE_REVOCATION)
    }

    /// Schedules a grace-delayed workspace registration, returning the pending
    /// action info.
    ///
    /// # Errors
    ///
    /// Returns an error if a duplicate registration is already pending.
    pub fn request_workspace_registration(
        &self,
        path: &str,
        requested_by: impl Into<String>,
        grace: Duration,
    ) -> Result<PendingActionInfo, PairingError> {
        // Duplicate check is type+path; no device lookup required.
        self.request_pending(grace, |inner| {
            if inner.pending.values().any(|p| {
                p.info.action_type == PENDING_ACTION_TYPE_WORKSPACE_REGISTRATION
                    && p.info.path == path
            }) {
                return Err(PairingError::DuplicatePendingAction);
            }
            Ok(pending_info(
                PENDING_ACTION_TYPE_WORKSPACE_REGISTRATION,
                "",
                "",
                path,
                requested_by.into(),
                grace,
            ))
        })
    }

    /// Build + schedule a pending action; execute immediately when grace is zero.
    fn request_pending(
        &self,
        grace: Duration,
        build: impl FnOnce(&mut Inner) -> Result<PendingActionInfo, PairingError>,
    ) -> Result<PendingActionInfo, PairingError> {
        let info = {
            let mut inner = lock(&self.inner);
            let info = build(&mut inner)?;
            schedule_pending(&self.inner, &mut inner, info.clone(), grace);
            info
        };
        if grace.is_zero() {
            execute_pending(&self.inner, &info.id);
        }
        Ok(info)
    }

    /// Cancels a pending workspace registration by action id.
    ///
    /// # Errors
    ///
    /// Returns [`PairingError::PendingActionNotFound`] if no pending action has
    /// the given id, or [`PairingError::PendingActionTypeMismatch`] if the action
    /// is not a workspace registration.
    pub fn cancel_workspace_registration(&self, action_id: &str) -> Result<(), PairingError> {
        cancel_pending(
            &self.inner,
            action_id,
            PENDING_ACTION_TYPE_WORKSPACE_REGISTRATION,
        )
    }

    #[must_use]
    pub fn list_pending_actions(&self) -> Vec<PendingActionInfo> {
        let inner = lock(&self.inner);
        let mut actions: Vec<_> = inner.pending.values().map(|p| p.info.clone()).collect();
        actions.sort_by(|a, b| a.requested_at.cmp(&b.requested_at).then(a.id.cmp(&b.id)));
        actions
    }

    /// Cancel pending timers; pairing sessions and devices remain valid.
    pub fn close(&self) {
        let mut inner = lock(&self.inner);
        for action in inner.pending.values() {
            action.cancellation.cancel();
        }
        inner.pending.clear();
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.close();
        }
    }
}

fn lock(inner: &Arc<Mutex<Inner>>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

fn issue_credential(
    inner: &mut Inner,
    session_id: &str,
    name: String,
) -> Result<DeviceCredential, PairingError> {
    let session = inner
        .sessions
        .remove(session_id)
        .ok_or(PairingError::InvalidPasscode)?;
    let id = random_hex(16);
    let secret = random_hex(32);
    let now = Utc::now();
    let stored = StoredDevice {
        id: id.clone(),
        name: name.clone(),
        secret_hash: hash_secret(&secret),
        paired_at: now,
        last_seen: Some(now),
    };
    inner.devices.insert(id.clone(), stored);
    if let Err(error) = save_devices(inner) {
        inner.devices.remove(&id);
        inner.sessions.insert(session.id.clone(), session);
        return Err(error);
    }
    let _ = fs::remove_file(&session.qr_path);
    Ok(DeviceCredential {
        id,
        name,
        secret,
        paired_at: now,
    })
}

fn schedule_pending(
    shared: &Arc<Mutex<Inner>>,
    inner: &mut Inner,
    info: PendingActionInfo,
    grace: Duration,
) {
    let cancellation = CancellationToken::new();
    let id = info.id.clone();
    inner.pending.insert(
        id.clone(),
        PendingAction {
            info,
            cancellation: cancellation.clone(),
        },
    );
    if grace.is_zero() {
        return;
    }
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        thread::sleep(grace);
        if !cancellation.is_cancelled() {
            execute_pending(&shared, &id);
        }
    });
}

fn execute_pending(shared: &Arc<Mutex<Inner>>, action_id: &str) {
    let mut inner = lock(shared);
    let Some(action) = inner.pending.remove(action_id) else {
        return;
    };
    if action.cancellation.is_cancelled() {
        return;
    }
    match action.info.action_type.as_str() {
        PENDING_ACTION_TYPE_REVOCATION => {
            let device_id = action.info.device_id.clone();
            if inner.devices.remove(&device_id).is_some() {
                let _ = save_devices(&mut inner);
            }
            // Notify the sync hub to drop the revoked device's active WebSocket
            // connections so it stops receiving events immediately.
            let listener = inner.revocation_listener.clone();
            drop(inner);
            if let Some(listener) = listener {
                listener.device_revoked(&device_id);
            }
        }
        PENDING_ACTION_TYPE_WORKSPACE_REGISTRATION => {
            let registrar = inner.workspace_registrar.clone();
            let path = action.info.path;
            drop(inner);
            if let Some(registrar) = registrar {
                let _ = registrar.register_workspace(&path);
            }
        }
        _ => {}
    }
}

fn cancel_pending(
    shared: &Arc<Mutex<Inner>>,
    action_id: &str,
    action_type: &str,
) -> Result<(), PairingError> {
    let mut inner = lock(shared);
    let action = inner
        .pending
        .get(action_id)
        .ok_or(PairingError::PendingActionNotFound)?;
    if action.info.action_type != action_type {
        return Err(PairingError::PendingActionTypeMismatch);
    }
    action.cancellation.cancel();
    inner.pending.remove(action_id);
    Ok(())
}

fn pending_info(
    action_type: &str,
    device_id: &str,
    device_name: &str,
    path: &str,
    requested_by: String,
    grace: Duration,
) -> PendingActionInfo {
    let requested_at = Utc::now();
    PendingActionInfo {
        id: random_hex(16),
        action_type: action_type.to_owned(),
        device_id: device_id.to_owned(),
        device_name: device_name.to_owned(),
        path: path.to_owned(),
        requested_by,
        requested_at,
        execute_at: requested_at + chrono_duration(grace),
    }
}

fn check_rate_limit(inner: &mut Inner, key: &str) -> Result<(), PairingError> {
    let now = Utc::now();
    let Some(state) = inner.lockouts.get_mut(key) else {
        return Ok(());
    };
    state
        .failures
        .retain(|time| now - *time < chrono_duration(RATE_LIMIT_WINDOW));
    if state.lockout_until.is_some_and(|until| now < until) {
        return Err(PairingError::RateLimited);
    }
    Ok(())
}

fn record_failure(inner: &mut Inner, key: &str) {
    let state = inner.lockouts.entry(key.to_string()).or_default();
    state.failures.push(Utc::now());
    if state.failures.len() >= MAX_VERIFY_ATTEMPTS {
        let multiplier = 1_u64 << state.lockout_count.min(8);
        let seconds = (BASE_LOCKOUT.as_secs() * multiplier).min(MAX_LOCKOUT.as_secs());
        // `seconds` is bounded by MAX_LOCKOUT (a Duration constant < i64::MAX secs).
        #[allow(clippy::cast_possible_wrap)]
        let seconds_i64 = seconds as i64;
        state.lockout_until = Some(Utc::now() + chrono::Duration::seconds(seconds_i64));
        state.lockout_count = state.lockout_count.saturating_add(1);
        state.failures.clear();
    }
}

fn cleanup_sessions(inner: &mut Inner) {
    let now = Utc::now();
    inner.sessions.retain(|_, session| {
        let active = !session.used && session.expires_at > now;
        if !active {
            let _ = fs::remove_file(&session.qr_path);
        }
        active
    });
}

/// Remove orphaned `pairing-*.png` files left by a crash or kill before
/// cleanup could run. Their in-memory sessions no longer exist, so the PNGs
/// contain stale (but potentially still-valid within TTL) pairing tokens.
fn cleanup_stale_qr_files(data_dir: &Path) {
    let Ok(entries) = fs::read_dir(data_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("pairing-")
                && name
                    .rsplit('.')
                    .next()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

fn load_devices(data_dir: &Path) -> Result<HashMap<String, StoredDevice>, PairingError> {
    let path = data_dir.join(DEVICES_FILE);
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(PairingError::Persistence(error)),
    };
    let records: Vec<StoredDevice> = serde_json::from_slice(&data).map_err(PairingError::State)?;
    let now = Utc::now();
    Ok(records
        .into_iter()
        .map(|mut device| {
            if device.last_seen.is_none() {
                device.last_seen = Some(now);
            }
            (device.id.clone(), device)
        })
        .collect())
}

fn save_devices(inner: &mut Inner) -> Result<(), PairingError> {
    let mut devices: Vec<_> = inner.devices.values().cloned().collect();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    let data = serde_json::to_vec_pretty(&devices).map_err(PairingError::State)?;
    fsutil::atomic_write(&inner.data_dir.join(DEVICES_FILE), &data, Some(0o600))
        .map_err(PairingError::Persistence)
}

fn random_hex(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    // `rand::rng()` is a CSPRNG seeded from the operating system. It does not
    // expose a fallible API, unlike Go's direct `crypto/rand.Reader`.
    rand::rng().fill_bytes(&mut value);
    hex::encode(value)
}

fn generate_passcode() -> String {
    let words = word_list();
    let mut result = Vec::with_capacity(4);
    for _ in 0..4 {
        let mut value = [0_u8; 8];
        rand::rng().fill_bytes(&mut value);
        // Rejection sampling avoids modulo bias across the 2048-word list.
        let limit = u64::MAX - (u64::MAX % words.len() as u64);
        let mut number = u64::from_le_bytes(value);
        while number >= limit {
            rand::rng().fill_bytes(&mut value);
            number = u64::from_le_bytes(value);
        }
        // `number % words.len()` < 2048, so truncation to usize is impossible.
        #[allow(clippy::cast_possible_truncation)]
        result.push(words[(number % words.len() as u64) as usize]);
    }
    result.join("-")
}

fn word_list() -> &'static Vec<&'static str> {
    static WORDS: std::sync::OnceLock<Vec<&str>> = std::sync::OnceLock::new();
    WORDS.get_or_init(|| {
        // BIP-39 English word list (formerly shared with Go via words.go).
        include_str!("words.txt")
            .lines()
            .filter(|word| !word.is_empty())
            .collect()
    })
}

fn hash_secret(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}

fn chrono_duration(duration: Duration) -> chrono::Duration {
    chrono::Duration::from_std(duration).unwrap_or(chrono::Duration::MAX)
}

fn write_qr(url: &str, path: &Path) -> Result<(), PairingError> {
    let code = QrCode::new(url.as_bytes()).map_err(PairingError::Qr)?;
    let image = code.render::<Luma<u8>>().min_dimensions(256, 256).build();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::L8.into(),
        )
        .map_err(PairingError::QrEncoding)?;
    fsutil::atomic_write(path, &png, Some(0o600)).map_err(PairingError::Persistence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn manager() -> (tempfile::TempDir, Manager) {
        let dir = tempfile::tempdir().expect("temporary data dir");
        let manager = Manager::new(dir.path(), None).expect("manager");
        (dir, manager)
    }

    fn pair(manager: &Manager) -> DeviceCredential {
        let session = manager.create_session("localhost", 7337).expect("session");
        manager
            .verify_passcode(&session.passcode, "test device", None)
            .expect("credential")
    }

    #[test]
    fn successful_pairing_resets_lockout_backoff() {
        let (_dir, manager) = manager();
        let session = manager.create_session("localhost", 7337).expect("session");
        {
            let mut inner = lock(&manager.inner);
            let state = inner
                .lockouts
                .entry(GLOBAL_LOCKOUT_KEY.to_string())
                .or_default();
            state.failures.push(Utc::now());
            state.lockout_count = 4;
            state.lockout_until = Some(Utc::now() - chrono::Duration::seconds(1));
        }

        manager
            .verify_token(&session.token, "test device", None)
            .expect("successful pairing");

        let inner = lock(&manager.inner);
        let state = inner.lockouts.get(GLOBAL_LOCKOUT_KEY);
        assert!(state.is_none_or(|s| s.failures.is_empty()));
        assert!(state.is_none_or(|s| s.lockout_count == 0));
        assert!(state.is_none_or(|s| s.lockout_until.is_none()));
    }

    #[test]
    fn session_is_four_words_and_writes_private_qr() {
        let (_dir, manager) = manager();
        let session = manager.create_session("localhost", 7337).expect("session");
        assert_eq!(session.passcode.split('-').count(), 4);
        assert_eq!(
            session.url,
            format!("http://localhost:7337?token={}", session.token)
        );
        assert!(fs::metadata(&session.qr_path).is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&session.qr_path)
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let debug = format!("{session:?}");
        assert!(!debug.contains(&session.token));
        assert!(!debug.contains(&session.passcode));
    }

    #[test]
    fn pairing_is_single_use_and_hash_only_at_rest() {
        let (dir, manager) = manager();
        let session = manager.create_session("localhost", 7337).expect("session");
        let credential = manager
            .verify_passcode(&session.passcode, "phone", None)
            .expect("credential");
        assert!(manager.validate_credential(&credential.id, &credential.secret));
        assert!(!manager.validate_credential(&credential.id, "wrong"));
        assert!(matches!(
            manager.verify_passcode(&session.passcode, "other", None),
            Err(PairingError::InvalidPasscode)
        ));
        let state = fs::read_to_string(dir.path().join(DEVICES_FILE)).expect("state");
        assert!(!state.contains(&credential.secret));
        assert!(state.contains(&hash_secret(&credential.secret)));
        let debug = format!("{credential:?}");
        assert!(!debug.contains(&credential.secret));
    }

    #[test]
    fn token_pairing_and_sliding_expiry_work() {
        let (_dir, manager) = manager();
        manager.set_inactivity_ttl(Duration::from_mins(1));
        let session = manager.create_session("localhost", 7337).expect("session");
        let credential = manager
            .verify_token(&session.token, "laptop", None)
            .expect("credential");
        {
            let mut inner = lock(&manager.inner);
            inner
                .devices
                .get_mut(&credential.id)
                .expect("stored device")
                .last_seen = Some(Utc::now() - chrono::Duration::seconds(30));
        }
        assert!(manager.validate_credential(&credential.id, &credential.secret));
        {
            let mut inner = lock(&manager.inner);
            inner
                .devices
                .get_mut(&credential.id)
                .expect("stored device")
                .last_seen = Some(Utc::now() - chrono::Duration::seconds(61));
        }
        assert!(!manager.validate_credential(&credential.id, &credential.secret));
    }

    #[test]
    fn verification_failures_lock_out_without_consuming_session() {
        let (_dir, manager) = manager();
        let session = manager.create_session("localhost", 7337).expect("session");
        for _ in 0..MAX_VERIFY_ATTEMPTS {
            assert!(matches!(
                manager.verify_passcode("wrong-wrong-wrong-wrong", "attacker", None),
                Err(PairingError::InvalidPasscode)
            ));
        }
        assert!(matches!(
            manager.verify_passcode(&session.passcode, "valid", None),
            Err(PairingError::RateLimited)
        ));
    }

    #[test]
    fn pending_revocation_can_be_cancelled_or_executes() {
        let (_dir, manager) = manager();
        let credential = pair(&manager);
        let pending = manager
            .request_revocation(&credential.id, "other", Duration::from_mins(1))
            .expect("pending");
        assert_eq!(manager.list_pending_actions(), vec![pending.clone()]);
        manager.cancel_revocation(&pending.id).expect("cancel");
        assert!(manager.validate_credential(&credential.id, &credential.secret));
        manager
            .request_revocation(&credential.id, "other", Duration::ZERO)
            .expect("immediate");
        assert!(!manager.validate_credential(&credential.id, &credential.secret));
    }

    struct Registrar(AtomicUsize);
    impl WorkspaceRegistrar for Registrar {
        fn register_workspace(&self, _path: &str) -> Result<(), PairingError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn injected_workspace_registrar_runs_after_immediate_grace() {
        let dir = tempfile::tempdir().expect("temporary data dir");
        let registrar = Arc::new(Registrar(AtomicUsize::new(0)));
        let manager = Manager::new(dir.path(), Some(registrar.clone())).expect("manager");
        manager
            .request_workspace_registration("/work", "device", Duration::ZERO)
            .expect("registration");
        assert_eq!(registrar.0.load(Ordering::SeqCst), 1);
        assert!(manager.list_pending_actions().is_empty());
    }

    #[test]
    fn loads_go_legacy_devices_json_and_backfills_last_seen() {
        let dir = tempfile::tempdir().expect("temporary data dir");
        let legacy = r#"[{"id":"go-device","name":"Go","secretHash":"abc","pairedAt":"2026-01-01T00:00:00Z"}]"#;
        fs::write(dir.path().join(DEVICES_FILE), legacy).expect("legacy state");
        let manager = Manager::new(dir.path(), None).expect("load Go state");
        let devices = manager.list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "go-device");
        assert!(devices[0].last_seen > devices[0].paired_at);
    }
}
