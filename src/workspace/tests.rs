use std::fs;
use std::sync::Arc;

use tempfile::TempDir;

use super::Manager;
use crate::interfaces::{
    AppError, SearchOptions, WorkspaceManager as _, FILE_NODE_TYPE_FILE, FILE_NODE_TYPE_FOLDER,
};

fn manager() -> Manager {
    Manager::new()
}

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src/routes")).unwrap();
    fs::write(root.join("src/server.js"), "console.log('hello');").unwrap();
    fs::write(root.join("src/routes/index.js"), "// route").unwrap();
    fs::write(root.join("package.json"), r#"{"name":"test"}"#).unwrap();
    fs::write(root.join("README.md"), "# Test").unwrap();
    fs::write(root.join(".hidden"), "hidden").unwrap();
    fs::create_dir_all(root.join("node_modules")).unwrap();
    fs::write(root.join("node_modules/noise.js"), "TODO noise").unwrap();
    dir
}

#[tokio::test]
async fn register_list_and_remove_are_stable() {
    let manager = manager();
    let first = fixture();
    let second = TempDir::new().unwrap();
    let first_info = manager
        .register(&first.path().to_string_lossy())
        .await
        .unwrap();
    let second_info = manager
        .register(&second.path().to_string_lossy())
        .await
        .unwrap();
    assert_eq!(first_info.id.len(), 16);
    assert_eq!(
        first_info.name,
        first.path().file_name().unwrap().to_string_lossy()
    );

    let listed = manager.list().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed.windows(2).all(|pair| pair[0].name <= pair[1].name));
    manager.remove(&first_info.id).await.unwrap();
    assert!(matches!(
        manager.remove(&first_info.id).await,
        Err(AppError::NotFound { .. })
    ));
    assert!(manager.file_tree(&first_info.id).await.is_err());
    assert!(manager.remove(&second_info.id).await.is_ok());
}

#[tokio::test]
async fn register_rejects_file_and_missing_directory() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("file.txt");
    fs::write(&file, "data").unwrap();
    assert!(manager.register(&file.to_string_lossy()).await.is_err());
    assert!(manager
        .register("/workspace-definitely-does-not-exist")
        .await
        .is_err());
}

#[tokio::test]
async fn file_tree_sorts_and_filters_entries() {
    let manager = manager();
    let dir = fixture();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    let tree = manager.file_tree(&workspace.id).await.unwrap();

    assert_eq!(tree[0].name, "src");
    assert_eq!(tree[0].node_type, FILE_NODE_TYPE_FOLDER);
    assert!(tree
        .iter()
        .all(|node| node.name != ".hidden" && node.name != "node_modules"));
    assert!(tree
        .iter()
        .skip(1)
        .all(|node| node.node_type == FILE_NODE_TYPE_FILE));
    let routes = tree[0]
        .children
        .iter()
        .find(|node| node.name == "routes")
        .unwrap();
    assert_eq!(routes.node_type, FILE_NODE_TYPE_FOLDER);
    assert_eq!(
        routes.children[0].path.replace('\\', "/"),
        "src/routes/index.js"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn tree_skips_symlinks_and_bounds_deep_recursion() {
    use std::os::unix::fs::symlink;

    let manager = manager();
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("real.txt"), "data").unwrap();
    symlink(dir.path().join("real.txt"), dir.path().join("link.txt")).unwrap();
    let mut deep = dir.path().to_path_buf();
    for index in 0..22 {
        deep.push(format!("d{index}"));
        fs::create_dir(&deep).unwrap();
    }
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    let tree = manager.file_tree(&workspace.id).await.unwrap();
    assert!(tree.iter().all(|node| node.name != "link.txt"));
    let mut current = &tree;
    for _ in 0..20 {
        current = &current[0].children;
    }
    assert!(current.is_empty(), "tree must stop at its depth bound");
}

#[tokio::test]
async fn read_reports_text_binary_and_preview_behavior() {
    let manager = manager();
    let dir = fixture();
    fs::write(dir.path().join("image.png"), [0x89, b'P', b'N', b'G']).unwrap();
    fs::write(dir.path().join("blob.bin"), [b'x', 0, b'y']).unwrap();
    fs::write(dir.path().join("diagram.svg"), "<svg/>").unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();

    let text = manager
        .read_file(&workspace.id, "package.json")
        .await
        .unwrap();
    assert_eq!(text.content, r#"{"name":"test"}"#);
    assert!(!text.is_binary);
    assert_eq!(
        text.revision,
        crate::files::content_revision(br#"{"name":"test"}"#)
    );
    assert!(!text.previewable);

    let binary = manager.read_file(&workspace.id, "blob.bin").await.unwrap();
    assert!(binary.is_binary);
    assert!(binary.content.is_empty());
    assert_ne!(binary.revision, 0);
    assert!(!binary.previewable);

    let image = manager.read_file(&workspace.id, "image.png").await.unwrap();
    assert!(image.is_binary && image.previewable);
    let svg = manager
        .read_file(&workspace.id, "diagram.svg")
        .await
        .unwrap();
    assert!(!svg.is_binary && svg.previewable);
}

#[tokio::test]
async fn read_enforces_size_limit_without_loading_file() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let big = dir.path().join("big.txt");
    let file = fs::File::create(&big).unwrap();
    file.set_len(super::MAX_READ_FILE_SIZE + 1).unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    let error = manager
        .read_file(&workspace.id, "big.txt")
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Validation(message) if message.contains("too large")));
}

#[tokio::test]
async fn file_path_enforces_size_limit() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let big = dir.path().join("big.bin");
    let file = fs::File::create(&big).unwrap();
    file.set_len(super::MAX_READ_FILE_SIZE + 1).unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    let error = manager
        .file_path(&workspace.id, "big.bin")
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Validation(message) if message.contains("too large")));
}

#[tokio::test]
#[cfg(unix)]
async fn file_operations_reject_traversal_and_symlink_escape() {
    use std::os::unix::fs::symlink;

    let manager = manager();
    let dir = fixture();
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("secret.txt");
    fs::write(&target, "secret").unwrap();
    symlink(&target, dir.path().join("link.txt")).unwrap();
    symlink(outside.path(), dir.path().join("escape")).unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();

    assert!(matches!(
        manager.read_file(&workspace.id, "../secret.txt").await,
        Err(AppError::Path(_))
    ));
    assert!(manager.read_file(&workspace.id, "link.txt").await.is_err());
    assert!(manager.file_path(&workspace.id, "link.txt").await.is_err());
    assert!(manager
        .write_file(&workspace.id, "escape/pwned.txt", "bad", 0)
        .await
        .is_err());
    assert!(!outside.path().join("pwned.txt").exists());
}

#[tokio::test]
async fn write_uses_content_revisions() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    let first = manager
        .write_file(&workspace.id, "file.txt", "one", 0)
        .await
        .unwrap();
    assert_eq!(first, crate::files::content_revision(b"one"));
    let second = manager
        .write_file(&workspace.id, "file.txt", "two", first)
        .await
        .unwrap();
    assert_eq!(second, crate::files::content_revision(b"two"));
    assert!(matches!(
        manager
            .write_file(&workspace.id, "file.txt", "three", first)
            .await,
        Err(AppError::StaleRevision)
    ));
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "two"
    );
}

#[tokio::test]
async fn file_path_returns_only_existing_files() {
    let manager = manager();
    let dir = fixture();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    assert_eq!(
        manager
            .file_path(&workspace.id, "package.json")
            .await
            .unwrap(),
        dir.path().join("package.json").to_string_lossy()
    );
    assert!(manager.file_path(&workspace.id, "src").await.is_err());
    assert!(manager
        .file_path(&workspace.id, "missing.txt")
        .await
        .is_err());
}

#[tokio::test]
async fn search_delegates_to_workspace_search() {
    let manager = manager();
    let dir = fixture();
    fs::write(dir.path().join("search.txt"), "TODO: ship it\n").unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    let results = manager
        .search(
            &workspace.id,
            "TODO",
            SearchOptions {
                max_results: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "search.txt");
}

#[tokio::test]
async fn concurrent_registry_access_is_safe() {
    let manager = Arc::new(manager());
    let stable = fixture();
    let stable_workspace = manager
        .register(&stable.path().to_string_lossy())
        .await
        .unwrap();
    let mut roots = Vec::new();
    for _ in 0..12 {
        roots.push(TempDir::new().unwrap());
    }

    let mut tasks = tokio::task::JoinSet::new();
    for root in &roots {
        let manager = Arc::clone(&manager);
        let path = root.path().to_string_lossy().into_owned();
        let stable_id = stable_workspace.id.clone();
        tasks.spawn(async move {
            for _ in 0..20 {
                let workspace = manager.register(&path).await.unwrap();
                manager.list().await.unwrap();
                manager.file_tree(&stable_id).await.unwrap();
                manager.read_file(&stable_id, "package.json").await.unwrap();
                manager.remove(&workspace.id).await.unwrap();
            }
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
}

#[tokio::test]
async fn missing_path_stays_listed_as_unavailable() {
    let manager = manager();
    let missing = "/tmp/local-agent-missing-workspace-definitely-gone";
    let info = manager
        .retain_unavailable(missing, "stat workspace: No such file or directory")
        .unwrap();
    assert!(!info.available);
    assert!(!info.error.is_empty());
    assert_eq!(info.path, missing);
    assert_eq!(info.id.len(), 16);

    let listed = manager.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(!listed[0].available);
    assert_eq!(listed[0].id, info.id);

    // File ops must fail clearly rather than panicking on a missing root.
    assert!(manager.file_tree(&info.id).await.is_err());

    manager.remove(&info.id).await.unwrap();
    assert!(manager.list().await.unwrap().is_empty());
    assert!(matches!(
        manager.remove(&info.id).await,
        Err(AppError::NotFound { .. })
    ));
}

#[tokio::test]
async fn successful_register_replaces_unavailable_entry() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    manager
        .retain_unavailable(&path, "temporary mount missing")
        .unwrap();
    let restored = manager.register(&path).await.unwrap();
    assert!(restored.available);
    assert!(restored.error.is_empty());
    let listed = manager.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].available);
}

#[tokio::test]
async fn write_file_invokes_on_write_hook_with_absolute_path() {
    use std::path::Path;
    use std::sync::Mutex;

    let manager = manager();
    let dir = fixture();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();

    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_hook = Arc::clone(&seen);
    manager.set_on_write(Arc::new(move |abs| {
        seen_hook.lock().unwrap().push(abs.to_string());
    }));

    manager
        .write_file(&workspace.id, "hooked.txt", "updated", 0)
        .await
        .unwrap();

    let paths = seen.lock().unwrap().clone();
    assert_eq!(paths.len(), 1);
    assert!(
        paths[0].ends_with("hooked.txt"),
        "expected abs path ending in hooked.txt, got {}",
        paths[0]
    );
    assert!(
        Path::new(&paths[0]).is_absolute(),
        "on_write must receive an absolute path"
    );
}

#[tokio::test]
async fn delete_rename_mkdir_happy_paths() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();

    manager.mkdir(&workspace.id, "src/foo").await.unwrap();
    assert!(dir.path().join("src/foo").is_dir());
    // Idempotent when the path already exists as a directory.
    manager.mkdir(&workspace.id, "src/foo").await.unwrap();

    manager
        .write_file(&workspace.id, "src/foo/a.txt", "hi", 0)
        .await
        .unwrap();
    manager
        .rename_path(&workspace.id, "src/foo/a.txt", "src/foo/b.txt")
        .await
        .unwrap();
    assert!(!dir.path().join("src/foo/a.txt").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("src/foo/b.txt")).unwrap(),
        "hi"
    );

    manager
        .delete_path(&workspace.id, "src/foo/b.txt")
        .await
        .unwrap();
    assert!(!dir.path().join("src/foo/b.txt").exists());
    manager.delete_path(&workspace.id, "src/foo").await.unwrap();
    assert!(!dir.path().join("src/foo").exists());
}

#[tokio::test]
async fn delete_missing_returns_not_found() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    assert!(matches!(
        manager.delete_path(&workspace.id, "missing.txt").await,
        Err(AppError::NotFound { .. })
    ));
}

#[tokio::test]
async fn mutations_reject_path_traversal() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    assert!(matches!(
        manager.delete_path(&workspace.id, "../escape.txt").await,
        Err(AppError::Path(_))
    ));
    assert!(matches!(
        manager
            .rename_path(&workspace.id, "a.txt", "../outside.txt")
            .await,
        Err(AppError::Path(_))
    ));
    assert!(matches!(
        manager.mkdir(&workspace.id, "../outside").await,
        Err(AppError::Path(_))
    ));
}

#[tokio::test]
async fn delete_rejects_non_empty_directory() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("keep")).unwrap();
    fs::write(dir.path().join("keep/file.txt"), "x").unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    assert!(matches!(
        manager.delete_path(&workspace.id, "keep").await,
        Err(AppError::Validation(msg)) if msg.contains("not empty")
    ));
}

#[tokio::test]
async fn rename_rejects_overwrite_and_mkdir_rejects_file() {
    let manager = manager();
    let dir = TempDir::new().unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    manager
        .write_file(&workspace.id, "a.txt", "one", 0)
        .await
        .unwrap();
    manager
        .write_file(&workspace.id, "b.txt", "two", 0)
        .await
        .unwrap();
    assert!(matches!(
        manager.rename_path(&workspace.id, "a.txt", "b.txt").await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        manager.mkdir(&workspace.id, "a.txt").await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn mutations_invoke_on_write_hook() {
    use std::sync::Mutex;

    let manager = manager();
    let dir = TempDir::new().unwrap();
    let workspace = manager
        .register(&dir.path().to_string_lossy())
        .await
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_hook = Arc::clone(&seen);
    manager.set_on_write(Arc::new(move |abs| {
        seen_hook.lock().unwrap().push(abs.to_string());
    }));

    manager.mkdir(&workspace.id, "d").await.unwrap();
    manager
        .write_file(&workspace.id, "d/f.txt", "x", 0)
        .await
        .unwrap();
    manager
        .rename_path(&workspace.id, "d/f.txt", "d/g.txt")
        .await
        .unwrap();
    manager.delete_path(&workspace.id, "d/g.txt").await.unwrap();

    let paths = seen.lock().unwrap().clone();
    assert!(
        paths.len() >= 4,
        "expected mkdir/write/rename/delete hooks, got {paths:?}"
    );
}
