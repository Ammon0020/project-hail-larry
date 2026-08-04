use std::path::Path;
use std::process::Command;

use super::*;

/// Build a throwaway repo with `git init` + an initial commit so the
/// detection + status probes have real state to read. Production code
/// never shells out — only the test fixture does.
fn fresh_repo(dir: &Path) {
    std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .arg("-b")
        .arg("main")
        .current_dir(dir)
        .status()
        .expect("git init");
    // Disable autocrlf so Windows checkout preserves LF in the working tree,
    // matching the content the tests write and expect. Without this, Windows
    // git converts LF to CRLF on checkout and the discard/restore assertions
    // fail with left="hello\r\n" vs right="hello\n".
    std::process::Command::new("git")
        .args(["config", "core.autocrlf", "false"])
        .current_dir(dir)
        .status()
        .expect("git config core.autocrlf");
    std::fs::write(dir.join("README.md"), "hello\n").expect("write");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .expect("git add");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "init",
        ])
        .current_dir(dir)
        .status()
        .expect("git commit");
}

#[test]
fn detect_returns_no_repo_for_plain_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let info = detect(dir.path()).expect("detect");
    assert!(!info.repo_detected);
    assert_eq!(info.head_branch, None);
    assert_eq!(info.head_oid, None);
    assert!(!info.is_shallow);
}

#[test]
fn detect_reports_branch_and_oid_for_real_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let info = detect(dir.path()).expect("detect");
    assert!(info.repo_detected);
    assert_eq!(info.head_branch.as_deref(), Some("main"));
    assert!(info.head_oid.is_some());
    assert!(!info.is_shallow);
    assert!(!info.has_uncommitted_changes);
}

#[test]
fn detect_flags_uncommitted_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    let info = detect(dir.path()).expect("detect");
    assert!(info.has_uncommitted_changes);
}

#[test]
#[cfg(unix)]
fn detect_rejects_symlinked_git_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = tempfile::tempdir().expect("tempdir");
    fresh_repo(real.path());
    std::os::unix::fs::symlink(real.path().join(".git"), dir.path().join(".git")).expect("symlink");
    let err = detect(dir.path()).expect_err("should reject symlinked .git");
    assert!(matches!(err, GitError::SymlinkedGitDir));
}

#[test]
fn status_returns_empty_for_clean_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    assert!(status(dir.path()).expect("status").files.is_empty());
}

#[test]
fn status_lists_modified_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    let files = status(dir.path()).expect("status").files;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "README.md");
    assert!(!files[0].staged);
}

#[test]
fn status_expands_untracked_directory_into_individual_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    std::fs::create_dir_all(dir.path().join("group")).expect("mkdir");
    std::fs::write(dir.path().join("group/a.txt"), "a\n").expect("write");
    std::fs::write(dir.path().join("group/b.txt"), "b\n").expect("write");
    let files = status(dir.path()).expect("status").files;
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.contains(&"group/a.txt"),
        "expected group/a.txt, got {paths:?}"
    );
    assert!(
        paths.contains(&"group/b.txt"),
        "expected group/b.txt, got {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with('/')),
        "no collapsed folder entries, got {paths:?}"
    );
}

#[test]
fn status_lists_staged_file_after_add() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(dir.path())
        .status()
        .expect("git add");
    assert!(status(dir.path()).expect("status").files[0].staged);
}

#[test]
fn diff_returns_base_and_head_for_modified_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    let result = diff(dir.path(), "README.md", false).expect("diff");
    assert!(!result.base.is_empty());
    assert!(!result.head.is_empty());
}

#[test]
fn stage_then_status_shows_staged() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    stage(dir.path(), &[String::from("README.md")]).expect("stage");
    assert!(status(dir.path())
        .expect("status")
        .files
        .iter()
        .any(|file| file.path == "README.md" && file.staged));
}

#[test]
fn commit_creates_new_oid() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let old_oid = detect(dir.path())
        .expect("detect")
        .head_oid
        .expect("head oid");
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    stage(dir.path(), &[String::from("README.md")]).expect("stage");
    let oid = commit(dir.path(), "change", Some(&old_oid), false).expect("commit");
    assert_ne!(oid, old_oid);
}

#[test]
fn commit_rejects_stale_expected_head() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    stage(dir.path(), &[String::from("README.md")]).expect("stage");
    assert!(matches!(
        commit(dir.path(), "change", Some("0000000"), false),
        Err(GitError::Operation(_))
    ));
}

#[test]
fn commit_allows_initial_commit_without_precondition() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Unborn repo: `git init` only, no initial commit, then stage a file.
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir.path())
        .status()
        .expect("git init");
    std::fs::write(dir.path().join("README.md"), "first\n").expect("write");
    stage(dir.path(), &[String::from("README.md")]).expect("stage");
    let oid = commit(dir.path(), "initial", None, false).expect("initial commit");
    assert!(!oid.is_empty());
    // A missing precondition against a born HEAD must still be rejected.
    std::fs::write(dir.path().join("README.md"), "second\n").expect("write");
    stage(dir.path(), &[String::from("README.md")]).expect("stage");
    assert!(matches!(
        commit(dir.path(), "second", None, false),
        Err(GitError::Operation(_))
    ));
}

#[test]
fn init_refuses_existing_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    assert!(init(dir.path()).is_err());
}

#[test]
fn init_creates_repo_in_plain_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(!init(dir.path()).expect("init").is_empty());
    assert!(detect(dir.path()).expect("detect").repo_detected);
}

#[test]
fn gitignore_creates_file_when_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let added = add_to_gitignore(dir.path(), &["target/".into()]).expect("add");
    let file = dir.path().join(".gitignore");
    assert!(file.exists(), ".gitignore should be created");
    let content = std::fs::read_to_string(file).expect("read");
    assert!(
        content.lines().any(|line| line == "target/"),
        "content: {content}"
    );
    assert_eq!(added, vec!["target/".to_string()]);
}

#[test]
fn gitignore_dedups_existing_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let file = dir.path().join(".gitignore");
    std::fs::write(&file, "target/\n").expect("write");
    let added = add_to_gitignore(dir.path(), &["target/".into()]).expect("add");
    assert!(added.is_empty(), "no new patterns: {added:?}");
    assert_eq!(std::fs::read_to_string(file).expect("read"), "target/\n");
}

#[test]
fn gitignore_appends_new_patterns() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let file = dir.path().join(".gitignore");
    std::fs::write(&file, "target/\n").expect("write");
    let added = add_to_gitignore(dir.path(), &["node_modules/".into()]).expect("add");
    let content = std::fs::read_to_string(file).expect("read");
    let lines: Vec<&str> = content.lines().collect();
    assert!(lines.contains(&"target/"), "content: {content}");
    assert!(lines.contains(&"node_modules/"), "content: {content}");
    assert_eq!(added, vec!["node_modules/".to_string()]);
}

/// Create an additional commit on top of `fresh_repo`'s initial commit.
/// Writes a unique file so each commit has a distinct tree.
fn add_commit(dir: &Path, name: &str, message: &str) {
    std::fs::write(dir.join(name), format!("{name}\n")).expect("write");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .expect("git add");
    std::process::Command::new("git")
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            message,
        ])
        .current_dir(dir)
        .status()
        .expect("git commit");
}

#[test]
fn log_returns_not_a_repo_for_plain_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err = log(dir.path(), 100, 0).expect_err("should error");
    assert!(matches!(err, GitError::NotARepo));
}

#[test]
fn log_returns_empty_for_unborn_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    // `git init` only — no commits, so HEAD is unborn.
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir.path())
        .status()
        .expect("git init");
    let result = log(dir.path(), 100, 0).expect("log");
    assert!(result.commits.is_empty());
    assert_eq!(result.total, 0);
    assert!(!result.has_more);
}

#[test]
fn log_returns_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let result = log(dir.path(), 100, 0).expect("log");
    assert_eq!(result.commits.len(), 1, "one commit in fresh repo");
    assert_eq!(result.total, 1);
    assert!(!result.has_more);

    let commit = &result.commits[0];
    assert!(!commit.oid.is_empty());
    assert!(commit.parents.is_empty(), "initial commit has no parents");
    assert_eq!(commit.message, "init");
    assert!(commit.is_head, "the only commit is HEAD");
    assert_eq!(commit.author.name, "t");
    assert_eq!(commit.author.email, "t@t");
    // ISO 8601 UTC ends with 'Z'.
    assert!(
        commit.author.time.ends_with('Z'),
        "time: {}",
        commit.author.time
    );
    // The default branch is `main` (from `fresh_repo`).
    assert!(
        commit.branch_labels.iter().any(|l| l == "main"),
        "branch_labels: {:?}",
        commit.branch_labels
    );
}

#[test]
fn log_paginates_with_limit_offset() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    add_commit(dir.path(), "a.txt", "second");
    add_commit(dir.path(), "b.txt", "third");

    // 3 commits total. Page 1: limit=2, offset=0 → 2 commits, has_more.
    let page1 = log(dir.path(), 2, 0).expect("log page 1");
    assert_eq!(page1.commits.len(), 2);
    assert_eq!(page1.total, 3);
    assert!(page1.has_more);

    // Page 2: limit=2, offset=2 → 1 commit, no has_more.
    let page2 = log(dir.path(), 2, 2).expect("log page 2");
    assert_eq!(page2.commits.len(), 1);
    assert_eq!(page2.total, 3);
    assert!(!page2.has_more);
}

#[test]
fn log_caps_limit_at_max() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    // Request limit=1000 — should be clamped to MAX_LOG_LIMIT (200) without
    // panicking. With only 1 commit, the result has 1 entry.
    let result = log(dir.path(), 1000, 0).expect("log");
    assert_eq!(result.commits.len(), 1);
    assert_eq!(result.total, 1);
}

#[test]
fn log_attaches_branch_labels_and_head_marker() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    add_commit(dir.path(), "a.txt", "second");

    let result = log(dir.path(), 100, 0).expect("log");
    // Newest-first ordering: the second commit is HEAD.
    let head = &result.commits[0];
    assert!(head.is_head);
    assert!(
        head.branch_labels.iter().any(|l| l == "main"),
        "HEAD branch_labels: {:?}",
        head.branch_labels
    );
    // The initial commit is not HEAD.
    let init = &result.commits[1];
    assert!(!init.is_head);
    // `main` points at HEAD only, so the initial commit has no labels.
    assert!(
        init.branch_labels.is_empty(),
        "init branch_labels: {:?}",
        init.branch_labels
    );
    // The initial commit is the parent of the second.
    assert_eq!(head.parents.len(), 1);
    assert_eq!(head.parents[0], init.oid);
}

#[test]
fn log_reports_parent_oids() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    add_commit(dir.path(), "a.txt", "second");

    let result = log(dir.path(), 100, 0).expect("log");
    let head = &result.commits[0];
    let init = &result.commits[1];
    assert_eq!(head.parents, vec![init.oid.clone()]);
    assert!(init.parents.is_empty());
}

#[test]
fn discard_restores_modified_tracked_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let file = dir.path().join("README.md");
    std::fs::write(&file, "changed\n").expect("write");
    assert_eq!(
        discard(dir.path(), &["README.md".into()]).expect("discard"),
        1
    );
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "hello\n");
}

#[test]
fn discard_deletes_untracked_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let file = dir.path().join("new.txt");
    std::fs::write(&file, "untracked\n").expect("write");
    assert!(file.exists());
    assert_eq!(
        discard(dir.path(), &["new.txt".into()]).expect("discard"),
        1
    );
    assert!(!file.exists());
}

#[test]
fn discard_restores_deleted_tracked_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let file = dir.path().join("README.md");
    std::fs::remove_file(&file).expect("remove");
    assert!(!file.exists());
    assert_eq!(
        discard(dir.path(), &["README.md".into()]).expect("discard"),
        1
    );
    assert!(file.exists());
    assert_eq!(std::fs::read_to_string(&file).expect("read"), "hello\n");
}

#[test]
fn discard_rejects_path_escaping_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    let err = discard(dir.path(), &["../outside".into()]).expect_err("should reject");
    assert!(matches!(err, GitError::PathEscapes(_)));
}

#[test]
fn discard_handles_mixed_tracked_and_untracked() {
    let dir = tempfile::tempdir().expect("tempdir");
    fresh_repo(dir.path());
    // Modify a tracked file and create an untracked file.
    std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
    std::fs::write(dir.path().join("new.txt"), "untracked\n").expect("write");
    assert_eq!(
        discard(dir.path(), &["README.md".into(), "new.txt".into()]).expect("discard"),
        2
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).expect("read"),
        "hello\n"
    );
    assert!(!dir.path().join("new.txt").exists());
}
