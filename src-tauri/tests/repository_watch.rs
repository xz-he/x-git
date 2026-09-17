use hq_git_lib::application::repository_watch::{RepositoryWatchService, RepositoryWatchSnapshot};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use std::path::Path;
use std::time::Duration;

async fn git(root: &Path, args: &[&str]) {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap();
}
async fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]).await;
    git(root.path(), &["config", "user.name", "Watch Test"]).await;
    git(root.path(), &["config", "user.email", "watch@example.test"]).await;
    std::fs::write(root.path().join("file.txt"), "base\n").unwrap();
    git(root.path(), &["add", "."]).await;
    git(root.path(), &["commit", "-m", "base"]).await;
    root
}
async fn changed(
    service: &RepositoryWatchService,
    root: &Path,
    previous: &RepositoryWatchSnapshot,
    metadata: bool,
) -> RepositoryWatchSnapshot {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let next = service.snapshot(root).await.unwrap();
            if if metadata {
                next.metadata_version > previous.metadata_version
            } else {
                next.worktree_version > previous.worktree_version
            } {
                return next;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("native watcher did not report repository change")
}

#[tokio::test]
async fn ignores_ignored_files_same_content_and_index_stat_updates() {
    let root = fixture().await;
    std::fs::write(root.path().join(".gitignore"), "ignored/\n").unwrap();
    git(root.path(), &["add", ".gitignore"]).await;
    git(root.path(), &["commit", "-m", "ignore generated files"]).await;
    let service = RepositoryWatchService::default();
    let before = service.snapshot(root.path()).await.unwrap();
    std::fs::create_dir(root.path().join("ignored")).unwrap();
    std::fs::write(root.path().join("ignored/log.txt"), "noise\n").unwrap();
    std::fs::write(root.path().join("file.txt"), "base\n").unwrap();
    git(root.path(), &["update-index", "--refresh"]).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = service.snapshot(root.path()).await.unwrap();
    assert_eq!(after.worktree_version, before.worktree_version);
    assert_eq!(after.metadata_version, before.metadata_version);
    service.stop().await;
}

#[tokio::test]
async fn detects_file_edits_rename_delete_staging_and_external_commits() {
    let root = fixture().await;
    let service = RepositoryWatchService::default();
    let mut before = service.snapshot(root.path()).await.unwrap();
    for contents in ["edited once\n", "edited again\n"] {
        std::fs::write(root.path().join("file.txt"), contents).unwrap();
        before = changed(&service, root.path(), &before, false).await;
        std::fs::write(root.path().join("file.txt"), contents).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        let unchanged = service.snapshot(root.path()).await.unwrap();
        assert_eq!(unchanged.worktree_version, before.worktree_version);
        assert_eq!(unchanged.metadata_version, before.metadata_version);
    }
    std::fs::rename(
        root.path().join("file.txt"),
        root.path().join("renamed.txt"),
    )
    .unwrap();
    before = changed(&service, root.path(), &before, false).await;
    std::fs::remove_file(root.path().join("renamed.txt")).unwrap();
    before = changed(&service, root.path(), &before, false).await;
    git(root.path(), &["add", "-A"]).await;
    before = changed(&service, root.path(), &before, false).await;
    git(root.path(), &["commit", "-m", "external commit"]).await;
    before = changed(&service, root.path(), &before, true).await;
    git(root.path(), &["switch", "-c", "feature/external"]).await;
    changed(&service, root.path(), &before, true).await;
}

#[tokio::test]
async fn detects_index_content_changes_with_identical_worktree_and_status() {
    let root = fixture().await;
    std::fs::write(root.path().join("file.txt"), "staged one\n").unwrap();
    git(root.path(), &["add", "file.txt"]).await;
    std::fs::write(root.path().join("file.txt"), "working draft\n").unwrap();
    let service = RepositoryWatchService::default();
    let before = service.snapshot(root.path()).await.unwrap();
    std::fs::write(root.path().join("file.txt"), "staged two\n").unwrap();
    git(root.path(), &["add", "file.txt"]).await;
    std::fs::write(root.path().join("file.txt"), "working draft\n").unwrap();
    let after = changed(&service, root.path(), &before, false).await;
    assert_eq!(after.metadata_version, before.metadata_version);
    service.stop().await;
}

#[tokio::test]
async fn follows_linked_worktree_head_and_shared_refs() {
    let main = fixture().await;
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("linked");
    git(
        main.path(),
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    )
    .await;
    let service = RepositoryWatchService::default();
    let before = service.snapshot(&linked).await.unwrap();
    git(&linked, &["switch", "-c", "linked-next"]).await;
    let before = changed(&service, &linked, &before, true).await;
    git(main.path(), &["branch", "shared-ref"]).await;
    changed(&service, &linked, &before, true).await;
    service.stop().await;
}

#[tokio::test]
async fn switches_watcher_identity_and_releases_handles_on_stop() {
    let first = fixture().await;
    let second = fixture().await;
    let service = RepositoryWatchService::default();
    let before = service.snapshot(first.path()).await.unwrap();
    let next = service.snapshot(second.path()).await.unwrap();
    assert_ne!(before.watch_id, next.watch_id);
    std::fs::write(first.path().join("file.txt"), "old repository\n").unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let idle = service.snapshot(second.path()).await.unwrap();
    assert_eq!(idle.worktree_version, next.worktree_version);
    service.stop().await;
    let restarted = service.snapshot(second.path()).await.unwrap();
    assert_ne!(idle.watch_id, restarted.watch_id);
}
