//! Registered workspace access with bounded, symlink-safe filesystem operations.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::interfaces::{
    AppError, FileNode, ReadFileResult, SearchOptions, SearchResult, WorkspaceInfo,
    WorkspaceManager, FILE_NODE_TYPE_FILE, FILE_NODE_TYPE_FOLDER,
};
use crate::pathutil::{clean_path, resolve_symlink};

const MAX_READ_FILE_SIZE: u64 = 50 * 1024 * 1024;
const BINARY_SNIFF_SIZE: usize = 512;
const MAX_FILE_TREE_DEPTH: usize = 20;
const MAX_FILE_TREE_NODES: usize = 100_000;

/// One registered workspace root, available or retained-as-unavailable.
#[derive(Clone)]
struct WorkspaceEntry {
    path: PathBuf,
    /// Set when the path failed validation; empty when the root is usable.
    error: String,
}

impl WorkspaceEntry {
    fn available(path: PathBuf) -> Self {
        Self {
            path,
            error: String::new(),
        }
    }

    fn unavailable(path: PathBuf, error: String) -> Self {
        Self { path, error }
    }

    fn is_available(&self) -> bool {
        self.error.is_empty()
    }

    fn to_info(&self, id: &str) -> WorkspaceInfo {
        WorkspaceInfo {
            id: id.to_string(),
            path: self.path.to_string_lossy().into_owned(),
            name: self
                .path
                .file_name()
                .map_or_else(String::new, |part| part.to_string_lossy().into_owned()),
            available: self.is_available(),
            error: self.error.clone(),
        }
    }
}

/// Callback after a successful app write (absolute path). Used by fswatch to
/// suppress the matching on-disk change event — Go `SetOnWrite`.
pub type OnWriteFn = Arc<dyn Fn(&str) + Send + Sync>;

/// In-memory registry and workspace-scoped filesystem service.
///
/// The registry lock protects only its map. Every disk operation first copies
/// a workspace root out of the lock, so a slow tree walk or search never
/// blocks registration and removal. Unavailable entries (missing paths) stay
/// listed so the UI/CLI can warn without pruning config.
pub struct Manager {
    workspaces: RwLock<HashMap<String, WorkspaceEntry>>,
    on_write: RwLock<Option<OnWriteFn>>,
}

impl Manager {
    /// Construct an empty workspace registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            workspaces: RwLock::new(HashMap::new()),
            on_write: RwLock::new(None),
        }
    }

    /// Register a hook invoked with the absolute path after each successful
    /// [`WorkspaceManager::write_file`]. Call once at daemon wiring time.
    pub fn set_on_write(&self, hook: OnWriteFn) {
        if let Ok(mut slot) = self.on_write.write() {
            *slot = Some(hook);
        }
    }

    /// Retain a configured path that failed to load so list/CLI can warn.
    ///
    /// Does not touch config. ID is the same path-hash used by successful
    /// register (hash of the configured path string). Replaces any prior entry
    /// for that id.
    pub fn retain_unavailable(
        &self,
        path: &str,
        error: impl Into<String>,
    ) -> Result<WorkspaceInfo, AppError> {
        let path_buf = PathBuf::from(path);
        let id = workspace_id(&path_buf);
        let entry = WorkspaceEntry::unavailable(path_buf, error.into());
        let info = entry.to_info(&id);
        self.workspaces
            .write()
            .map_err(|_| AppError::internal("workspace registry lock poisoned"))?
            .insert(id, entry);
        Ok(info)
    }

    fn root_for(&self, id: &str) -> Result<PathBuf, AppError> {
        let entry = self
            .workspaces
            .read()
            .map_err(|_| AppError::internal("workspace registry lock poisoned"))?
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::not_found_id("workspace", id))?;
        if !entry.is_available() {
            return Err(AppError::validation(format!(
                "workspace unavailable: {}",
                entry.error
            )));
        }
        Ok(entry.path)
    }

    fn safe_path(root: &Path, rel_path: &str) -> Result<PathBuf, AppError> {
        let path = clean_path(root, rel_path)?;
        Ok(resolve_symlink(root, &path)?)
    }

    /// Invoke the optional on-write hook (outside the registry lock).
    fn notify_write(&self, abs_path: &str) {
        if let Ok(guard) = self.on_write.read() {
            if let Some(hook) = guard.as_ref() {
                hook(abs_path);
            }
        }
    }
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WorkspaceManager for Manager {
    async fn register(&self, path: &str) -> Result<WorkspaceInfo, AppError> {
        let input = PathBuf::from(path);
        let root = tokio::task::spawn_blocking(move || {
            let metadata = fs::metadata(&input)
                .map_err(|err| AppError::internal(format!("stat workspace: {err}")))?;
            if !metadata.is_dir() {
                return Err(AppError::validation("workspace path is not a directory"));
            }
            input
                .canonicalize()
                .map_err(|err| AppError::internal(format!("canonicalize workspace: {err}")))
        })
        .await
        .map_err(|err| AppError::internal(format!("register task failed: {err}")))??;

        let id = workspace_id(&root);
        let entry = WorkspaceEntry::available(root);
        let info = entry.to_info(&id);
        self.workspaces
            .write()
            .map_err(|_| AppError::internal("workspace registry lock poisoned"))?
            .insert(id, entry);
        Ok(info)
    }

    async fn list(&self) -> Result<Vec<WorkspaceInfo>, AppError> {
        let mut workspaces: Vec<_> = self
            .workspaces
            .read()
            .map_err(|_| AppError::internal("workspace registry lock poisoned"))?
            .iter()
            .map(|(id, entry)| entry.to_info(id))
            .collect();
        workspaces
            .sort_by(|left, right| left.name.cmp(&right.name).then(left.path.cmp(&right.path)));
        Ok(workspaces)
    }

    async fn remove(&self, id: &str) -> Result<(), AppError> {
        self.workspaces
            .write()
            .map_err(|_| AppError::internal("workspace registry lock poisoned"))?
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| AppError::not_found_id("workspace", id))
    }

    async fn file_tree(&self, workspace_id: &str) -> Result<Vec<FileNode>, AppError> {
        let root = self.root_for(workspace_id)?;
        tokio::task::spawn_blocking(move || {
            let mut nodes = 0;
            build_tree(&root, Path::new(""), 0, &mut nodes)
        })
        .await
        .map_err(|err| AppError::internal(format!("file tree task failed: {err}")))?
    }

    async fn read_file(
        &self,
        workspace_id: &str,
        rel_path: &str,
    ) -> Result<ReadFileResult, AppError> {
        let root = self.root_for(workspace_id)?;
        let rel_path = rel_path.to_string();
        let root_for_read = root.clone();
        let rel_for_read = rel_path.clone();
        tokio::task::spawn_blocking(move || read_file(&root_for_read, &rel_for_read))
            .await
            .map_err(|err| AppError::internal(format!("read task failed: {err}")))?
    }

    async fn file_path(&self, workspace_id: &str, rel_path: &str) -> Result<String, AppError> {
        let root = self.root_for(workspace_id)?;
        let rel_path = rel_path.to_string();
        tokio::task::spawn_blocking(move || {
            let path = Self::safe_path(&root, &rel_path)?;
            let metadata = fs::symlink_metadata(&path)
                .map_err(|err| AppError::internal(format!("stat file: {err}")))?;
            if metadata.is_dir() {
                return Err(AppError::validation("path is a directory, not a file"));
            }
            if !metadata.file_type().is_file() {
                return Err(AppError::validation("path is not a regular file"));
            }
            // file_path feeds raw/preview serving, which buffers the whole file
            // into memory (tokio::fs::read). Reject oversized files before the
            // read to avoid unbounded allocation (DoS). Mirrors read_file.
            if metadata.len() > MAX_READ_FILE_SIZE {
                return Err(AppError::validation(format!(
                    "file too large (max {MAX_READ_FILE_SIZE} bytes, file is {} bytes)",
                    metadata.len()
                )));
            }
            Ok(path.to_string_lossy().into_owned())
        })
        .await
        .map_err(|err| AppError::internal(format!("file path task failed: {err}")))?
    }

    async fn write_file(
        &self,
        workspace_id: &str,
        rel_path: &str,
        content: &str,
        expected_revision: i64,
    ) -> Result<i64, AppError> {
        let root = self.root_for(workspace_id)?;
        let rel_path = rel_path.to_string();
        let content = content.to_string();
        let (revision, abs_path) = tokio::task::spawn_blocking(move || {
            write_file(&root, &rel_path, &content, expected_revision)
        })
        .await
        .map_err(|err| AppError::internal(format!("write task failed: {err}")))??;

        // Notify outside the registry lock (Go WriteFile → onWrite).
        self.notify_write(&abs_path);
        Ok(revision)
    }

    async fn delete_path(&self, workspace_id: &str, rel_path: &str) -> Result<(), AppError> {
        let root = self.root_for(workspace_id)?;
        let rel_path = rel_path.to_string();
        let abs_path = tokio::task::spawn_blocking(move || delete_path(&root, &rel_path))
            .await
            .map_err(|err| AppError::internal(format!("delete task failed: {err}")))??;
        self.notify_write(&abs_path);
        Ok(())
    }

    async fn rename_path(&self, workspace_id: &str, from: &str, to: &str) -> Result<(), AppError> {
        let root = self.root_for(workspace_id)?;
        let from = from.to_string();
        let to = to.to_string();
        let (from_abs, to_abs) =
            tokio::task::spawn_blocking(move || rename_path(&root, &from, &to))
                .await
                .map_err(|err| AppError::internal(format!("rename task failed: {err}")))??;
        // Notify both ends so fswatch suppresses remove + create echoes.
        self.notify_write(&from_abs);
        self.notify_write(&to_abs);
        Ok(())
    }

    async fn mkdir(&self, workspace_id: &str, rel_path: &str) -> Result<(), AppError> {
        let root = self.root_for(workspace_id)?;
        let rel_path = rel_path.to_string();
        let abs_path = tokio::task::spawn_blocking(move || mkdir_path(&root, &rel_path))
            .await
            .map_err(|err| AppError::internal(format!("mkdir task failed: {err}")))??;
        self.notify_write(&abs_path);
        Ok(())
    }

    async fn search(
        &self,
        workspace_id: &str,
        pattern: &str,
        mut opts: SearchOptions,
    ) -> Result<Vec<SearchResult>, AppError> {
        let root = self.root_for(workspace_id)?;
        opts.pattern = pattern.to_string();
        crate::search::search(&root, &opts, CancellationToken::new())
            .await
            .map_err(|err| AppError::validation(err.to_string()))
    }
}

fn workspace_id(root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_file(root: &Path, rel_path: &str) -> Result<ReadFileResult, AppError> {
    let path = Manager::safe_path(root, rel_path)?;
    let metadata = fs::symlink_metadata(&path).map_err(|err| {
        // Go returns `stat file: lstat <path>: no such file or directory` as a
        // 404 via handleReadFile; mirror that client-facing string.
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!(
                "stat file: lstat {}: no such file or directory",
                path.display()
            ))
        } else {
            AppError::internal(format!("stat file: {err}"))
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(AppError::validation("path is not a regular file"));
    }
    if metadata.len() > MAX_READ_FILE_SIZE {
        return Err(AppError::validation(format!(
            "file too large (max {MAX_READ_FILE_SIZE} bytes, file is {} bytes)",
            metadata.len()
        )));
    }
    let previewable = is_previewable(rel_path);
    let data = fs::read(&path).map_err(|err| AppError::internal(format!("read file: {err}")))?;
    let is_binary =
        is_binary_preview(rel_path) || data.iter().take(BINARY_SNIFF_SIZE).any(|byte| *byte == 0);
    if is_binary {
        return Ok(ReadFileResult {
            content: String::new(),
            revision: crate::files::content_revision(&data[..data.len().min(BINARY_SNIFF_SIZE)]),
            is_binary: true,
            previewable,
        });
    }
    Ok(ReadFileResult {
        content: String::from_utf8_lossy(&data).into_owned(),
        revision: crate::files::content_revision(&data),
        is_binary: false,
        previewable,
    })
}

fn write_file(
    root: &Path,
    rel_path: &str,
    content: &str,
    expected_revision: i64,
) -> Result<(i64, String), AppError> {
    let path = Manager::safe_path(root, rel_path)?;
    if expected_revision > 0 {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|err| AppError::internal(format!("stat file for revision check: {err}")))?;
        if metadata.len() > MAX_READ_FILE_SIZE {
            return Err(AppError::validation(format!(
                "file too large (max {MAX_READ_FILE_SIZE} bytes, file is {} bytes)",
                metadata.len()
            )));
        }
        let current =
            fs::read(&path).map_err(|err| AppError::internal(format!("read file: {err}")))?;
        if crate::files::content_revision(&current) != expected_revision {
            return Err(AppError::StaleRevision);
        }
    }
    fs::write(&path, content).map_err(|err| AppError::internal(format!("write file: {err}")))?;
    let abs = path.to_string_lossy().into_owned();
    Ok((crate::files::content_revision(content.as_bytes()), abs))
}

/// Delete a file or empty directory under `root`. Returns the absolute path
/// deleted (for on-write / fswatch suppression).
fn delete_path(root: &Path, rel_path: &str) -> Result<String, AppError> {
    let path = Manager::safe_path(root, rel_path)?;
    // Never allow deleting the workspace root itself.
    let root_canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if path == root_canon {
        return Err(AppError::validation("cannot delete workspace root"));
    }
    let metadata = fs::symlink_metadata(&path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("path not found: {rel_path}"))
        } else {
            AppError::internal(format!("stat path: {err}"))
        }
    })?;
    if metadata.is_dir() {
        let mut entries = fs::read_dir(&path)
            .map_err(|err| AppError::internal(format!("read directory: {err}")))?;
        if entries.next().is_some() {
            return Err(AppError::validation(
                "directory is not empty; delete contents first",
            ));
        }
        fs::remove_dir(&path)
            .map_err(|err| AppError::internal(format!("remove directory: {err}")))?;
    } else if metadata.file_type().is_file() {
        fs::remove_file(&path).map_err(|err| AppError::internal(format!("remove file: {err}")))?;
    } else {
        return Err(AppError::validation(
            "path is not a regular file or directory",
        ));
    }
    Ok(path.to_string_lossy().into_owned())
}

/// Rename/move within the workspace. Destination must not already exist
/// (overwrite rejected → 409 Conflict).
fn rename_path(root: &Path, from: &str, to: &str) -> Result<(String, String), AppError> {
    let from_path = Manager::safe_path(root, from)?;
    let to_path = Manager::safe_path(root, to)?;
    if from_path == to_path {
        return Ok((
            from_path.to_string_lossy().into_owned(),
            to_path.to_string_lossy().into_owned(),
        ));
    }
    fs::symlink_metadata(&from_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("path not found: {from}"))
        } else {
            AppError::internal(format!("stat source: {err}"))
        }
    })?;
    if fs::symlink_metadata(&to_path).is_ok() {
        return Err(AppError::conflict(format!(
            "destination already exists: {to}"
        )));
    }
    if let Some(parent) = to_path.parent() {
        if !parent.exists() {
            return Err(AppError::validation(
                "destination parent directory does not exist",
            ));
        }
    }
    fs::rename(&from_path, &to_path)
        .map_err(|err| AppError::internal(format!("rename path: {err}")))?;
    Ok((
        from_path.to_string_lossy().into_owned(),
        to_path.to_string_lossy().into_owned(),
    ))
}

/// Create a directory and any missing parents (`create_dir_all`). Idempotent
/// when the path already exists as a directory; 409 if it exists as a file.
fn mkdir_path(root: &Path, rel_path: &str) -> Result<String, AppError> {
    let path = Manager::safe_path(root, rel_path)?;
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_dir() => Ok(path.to_string_lossy().into_owned()),
        Ok(_) => Err(AppError::conflict(format!(
            "path exists as a file: {rel_path}"
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&path)
                .map_err(|err| AppError::internal(format!("create directory: {err}")))?;
            Ok(path.to_string_lossy().into_owned())
        }
        Err(err) => Err(AppError::internal(format!("stat path: {err}"))),
    }
}

fn build_tree(
    root: &Path,
    rel: &Path,
    depth: usize,
    node_count: &mut usize,
) -> Result<Vec<FileNode>, AppError> {
    if depth >= MAX_FILE_TREE_DEPTH {
        return Ok(Vec::new());
    }
    let directory = root.join(rel);
    let entries = fs::read_dir(&directory).map_err(|err| {
        AppError::internal(format!("read directory {}: {err}", directory.display()))
    })?;
    let mut nodes = Vec::new();
    for entry in entries {
        if *node_count >= MAX_FILE_TREE_NODES {
            break;
        }
        let entry =
            entry.map_err(|err| AppError::internal(format!("read directory entry: {err}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry
            .file_type()
            .map_err(|err| AppError::internal(format!("stat directory entry: {err}")))?;
        if name.starts_with('.')
            || file_type.is_symlink()
            || (file_type.is_dir() && is_noise_dir(&name))
        {
            continue;
        }
        *node_count += 1;
        let child_rel = rel.join(&name);
        let is_dir = file_type.is_dir();
        let children = if is_dir {
            build_tree(root, &child_rel, depth + 1, node_count)?
        } else {
            Vec::new()
        };
        nodes.push(FileNode {
            name,
            node_type: if is_dir {
                FILE_NODE_TYPE_FOLDER
            } else {
                FILE_NODE_TYPE_FILE
            }
            .to_string(),
            path: child_rel.to_string_lossy().into_owned(),
            children,
        });
    }
    nodes.sort_by(|left, right| {
        (left.node_type != FILE_NODE_TYPE_FOLDER)
            .cmp(&(right.node_type != FILE_NODE_TYPE_FOLDER))
            .then(left.name.cmp(&right.name))
    });
    Ok(nodes)
}

fn extension(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or_default()
}

fn is_binary_preview(path: &str) -> bool {
    matches!(
        extension(path).to_ascii_lowercase().as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "bmp"
            | "ico"
            | "avif"
            | "tiff"
            | "tif"
            | "heic"
            | "heif"
            | "pdf"
            | "docx"
            | "xlsx"
            | "epub"
            | "mp4"
            | "webm"
            | "ogv"
            | "mov"
            | "mkv"
            | "mp3"
            | "wav"
            | "oga"
            | "ogg"
            | "flac"
            | "m4a"
            | "aac"
            | "opus"
            | "stl"
            | "glb"
            | "ply"
            | "step"
            | "stp"
    )
}

fn is_previewable(path: &str) -> bool {
    is_binary_preview(path)
        || matches!(
            extension(path).to_ascii_lowercase().as_str(),
            "svg" | "obj" | "gltf" | "3mf" | "dae" | "wrl" | "vrml" | "csv" | "html" | "htm"
        )
}

fn is_noise_dir(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "node_modules"
            | "vendor"
            | "dist"
            | "build"
            | "target"
            | "bin"
            | "obj"
            | ".next"
            | ".nuxt"
            | ".output"
            | ".turbo"
            | ".gradle"
            | "__pycache__"
            | ".pytest_cache"
            | "coverage"
            | "tmp"
            | "cache"
    )
}

#[cfg(test)]
mod tests;
