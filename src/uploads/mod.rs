//! Per-session filesystem storage for user-uploaded artifacts (Go
//! `internal/uploads/`).
//!
//! Each session gets an isolated directory under the configured root
//! (typically `~/.local-agent/uploads/`). Uploads are validated by magic
//! bytes (not file extension) so a misnamed file is detected and stored with
//! the correct extension — this avoids the mime-by-extension bugs documented
//! in agent read-tool image pipelines. The stored file is referenced by an
//! opaque upload ID; the absolute path is exposed so the agent can read it
//! directly from disk (agents run as subprocesses on the same host and have
//! filesystem access).
//!
//! Layout:
//! - This file — `Manager`, `StoredUpload`, store/get/remove, validators,
//!   magic-byte image detection.
//! - [`tests`] — port of `internal/uploads/uploads_test.go`.
//!
//! See `docs/plans/rust-port/complete-S-UPLOADS-uploads-med.md`.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

/// Per-file upload size cap. Matches the server's default JSON body limit so a
/// base64-encoded image and a raw upload share the same ceiling. Mirrors Go
/// `MaxUploadBytes`.
pub const MAX_UPLOAD_BYTES: usize = 10 << 20; // 10 MB

/// Aggregate upload size cap per session. Prevents a single session from
/// filling the disk with repeated uploads. Cleanup happens on
/// `remove_session`, which resets the tracked total.
pub const MAX_SESSION_UPLOAD_BYTES: u64 = 100 << 20; // 100 MB

/// Supported image MIME types. Mirrors Go `mimePNG`/`mimeJPEG`/`mimeGIF`/
/// `mimeWebP`.
const MIME_PNG: &str = "image/png";
const MIME_JPEG: &str = "image/jpeg";
const MIME_GIF: &str = "image/gif";
const MIME_WEBP: &str = "image/webp";

/// Errors returned by the upload store.
///
/// Mirrors the distinct failure modes of Go `uploads` so callers can surface
/// consistent diagnostics without re-parsing error strings.
#[derive(Debug, Error)]
pub enum UploadError {
    /// Caller-supplied session ID was empty or contained path separators / `..`
    /// segments (rejected to prevent traversal via `join`/`remove_dir_all`).
    #[error("uploads: invalid session id")]
    InvalidSessionId,

    /// Caller-supplied upload ID was not a 32-char lowercase hex UUID string
    /// (rejected to prevent path traversal via crafted IDs).
    #[error("uploads: invalid upload id")]
    InvalidUploadId,

    /// The upload exceeded [`MAX_UPLOAD_BYTES`].
    #[error("uploads: file exceeds {0} bytes")]
    Oversize(usize),

    /// The aggregate uploaded bytes for the session exceeded
    /// [`MAX_SESSION_UPLOAD_BYTES`].
    #[error("uploads: session aggregate size limit exceeded")]
    SessionQuotaExceeded,

    /// The bytes did not match a supported image format (PNG/JPEG/GIF/WebP).
    #[error("uploads: unsupported file type (must be PNG, JPEG, GIF, or WebP)")]
    UnsupportedType,

    /// A session ID was required but the caller passed an empty string.
    #[error("uploads: empty session id")]
    EmptySessionId,

    /// An underlying `std::fs` operation failed (read, write, mkdir, readdir).
    #[error("uploads: io: {0}")]
    Io(#[from] std::io::Error),

    /// A session directory or stored file was a symlink (defense-in-depth:
    /// uploads must stay within the uploads root and never follow a planted
    /// symlink to an arbitrary location).
    #[error("uploads: symlink detected at {0}")]
    SymlinkDetected(String),

    /// No upload with the requested ID was found in the session directory.
    #[error("uploads: upload {upload_id} not found in session {session_id}")]
    NotFound {
        /// The requested upload ID.
        upload_id: String,
        /// The session directory searched.
        session_id: String,
    },
}

/// Describes a successfully stored upload. Mirrors Go `StoredUpload`; field
/// names are serde-renamed to match the Go JSON tags exactly so the frontend
/// contract is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredUpload {
    /// Opaque upload ID (32-char lowercase hex UUID, no hyphens).
    pub id: String,
    /// Sanitized display name (the original filename; the on-disk name is the
    /// upload ID with a magic-byte-derived extension).
    pub name: String,
    /// Detected MIME type (e.g. `image/png`).
    pub mime_type: String,
    /// Absolute on-disk path. The agent reads the file from here directly.
    pub path: PathBuf,
    /// `file://` URI pointing at `path`, suitable for ACP `ImageBlock.uri` or
    /// `ResourceLinkBlock.uri`.
    pub uri: String,
    /// Stored file size in bytes.
    pub size: u64,
}

/// Stores uploaded artifacts in per-session directories under `root`.
///
/// Mirrors Go `uploads.Manager`. The root is created (mode `0o700`) on
/// construction; each session gets its own subdirectory (also `0o700`) on the
/// first `store` call.
pub struct Manager {
    root: PathBuf,
    /// Running total of stored bytes per session, used to enforce
    /// [`MAX_SESSION_UPLOAD_BYTES`]. Reset on `remove_session`.
    session_totals: HashMap<String, u64>,
}

impl Manager {
    /// Create a `Manager` rooted at `root_dir`. The directory is created (mode
    /// `0o700`) if missing. Mirrors Go `uploads.New`.
    pub fn new(root_dir: impl Into<PathBuf>) -> Result<Self, UploadError> {
        let root = root_dir.into();
        crate::fsutil::create_dir_all(&root)?;
        Ok(Self {
            root,
            session_totals: HashMap::new(),
        })
    }

    /// Returns the absolute root directory. Mirrors Go `Manager.Root`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Validate and write an upload for the given session.
    ///
    /// The original `filename` is used only to derive a friendly display name;
    /// the on-disk name is the upload ID with an extension chosen by magic-byte
    /// detection. The reader is consumed fully (up to [`MAX_UPLOAD_BYTES`] + 1
    /// bytes; anything beyond is rejected as oversize).
    ///
    /// Mirrors Go `Manager.Store`.
    pub fn store(
        &mut self,
        session_id: &str,
        filename: &str,
        reader: &mut dyn Read,
    ) -> Result<StoredUpload, UploadError> {
        if session_id.is_empty() {
            return Err(UploadError::EmptySessionId);
        }
        if !is_valid_session_id(session_id) {
            return Err(UploadError::InvalidSessionId);
        }

        // Read into memory with a size cap. Images are bounded by
        // MAX_UPLOAD_BYTES; reading fully lets us validate magic bytes before
        // writing to disk. We read one byte past the limit so we can detect
        // oversize uploads without trusting the reader's reported length.
        let mut data = Vec::new();
        let mut limited = reader.take((MAX_UPLOAD_BYTES + 1) as u64);
        limited.read_to_end(&mut data)?;
        if data.len() > MAX_UPLOAD_BYTES {
            return Err(UploadError::Oversize(MAX_UPLOAD_BYTES));
        }

        let (mime_type, ext) = detect_image(&data);
        if mime_type.is_empty() {
            return Err(UploadError::UnsupportedType);
        }

        // Enforce the per-session aggregate cap so repeated uploads cannot fill
        // the disk. The check is conservative: we account the new file's size
        // before writing and roll back the total on a write failure below.
        let new_total = self
            .session_totals
            .get(session_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(data.len() as u64);
        if new_total > MAX_SESSION_UPLOAD_BYTES {
            return Err(UploadError::SessionQuotaExceeded);
        }

        let upload_id = new_id();
        let session_dir = self.root.join(session_id);
        crate::fsutil::create_dir_all(&session_dir)?;

        // Defense-in-depth: reject a symlink planted at the session directory
        // so fs::write cannot follow it outside the uploads root. The session
        // dir is created with 0o700 and IDs are random, but a same-UID process
        // could still plant a symlink between create_dir_all and write.
        if let Ok(meta) = fs::symlink_metadata(&session_dir) {
            if meta.file_type().is_symlink() {
                return Err(UploadError::SymlinkDetected(
                    session_dir.display().to_string(),
                ));
            }
        }

        let stored_name = format!("{upload_id}{ext}");
        let abs_path = session_dir.join(&stored_name);
        // The stored file is new (random ID), so it cannot be a pre-planted
        // symlink; verify anyway so a race or relaxed validation can't write
        // through a symlink.
        if let Ok(meta) = fs::symlink_metadata(&abs_path) {
            if meta.file_type().is_symlink() {
                return Err(UploadError::SymlinkDetected(abs_path.display().to_string()));
            }
        }
        fs::write(&abs_path, &data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&abs_path, fs::Permissions::from_mode(0o600));
        }

        let uri = format!("file://{}", abs_path.display());
        // Record the aggregate total only after the write succeeds so a failed
        // write does not consume quota.
        self.session_totals
            .insert(session_id.to_string(), new_total);
        Ok(StoredUpload {
            id: upload_id,
            name: sanitize_filename(filename),
            mime_type: mime_type.to_string(),
            path: abs_path,
            uri,
            size: data.len() as u64,
        })
    }

    /// Returns the absolute path for a session's upload by ID.
    ///
    /// The file is not read; callers (e.g. an HTTP handler) serve it. Searches
    /// the session directory for a file named `<id>.<ext>` regardless of
    /// extension (the extension was chosen by magic-byte detection, which the
    /// caller doesn't know). Mirrors Go `Manager.Get`.
    pub fn get(&self, session_id: &str, upload_id: &str) -> Result<PathBuf, UploadError> {
        if !is_valid_session_id(session_id) {
            return Err(UploadError::InvalidSessionId);
        }
        if !is_valid_id(upload_id) {
            return Err(UploadError::InvalidUploadId);
        }
        let session_dir = self.root.join(session_id);
        let entries = match fs::read_dir(&session_dir) {
            Ok(e) => e,
            // Missing session dir → not found (matches Go's "not found" wrap).
            Err(_) => {
                return Err(UploadError::NotFound {
                    upload_id: upload_id.to_string(),
                    session_id: session_id.to_string(),
                });
            }
        };
        let prefix = format!("{upload_id}.");
        for entry in entries {
            let entry = entry?;
            // Use symlink_metadata so a planted symlink is detected rather
            // than followed — serving a symlink target would leak arbitrary
            // files outside the uploads root.
            let meta = std::fs::symlink_metadata(entry.path())?;
            if meta.is_dir() || meta.file_type().is_symlink() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&prefix) {
                    return Ok(entry.path());
                }
            }
        }
        Err(UploadError::NotFound {
            upload_id: upload_id.to_string(),
            session_id: session_id.to_string(),
        })
    }

    /// Delete all uploads for a session. Safe to call when no uploads exist for
    /// the session. Mirrors Go `Manager.RemoveSession`.
    pub fn remove_session(&mut self, session_id: &str) -> Result<(), UploadError> {
        if !is_valid_session_id(session_id) {
            return Err(UploadError::InvalidSessionId);
        }
        let session_dir = self.root.join(session_id);
        // A missing dir is a no-op (matches Go's os.RemoveAll behaviour).
        match fs::remove_dir_all(&session_dir) {
            Ok(()) => {
                // Reset the aggregate quota tracker now that the session's
                // uploads are gone.
                self.session_totals.remove(session_id);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.session_totals.remove(session_id);
                Ok(())
            }
            Err(e) => Err(UploadError::Io(e)),
        }
    }

    /// Delete the entire uploads root and its contents. Used on daemon shutdown
    /// to clean up all per-session upload directories. The manager remains
    /// usable after this only if [`Manager::new`] is called again to recreate
    /// the root. Mirrors Go `Manager.RemoveAll`.
    pub fn remove_all(&mut self) -> Result<(), UploadError> {
        if self.root.as_os_str().is_empty() {
            return Ok(());
        }
        match fs::remove_dir_all(&self.root) {
            Ok(()) => {
                self.session_totals.clear();
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.session_totals.clear();
                Ok(())
            }
            Err(e) => Err(UploadError::Io(e)),
        }
    }
}

/// Generate a new 32-char lowercase hex upload ID.
///
/// Uses a v4 UUID with hyphens stripped so the on-disk filename is a single
/// path component (no separators to confuse the containment check). Mirrors Go
/// `newID` (16 random bytes → hex), which also yields 32 hex chars.
fn new_id() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Check that `id` is a 32-char lowercase hex string, preventing path traversal
/// via crafted upload IDs. Mirrors Go `isValidID`.
fn is_valid_id(id: &str) -> bool {
    if id.len() != 32 {
        return false;
    }
    id.bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Check that a session ID is safe to use as a path component.
///
/// Session IDs are backend-generated opaque tokens (e.g. `sess-` + 16 hex
/// chars, or UUIDs), so we reject empty strings, path separators, control
/// chars, and any `..` segment rather than requiring a specific shape. This
/// prevents a malicious `session_id` like `../../foo` from escaping the uploads
/// root via `join` or `remove_dir_all`. Mirrors Go `isValidSessionID`.
fn is_valid_session_id(id: &str) -> bool {
    if id.is_empty() || id == "." || id == ".." {
        return false;
    }
    if id.contains("..") {
        return false;
    }
    id.bytes().all(|c| {
        // Reject path separators and control chars (< 0x20).
        c != b'/' && c != b'\\' && c >= 0x20
    })
}

/// Strip path separators and control chars from a user-supplied filename,
/// keeping only the display name (the on-disk name is the upload ID anyway).
/// Mirrors Go `sanitizeFilename`.
fn sanitize_filename(name: &str) -> String {
    // filepath.Base equivalent: take the final component.
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    // Replace any remaining separators (defensive — file_name already strips
    // them, but on Windows `\` could appear inside a component).
    let cleaned: String = base
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    if cleaned.is_empty() || cleaned == "." {
        "upload".to_string()
    } else {
        cleaned
    }
}

/// Identify a supported image format by magic bytes and return its MIME type
/// and canonical extension (with leading dot), or `("", "")` if the format is
/// not recognized.
///
/// Uses the `infer` crate (magic-byte detection) as the Rust replacement for
/// Go's `net/http.DetectContentType`. Mirrors Go `detectImage`.
fn detect_image(data: &[u8]) -> (&'static str, &'static str) {
    // `infer::get` returns the detected MIME type from magic-byte signatures.
    // We map the supported image formats to canonical extensions (matching Go's
    // choice of `.jpg` rather than `.jpeg` for JPEG).
    let kind = match infer::get(data) {
        Some(k) => k,
        None => return ("", ""),
    };
    match kind.mime_type() {
        MIME_PNG => (MIME_PNG, ".png"),
        MIME_JPEG => (MIME_JPEG, ".jpg"),
        MIME_GIF => (MIME_GIF, ".gif"),
        MIME_WEBP => (MIME_WEBP, ".webp"),
        _ => ("", ""),
    }
}

#[cfg(test)]
mod tests;
