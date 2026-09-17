use std::path::Path;

use hq_git_lib::application::operation_state::{
    MutationIntent, ensure_mutation_allowed, read_operation_state,
};
use hq_git_lib::domain::error::ErrorCode;
use hq_git_lib::domain::operation::{
    AbortAction, ConflictFileSummary, RepositoryOperationKind, RepositoryOperationState,
};
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
    std::fs::write(directory.path().join("note.txt"), "base\n").unwrap();
    run_git(directory.path(), &["add", "note.txt"]).await;
    run_git(directory.path(), &["commit", "-m", "base"]).await;
    directory
}

async fn conflicted_merge_fixture() -> TempDir {
    let directory = committed_fixture().await;
    run_git(directory.path(), &["switch", "-c", "topic"]).await;
    std::fs::write(directory.path().join("note.txt"), "topic\n").unwrap();
    run_git(directory.path(), &["commit", "-am", "topic"]).await;
    run_git(directory.path(), &["switch", "main"]).await;
    std::fs::write(directory.path().join("note.txt"), "main\n").unwrap();
    run_git(directory.path(), &["commit", "-am", "main"]).await;
    let _ = GitCommandRunner::default()
        .run(Some(directory.path()), ["merge", "--no-edit", "topic"])
        .await
        .unwrap_err();
    directory
}

#[tokio::test]
async fn detects_merge_rebase_cherry_pick_and_unresolved_only_states() {
    let merge = conflicted_merge_fixture().await;
    let runner = GitCommandRunner::default();
    let state = read_operation_state(merge.path(), &runner).await.unwrap();
    assert_eq!(state.kind, RepositoryOperationKind::Merge);
    assert_eq!(state.abort_action, Some(AbortAction::Merge));
    assert!(state.conflicts.iter().any(|file| file.path == "note.txt"));

    std::fs::remove_file(merge.path().join(".git").join("MERGE_HEAD")).unwrap();
    let unresolved = read_operation_state(merge.path(), &runner).await.unwrap();
    assert_eq!(unresolved.kind, RepositoryOperationKind::None);
    assert_eq!(unresolved.abort_action, None);
    assert_eq!(unresolved.conflicts.len(), 1);

    let sequencers = committed_fixture().await;
    std::fs::create_dir(sequencers.path().join(".git").join("rebase-merge")).unwrap();
    let rebase = read_operation_state(sequencers.path(), &runner)
        .await
        .unwrap();
    assert_eq!(rebase.kind, RepositoryOperationKind::Rebase);
    assert_eq!(rebase.abort_action, Some(AbortAction::Rebase));
    std::fs::remove_dir(sequencers.path().join(".git").join("rebase-merge")).unwrap();
    std::fs::write(
        sequencers.path().join(".git").join("CHERRY_PICK_HEAD"),
        "a".repeat(40),
    )
    .unwrap();
    let cherry_pick = read_operation_state(sequencers.path(), &runner)
        .await
        .unwrap();
    assert_eq!(cherry_pick.kind, RepositoryOperationKind::CherryPick);
    assert_eq!(cherry_pick.abort_action, Some(AbortAction::CherryPick));
}

#[test]
fn gate_allows_only_matching_abort_during_an_operation() {
    let conflict = ConflictFileSummary {
        path: "note.txt".into(),
        status: "UU".into(),
    };
    let state = RepositoryOperationState::merge(vec![conflict]);

    assert!(ensure_mutation_allowed(&state, MutationIntent::AbortMerge).is_ok());
    assert_eq!(
        ensure_mutation_allowed(&state, MutationIntent::SwitchBranch)
            .unwrap_err()
            .code,
        ErrorCode::GitOperationInProgress,
    );
    assert_eq!(
        ensure_mutation_allowed(&state, MutationIntent::AbortRebase)
            .unwrap_err()
            .code,
        ErrorCode::GitOperationInProgress,
    );
}

#[test]
fn gate_blocks_ordinary_mutations_for_unresolved_only_conflicts() {
    let state = RepositoryOperationState {
        kind: RepositoryOperationKind::None,
        conflicts: vec![ConflictFileSummary {
            path: "note.txt".into(),
            status: "UU".into(),
        }],
        abort_action: None,
    };

    assert_eq!(
        ensure_mutation_allowed(&state, MutationIntent::Commit)
            .unwrap_err()
            .code,
        ErrorCode::GitOperationInProgress,
    );
}
