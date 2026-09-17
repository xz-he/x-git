use hq_git_lib::{
    application::{
        activity_service::ActivityService, changes_service::ChangesService,
        mutation_coordinator::RepositoryMutationCoordinator,
    },
    infrastructure::git_runner::GitCommandRunner,
};
use serde_json::{Value, json};
use std::path::Path;

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
}
async fn fixture() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    git(root, &["init", "-b", "main"]).await;
    git(root, &["config", "user.name", "History Test"]).await;
    git(root, &["config", "user.email", "history@example.test"]).await;
    git(root, &["config", "core.autocrlf", "false"]).await;
    std::fs::write(root.join("a.txt"), "base\n").unwrap();
    std::fs::write(root.join("b.txt"), "base\n").unwrap();
    git(root, &["add", "."]).await;
    git(root, &["commit", "-m", "base"]).await;
    repo
}
fn services() -> (ActivityService, ChangesService) {
    let coordinator = RepositoryMutationCoordinator::default();
    let runner = GitCommandRunner::default();
    (
        ActivityService::new(runner.clone(), coordinator.clone()),
        ChangesService::new(runner, coordinator),
    )
}

#[tokio::test]
async fn restores_exact_partial_index_after_restart_without_touching_working_files() {
    let repo = fixture().await;
    let root = repo.path();
    let (history, changes) = services();
    std::fs::write(root.join("a.txt"), "partly staged\n").unwrap();
    git(root, &["add", "a.txt"]).await;
    std::fs::write(root.join("a.txt"), "latest working\n").unwrap();
    std::fs::write(root.join("b.txt"), "other staged\n").unwrap();
    git(root, &["add", "b.txt"]).await;
    let reply = history
        .execute(root, "changes_unstage_files", "批量取消暂存", async {
            Ok(
                serde_json::to_value(changes.unstage_files(root, &["a.txt".into()]).await?)
                    .unwrap(),
            )
        })
        .await
        .unwrap();
    assert!(reply.error.is_none());
    std::fs::write(root.join("a.txt"), "even newer work\n").unwrap();
    let restarted = services().0;
    let records = restarted.list(root).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].rollback_kind.as_deref(), Some("index"));
    restarted.rollback(root, &records[0].id).await.unwrap();
    assert_eq!(git(root, &["show", ":a.txt"]).await, "partly staged\n");
    assert_eq!(git(root, &["show", ":b.txt"]).await, "other staged\n");
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "even newer work\n"
    );
    assert!(restarted.rollback(root, &records[0].id).await.is_err());
    assert_eq!(restarted.list(root).await.unwrap().len(), 2);
}

#[tokio::test]
async fn refuses_stale_index_without_changing_it_and_isolates_repositories() {
    let repo = fixture().await;
    let root = repo.path();
    let (history, changes) = services();
    std::fs::write(root.join("a.txt"), "updated\n").unwrap();
    history
        .execute(root, "changes_stage_file", "暂存", async {
            Ok(serde_json::to_value(changes.stage_file(root, "a.txt").await?).unwrap())
        })
        .await
        .unwrap();
    let records = history.list(root).await.unwrap();
    std::fs::write(root.join("b.txt"), "later change\n").unwrap();
    git(root, &["add", "b.txt"]).await;
    let before = git(root, &["write-tree"]).await;
    assert!(history.rollback(root, &records[0].id).await.is_err());
    assert_eq!(git(root, &["write-tree"]).await, before);
    let other = fixture().await;
    assert!(history.list(other.path()).await.unwrap().is_empty());
    assert!(
        history
            .rollback(other.path(), &records[0].id)
            .await
            .is_err()
    );
    assert!(history.rollback(root, "../config").await.is_err());
}

#[tokio::test]
async fn binary_add_and_unborn_index_can_be_rolled_back() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    let (history, changes) = services();
    git(root, &["init", "-b", "main"]).await;
    let binary = [0u8, 1, 2, 255, 0, 128];
    std::fs::write(root.join("[中文].bin"), binary).unwrap();
    history
        .execute(root, "changes_stage_files", "批量暂存", async {
            Ok(
                serde_json::to_value(changes.stage_files(root, &["[中文].bin".into()]).await?)
                    .unwrap(),
            )
        })
        .await
        .unwrap();
    let records = history.list(root).await.unwrap();
    history.rollback(root, &records[0].id).await.unwrap();
    assert!(git(root, &["ls-files"]).await.is_empty());
    assert_eq!(std::fs::read(root.join("[中文].bin")).unwrap(), binary);
}

#[tokio::test]
async fn binary_deletion_rollback_restores_index_but_keeps_file_deleted() {
    let repo = fixture().await;
    let root = repo.path();
    let (history, changes) = services();
    let binary = [0u8, 255, 9, 0, 128];
    std::fs::write(root.join("a.txt"), binary).unwrap();
    git(root, &["add", "a.txt"]).await;
    let before = git(root, &["write-tree"]).await;
    std::fs::remove_file(root.join("a.txt")).unwrap();
    history
        .execute(root, "changes_stage_file", "暂存删除", async {
            Ok(serde_json::to_value(changes.stage_file(root, "a.txt").await?).unwrap())
        })
        .await
        .unwrap();
    let record = history.list(root).await.unwrap().remove(0);
    history.rollback(root, &record.id).await.unwrap();
    assert_eq!(git(root, &["write-tree"]).await, before);
    assert!(!root.join("a.txt").exists());
}

#[tokio::test]
async fn recording_queries_does_not_change_watch_fingerprint() {
    let repo = fixture().await;
    let root = repo.path();
    let (history, _) = services();
    let watcher = hq_git_lib::application::repository_watch::RepositoryWatchService::default();
    let before = watcher.snapshot(root).await.unwrap();
    history
        .record_external(root, "Fetch 远程", true, "操作完成")
        .await
        .unwrap();
    let after = watcher.snapshot(root).await.unwrap();
    assert_eq!(before.worktree_version, after.worktree_version);
    assert_eq!(before.metadata_version, after.metadata_version);
    let records = history.list(root).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, "success");
    assert!(records[0].rollback_kind.is_none());
}

#[tokio::test]
async fn commit_rollback_creates_revert_and_rejects_dirty_worktree() {
    let repo = fixture().await;
    let root = repo.path();
    let (history, changes) = services();
    std::fs::write(root.join("a.txt"), "new commit\n").unwrap();
    git(root, &["add", "a.txt"]).await;
    history
        .execute(root, "changes_commit", "创建提交", async {
            Ok(serde_json::to_value(changes.commit(root, "feature").await?).unwrap())
        })
        .await
        .unwrap();
    let records = history.list(root).await.unwrap();
    assert_eq!(records[0].rollback_kind.as_deref(), Some("revert"));
    std::fs::write(root.join("b.txt"), "keep\n").unwrap();
    assert!(history.rollback(root, &records[0].id).await.is_err());
    std::fs::write(root.join("b.txt"), "base\n").unwrap();
    history.rollback(root, &records[0].id).await.unwrap();
    assert_eq!(
        git(root, &["rev-list", "--count", "HEAD"]).await.trim(),
        "3"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "base\n"
    );
}

#[tokio::test]
async fn branch_switch_can_return_and_failures_are_not_rollbackable() {
    let repo = fixture().await;
    let root = repo.path();
    let (history, _) = services();
    git(root, &["branch", "feature"]).await;
    history
        .execute(root, "refs_switch_branch", "切换分支", async {
            git(root, &["switch", "feature"]).await;
            Ok(json!({}))
        })
        .await
        .unwrap();
    let records = history.list(root).await.unwrap();
    history.rollback(root, &records[0].id).await.unwrap();
    assert_eq!(
        git(root, &["branch", "--show-current"]).await.trim(),
        "main"
    );
    let reply = history
        .execute(root, "changes_stage_file", "失败操作", async {
            Err::<Value, _>(hq_git_lib::domain::error::BackendError::new(
                hq_git_lib::domain::error::ErrorCode::InvalidPath,
                "文件已失效",
            ))
        })
        .await
        .unwrap();
    assert!(reply.error.is_some());
    let failed = history
        .list(root)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.title == "失败操作")
        .unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.rollback_kind.is_none());
    assert!(history.rollback(root, &failed.id).await.is_err());
}
