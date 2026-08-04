//! Workspace git operations — read-only repo detection (S-GIT-DETECT) plus
//! the status/diff/stage/unstage/commit/push API surface (S-GIT-API).
//!
//! Backed by [`gix`] (pure-Rust git, no libgit2 C dependency). Every entry
//! point takes an already-validated, canonical workspace root path from the
//! [`WorkspaceManager`](crate::interfaces::WorkspaceManager) trait so path
//! containment and symlink rejection stay enforced by the existing workspace
//! policy — this module never re-derives a root from a client-supplied path.
//!
//! Security notes:
//! - `push` is the only operation that shells out to the `git` CLI, because
//!   `gix` lacks a credential-aware transport. The daemon never stores or
//!   proxies git credentials; `push` inherits the agent process environment
//!   (SSH agent, credential helper, `GIT_ASKPASS`).
//! - Diff output is bounded per file (`MAX_DIFF_BYTES`) to prevent a huge
//!   generated file from exhausting daemon memory or the LAN response budget.
//! - Symlinks inside `.git/` are rejected up front by `open_repo`, matching
//!   the workspace symlink policy.

mod cli;
mod history;
mod repo;
mod types;
mod worktree;

pub use history::{log, MAX_LOG_LIMIT};
pub use repo::{detect, status};
pub use types::{
    CommitAuthor, CommitDiffFile, CommitDiffResult, DiffResult, FileStatus, GitError, GitRepoInfo,
    LogCommit, LogResult, StatusResult,
};
pub use worktree::{
    add_to_gitignore, checkout, commit, commit_diff, diff, discard, fetch, init, pull, push, stage,
    unstage, MAX_DIFF_BYTES,
};

#[cfg(test)]
mod tests;
