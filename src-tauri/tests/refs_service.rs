use std::path::Path;

use hq_git_lib::application::changes_service::ChangesService;
use hq_git_lib::application::refs_service::RefsService;
use hq_git_lib::domain::error::ErrorCode;
use hq_git_lib::domain::operation::{AbortAction, RepositoryOperationKind};
use hq_git_lib::domain::refs::{BranchKind, CreateBranchRequest, DeleteBranchRequest};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use tempfile::TempDir;

async fn run_git(root: &Path, args: &[&str]) {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap();
}

async fn committed_fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    run_git(directory.path(), &["init", "-b", "main"]).await;
    run_git(directory.path(), &["config", "user.name", "HQ Test"]).await;
    run_git(
        directory.path(),
        &["config", "user.email", "hq@example.test"],
    )
    .await;
    run_git(directory.path(), &["config", "core.autocrlf", "false"]).await;
    std::fs::write(directory.path().join("README.md"), "base\n").unwrap();
    run_git(directory.path(), &["add", "README.md"]).await;
    run_git(directory.path(), &["commit", "-m", "initial subject"]).await;
    directory
}

#[tokio::test]
async fn snapshot_reads_branches_tracking_and_both_tag_kinds() {
    let fixture = committed_fixture().await;
    run_git(fixture.path(), &["branch", "feature/功能"]).await;
    run_git(fixture.path(), &["remote", "add", "origin", "."]).await;
    run_git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .await;
    run_git(fixture.path(), &["config", "branch.main.remote", "origin"]).await;
    run_git(
        fixture.path(),
        &["config", "branch.main.merge", "refs/heads/main"],
    )
    .await;
    std::fs::write(fixture.path().join("README.md"), "base\nsecond\n").unwrap();
    run_git(fixture.path(), &["add", "README.md"]).await;
    run_git(fixture.path(), &["commit", "-m", "second subject"]).await;
    run_git(fixture.path(), &["tag", "v1-light"]).await;
    run_git(
        fixture.path(),
        &["tag", "-a", "v2-annotated", "-m", "release notes"],
    )
    .await;

    let snapshot = RefsService::default()
        .snapshot(fixture.path())
        .await
        .unwrap();

    let main = snapshot
        .local_branches
        .iter()
        .find(|branch| branch.name == "main")
        .unwrap();
    assert_eq!(main.kind, BranchKind::Local);
    assert!(main.current);
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!((main.ahead, main.behind), (Some(1), Some(0)));
    assert_eq!(main.tip.subject, "second subject");
    assert_eq!(snapshot.remote_branches[0].name, "origin/main");

    let lightweight = snapshot
        .tags
        .iter()
        .find(|tag| tag.name == "v1-light")
        .unwrap();
    assert!(!lightweight.annotated);
    assert_eq!(lightweight.commit_subject, "second subject");

    let annotated = snapshot
        .tags
        .iter()
        .find(|tag| tag.name == "v2-annotated")
        .unwrap();
    assert!(annotated.annotated);
    assert_eq!(annotated.annotation.as_deref(), Some("release notes"));
    assert_eq!(annotated.tagger.as_deref(), Some("HQ Test"));
}

#[tokio::test]
async fn snapshot_handles_detached_head_and_unborn_repository() {
    let committed = committed_fixture().await;
    run_git(committed.path(), &["switch", "--detach", "HEAD"]).await;
    let detached = RefsService::default()
        .snapshot(committed.path())
        .await
        .unwrap();
    assert!(detached.local_branches.iter().all(|branch| !branch.current));

    let unborn = tempfile::tempdir().unwrap();
    run_git(unborn.path(), &["init", "-b", "main"]).await;
    let empty = RefsService::default()
        .snapshot(unborn.path())
        .await
        .unwrap();
    assert!(empty.local_branches.is_empty());
    assert!(empty.remote_branches.is_empty());
    assert!(empty.tags.is_empty());
}

#[tokio::test]
async fn branch_create_switch_and_delete_return_authoritative_snapshots() {
    let fixture = committed_fixture().await;
    let service = RefsService::default();

    let created = service
        .create_branch(
            fixture.path(),
            CreateBranchRequest {
                name: "topic".into(),
                start_point: None,
                switch: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        created.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    assert_eq!(created.operation_state.kind, RepositoryOperationKind::None);
    assert!(
        created
            .refs
            .local_branches
            .iter()
            .any(|branch| branch.name == "topic" && !branch.current)
    );

    let switched = service
        .switch_branch(fixture.path(), "topic")
        .await
        .unwrap();
    assert_eq!(
        switched.workspace.repository.current_branch.as_deref(),
        Some("topic")
    );
    assert!(
        switched
            .refs
            .local_branches
            .iter()
            .any(|branch| branch.name == "topic" && branch.current)
    );

    service.switch_branch(fixture.path(), "main").await.unwrap();
    let deleted = service
        .delete_branch(
            fixture.path(),
            DeleteBranchRequest {
                name: "topic".into(),
                force: false,
                confirmation: None,
            },
        )
        .await
        .unwrap();
    assert!(
        deleted
            .refs
            .local_branches
            .iter()
            .all(|branch| branch.name != "topic")
    );
}

#[tokio::test]
async fn branch_create_rejects_invalid_names_and_start_points() {
    let fixture = committed_fixture().await;
    let service = RefsService::default();

    let invalid_name = service
        .create_branch(
            fixture.path(),
            CreateBranchRequest {
                name: "-danger".into(),
                start_point: None,
                switch: false,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(invalid_name.code, ErrorCode::InvalidReference);

    let padded_name = service
        .create_branch(
            fixture.path(),
            CreateBranchRequest {
                name: " topic ".into(),
                start_point: None,
                switch: false,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(padded_name.code, ErrorCode::InvalidReference);

    let invalid_start = service
        .create_branch(
            fixture.path(),
            CreateBranchRequest {
                name: "topic".into(),
                start_point: Some("missing-start".into()),
                switch: false,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(invalid_start.code, ErrorCode::InvalidReference);

    let snapshot = service.snapshot(fixture.path()).await.unwrap();
    assert_eq!(snapshot.local_branches.len(), 1);
    assert_eq!(snapshot.local_branches[0].name, "main");
}

#[tokio::test]
async fn branch_create_can_switch_from_an_explicit_valid_start_point() {
    let fixture = committed_fixture().await;
    let service = RefsService::default();

    let result = service
        .create_branch(
            fixture.path(),
            CreateBranchRequest {
                name: "topic".into(),
                start_point: Some("HEAD".into()),
                switch: true,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("topic")
    );
    assert!(
        result
            .refs
            .local_branches
            .iter()
            .any(|branch| branch.name == "topic" && branch.current)
    );
}

#[tokio::test]
async fn branch_switch_allows_safe_dirty_changes_and_rejects_overwrites() {
    let fixture = committed_fixture().await;
    run_git(fixture.path(), &["branch", "safe-target"]).await;
    run_git(fixture.path(), &["switch", "-c", "overwriting-target"]).await;
    std::fs::write(fixture.path().join("README.md"), "target\n").unwrap();
    run_git(fixture.path(), &["commit", "-am", "target change"]).await;
    run_git(fixture.path(), &["switch", "main"]).await;
    std::fs::write(fixture.path().join("README.md"), "local dirty\n").unwrap();
    let service = RefsService::default();

    let safe = service
        .switch_branch(fixture.path(), "safe-target")
        .await
        .unwrap();
    assert_eq!(
        safe.workspace.repository.current_branch.as_deref(),
        Some("safe-target")
    );
    assert_eq!(safe.workspace.changes.unstaged_count, 1);

    let overwrite = service
        .switch_branch(fixture.path(), "overwriting-target")
        .await
        .unwrap_err();
    assert_eq!(overwrite.code, ErrorCode::DirtyWorktree);

    let create_overwrite = service
        .create_branch(
            fixture.path(),
            CreateBranchRequest {
                name: "new-overwriting-target".into(),
                start_point: Some("overwriting-target".into()),
                switch: true,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(create_overwrite.code, ErrorCode::DirtyWorktree);

    let snapshot = service.snapshot(fixture.path()).await.unwrap();
    assert!(
        snapshot
            .local_branches
            .iter()
            .any(|branch| branch.name == "safe-target" && branch.current)
    );
    assert!(
        snapshot
            .local_branches
            .iter()
            .all(|branch| branch.name != "new-overwriting-target")
    );
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("README.md")).unwrap(),
        "local dirty\n"
    );
}

#[tokio::test]
async fn branch_delete_protects_current_and_unmerged_branches() {
    let fixture = committed_fixture().await;
    run_git(fixture.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(fixture.path().join("topic.txt"), "topic\n").unwrap();
    run_git(fixture.path(), &["add", "topic.txt"]).await;
    run_git(fixture.path(), &["commit", "-m", "topic"]).await;
    run_git(fixture.path(), &["switch", "main"]).await;
    let service = RefsService::default();

    let current = service
        .delete_branch(
            fixture.path(),
            DeleteBranchRequest {
                name: "main".into(),
                force: false,
                confirmation: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(current.code, ErrorCode::CurrentBranchDeletion);

    let unmerged = service
        .delete_branch(
            fixture.path(),
            DeleteBranchRequest {
                name: "topic".into(),
                force: false,
                confirmation: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(unmerged.code, ErrorCode::UnmergedBranchDeletion);

    let wrong_confirmation = service
        .delete_branch(
            fixture.path(),
            DeleteBranchRequest {
                name: "topic".into(),
                force: true,
                confirmation: Some("wrong".into()),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(wrong_confirmation.code, ErrorCode::InvalidReference);

    let deleted = service
        .delete_branch(
            fixture.path(),
            DeleteBranchRequest {
                name: "topic".into(),
                force: true,
                confirmation: Some("topic".into()),
            },
        )
        .await
        .unwrap();
    assert!(
        deleted
            .refs
            .local_branches
            .iter()
            .all(|branch| branch.name != "topic")
    );
}

#[tokio::test]
async fn branch_mutations_handle_detached_head_stale_selection_and_operation_gate() {
    let fixture = committed_fixture().await;
    run_git(fixture.path(), &["branch", "topic"]).await;
    run_git(fixture.path(), &["switch", "--detach", "HEAD"]).await;
    let service = RefsService::default();

    let detached_delete = service
        .delete_branch(
            fixture.path(),
            DeleteBranchRequest {
                name: "topic".into(),
                force: false,
                confirmation: None,
            },
        )
        .await
        .unwrap();
    assert!(
        detached_delete
            .refs
            .local_branches
            .iter()
            .all(|branch| !branch.current)
    );

    let stale_switch = service
        .switch_branch(fixture.path(), "missing")
        .await
        .unwrap_err();
    assert_eq!(stale_switch.code, ErrorCode::BranchUnavailable);
    let stale_delete = service
        .delete_branch(
            fixture.path(),
            DeleteBranchRequest {
                name: "missing".into(),
                force: false,
                confirmation: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(stale_delete.code, ErrorCode::BranchUnavailable);

    std::fs::write(
        fixture.path().join(".git").join("CHERRY_PICK_HEAD"),
        "a".repeat(40),
    )
    .unwrap();
    let blocked = service
        .switch_branch(fixture.path(), "main")
        .await
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::GitOperationInProgress);
}

async fn conflicting_branches_fixture() -> TempDir {
    let directory = committed_fixture().await;
    run_git(directory.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(directory.path().join("README.md"), "topic\n").unwrap();
    run_git(directory.path(), &["commit", "-am", "topic change"]).await;
    run_git(directory.path(), &["switch", "main"]).await;
    std::fs::write(directory.path().join("README.md"), "main\n").unwrap();
    run_git(directory.path(), &["commit", "-am", "main change"]).await;
    directory
}

#[tokio::test]
async fn merge_returns_clean_and_recoverable_conflict_states() {
    let clean = committed_fixture().await;
    run_git(clean.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(clean.path().join("topic.txt"), "topic\n").unwrap();
    run_git(clean.path(), &["add", "topic.txt"]).await;
    run_git(clean.path(), &["commit", "-m", "topic"]).await;
    run_git(clean.path(), &["switch", "main"]).await;
    let service = RefsService::default();

    let merged = service.merge(clean.path(), "topic").await.unwrap();
    assert_eq!(merged.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(merged.workspace.repository.conflict_count, 0);
    assert!(clean.path().join("topic.txt").exists());

    let conflict = conflicting_branches_fixture().await;
    let conflicted = service.merge(conflict.path(), "topic").await.unwrap();
    assert_eq!(
        conflicted.operation_state.kind,
        RepositoryOperationKind::Merge
    );
    assert_eq!(
        conflicted.operation_state.abort_action,
        Some(AbortAction::Merge)
    );
    assert_eq!(conflicted.workspace.repository.conflict_count, 1);
    assert!(
        conflicted
            .workspace
            .changes
            .files
            .iter()
            .any(|file| file.path == "README.md" && file.conflict)
    );

    let blocked = service
        .switch_branch(conflict.path(), "topic")
        .await
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::GitOperationInProgress);
    let mismatch = service
        .abort(conflict.path(), AbortAction::Rebase)
        .await
        .unwrap_err();
    assert_eq!(mismatch.code, ErrorCode::GitOperationInProgress);

    let aborted = service
        .abort(conflict.path(), AbortAction::Merge)
        .await
        .unwrap();
    assert_eq!(aborted.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(aborted.workspace.repository.conflict_count, 0);
    assert_eq!(
        std::fs::read_to_string(conflict.path().join("README.md")).unwrap(),
        "main\n"
    );
}

#[tokio::test]
async fn merge_into_selected_destination_preserves_other_branch_tips() {
    let fixture = committed_fixture().await;
    let root = fixture.path();
    run_git(root, &["branch", "destination"]).await;
    run_git(root, &["switch", "-c", "source"]).await;
    std::fs::write(root.join("feature.txt"), "feature\n").unwrap();
    run_git(root, &["add", "feature.txt"]).await;
    run_git(root, &["commit", "-m", "feature"]).await;
    let before = RefsService::default().snapshot(root).await.unwrap();
    run_git(root, &["switch", "main"]).await;
    let result = RefsService::default()
        .merge_into(root, "source", Some("destination"))
        .await
        .unwrap();
    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("destination")
    );
    assert!(root.join("feature.txt").exists());
    for name in ["main", "source"] {
        assert_eq!(
            result
                .refs
                .local_branches
                .iter()
                .find(|b| b.name == name)
                .unwrap()
                .tip
                .full_hash,
            before
                .local_branches
                .iter()
                .find(|b| b.name == name)
                .unwrap()
                .tip
                .full_hash
        );
    }
}

#[tokio::test]
async fn merge_into_rejects_dirty_switch_missing_and_identical_branches() {
    let fixture = committed_fixture().await;
    let root = fixture.path();
    run_git(root, &["branch", "destination"]).await;
    std::fs::write(root.join("private.txt"), "unsaved").unwrap();
    let service = RefsService::default();
    for (source, destination, code) in [
        ("main", "destination", ErrorCode::DirtyWorktree),
        ("main", "main", ErrorCode::InvalidReference),
        ("missing", "destination", ErrorCode::BranchUnavailable),
        ("main", "missing", ErrorCode::BranchUnavailable),
    ] {
        assert_eq!(
            service
                .merge_into(root, source, Some(destination))
                .await
                .unwrap_err()
                .code,
            code
        );
    }
    assert!(
        service
            .snapshot(root)
            .await
            .unwrap()
            .local_branches
            .iter()
            .any(|b| b.name == "main" && b.current)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("private.txt")).unwrap(),
        "unsaved"
    );
}

#[tokio::test]
async fn merge_into_conflict_remains_on_selected_destination_and_can_abort() {
    let fixture = conflicting_branches_fixture().await;
    run_git(fixture.path(), &["switch", "topic"]).await;
    let service = RefsService::default();
    let result = service
        .merge_into(fixture.path(), "topic", Some("main"))
        .await
        .unwrap();
    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    assert_eq!(result.operation_state.kind, RepositoryOperationKind::Merge);
    let aborted = service
        .abort(fixture.path(), AbortAction::Merge)
        .await
        .unwrap();
    assert_eq!(
        aborted.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    assert_eq!(
        std::fs::read_to_string(fixture.path().join("README.md")).unwrap(),
        "main\n"
    );
}

#[tokio::test]
async fn rebase_returns_clean_and_recoverable_conflict_states() {
    let clean = committed_fixture().await;
    run_git(clean.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(clean.path().join("topic.txt"), "topic\n").unwrap();
    run_git(clean.path(), &["add", "topic.txt"]).await;
    run_git(clean.path(), &["commit", "-m", "topic"]).await;
    run_git(clean.path(), &["switch", "main"]).await;
    std::fs::write(clean.path().join("main.txt"), "main\n").unwrap();
    run_git(clean.path(), &["add", "main.txt"]).await;
    run_git(clean.path(), &["commit", "-m", "main"]).await;
    let service = RefsService::default();

    let rebased = service.rebase(clean.path(), "topic").await.unwrap();
    assert_eq!(rebased.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(rebased.workspace.repository.conflict_count, 0);
    assert!(clean.path().join("topic.txt").exists());
    assert!(clean.path().join("main.txt").exists());

    let conflict = conflicting_branches_fixture().await;
    let conflicted = service.rebase(conflict.path(), "topic").await.unwrap();
    assert_eq!(
        conflicted.operation_state.kind,
        RepositoryOperationKind::Rebase
    );
    assert_eq!(
        conflicted.operation_state.abort_action,
        Some(AbortAction::Rebase)
    );
    assert_eq!(conflicted.workspace.repository.conflict_count, 1);

    let blocked = ChangesService::default()
        .stage_file(conflict.path(), "README.md")
        .await
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::GitOperationInProgress);

    let mismatch = service
        .abort(conflict.path(), AbortAction::Merge)
        .await
        .unwrap_err();
    assert_eq!(mismatch.code, ErrorCode::GitOperationInProgress);
    let aborted = service
        .abort(conflict.path(), AbortAction::Rebase)
        .await
        .unwrap();
    assert_eq!(aborted.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(
        aborted.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    assert_eq!(
        std::fs::read_to_string(conflict.path().join("README.md")).unwrap(),
        "main\n"
    );
}

#[tokio::test]
async fn abort_without_a_matching_operation_is_rejected() {
    let fixture = committed_fixture().await;

    let error = RefsService::default()
        .abort(fixture.path(), AbortAction::Merge)
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::GitOperationInProgress);
}
