//! Prompt context pipeline: first-prompt workspace context and live editor context.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, RwLock};

use chrono::Local;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use super::conversation::{ConversationTransfer, TransferQueue};
use super::profile::ProfileMiddleware;
use crate::interfaces::{AppError, FileNode, WorkspaceManager, FILE_NODE_TYPE_FILE};

const CONTEXT_MIME_TYPE: &str = "text/markdown";
const MAX_CONTEXT_FILES: usize = 200;
const MAX_FILE_TREE_DEPTH: usize = 3;
const MAX_CONTEXT_BYTES: usize = 8 * 1024;
const MAX_OPEN_FILES: usize = 20;
const MAX_RECENT_EDITS: usize = 10;
const MAX_OPEN_FILE_BYTES: usize = 32 * 1024;
const MAX_OPEN_FILES_TOTAL_BYTES: usize = 128 * 1024;

/// Structured context emitted as ACP `ContentBlock::Resource` when supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextResource {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
    pub text: String,
}

/// Output from the prompt pipeline before it is translated to ACP content blocks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreparedPrompt {
    /// Prompt text, including fallback context but not the user's original text.
    pub prefix: String,
    /// Resources sent separately when `embeddedContext` was negotiated.
    pub resources: Vec<ContextResource>,
}

impl PreparedPrompt {
    /// Adds the user's request after injected context with a stable separator.
    #[must_use]
    pub fn with_user_text(&self, user_text: &str) -> String {
        if self.prefix.is_empty() {
            user_text.to_string()
        } else {
            format!("{}\n\n---\n\n{}", self.prefix, user_text)
        }
    }
}

/// Frontend-reported editor state used by the prompt middlewares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorSelection {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

/// Concurrent tracker for open files, recent edits, and a selected range.
#[derive(Default)]
pub struct OpenFilesTracker {
    open_files: RwLock<Vec<String>>,
    recent_edits: RwLock<Vec<String>>,
    selection: RwLock<EditorSelection>,
}

impl OpenFilesTracker {
    /// Replaces the active editor file paths.
    pub fn set_open_files(&self, paths: Vec<String>) -> Result<(), AppError> {
        *self
            .open_files
            .write()
            .map_err(|_| AppError::internal("open-files tracker lock poisoned"))? = paths;
        Ok(())
    }

    /// Replaces recently edited paths.
    pub fn set_recent_edits(&self, paths: Vec<String>) -> Result<(), AppError> {
        *self
            .recent_edits
            .write()
            .map_err(|_| AppError::internal("recent-edits tracker lock poisoned"))? = paths;
        Ok(())
    }

    /// Replaces the selected editor range.
    pub fn set_selection(&self, selection: EditorSelection) -> Result<(), AppError> {
        *self
            .selection
            .write()
            .map_err(|_| AppError::internal("open-files selection lock poisoned"))? = selection;
        Ok(())
    }

    fn open_files(&self) -> Result<Vec<String>, AppError> {
        self.open_files
            .read()
            .map(|paths| paths.clone())
            .map_err(|_| AppError::internal("open-files tracker lock poisoned"))
    }

    fn recent_edits(&self) -> Result<Vec<String>, AppError> {
        self.recent_edits
            .read()
            .map(|paths| paths.clone())
            .map_err(|_| AppError::internal("recent-edits tracker lock poisoned"))
    }

    fn selection(&self) -> Result<EditorSelection, AppError> {
        self.selection
            .read()
            .map(|selection| selection.clone())
            .map_err(|_| AppError::internal("open-files selection lock poisoned"))
    }
}

/// Stateful pipeline shared by all ACP sessions.
pub struct PromptPipeline {
    counts: Mutex<HashMap<String, usize>>,
    pub tracker: OpenFilesTracker,
    pub profiles: ProfileMiddleware,
    pub transfers: TransferQueue,
}

impl Default for PromptPipeline {
    fn default() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
            tracker: OpenFilesTracker::default(),
            profiles: ProfileMiddleware::default(),
            transfers: TransferQueue::default(),
        }
    }
}

impl PromptPipeline {
    /// Clears first-prompt state after the transport has been rebound.
    pub fn reset(&self, session_id: &str) {
        if let Ok(mut counts) = self.counts.lock() {
            counts.remove(session_id);
        }
    }

    /// Drops all per-session prompt middleware state on close.
    pub fn clear(&self, session_id: &str) {
        self.reset(session_id);
        self.profiles.clear(session_id);
        self.transfers.clear(session_id);
    }

    /// Queues the durable transcript for a first prompt after rebind.
    pub fn queue_transfer(
        &self,
        session_id: String,
        transfer: ConversationTransfer,
    ) -> Result<(), AppError> {
        self.transfers.insert(session_id, transfer)
    }

    /// Builds context. Workspace/git failures are logged and omitted so an
    /// unavailable optional context source never prevents a user prompt.
    pub async fn prepare(
        &self,
        session_id: &str,
        workspace_id: &str,
        workspace_path: &Path,
        embedded_context: bool,
        workspace: &dyn WorkspaceManager,
    ) -> Result<PreparedPrompt, AppError> {
        let prompt_count = *self
            .counts
            .lock()
            .map_err(|_| AppError::internal("prompt pipeline lock poisoned"))?
            .get(session_id)
            .unwrap_or(&0);
        let mut text_sections = Vec::new();
        let mut resources = Vec::new();

        if prompt_count == 0 {
            let first_resources =
                first_prompt_resources(workspace_id, workspace_path, workspace).await;
            if embedded_context {
                resources.extend(first_resources);
            } else {
                text_sections.extend(first_resources.into_iter().map(render_resource));
            }
            if let Some(transfer) = self
                .transfers
                .take_for_first_prompt(session_id, prompt_count)?
            {
                text_sections.push(format!(
                    "## Previous Conversation (transferred from {})\n\n{}",
                    transfer.from_agent_name, transfer.markdown
                ));
            }
        }

        text_sections.push(format!("## Current Time\n\n{}", Local::now().to_rfc3339()));
        append_paths(
            &mut text_sections,
            "## Open Files",
            self.tracker.open_files()?,
            MAX_OPEN_FILES,
        );
        append_paths(
            &mut text_sections,
            "## Recently Edited Files",
            self.tracker.recent_edits()?,
            MAX_RECENT_EDITS,
        );

        let profile = self.profiles.instructions(session_id)?;
        if embedded_context {
            resources.push(ContextResource {
                name: "Profile Instructions".to_string(),
                uri: "context://profile".to_string(),
                mime_type: CONTEXT_MIME_TYPE.to_string(),
                text: profile,
            });
            resources.extend(
                open_file_resources(&self.tracker, workspace_id, workspace_path, workspace).await,
            );
        } else {
            text_sections.push(profile);
        }

        self.counts
            .lock()
            .map_err(|_| AppError::internal("prompt pipeline lock poisoned"))?
            .insert(session_id.to_string(), prompt_count + 1);
        Ok(PreparedPrompt {
            prefix: text_sections.join("\n\n---\n\n"),
            resources,
        })
    }
}

async fn first_prompt_resources(
    workspace_id: &str,
    workspace_path: &Path,
    workspace: &dyn WorkspaceManager,
) -> Vec<ContextResource> {
    let mut body = format!(
        "## Workspace Context\n\n- Workspace root: {}\n- Platform: {}/{}",
        workspace_path.display(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    match workspace.file_tree(workspace_id).await {
        Ok(nodes) => {
            let mut paths = Vec::new();
            flatten_nodes(&nodes, 0, &mut paths);
            paths.truncate(MAX_CONTEXT_FILES);
            if !paths.is_empty() {
                body.push_str(&format!(
                    "\n\n## Files (first {}, depth ≤ {MAX_FILE_TREE_DEPTH})\n\n{}",
                    paths.len(),
                    paths.join("\n")
                ));
            }
        }
        Err(error) => {
            tracing::warn!(workspace_id, error = %error, "workspace context file tree unavailable")
        }
    }
    if let Some(status) = git_status(workspace_path).await {
        body.push_str(&format!("\n\n## Git\n\n```text\n{status}\n```"));
    }
    truncate_string(&mut body, MAX_CONTEXT_BYTES);
    let mut resources = vec![ContextResource {
        name: "Workspace Context".to_string(),
        uri: "context://workspace".to_string(),
        mime_type: CONTEXT_MIME_TYPE.to_string(),
        text: body,
    }];
    match workspace.read_file(workspace_id, "AGENTS.md").await {
        Ok(file) if !file.is_binary && !file.content.is_empty() => {
            let mut text = file.content;
            truncate_string(&mut text, MAX_CONTEXT_BYTES);
            resources.push(ContextResource {
                name: "AGENTS.md".to_string(),
                uri: format!("file://{}", workspace_path.join("AGENTS.md").display()),
                mime_type: CONTEXT_MIME_TYPE.to_string(),
                text: format!("## AGENTS.md\n\n{text}"),
            });
        }
        Ok(_) => {}
        Err(error) => {
            tracing::debug!(workspace_id, error = %error, "AGENTS.md context unavailable")
        }
    }
    resources
}

async fn git_status(workspace_path: &Path) -> Option<String> {
    let output = timeout(
        Duration::from_secs(2),
        Command::new("git")
            .arg("-C")
            .arg(workspace_path)
            .args(["status", "--short", "-b"])
            .output(),
    )
    .await;
    match output {
        Ok(Ok(output)) if output.status.success() => String::from_utf8(output.stdout)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty()),
        Ok(Ok(_)) => None,
        Ok(Err(error)) => {
            tracing::debug!(error = %error, "git context command unavailable");
            None
        }
        Err(_) => {
            tracing::debug!("git context command timed out");
            None
        }
    }
}

fn flatten_nodes(nodes: &[FileNode], depth: usize, paths: &mut Vec<String>) {
    for node in nodes {
        if node.node_type == FILE_NODE_TYPE_FILE {
            paths.push(node.path.clone());
        } else if depth < MAX_FILE_TREE_DEPTH {
            flatten_nodes(&node.children, depth + 1, paths);
        }
    }
}

fn append_paths(sections: &mut Vec<String>, heading: &str, mut paths: Vec<String>, limit: usize) {
    paths.truncate(limit);
    if !paths.is_empty() {
        sections.push(format!("{heading}\n\n- {}", paths.join("\n- ")));
    }
}

async fn open_file_resources(
    tracker: &OpenFilesTracker,
    workspace_id: &str,
    workspace_path: &Path,
    workspace: &dyn WorkspaceManager,
) -> Vec<ContextResource> {
    let Ok(paths) = tracker.open_files() else {
        return Vec::new();
    };
    let mut total = 0;
    let mut resources = Vec::new();
    for path in paths.into_iter().take(MAX_OPEN_FILES) {
        let Ok(file) = workspace.read_file(workspace_id, &path).await else {
            continue;
        };
        if file.is_binary || file.content.is_empty() {
            continue;
        }
        let mut text = file.content;
        truncate_string(&mut text, MAX_OPEN_FILE_BYTES);
        total += text.len();
        resources.push(ContextResource {
            name: path.clone(),
            uri: format!("file://{}", workspace_path.join(&path).display()),
            mime_type: mime_by_extension(&path).to_string(),
            text,
        });
        if total >= MAX_OPEN_FILES_TOTAL_BYTES {
            break;
        }
    }
    let Ok(selection) = tracker.selection() else {
        return resources;
    };
    if !selection.text.is_empty() {
        resources.push(ContextResource {
            name: format!(
                "{}:{}-{}",
                selection.path, selection.start_line, selection.end_line
            ),
            uri: format!(
                "file://{}#L{}-L{}",
                workspace_path.join(&selection.path).display(),
                selection.start_line,
                selection.end_line
            ),
            mime_type: mime_by_extension(&selection.path).to_string(),
            text: selection.text,
        });
    }
    resources
}

fn render_resource(resource: ContextResource) -> String {
    resource.text
}

fn truncate_string(text: &mut String, max_bytes: usize) {
    if text.len() > max_bytes {
        let mut end = max_bytes;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}

fn mime_by_extension(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "ts" | "tsx" => "text/typescript",
        "js" | "jsx" => "text/javascript",
        "py" => "text/x-python",
        "json" => "application/json",
        "md" => "text/markdown",
        "yaml" | "yml" => "text/yaml",
        "html" => "text/html",
        "css" => "text/css",
        _ => "text/plain",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::PromptPipeline;
    use crate::interfaces::{
        AppError, FileNode, ReadFileResult, SearchOptions, SearchResult, WorkspaceInfo,
        WorkspaceManager,
    };

    struct EmptyWorkspace;

    #[async_trait]
    impl WorkspaceManager for EmptyWorkspace {
        async fn register(&self, _path: &str) -> Result<WorkspaceInfo, AppError> {
            Err(AppError::unsupported("not needed"))
        }
        async fn list(&self) -> Result<Vec<WorkspaceInfo>, AppError> {
            Ok(Vec::new())
        }
        async fn remove(&self, _id: &str) -> Result<(), AppError> {
            Err(AppError::unsupported("not needed"))
        }
        async fn file_tree(&self, _id: &str) -> Result<Vec<FileNode>, AppError> {
            Ok(Vec::new())
        }
        async fn read_file(&self, _id: &str, _path: &str) -> Result<ReadFileResult, AppError> {
            Err(AppError::not_found("file"))
        }
        async fn file_path(&self, _id: &str, _path: &str) -> Result<String, AppError> {
            Err(AppError::not_found("file"))
        }
        async fn write_file(
            &self,
            _id: &str,
            _path: &str,
            _content: &str,
            _revision: i64,
        ) -> Result<i64, AppError> {
            Err(AppError::unsupported("not needed"))
        }
        async fn search(
            &self,
            _id: &str,
            _pattern: &str,
            _opts: SearchOptions,
        ) -> Result<Vec<SearchResult>, AppError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn pipeline_assembles_text_fallback_in_order() {
        let pipeline = PromptPipeline::default();
        pipeline
            .tracker
            .set_open_files(vec!["src/main.rs".to_string()])
            .expect("set open files");
        pipeline
            .tracker
            .set_recent_edits(vec!["Cargo.toml".to_string()])
            .expect("set recent edits");
        let workspace = Arc::new(EmptyWorkspace);

        let prepared = pipeline
            .prepare(
                "session",
                "workspace",
                Path::new("/tmp"),
                false,
                workspace.as_ref(),
            )
            .await
            .expect("prepare prompt");
        assert!(prepared.prefix.contains("## Workspace Context"));
        assert!(prepared.prefix.contains("## Current Time"));
        assert!(prepared.prefix.contains("## Open Files"));
        assert!(prepared.prefix.contains("## Recently Edited Files"));
        assert!(prepared.prefix.contains("## Active Profile: Code"));
        assert!(prepared.resources.is_empty());
        assert!(prepared
            .with_user_text("hello")
            .ends_with("\n\n---\n\nhello"));
    }
}
