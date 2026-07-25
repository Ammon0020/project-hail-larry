//! Prompt context pipeline: first-prompt workspace context and live editor context.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};
use std::sync::{Mutex, RwLock};

use chrono::Local;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use super::conversation::{ConversationTransfer, TransferQueue};
use super::profile::ProfileMiddleware;
use crate::config::{PromptContextSettings, MAX_PROMPT_CONTEXT_PATHS};
use crate::interfaces::{AppError, FileNode, InjectedContext, WorkspaceManager};

const CONTEXT_MIME_TYPE: &str = "text/markdown";
const MAX_CONTEXT_BYTES: usize = 8 * 1024;
const MAX_SELECTION_BYTES: usize = 32 * 1024;

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

    /// Snapshot the additions that will accompany a user prompt.
    ///
    /// The actor persists this before the resources are moved into the ACP
    /// request, letting the client disclose exact daemon-provided context.
    pub(in crate::acp) fn injected_context(&self) -> Vec<InjectedContext> {
        let mut context =
            Vec::with_capacity(self.resources.len() + usize::from(!self.prefix.is_empty()));
        if !self.prefix.is_empty() {
            context.push(InjectedContext {
                name: "Prompt additions".to_string(),
                content: self.prefix.clone(),
            });
        }
        context.extend(self.resources.iter().map(|resource| InjectedContext {
            name: resource.name.clone(),
            content: resource.text.clone(),
        }));
        context
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

/// Per-session editor state (open files, recent edits, selection).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OpenFilesEntry {
    open_files: Vec<String>,
    recent_edits: Vec<String>,
    selection: EditorSelection,
}

/// Concurrent tracker for open files, recent edits, and a selected range,
/// keyed by session id so editor context cannot leak across sessions.
#[derive(Default)]
pub struct OpenFilesTracker {
    sessions: RwLock<HashMap<String, OpenFilesEntry>>,
}

impl OpenFilesTracker {
    fn entry(&self, session_id: &str) -> Result<Option<OpenFilesEntry>, AppError> {
        self.sessions
            .read()
            .map(|sessions| sessions.get(session_id).cloned())
            .map_err(|_| AppError::internal("open-files tracker lock poisoned"))
    }

    fn with_entry_mut(
        &self,
        session_id: &str,
        f: impl FnOnce(&mut OpenFilesEntry),
    ) -> Result<(), AppError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| AppError::internal("open-files tracker lock poisoned"))?;
        f(sessions.entry(session_id.to_string()).or_default());
        Ok(())
    }

    /// Replaces the active editor file paths for a session.
    pub fn set_open_files(&self, session_id: &str, paths: Vec<String>) -> Result<(), AppError> {
        self.with_entry_mut(session_id, |entry| entry.open_files = paths)
    }

    /// Replaces recently edited paths for a session.
    pub fn set_recent_edits(&self, session_id: &str, paths: Vec<String>) -> Result<(), AppError> {
        self.with_entry_mut(session_id, |entry| entry.recent_edits = paths)
    }

    /// Replaces the selected editor range for a session.
    pub fn set_selection(
        &self,
        session_id: &str,
        selection: EditorSelection,
    ) -> Result<(), AppError> {
        self.with_entry_mut(session_id, |entry| entry.selection = selection)
    }

    /// Drops per-session editor state when a session closes.
    pub fn clear(&self, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(session_id);
        }
    }

    fn open_files(&self, session_id: &str) -> Result<Vec<String>, AppError> {
        Ok(self
            .entry(session_id)?
            .map(|e| e.open_files)
            .unwrap_or_default())
    }

    fn recent_edits(&self, session_id: &str) -> Result<Vec<String>, AppError> {
        Ok(self
            .entry(session_id)?
            .map(|e| e.recent_edits)
            .unwrap_or_default())
    }

    fn selection(&self, session_id: &str) -> Result<EditorSelection, AppError> {
        Ok(self
            .entry(session_id)?
            .map(|e| e.selection)
            .unwrap_or_default())
    }
}

/// Stateful pipeline shared by all ACP sessions.
pub struct PromptPipeline {
    counts: Mutex<HashMap<String, usize>>,
    pub tracker: OpenFilesTracker,
    context_settings: RwLock<PromptContextSettings>,
    /// Shared so ACP session setup can read the tool whitelist (S-PROF-TOOLS).
    pub profiles: std::sync::Arc<ProfileMiddleware>,
    pub transfers: TransferQueue,
}

impl Default for PromptPipeline {
    fn default() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
            tracker: OpenFilesTracker::default(),
            context_settings: RwLock::new(PromptContextSettings::default()),
            profiles: std::sync::Arc::new(ProfileMiddleware::default()),
            transfers: TransferQueue::default(),
        }
    }
}

impl PromptPipeline {
    pub(in crate::acp) fn replace_context_settings(
        &self,
        settings: PromptContextSettings,
    ) -> Result<(), AppError> {
        *self
            .context_settings
            .write()
            .map_err(|_| AppError::internal("prompt context settings lock poisoned"))? = settings;
        Ok(())
    }

    fn context_settings(&self) -> Result<PromptContextSettings, AppError> {
        self.context_settings
            .read()
            .map(|settings| settings.clone())
            .map_err(|_| AppError::internal("prompt context settings lock poisoned"))
    }

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
        self.tracker.clear(session_id);
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
        let settings = self.context_settings()?;
        let mut text_sections = Vec::new();
        let mut resources = Vec::new();

        if prompt_count == 0 {
            let first_resources =
                first_prompt_resources(workspace_id, workspace_path, workspace, &settings).await;
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
        // Open and recently edited paths share one budget so the default of
        // ten means ten total automatic editor paths, not ten per section.
        let path_limit = settings.open_file_limit.min(MAX_PROMPT_CONTEXT_PATHS);
        let mut seen_paths = HashSet::new();
        let open_files = bounded_relative_paths(
            self.tracker.open_files(session_id)?,
            path_limit,
            &mut seen_paths,
        );
        let recent_edits = bounded_relative_paths(
            self.tracker.recent_edits(session_id)?,
            path_limit.saturating_sub(open_files.len()),
            &mut seen_paths,
        );
        append_paths(&mut text_sections, "## Open Files", open_files);
        append_paths(&mut text_sections, "## Recently Edited Files", recent_edits);

        let profile = self.profiles.instructions(session_id)?;
        if embedded_context {
            resources.push(ContextResource {
                name: "Profile Instructions".to_string(),
                uri: "context://profile".to_string(),
                mime_type: CONTEXT_MIME_TYPE.to_string(),
                text: profile,
            });
            if let Some(selection) = selection_resource(&self.tracker, session_id, workspace_path)?
            {
                resources.push(selection);
            }
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
    settings: &PromptContextSettings,
) -> Vec<ContextResource> {
    let mut body = format!(
        "## Workspace Context\n\n- Workspace root: {}\n- Platform: {}/{}",
        workspace_path.display(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    match workspace.file_tree(workspace_id).await {
        Ok(nodes) => {
            let paths = top_level_paths(&nodes, settings.workspace_file_list_limit);
            if !paths.is_empty() {
                body.push_str(&format!(
                    "\n\n## Workspace entries (first {}, top level only)\n\n{}",
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

fn top_level_paths(nodes: &[FileNode], limit: usize) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|node| top_level_path(&node.path))
        .take(limit.min(MAX_PROMPT_CONTEXT_PATHS))
        .collect()
}

fn bounded_relative_paths(
    paths: Vec<String>,
    limit: usize,
    seen: &mut HashSet<String>,
) -> Vec<String> {
    paths
        .into_iter()
        .filter_map(|path| relative_path(&path))
        .filter(|path| seen.insert(path.clone()))
        .take(limit)
        .collect()
}

fn append_paths(sections: &mut Vec<String>, heading: &str, paths: Vec<String>) {
    if !paths.is_empty() {
        sections.push(format!("{heading}\n\n- {}", paths.join("\n- ")));
    }
}

fn relative_path(path: &str) -> Option<String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path.to_string_lossy().to_string())
}

fn top_level_path(path: &str) -> Option<String> {
    let path = relative_path(path)?;
    let components = Path::new(&path).components().collect::<Vec<_>>();
    (components.len() == 1 && !matches!(components[0], Component::CurDir)).then_some(path)
}

fn selection_resource(
    tracker: &OpenFilesTracker,
    session_id: &str,
    workspace_path: &Path,
) -> Result<Option<ContextResource>, AppError> {
    let selection = tracker.selection(session_id)?;
    if !selection.text.is_empty() {
        let Some(path) = relative_path(&selection.path) else {
            return Ok(None);
        };
        let mut text = selection.text;
        truncate_string(&mut text, MAX_SELECTION_BYTES);
        return Ok(Some(ContextResource {
            name: format!("{}:{}-{}", path, selection.start_line, selection.end_line),
            uri: format!(
                "file://{}#L{}-L{}",
                workspace_path.join(&path).display(),
                selection.start_line,
                selection.end_line
            ),
            mime_type: mime_by_extension(&path).to_string(),
            text,
        }));
    }
    Ok(None)
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

    use super::{top_level_paths, PromptPipeline};
    use crate::config::PromptContextSettings;
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
            Err(AppError::not_found_kind("file"))
        }
        async fn file_path(&self, _id: &str, _path: &str) -> Result<String, AppError> {
            Err(AppError::not_found_kind("file"))
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
        async fn delete_path(&self, _id: &str, _path: &str) -> Result<(), AppError> {
            Err(AppError::unsupported("not needed"))
        }
        async fn rename_path(&self, _id: &str, _from: &str, _to: &str) -> Result<(), AppError> {
            Err(AppError::unsupported("not needed"))
        }
        async fn mkdir(&self, _id: &str, _path: &str) -> Result<(), AppError> {
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
            .set_open_files("session", vec!["src/main.rs".to_string()])
            .expect("set open files");
        pipeline
            .tracker
            .set_recent_edits("session", vec!["Cargo.toml".to_string()])
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
        assert_eq!(prepared.injected_context().len(), 1);
        assert_eq!(prepared.injected_context()[0].name, "Prompt additions");
        assert!(prepared.injected_context()[0]
            .content
            .contains("## Workspace Context"));
    }

    #[tokio::test]
    async fn pipeline_records_embedded_resources_for_prompt_inspection() {
        let pipeline = PromptPipeline::default();
        let workspace = Arc::new(EmptyWorkspace);

        let prepared = pipeline
            .prepare(
                "session",
                "workspace",
                Path::new("/tmp"),
                true,
                workspace.as_ref(),
            )
            .await
            .expect("prepare prompt");
        let context = prepared.injected_context();

        assert_eq!(context[0].name, "Prompt additions");
        assert!(context[0].content.contains("## Current Time"));
        assert!(context.iter().any(|item| item.name == "Workspace Context"));
        assert!(context
            .iter()
            .any(|item| item.name == "Profile Instructions"));
    }

    #[tokio::test]
    async fn open_file_context_is_relative_paths_only_and_respects_its_limit() {
        let pipeline = PromptPipeline::default();
        pipeline
            .replace_context_settings(PromptContextSettings {
                open_file_limit: 1,
                workspace_file_list_limit: 1,
            })
            .expect("set context settings");
        pipeline
            .tracker
            .set_open_files(
                "session",
                vec![
                    "demo1.html".to_string(),
                    "demo2.html".to_string(),
                    "/not-relative.html".to_string(),
                ],
            )
            .expect("set open files");
        let workspace = Arc::new(EmptyWorkspace);

        let prepared = pipeline
            .prepare(
                "session",
                "workspace",
                Path::new("/tmp"),
                true,
                workspace.as_ref(),
            )
            .await
            .expect("prepare prompt");

        assert!(prepared.prefix.contains("demo1.html"));
        assert!(!prepared.prefix.contains("demo2.html"));
        assert!(!prepared.prefix.contains("not-relative.html"));
        assert!(!prepared
            .resources
            .iter()
            .any(|resource| resource.name == "demo1.html"));
    }

    #[test]
    fn workspace_context_keeps_only_limited_top_level_entries() {
        let nodes = vec![
            FileNode {
                name: "demo1.html".to_string(),
                node_type: "file".to_string(),
                path: "demo1.html".to_string(),
                children: Vec::new(),
            },
            FileNode {
                name: "src".to_string(),
                node_type: "folder".to_string(),
                path: "src".to_string(),
                children: vec![FileNode {
                    name: "lib.rs".to_string(),
                    node_type: "file".to_string(),
                    path: "src/lib.rs".to_string(),
                    children: Vec::new(),
                }],
            },
            FileNode {
                name: "nested".to_string(),
                node_type: "file".to_string(),
                path: "nested/file.rs".to_string(),
                children: Vec::new(),
            },
        ];

        assert_eq!(top_level_paths(&nodes, 10), ["demo1.html", "src"]);
        assert_eq!(top_level_paths(&nodes, 1), ["demo1.html"]);
    }
}
