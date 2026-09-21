use std::path::Path;

use hq_git_lib::application::history_service::HistoryService;
use hq_git_lib::application::refs_service::RefsService;
use hq_git_lib::domain::changes::{DiffLineKind, DiffScope};
use hq_git_lib::domain::error::ErrorCode;
use hq_git_lib::domain::history::{
    CherryPickRequest, HistoryQuery, ResetMode, ResetRequest, RevertRequest,
};
use hq_git_lib::domain::operation::{AbortAction, RepositoryOperationKind};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use tempfile::TempDir;

async fn run_git(root: &Path, args: &[&str]) {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap();
}

async fn repository_fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    run_git(directory.path(), &["init", "-b", "main"]).await;
    run_git(directory.path(), &["config", "user.name", "HQ Test"]).await;
    run_git(
        directory.path(),
        &["config", "user.email", "hq@example.test"],
    )
    .await;
    run_git(directory.path(), &["config", "core.autocrlf", "false"]).await;
    directory
}

async fn commit_file(root: &Path, contents: &str, subject: &str) {
    std::fs::write(root.join("history.txt"), contents).unwrap();
    run_git(root, &["add", "history.txt"]).await;
    run_git(root, &["commit", "-m", subject]).await;
}

async fn head_hash(root: &Path) -> String {
    GitCommandRunner::default()
        .run(Some(root), ["rev-parse", "HEAD"])
        .await
        .unwrap()
        .stdout
        .trim()
        .to_owned()
}

#[tokio::test]
async fn merge_detail_lists_files_against_first_parent() {
    let directory = repository_fixture().await;
    let root = directory.path();
    commit_file(root, "base\n", "base").await;
    run_git(root, &["switch", "-c", "feature"]).await;
    commit_file(root, "feature\n", "feature change").await;
    run_git(root, &["switch", "main"]).await;
    run_git(
        root,
        &["merge", "--no-ff", "feature", "-m", "merge feature"],
    )
    .await;
    let detail = HistoryService::default()
        .detail(root, "HEAD")
        .await
        .unwrap();
    assert_eq!(detail.parent_hashes.len(), 2);
    assert_eq!(detail.files.len(), 1);
    assert_eq!(detail.files[0].path, "history.txt");
    assert_eq!(detail.files[0].additions, Some(1));
    assert_eq!(detail.files[0].deletions, Some(1));
    let diff = HistoryService::default()
        .file_diff(root, "HEAD", "history.txt")
        .await
        .unwrap();
    assert!(!diff.hunks.is_empty());
}

#[tokio::test]
async fn revert_older_commit_preserves_history_and_later_unrelated_changes() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "one\n", "base").await;
    commit_file(root, "two\n", "target").await;
    let target = head_hash(root).await;
    std::fs::write(root.join("later.txt"), "keep me\n").unwrap();
    run_git(root, &["add", "later.txt"]).await;
    run_git(root, &["commit", "-m", "later"]).await;
    let previous_head = head_hash(root).await;
    let result = HistoryService::default()
        .revert(
            root,
            RevertRequest {
                commit: target.clone(),
                mainline: None,
            },
        )
        .await
        .unwrap();
    assert!(result.history.commits[0].subject.starts_with("Revert"));
    assert_eq!(result.history.commits[0].parent_hashes, vec![previous_head]);
    assert!(
        result
            .history
            .commits
            .iter()
            .any(|commit| commit.hash == target)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("history.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("later.txt")).unwrap(),
        "keep me\n"
    );
    assert!(result.workspace.repository.is_clean);
}

#[tokio::test]
async fn revert_rejects_dirty_worktree_and_can_revert_root_commit() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "one\n", "initial").await;
    let target = head_hash(root).await;
    std::fs::write(root.join("history.txt"), "unsaved\n").unwrap();
    let error = HistoryService::default()
        .revert(
            root,
            RevertRequest {
                commit: target.clone(),
                mainline: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DirtyWorktree);
    assert_eq!(head_hash(root).await, target);
    assert_eq!(
        std::fs::read_to_string(root.join("history.txt")).unwrap(),
        "unsaved\n"
    );
    std::fs::write(root.join("history.txt"), "one\n").unwrap();
    let result = HistoryService::default()
        .revert(
            root,
            RevertRequest {
                commit: target,
                mainline: None,
            },
        )
        .await
        .unwrap();
    assert!(!root.join("history.txt").exists());
    assert_eq!(result.history.commits.len(), 2);
}

#[tokio::test]
async fn revert_merge_requires_mainline_and_reverses_only_selected_parent_delta() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "base\n", "base").await;
    run_git(root, &["switch", "-c", "feature"]).await;
    commit_file(root, "feature\n", "feature change").await;
    run_git(root, &["switch", "main"]).await;
    std::fs::write(root.join("main.txt"), "main stays\n").unwrap();
    run_git(root, &["add", "main.txt"]).await;
    run_git(root, &["commit", "-m", "main change"]).await;
    run_git(root, &["merge", "--no-ff", "--no-edit", "feature"]).await;
    let target = head_hash(root).await;
    for mainline in [None, Some(0), Some(3)] {
        let error = HistoryService::default()
            .revert(
                root,
                RevertRequest {
                    commit: target.clone(),
                    mainline,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidReference);
        assert_eq!(head_hash(root).await, target);
    }
    HistoryService::default()
        .revert(
            root,
            RevertRequest {
                commit: target,
                mainline: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("history.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("main.txt")).unwrap(),
        "main stays\n"
    );
}

#[tokio::test]
async fn revert_conflict_can_be_aborted_or_resolved_and_continued() {
    use hq_git_lib::application::conflict_service::ConflictService;
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "one\n", "base").await;
    commit_file(root, "two\n", "target").await;
    let target = head_hash(root).await;
    commit_file(root, "three\n", "later").await;
    let previous = head_hash(root).await;
    let result = HistoryService::default()
        .revert(
            root,
            RevertRequest {
                commit: target.clone(),
                mainline: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.operation_state.kind, RepositoryOperationKind::Revert);
    assert_eq!(
        result.operation_state.abort_action,
        Some(AbortAction::Revert)
    );
    assert_eq!(result.operation_state.conflicts.len(), 1);
    let blocked = HistoryService::default()
        .revert(
            root,
            RevertRequest {
                commit: target.clone(),
                mainline: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::GitOperationInProgress);
    RefsService::default()
        .abort(root, AbortAction::Revert)
        .await
        .unwrap();
    assert_eq!(head_hash(root).await, previous);
    assert_eq!(
        std::fs::read_to_string(root.join("history.txt")).unwrap(),
        "three\n"
    );
    HistoryService::default()
        .revert(
            root,
            RevertRequest {
                commit: target,
                mainline: None,
            },
        )
        .await
        .unwrap();
    std::fs::write(root.join("history.txt"), "resolved\n").unwrap();
    run_git(root, &["add", "history.txt"]).await;
    let conflicts = ConflictService::default();
    let snapshot = conflicts.snapshot(root).await.unwrap();
    let continued = conflicts
        .continue_operation(root, &snapshot.operation_token)
        .await
        .unwrap();
    assert_eq!(
        continued.operation_state.kind,
        RepositoryOperationKind::None
    );
    assert!(continued.workspace.repository.is_clean);
    assert_ne!(head_hash(root).await, previous);
}

#[tokio::test]
#[ignore = "manual history detail timing; excludes fixture setup"]
async fn history_many_files_timing() {
    let fixture = repository_fixture().await;
    let service = HistoryService::default();
    for count in [1, 100] {
        for index in 0..count {
            std::fs::write(
                fixture.path().join(format!("document-{index:04}.md")),
                format!("version {count}\nsecond line\n"),
            )
            .unwrap();
        }
        run_git(fixture.path(), &["add", "."]).await;
        run_git(fixture.path(), &["commit", "-m", "many documents"]).await;
        let hash = head_hash(fixture.path()).await;
        let started = std::time::Instant::now();
        let detail = service.detail(fixture.path(), &hash).await.unwrap();
        println!(
            "history detail files={count} elapsed_ms={}",
            started.elapsed().as_millis()
        );
        assert_eq!(detail.files.len(), count);
        assert!(detail.files.iter().all(|file| file.additions.is_some()));
    }
}

#[tokio::test]
async fn detail_batch_stats_cover_mixed_files() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    std::fs::write(root.join("deleted.md"), "removed\n").unwrap();
    std::fs::write(root.join("modified.md"), "before\nkeep\n").unwrap();
    run_git(root, &["add", "."]).await;
    run_git(root, &["commit", "-m", "initial"]).await;
    std::fs::remove_file(root.join("deleted.md")).unwrap();
    std::fs::write(root.join("modified.md"), "after\nkeep\nextra\n").unwrap();
    std::fs::write(root.join("中文 文档.md"), "new document\nsecond line\n").unwrap();
    std::fs::write(root.join("image.bin"), [0, 1, 2, 0, 3]).unwrap();
    run_git(root, &["add", "-A"]).await;
    run_git(root, &["commit", "-m", "mixed changes"]).await;
    let detail = HistoryService::default()
        .detail(root, &head_hash(root).await)
        .await
        .unwrap();
    assert_eq!(detail.files.len(), 4);
    for (path, status, additions, deletions) in [
        ("deleted.md", "D", Some(0), Some(1)),
        ("modified.md", "M", Some(2), Some(1)),
        ("中文 文档.md", "A", Some(2), Some(0)),
        ("image.bin", "A", None, None),
    ] {
        let file = detail.files.iter().find(|file| file.path == path).unwrap();
        assert_eq!(file.status, status);
        assert_eq!(
            (file.additions, file.deletions),
            (additions, deletions),
            "{path}"
        );
    }
}

async fn three_commit_fixture() -> (TempDir, Vec<String>) {
    let directory = repository_fixture().await;
    let mut hashes = Vec::new();
    for (contents, subject) in [
        ("one\n", "first"),
        ("two\n", "second"),
        ("three\n", "third"),
    ] {
        commit_file(directory.path(), contents, subject).await;
        hashes.push(head_hash(directory.path()).await);
    }
    (directory, hashes)
}

#[tokio::test]
async fn history_pages_two_hundred_and_preserves_selected_order() {
    let fixture = repository_fixture().await;
    for index in 0..205 {
        commit_file(
            fixture.path(),
            &format!("commit {index}\n"),
            &format!("commit {index:03}"),
        )
        .await;
    }
    let service = HistoryService::default();

    let first = service
        .page(fixture.path(), HistoryQuery::default())
        .await
        .unwrap();
    let second = service
        .page(
            fixture.path(),
            HistoryQuery {
                cursor: first.next_cursor.clone(),
                ..HistoryQuery::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(first.commits.len(), 200);
    assert_eq!(first.commits[0].subject, "commit 204");
    assert_eq!(first.commits[199].subject, "commit 005");
    assert!(first.next_cursor.is_some());
    assert!(!first.query_fingerprint.is_empty());
    assert_eq!(second.commits.len(), 5);
    assert_eq!(second.commits[0].subject, "commit 004");
    assert_eq!(second.commits[4].subject, "commit 000");
    assert!(second.next_cursor.is_none());
    assert_eq!(first.query_fingerprint, second.query_fingerprint);
}

#[tokio::test]
async fn search_merges_message_author_and_hash_matches_in_history_order() {
    let fixture = repository_fixture().await;
    commit_file(fixture.path(), "oldest\n", "oldest body match").await;
    run_git(
        fixture.path(),
        &[
            "commit",
            "--amend",
            "-m",
            "oldest body match",
            "-m",
            "needle",
        ],
    )
    .await;
    run_git(fixture.path(), &["config", "user.name", "Needle Author"]).await;
    commit_file(fixture.path(), "authored\n", "authored match").await;
    run_git(fixture.path(), &["config", "user.name", "HQ Test"]).await;
    commit_file(fixture.path(), "newest\n", "newest needle").await;
    let hash_prefix = GitCommandRunner::default()
        .run(Some(fixture.path()), ["rev-parse", "--short=10", "HEAD~1"])
        .await
        .unwrap()
        .stdout;
    let service = HistoryService::default();

    let page = service
        .page(
            fixture.path(),
            HistoryQuery {
                search: "needle".into(),
                ..HistoryQuery::default()
            },
        )
        .await
        .unwrap();
    let hash_page = service
        .page(
            fixture.path(),
            HistoryQuery {
                search: hash_prefix.trim().into(),
                ..HistoryQuery::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(
        page.commits
            .iter()
            .map(|commit| commit.subject.as_str())
            .collect::<Vec<_>>(),
        ["newest needle", "authored match", "oldest body match"]
    );
    assert_eq!(hash_page.commits.len(), 1);
    assert_eq!(hash_page.commits[0].subject, "authored match");
}

#[tokio::test]
async fn detail_reports_root_and_rename_stats_and_commit_file_diff() {
    let fixture = repository_fixture().await;
    std::fs::write(fixture.path().join("old name.txt"), "one\ntwo\n").unwrap();
    run_git(fixture.path(), &["add", "old name.txt"]).await;
    run_git(
        fixture.path(),
        &["commit", "-m", "root subject", "-m", "root body"],
    )
    .await;
    let root_hash = GitCommandRunner::default()
        .run(Some(fixture.path()), ["rev-parse", "HEAD"])
        .await
        .unwrap()
        .stdout;
    run_git(fixture.path(), &["mv", "old name.txt", "new name.txt"]).await;
    std::fs::write(fixture.path().join("new name.txt"), "one\nchanged\nthree\n").unwrap();
    run_git(fixture.path(), &["add", "new name.txt"]).await;
    run_git(
        fixture.path(),
        &["commit", "-m", "rename subject", "-m", "rename body"],
    )
    .await;
    let rename_hash = GitCommandRunner::default()
        .run(Some(fixture.path()), ["rev-parse", "HEAD"])
        .await
        .unwrap()
        .stdout;
    let service = HistoryService::default();

    let root = service
        .detail(fixture.path(), root_hash.trim())
        .await
        .unwrap();
    let renamed = service
        .detail(fixture.path(), rename_hash.trim())
        .await
        .unwrap();
    let diff = service
        .file_diff(fixture.path(), rename_hash.trim(), "new name.txt")
        .await
        .unwrap();

    assert!(root.parent_hashes.is_empty());
    assert_eq!(root.message, "root subject\n\nroot body");
    assert_eq!(root.files.len(), 1);
    assert_eq!(root.files[0].status, "A");
    assert_eq!(root.files[0].additions, Some(2));
    assert_eq!(renamed.parent_hashes.len(), 1);
    assert_eq!(renamed.files.len(), 1);
    assert_eq!(renamed.files[0].path, "new name.txt");
    assert_eq!(renamed.files[0].old_path.as_deref(), Some("old name.txt"));
    assert!(renamed.files[0].status.starts_with('R'));
    assert_eq!(renamed.files[0].additions, Some(2));
    assert_eq!(renamed.files[0].deletions, Some(1));
    assert_eq!(diff.scope, DiffScope::Commit);
    assert!(
        diff.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| { line.kind == DiffLineKind::Addition && line.content == "changed" })
    );
}

#[tokio::test]
async fn empty_history_is_valid_and_cursor_rejects_reference_changes() {
    let empty = repository_fixture().await;
    let service = HistoryService::default();
    let page = service
        .page(empty.path(), HistoryQuery::default())
        .await
        .unwrap();
    assert!(page.commits.is_empty());
    assert!(page.next_cursor.is_none());

    for index in 0..201 {
        commit_file(
            empty.path(),
            &format!("commit {index}\n"),
            &format!("commit {index:03}"),
        )
        .await;
    }
    let first = service
        .page(empty.path(), HistoryQuery::default())
        .await
        .unwrap();
    commit_file(empty.path(), "moved\n", "reference moved").await;
    let error = service
        .page(
            empty.path(),
            HistoryQuery {
                cursor: first.next_cursor,
                ..HistoryQuery::default()
            },
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidHistoryCursor);
}

#[tokio::test]
async fn checkout_resolves_a_commit_and_enters_detached_head() {
    let (fixture, hashes) = three_commit_fixture().await;
    let target = &hashes[0];

    let result = HistoryService::default()
        .checkout(fixture.path(), target)
        .await
        .unwrap();

    assert_eq!(result.workspace.repository.current_branch, None);
    assert_eq!(
        result.workspace.repository.head_short_hash.as_deref(),
        Some(&target[..7])
    );
    assert_eq!(result.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(result.history.commits[0].hash, *target);
}

#[tokio::test]
async fn soft_reset_preserves_the_index_and_worktree() {
    let (fixture, hashes) = three_commit_fixture().await;
    std::fs::write(fixture.path().join("history.txt"), "local\n").unwrap();

    let result = HistoryService::default()
        .reset(
            fixture.path(),
            ResetRequest {
                target: hashes[1].clone(),
                mode: ResetMode::Soft,
                confirmation: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(head_hash(fixture.path()).await, hashes[1]);
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("history.txt")).unwrap(),
        "local\n"
    );
    assert_eq!(result.workspace.changes.staged_count, 1);
    assert_eq!(result.workspace.changes.unstaged_count, 1);
}

#[tokio::test]
async fn mixed_reset_clears_the_index_and_preserves_the_worktree() {
    let (fixture, hashes) = three_commit_fixture().await;
    std::fs::write(fixture.path().join("history.txt"), "local\n").unwrap();

    let result = HistoryService::default()
        .reset(
            fixture.path(),
            ResetRequest {
                target: hashes[1].clone(),
                mode: ResetMode::Mixed,
                confirmation: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(head_hash(fixture.path()).await, hashes[1]);
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("history.txt")).unwrap(),
        "local\n"
    );
    assert_eq!(result.workspace.changes.staged_count, 0);
    assert_eq!(result.workspace.changes.unstaged_count, 1);
}

#[tokio::test]
async fn hard_reset_requires_the_resolved_short_hash_and_replaces_local_changes() {
    let (fixture, hashes) = three_commit_fixture().await;
    let target = &hashes[0];
    std::fs::write(fixture.path().join("history.txt"), "local\n").unwrap();
    let service = HistoryService::default();

    let error = service
        .reset(
            fixture.path(),
            ResetRequest {
                target: target.clone(),
                mode: ResetMode::Hard,
                confirmation: Some("wrong".into()),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidReference);
    assert_eq!(head_hash(fixture.path()).await, hashes[2]);

    let result = service
        .reset(
            fixture.path(),
            ResetRequest {
                target: target.clone(),
                mode: ResetMode::Hard,
                confirmation: Some(target[..7].into()),
            },
        )
        .await
        .unwrap();

    assert_eq!(head_hash(fixture.path()).await, *target);
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("history.txt")).unwrap(),
        "one\n"
    );
    assert!(result.workspace.repository.is_clean);
    assert_eq!(result.history.commits[0].hash, *target);
}

#[tokio::test]
async fn checkout_and_reset_reject_invalid_or_blocked_targets() {
    let (fixture, hashes) = three_commit_fixture().await;
    let service = HistoryService::default();

    let invalid = service
        .checkout(fixture.path(), "missing-commit")
        .await
        .unwrap_err();
    assert_eq!(invalid.code, ErrorCode::InvalidReference);

    run_git(fixture.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(fixture.path().join("history.txt"), "topic\n").unwrap();
    run_git(fixture.path(), &["commit", "-am", "topic change"]).await;
    run_git(fixture.path(), &["switch", "main"]).await;
    std::fs::write(fixture.path().join("history.txt"), "main\n").unwrap();
    run_git(fixture.path(), &["commit", "-am", "main change"]).await;
    GitCommandRunner::default()
        .run_allowing_failure(Some(fixture.path()), ["merge", "--no-edit", "topic"])
        .await
        .unwrap();

    let checkout_error = service
        .checkout(fixture.path(), &hashes[0])
        .await
        .unwrap_err();
    assert_eq!(checkout_error.code, ErrorCode::GitOperationInProgress);
    let reset_error = service
        .reset(
            fixture.path(),
            ResetRequest {
                target: hashes[0].clone(),
                mode: ResetMode::Mixed,
                confirmation: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(reset_error.code, ErrorCode::GitOperationInProgress);
}

#[tokio::test]
async fn cherry_pick_applies_to_the_current_branch() {
    let fixture = repository_fixture().await;
    commit_file(fixture.path(), "base\n", "base").await;
    run_git(fixture.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(fixture.path().join("topic.txt"), "topic\n").unwrap();
    run_git(fixture.path(), &["add", "topic.txt"]).await;
    run_git(fixture.path(), &["commit", "-m", "topic change"]).await;
    let topic_commit = head_hash(fixture.path()).await;
    run_git(fixture.path(), &["switch", "main"]).await;

    let result = HistoryService::default()
        .cherry_pick(
            fixture.path(),
            CherryPickRequest {
                commit: topic_commit,
                target_branch: None,
                return_after_success: false,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    assert_eq!(result.history.commits[0].subject, "topic change");
    assert!(fixture.path().join("topic.txt").exists());
}

#[tokio::test]
async fn cherry_pick_selected_branch_returns_only_after_clean_success() {
    let fixture = repository_fixture().await;
    commit_file(fixture.path(), "base\n", "base").await;
    run_git(fixture.path(), &["branch", "release"]).await;
    run_git(fixture.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(fixture.path().join("topic.txt"), "topic\n").unwrap();
    run_git(fixture.path(), &["add", "topic.txt"]).await;
    run_git(fixture.path(), &["commit", "-m", "topic change"]).await;
    let topic_commit = head_hash(fixture.path()).await;
    run_git(fixture.path(), &["switch", "main"]).await;

    let result = HistoryService::default()
        .cherry_pick(
            fixture.path(),
            CherryPickRequest {
                commit: topic_commit,
                target_branch: Some("release".into()),
                return_after_success: true,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    let release_tip = GitCommandRunner::default()
        .run(
            Some(fixture.path()),
            ["log", "-1", "--format=%s", "release"],
        )
        .await
        .unwrap();
    assert_eq!(release_tip.stdout.trim(), "topic change");
    assert_eq!(result.history.commits[0].subject, "base");
}

#[tokio::test]
async fn cherry_pick_conflict_stays_on_target_and_exposes_abort() {
    let fixture = repository_fixture().await;
    commit_file(fixture.path(), "base\n", "base").await;
    run_git(fixture.path(), &["branch", "release"]).await;
    run_git(fixture.path(), &["switch", "-c", "topic"]).await;
    commit_file(fixture.path(), "topic\n", "topic change").await;
    let topic_commit = head_hash(fixture.path()).await;
    run_git(fixture.path(), &["switch", "release"]).await;
    commit_file(fixture.path(), "release\n", "release change").await;
    run_git(fixture.path(), &["switch", "main"]).await;

    let result = HistoryService::default()
        .cherry_pick(
            fixture.path(),
            CherryPickRequest {
                commit: topic_commit,
                target_branch: Some("release".into()),
                return_after_success: true,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("release")
    );
    assert_eq!(
        result.operation_state.kind,
        RepositoryOperationKind::CherryPick
    );
    assert_eq!(
        result.operation_state.abort_action,
        Some(AbortAction::CherryPick)
    );
    assert_eq!(result.workspace.repository.conflict_count, 1);

    let aborted = RefsService::default()
        .abort(fixture.path(), AbortAction::CherryPick)
        .await
        .unwrap();
    assert_eq!(aborted.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(
        aborted.workspace.repository.current_branch.as_deref(),
        Some("release")
    );
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("history.txt")).unwrap(),
        "release\n"
    );
}

#[tokio::test]
async fn cherry_pick_preserves_unstaged_and_untracked_changes_and_existing_stashes() {
    for target in [None, Some(false), Some(true)] {
        let fixture = repository_fixture().await;
        let root = fixture.path();
        commit_file(root, "base\n", "base").await;
        commit_file(root, "original stash\n", "temporary").await;
        std::fs::write(root.join("history.txt"), "old backup\n").unwrap();
        run_git(root, &["stash", "push", "-m", "user backup"]).await;
        let old_stashes = git_text(root, &["stash", "list", "--format=%H"]).await;
        run_git(root, &["branch", "release"]).await;
        run_git(root, &["switch", "-c", "topic"]).await;
        std::fs::write(root.join("picked.txt"), "committed\n").unwrap();
        run_git(root, &["add", "picked.txt"]).await;
        run_git(root, &["commit", "-m", "picked change"]).await;
        let commit = head_hash(root).await;
        run_git(root, &["switch", "main"]).await;
        std::fs::write(root.join("history.txt"), "local unstaged\n").unwrap();
        let binary = [0, 255, 10, 128, 13];
        std::fs::write(root.join("本地文件.bin"), binary).unwrap();
        let result = HistoryService::default()
            .cherry_pick(
                root,
                CherryPickRequest {
                    commit,
                    target_branch: target.map(|_| "release".into()),
                    return_after_success: target == Some(true),
                },
            )
            .await
            .unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(
            result.workspace.repository.current_branch.as_deref(),
            Some(if target == Some(false) {
                "release"
            } else {
                "main"
            })
        );
        assert_eq!(
            std::fs::read_to_string(root.join("history.txt")).unwrap(),
            if target == Some(false) {
                "original stash\n"
            } else {
                "local unstaged\n"
            }
        );
        if target == Some(false) {
            assert!(!root.join("本地文件.bin").exists());
            assert_eq!(
                git_text(root, &["show", "stash@{0}:history.txt"]).await,
                "local unstaged"
            );
            let retained = git_text(root, &["stash", "list", "--format=%H"]).await;
            assert!(retained.ends_with(&old_stashes));
            assert_eq!(retained.lines().count(), 2);
            assert!(
                result
                    .notice
                    .as_deref()
                    .unwrap_or_default()
                    .contains("main")
            );
        } else {
            assert_eq!(std::fs::read(root.join("本地文件.bin")).unwrap(), binary);
            assert_eq!(
                git_text(root, &["stash", "list", "--format=%H"]).await,
                old_stashes
            );
        }
        assert_eq!(git_text(root, &["diff", "--cached"]).await, "");
        let picked_branch = if target.is_some() { "release" } else { "main" };
        assert_eq!(
            git_text(root, &["show", &format!("{picked_branch}:history.txt")]).await,
            "original stash"
        );
        assert_eq!(
            git_text(root, &["show", &format!("{picked_branch}:picked.txt")]).await,
            "committed"
        );
    }
}

#[path = "support/nested_work.rs"]
mod nested_work;

#[tokio::test]
async fn cherry_pick_preserves_nested_residuals_on_current_target_and_return_branches() {
    for submodule in [false, true] {
        for target in [None, Some(false), Some(true)] {
            let fixture = repository_fixture().await;
            let root = fixture.path();
            commit_file(root, "base\n", "base").await;
            nested_work::prepare(root, submodule).await;
            std::fs::write(root.join("history.txt"), "old user backup\n").unwrap();
            run_git(root, &["stash", "push", "-m", "user backup"]).await;
            let stashes = git_text(root, &["stash", "list", "--format=%H"]).await;
            run_git(root, &["branch", "release"]).await;
            run_git(root, &["switch", "-c", "topic"]).await;
            std::fs::write(root.join("picked.txt"), "picked\n").unwrap();
            run_git(root, &["add", "picked.txt"]).await;
            run_git(root, &["commit", "-m", "picked"]).await;
            let commit = head_hash(root).await;
            run_git(root, &["switch", "main"]).await;
            let before = nested_work::dirty(root).await;
            std::fs::write(root.join("history.txt"), "local\n").unwrap();
            std::fs::write(root.join("local.txt"), "untracked\n").unwrap();
            let result = HistoryService::default()
                .cherry_pick(
                    root,
                    CherryPickRequest {
                        commit,
                        target_branch: target.map(|_| "release".into()),
                        return_after_success: target == Some(true),
                    },
                )
                .await
                .unwrap();
            assert!(result.error.is_none(), "{:?}", result.error);
            assert_eq!(
                result.workspace.repository.current_branch.as_deref(),
                Some(if target == Some(false) {
                    "release"
                } else {
                    "main"
                })
            );
            let branch = if target.is_some() { "release" } else { "main" };
            assert_eq!(
                git_text(root, &["show", &format!("{branch}:picked.txt")]).await,
                "picked"
            );
            if target == Some(false) {
                assert_eq!(std::fs::read(root.join("history.txt")).unwrap(), b"base\n");
                assert!(!root.join("local.txt").exists());
                assert_eq!(
                    git_text(root, &["show", "stash@{0}^3:local.txt"]).await,
                    "untracked"
                );
                assert!(
                    git_text(root, &["stash", "list", "--format=%H"])
                        .await
                        .ends_with(&stashes)
                );
            } else {
                assert_eq!(std::fs::read(root.join("history.txt")).unwrap(), b"local\n");
                assert_eq!(
                    std::fs::read(root.join("local.txt")).unwrap(),
                    b"untracked\n"
                );
                assert_eq!(
                    git_text(root, &["stash", "list", "--format=%H"]).await,
                    stashes
                );
            }
            assert_eq!(git_text(root, &["diff", "--cached"]).await, "");
            nested_work::assert_preserved(root, &before).await;
        }
    }
}

async fn git_text(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
        .trim()
        .to_owned()
}

#[tokio::test]
async fn cherry_pick_isolates_source_ignored_files_and_restores_only_after_return() {
    for return_after in [false, true] {
        let fixture = repository_fixture().await;
        let root = fixture.path();
        commit_file(root, "base\n", "base").await;
        run_git(root, &["branch", "release"]).await;
        std::fs::write(root.join(".gitignore"), ".agents/\n").unwrap();
        run_git(root, &["add", ".gitignore"]).await;
        run_git(root, &["commit", "-m", "local ignore rules"]).await;
        run_git(root, &["switch", "-c", "topic"]).await;
        std::fs::write(root.join("picked.txt"), "picked\n").unwrap();
        run_git(root, &["add", "picked.txt"]).await;
        run_git(root, &["commit", "-m", "picked change"]).await;
        let commit = head_hash(root).await;
        run_git(root, &["switch", "main"]).await;
        std::fs::create_dir(root.join(".agents")).unwrap();
        std::fs::write(root.join(".agents/local.md"), "source ignored\n").unwrap();
        let result = HistoryService::default()
            .cherry_pick(
                root,
                CherryPickRequest {
                    commit,
                    target_branch: Some("release".into()),
                    return_after_success: return_after,
                },
            )
            .await
            .unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(git_text(root, &["status", "--porcelain=v1"]).await, "");
        assert_eq!(
            git_text(root, &["branch", "--show-current"]).await,
            if return_after { "main" } else { "release" }
        );
        if return_after {
            assert_eq!(
                std::fs::read(root.join(".agents/local.md")).unwrap(),
                b"source ignored\n"
            );
            assert_eq!(git_text(root, &["stash", "list"]).await, "");
        } else {
            assert!(!root.join(".agents/local.md").exists());
            assert_eq!(
                git_text(root, &["show", "stash@{0}^3:.agents/local.md"]).await,
                "source ignored"
            );
            assert!(
                result
                    .notice
                    .as_deref()
                    .unwrap_or_default()
                    .contains("main")
            );
            run_git(root, &["switch", "main"]).await;
            run_git(root, &["stash", "apply", "stash@{0}"]).await;
            assert_eq!(
                std::fs::read(root.join(".agents/local.md")).unwrap(),
                b"source ignored\n"
            );
        }
    }
}

#[tokio::test]
async fn cherry_pick_switch_does_not_overwrite_source_ignored_files() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "base\n", "base").await;
    run_git(root, &["switch", "-c", "release"]).await;
    std::fs::create_dir(root.join(".agents")).unwrap();
    std::fs::write(root.join(".agents/local.md"), "target committed\n").unwrap();
    run_git(root, &["add", ".agents/local.md"]).await;
    run_git(root, &["commit", "-m", "target file"]).await;
    let target_head = head_hash(root).await;
    run_git(root, &["switch", "main"]).await;
    std::fs::write(root.join(".gitignore"), ".agents/\n").unwrap();
    run_git(root, &["add", ".gitignore"]).await;
    run_git(root, &["commit", "-m", "source ignores"]).await;
    let source_head = head_hash(root).await;
    run_git(root, &["switch", "-c", "topic"]).await;
    std::fs::write(root.join("picked.txt"), "picked\n").unwrap();
    run_git(root, &["add", "picked.txt"]).await;
    run_git(root, &["commit", "-m", "picked"]).await;
    let commit = head_hash(root).await;
    run_git(root, &["switch", "main"]).await;
    std::fs::create_dir_all(root.join(".agents")).unwrap();
    std::fs::write(root.join(".agents/local.md"), "source unsaved\n").unwrap();
    let result = HistoryService::default()
        .cherry_pick(
            root,
            CherryPickRequest {
                commit,
                target_branch: Some("release".into()),
                return_after_success: false,
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_some());
    assert_eq!(head_hash(root).await, source_head);
    assert_eq!(git_text(root, &["rev-parse", "release"]).await, target_head);
    assert_eq!(
        std::fs::read(root.join(".agents/local.md")).unwrap(),
        b"source unsaved\n"
    );
    assert_eq!(git_text(root, &["stash", "list"]).await, "");
}

#[tokio::test]
async fn cherry_pick_restore_conflict_retains_recoverable_backup_and_completed_commit() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "base\n", "base").await;
    run_git(root, &["switch", "-c", "topic"]).await;
    commit_file(root, "picked\n", "picked change").await;
    let commit = head_hash(root).await;
    run_git(root, &["switch", "main"]).await;
    std::fs::write(root.join("history.txt"), "unsaved local\n").unwrap();
    let result = HistoryService::default()
        .cherry_pick(
            root,
            CherryPickRequest {
                commit,
                target_branch: None,
                return_after_success: false,
            },
        )
        .await
        .unwrap();
    assert!(result.error.unwrap().message.contains("移植操作已完成"));
    assert_eq!(
        git_text(root, &["show", "HEAD:history.txt"]).await,
        "picked"
    );
    assert_eq!(
        git_text(root, &["show", "stash@{0}:history.txt"]).await,
        "unsaved local"
    );
    assert_eq!(result.operation_state.kind, RepositoryOperationKind::None);
    assert!(!result.operation_state.conflicts.is_empty());
}

#[tokio::test]
async fn cherry_pick_does_not_hide_or_commit_staged_changes() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "base\n", "base").await;
    let commit = head_hash(root).await;
    std::fs::write(root.join("history.txt"), "staged\n").unwrap();
    run_git(root, &["add", "history.txt"]).await;
    std::fs::write(root.join("history.txt"), "unstaged\n").unwrap();
    let index = git_text(root, &["write-tree"]).await;
    let error = HistoryService::default()
        .cherry_pick(
            root,
            CherryPickRequest {
                commit: commit.clone(),
                target_branch: None,
                return_after_success: false,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DirtyWorktree);
    assert_eq!(head_hash(root).await, commit);
    assert_eq!(git_text(root, &["write-tree"]).await, index);
    assert_eq!(
        std::fs::read_to_string(root.join("history.txt")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(git_text(root, &["stash", "list"]).await, "");
}

#[tokio::test]
async fn cherry_pick_failed_switch_restores_unstaged_work() {
    let fixture = repository_fixture().await;
    let linked = tempfile::tempdir().unwrap();
    let root = fixture.path();
    commit_file(root, "base\n", "base").await;
    let commit = head_hash(root).await;
    run_git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "occupied",
            linked.path().to_str().unwrap(),
        ],
    )
    .await;
    std::fs::write(root.join("history.txt"), "local\n").unwrap();
    let result = HistoryService::default()
        .cherry_pick(
            root,
            CherryPickRequest {
                commit: commit.clone(),
                target_branch: Some("occupied".into()),
                return_after_success: true,
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_some());
    assert_eq!(head_hash(root).await, commit);
    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("history.txt")).unwrap(),
        "local\n"
    );
    assert_eq!(git_text(root, &["stash", "list"]).await, "");
}

#[tokio::test]
async fn cherry_pick_conflict_keeps_unstaged_backup_out_of_conflict_index() {
    let fixture = repository_fixture().await;
    let root = fixture.path();
    commit_file(root, "base\n", "base").await;
    run_git(root, &["switch", "-c", "topic"]).await;
    commit_file(root, "topic\n", "topic").await;
    let commit = head_hash(root).await;
    run_git(root, &["switch", "main"]).await;
    commit_file(root, "main\n", "main").await;
    std::fs::write(root.join("history.txt"), "unstaged\n").unwrap();
    std::fs::write(root.join("local.txt"), "untracked\n").unwrap();
    let result = HistoryService::default()
        .cherry_pick(
            root,
            CherryPickRequest {
                commit,
                target_branch: None,
                return_after_success: false,
            },
        )
        .await
        .unwrap();
    assert!(result.error.unwrap().message.contains("自动贮藏备份"));
    assert_eq!(
        result.operation_state.kind,
        RepositoryOperationKind::CherryPick
    );
    assert_eq!(git_text(root, &["show", ":2:history.txt"]).await, "main");
    assert_eq!(git_text(root, &["show", ":3:history.txt"]).await, "topic");
    assert_eq!(
        git_text(root, &["show", "stash@{0}:history.txt"]).await,
        "unstaged"
    );
    assert_eq!(
        git_text(root, &["show", "stash@{0}^3:local.txt"]).await,
        "untracked"
    );
    run_git(root, &["cherry-pick", "--abort"]).await;
    run_git(root, &["stash", "apply", "stash@{0}"]).await;
    assert_eq!(
        std::fs::read_to_string(root.join("history.txt")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("local.txt")).unwrap(),
        "untracked\n"
    );
}

#[tokio::test]
async fn cherry_pick_rejects_invalid_commits_and_non_local_target_branches() {
    let fixture = repository_fixture().await;
    commit_file(fixture.path(), "base\n", "base").await;
    let service = HistoryService::default();

    let invalid_commit = service
        .cherry_pick(
            fixture.path(),
            CherryPickRequest {
                commit: "missing".into(),
                target_branch: None,
                return_after_success: false,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(invalid_commit.code, ErrorCode::InvalidReference);

    let invalid_target = service
        .cherry_pick(
            fixture.path(),
            CherryPickRequest {
                commit: head_hash(fixture.path()).await,
                target_branch: Some("missing".into()),
                return_after_success: false,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(invalid_target.code, ErrorCode::BranchUnavailable);
}
