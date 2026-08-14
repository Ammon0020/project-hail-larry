use std::path::Path;

use agent_client_protocol::schema::v1::{
    ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest, WriteTextFileResponse,
};

use super::super::events::append_payload;
use super::HandlerDeps;
use crate::interfaces::{AppError, EventPayload};

pub(in crate::acp::core) async fn read_text_file(
    deps: HandlerDeps,
    request: ReadTextFileRequest,
) -> Result<ReadTextFileResponse, AppError> {
    let path = workspace_relative_path(&deps.workspace_path, &request.path)?;
    deps.workspaces
        .read_file(&deps.workspace_id, &path)
        .await
        .map(|result| ReadTextFileResponse::new(result.content))
}

pub(in crate::acp::core) async fn write_text_file(
    deps: HandlerDeps,
    request: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, AppError> {
    let path = workspace_relative_path(&deps.workspace_path, &request.path)?;
    if request.content.contains('\0') {
        return Err(AppError::validation(
            "ACP text-file content must not contain NUL bytes",
        ));
    }
    // Keep the pre-read, write, and cache update ordered with other agent
    // writes/reverts. This makes repeated writes to one path capture the actual
    // prior agent version rather than racing with another callback.
    let _write_guard = deps.edited_files.lock_writes().await;
    let (content_before, expected_revision, existed_before) =
        match deps.workspaces.read_file(&deps.workspace_id, &path).await {
            Ok(result) if result.is_binary => {
                return Err(AppError::validation(
                    "cannot overwrite a binary file through the ACP text-file API",
                ));
            }
            // Workspace reads use lossy UTF-8 conversion for editor display.
            // A revision mismatch proves bytes were replaced during decoding,
            // so caching that string would make revert corrupt the original.
            Ok(result)
                if crate::files::content_revision(result.content.as_bytes()) != result.revision =>
            {
                return Err(AppError::validation(
                    "cannot overwrite a non-UTF-8 file through the ACP text-file API",
                ));
            }
            Ok(result) => (result.content, result.revision, true),
            // A missing file is a legitimate create. Other read failures must
            // fail loudly; treating permissions/I/O errors as an empty base
            // would make the revert action destructively wrong.
            Err(AppError::NotFound { .. }) => (String::new(), 0, false),
            Err(error) => return Err(error),
        };
    let written_revision = deps
        .workspaces
        .write_file(
            &deps.workspace_id,
            &path,
            &request.content,
            expected_revision,
        )
        .await?;
    // A newly-created path had no pre-read result to classify. Re-read only
    // that case so extension-classified binary previews (for example `.png`)
    // never get a misleading empty text diff/revert entry.
    let cacheable_new_file = existed_before
        || deps
            .workspaces
            .read_file(&deps.workspace_id, &path)
            .await
            .is_ok_and(|result| !result.is_binary && result.revision == written_revision);
    if !cacheable_new_file {
        tracing::warn!(
            session_id = %deps.local_session_id,
            workspace_id = %deps.workspace_id,
            path,
            "agent-created file is not cacheable as text; diff/revert unavailable"
        );
    } else if !deps.edited_files.record(
        &deps.local_session_id,
        &path,
        content_before,
        &request.content,
        existed_before,
        written_revision,
    ) {
        tracing::warn!(
            session_id = %deps.local_session_id,
            workspace_id = %deps.workspace_id,
            path,
            "agent write exceeded edited-file cache limits; diff/revert unavailable"
        );
    }
    // Broadcast FileWritten so the UI refreshes the explorer. App writes are
    // suppressed in fswatch (note_app_write) to avoid a duplicate
    // FileChangedOnDisk for the same change — without this event the tree
    // would stay stale for agent-created files.
    if let Err(error) = append_payload(
        &deps.event_bus,
        &deps.local_session_id,
        EventPayload::FileWritten {
            workspace_id: deps.workspace_id.clone(),
            target: path,
        },
    )
    .await
    {
        // File is already on disk; failing the ACP response would mislead the
        // agent. Log loudly so a broken event bus is still visible.
        tracing::error!(
            session_id = %deps.local_session_id,
            workspace_id = %deps.workspace_id,
            %error,
            "failed to publish FileWritten after agent write"
        );
    }
    Ok(WriteTextFileResponse::new())
}

fn workspace_relative_path(root: &Path, path: &Path) -> Result<String, AppError> {
    if path.is_absolute() {
        path_to_workspace_relative(root, path)
    } else {
        Ok(path.to_string_lossy().into_owned())
    }
}

fn path_to_workspace_relative(root: &Path, path: &Path) -> Result<String, AppError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| AppError::validation("agent path is outside the workspace"))?;
    Ok(relative.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::workspace_relative_path;

    #[test]
    fn workspace_relative_path_rejects_absolute_path_escapes() {
        #[cfg(unix)]
        let (root, outside) = (Path::new("/workspace"), Path::new("/outside/file"));
        #[cfg(windows)]
        let (root, outside) = (Path::new(r"C:\workspace"), Path::new(r"D:\outside\file"));
        let error = workspace_relative_path(root, outside)
            .expect_err("outside workspace path must be rejected");
        assert!(error.to_string().contains("outside the workspace"));
    }
}
