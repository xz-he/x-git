use hq_git_lib::application::conflict_service::ConflictService;
use hq_git_lib::application::history_service::HistoryService;
use hq_git_lib::application::refs_service::RefsService;
use hq_git_lib::domain::history::SquashRequest;
use hq_git_lib::domain::operation::{AbortAction, RepositoryOperationKind};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use std::path::Path;

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
        .trim()
        .into()
}
async fn repo() -> tempfile::TempDir {
    // Spaces, quotes and Unicode exercise the fixed sequence editor's argv handling.
    let dir = tempfile::Builder::new()
        .prefix("hq squash '中文 ")
        .tempdir()
        .unwrap();
    git(dir.path(), &["init", "-b", "main"]).await;
    git(dir.path(), &["config", "user.name", "HQ Test"]).await;
    git(dir.path(), &["config", "user.email", "hq@example.test"]).await;
    git(dir.path(), &["config", "core.autocrlf", "false"]).await;
    dir
}
async fn commit(root: &Path, path: &str, content: &str, title: &str) -> String {
    std::fs::write(root.join(path), content).unwrap();
    git(root, &["add", "--", path]).await;
    git(root, &["commit", "-m", title]).await;
    git(root, &["rev-parse", "HEAD"]).await
}
async fn request(root: &Path, commits: Vec<String>) -> SquashRequest {
    let preview = HistoryService::default()
        .squash_preview(root, commits.clone())
        .await
        .unwrap();
    SquashRequest {
        commits,
        message: "合并说明 ' $() ` text\n\n正文".into(),
        expected_head: preview.head,
        expected_branch: preview.branch,
    }
}

#[tokio::test]
async fn squash_nonconsecutive_commits_preserves_unselected_changes_and_other_refs() {
    let dir = repo().await;
    let root = dir.path();
    commit(root, "base", "base", "base").await;
    let a = commit(root, "a", "a", "selected A").await;
    let b = commit(root, "b", "b", "keep B").await;
    let c = commit(root, "c", "c", "selected C").await;
    let head = commit(root, "d", "d", "keep D").await;
    let tree = git(root, &["rev-parse", "HEAD^{tree}"]).await;
    git(root, &["branch", "other", &b]).await;
    git(root, &["tag", "old", &head]).await;
    git(root, &["config", "rebase.updateRefs", "true"]).await;
    git(root, &["config", "rebase.autoSquash", "true"]).await;
    let req = request(root, vec![c.clone(), a.clone()]).await;
    let message = req.message.clone();
    let result = HistoryService::default().squash(root, req).await.unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(git(root, &["rev-parse", "HEAD^{tree}"]).await, tree);
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).await, "4");
    assert_eq!(
        git(root, &["log", "-2", "--format=%s"]).await,
        "keep D\nkeep B"
    );
    assert_eq!(
        git(root, &["show", "-s", "--format=%B", "HEAD~2"]).await,
        message
    );
    assert_eq!(git(root, &["rev-parse", "other"]).await, b);
    assert_eq!(git(root, &["rev-parse", "old"]).await, head);
    assert_eq!(
        git(
            root,
            &[
                "for-each-ref",
                "--format=%(objectname)",
                "refs/heads/hq-git-backup/"
            ]
        )
        .await,
        head
    );
    assert_eq!(git(root, &["status", "--porcelain"]).await, "");
    assert!(result.notice.unwrap().contains("hq-git-backup/squash-"));
}

#[tokio::test]
async fn squash_including_root_keeps_empty_selected_commits_and_message() {
    let dir = repo().await;
    let root = dir.path();
    let first = commit(root, "a", "a", "root").await;
    git(root, &["commit", "--allow-empty", "-m", "empty"]).await;
    let empty = git(root, &["rev-parse", "HEAD"]).await;
    commit(root, "b", "b", "keep").await;
    let tree = git(root, &["rev-parse", "HEAD^{tree}"]).await;
    let req = request(root, vec![empty, first]).await;
    let result = HistoryService::default().squash(root, req).await.unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).await, "2");
    assert_eq!(git(root, &["rev-parse", "HEAD^{tree}"]).await, tree);
}

#[tokio::test]
async fn squash_conflicts_can_abort_or_continue_using_native_rebase() {
    for abort in [true, false] {
        let dir = repo().await;
        let root = dir.path();
        commit(root, "f", "0\n", "base").await;
        let a = commit(root, "a", "a\n", "selected A").await;
        commit(root, "f", "1\n", "keep B").await;
        let c = commit(root, "f", "2\n", "selected C").await;
        let req = request(root, vec![a, c.clone()]).await;
        let result = HistoryService::default().squash(root, req).await.unwrap();
        assert_eq!(result.operation_state.kind, RepositoryOperationKind::Rebase);
        assert!(!result.operation_state.conflicts.is_empty());
        assert!(result.notice.unwrap().contains("hq-git-backup"));
        if abort {
            RefsService::default()
                .abort(root, AbortAction::Rebase)
                .await
                .unwrap();
            assert_eq!(git(root, &["rev-parse", "HEAD"]).await, c);
            assert_eq!(std::fs::read_to_string(root.join("f")).unwrap(), "2\n");
        } else {
            let conflicts = ConflictService::default();
            for attempt in 0..3 {
                if !git(root, &["status", "--porcelain"]).await.is_empty() {
                    // Preserve a distinct resolution for the unselected commit.
                    // Resolving it to the unchanged index intentionally drops it in Git.
                    std::fs::write(
                        root.join("f"),
                        if attempt == 0 { "2\n" } else { "resolved B\n" },
                    )
                    .unwrap();
                    git(root, &["add", "f"]).await;
                }
                let snapshot = conflicts.snapshot(root).await.unwrap();
                let output = conflicts
                    .continue_operation(root, &snapshot.operation_token)
                    .await
                    .unwrap();
                if output.operation_state.kind == RepositoryOperationKind::None {
                    break;
                }
            }
            assert_eq!(git(root, &["branch", "--show-current"]).await, "main");
            assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).await, "3");
            assert_eq!(git(root, &["status", "--porcelain"]).await, "");
        }
    }
}

#[tokio::test]
async fn squash_refuses_dirty_stale_detached_and_foreign_selections_without_changes() {
    let dir = repo().await;
    let root = dir.path();
    let a = commit(root, "a", "a", "A").await;
    let b = commit(root, "b", "b", "B").await;
    let service = HistoryService::default();
    let mut stale = request(root, vec![a.clone(), b.clone()]).await;
    stale.expected_head = a.clone();
    assert!(service.squash(root, stale).await.is_err());
    let dirty = request(root, vec![a.clone(), b.clone()]).await;
    std::fs::write(root.join("local"), "unsaved").unwrap();
    assert!(service.squash(root, dirty).await.is_err());
    assert_eq!(std::fs::read(root.join("local")).unwrap(), b"unsaved");
    std::fs::remove_file(root.join("local")).unwrap();
    assert!(
        service
            .squash_preview(root, vec![b.clone(), b.clone()])
            .await
            .is_err()
    );
    git(root, &["switch", "-c", "other", &a]).await;
    let foreign = commit(root, "c", "c", "foreign").await;
    git(root, &["switch", "main"]).await;
    assert!(
        service
            .squash_preview(root, vec![a.clone(), foreign])
            .await
            .is_err()
    );
    git(root, &["switch", "--detach"]).await;
    assert!(
        service
            .squash_preview(root, vec![a, b.clone()])
            .await
            .is_err()
    );
    assert_eq!(git(root, &["rev-parse", "main"]).await, b);
    assert_eq!(
        git(root, &["for-each-ref", "refs/heads/hq-git-backup/"]).await,
        ""
    );
}

#[tokio::test]
async fn squash_rejects_merge_boundaries_without_flattening_history() {
    let dir = repo().await;
    let root = dir.path();
    let a = commit(root, "a", "a", "A").await;
    git(root, &["switch", "-c", "topic"]).await;
    commit(root, "b", "b", "B").await;
    git(root, &["switch", "main"]).await;
    git(root, &["merge", "--no-ff", "topic", "-m", "merge"]).await;
    let head = git(root, &["rev-parse", "HEAD"]).await;
    let error = HistoryService::default()
        .squash_preview(root, vec![a, head.clone()])
        .await
        .unwrap_err();
    assert!(error.message.contains("Merge"));
    assert_eq!(git(root, &["rev-parse", "HEAD"]).await, head);
}

#[tokio::test]
async fn squash_protects_ignored_local_files_that_were_tracked_in_history() {
    let dir = repo().await;
    let root = dir.path();
    commit(root, "base", "base", "base").await;
    let first = commit(root, "local.txt", "old tracked", "track local file").await;
    git(root, &["rm", "local.txt"]).await;
    std::fs::write(root.join(".gitignore"), "local.txt\ncache/\n").unwrap();
    git(root, &["add", ".gitignore"]).await;
    git(root, &["commit", "-m", "stop tracking local file"]).await;
    let last = git(root, &["rev-parse", "HEAD"]).await;
    let req = request(root, vec![first, last.clone()]).await;
    std::fs::write(root.join("local.txt"), "precious ignored work").unwrap();
    let error = HistoryService::default()
        .squash(root, req)
        .await
        .unwrap_err();
    assert!(error.message.contains("被忽略"));
    assert_eq!(git(root, &["rev-parse", "HEAD"]).await, last);
    assert_eq!(
        std::fs::read(root.join("local.txt")).unwrap(),
        b"precious ignored work"
    );
    assert_eq!(
        git(root, &["for-each-ref", "refs/heads/hq-git-backup/"]).await,
        ""
    );
}
