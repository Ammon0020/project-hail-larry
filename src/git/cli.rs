use std::path::Path;
use std::process::{Command, Output};

use super::GitError;

pub(super) fn git_output<const N: usize>(
    root: &Path,
    args: [&str; N],
) -> Result<Vec<u8>, GitError> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    Ok(run_git(command)?.stdout)
}

pub(super) fn git_file(root: &Path, spec: &str) -> Result<Vec<u8>, GitError> {
    let output = Command::new("git")
        .current_dir(root)
        .arg("show")
        .arg(spec)
        .output()
        .map_err(|err| GitError::Operation(err.to_string()))?;
    // A missing path is expected for additions/deletions.
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Ok(Vec::new())
    }
}

pub(super) fn run_git(mut command: Command) -> Result<Output, GitError> {
    let output = command
        .output()
        .map_err(|err| GitError::Operation(err.to_string()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitError::Operation(output_text(&output)))
    }
}

pub(super) fn configure_default_identity(command: &mut Command, root: &Path) {
    let has_email = Command::new("git")
        .current_dir(root)
        .args(["config", "user.email"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_email {
        command
            .env("GIT_AUTHOR_NAME", "Local Agent")
            .env("GIT_AUTHOR_EMAIL", "agent@local")
            .env("GIT_COMMITTER_NAME", "Local Agent")
            .env("GIT_COMMITTER_EMAIL", "agent@local");
    }
}

pub(super) fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
