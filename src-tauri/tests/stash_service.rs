use std::path::Path;

use hq_git_lib::application::stash_service::StashService;
use hq_git_lib::domain::error::ErrorCode;
use hq_git_lib::domain::operation::RepositoryOperationKind;
use hq_git_lib::domain::stash::{StashCreateRequest, StashMutationOutcome, StashSelection};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use tempfile::TempDir;

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
}

async fn fixture(commit: bool) -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-b", "main"]).await;
    git(root, &["config", "user.name", "HQ Test"]).await;
    git(root, &["config", "user.email", "hq@example.test"]).await;
    git(root, &["config", "core.autocrlf", "false"]).await;
    if commit {
        std::fs::write(root.join("tracked.txt"), "base\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-m", "base"]).await;
    }
    directory
}

fn request(message: Option<&str>, include_untracked: bool) -> StashCreateRequest {
    StashCreateRequest {
        message: message.map(str::to_owned),
        include_untracked,
        paths: None,
    }
}

async fn selection(root: &Path) -> StashSelection {
    let entry = StashService::default()
        .snapshot(root)
        .await
        .unwrap()
        .entries
        .remove(0);
    StashSelection {
        selector: entry.selector,
        expected_object_id: entry.object_id,
    }
}

#[tokio::test]
async fn partial_stash_only_saves_selected_files_and_keeps_other_staged_changes() {
    let repo = fixture(true).await;
    let root = repo.path();
    std::fs::write(root.join("other.txt"), "base other\n").unwrap();
    git(root, &["add", "."]).await;
    git(root, &["commit", "-m", "other"]).await;
    std::fs::write(root.join("tracked.txt"), "selected staged\n").unwrap();
    std::fs::write(root.join("other.txt"), "keep staged\n").unwrap();
    git(root, &["add", "."]).await;
    std::fs::write(root.join("tracked.txt"), "selected working\n").unwrap();
    std::fs::write(root.join("other.txt"), "keep working\n").unwrap();
    let mut selected = request(Some("partial"), false);
    selected.paths = Some(vec!["tracked.txt".into()]);
    let service = StashService::default();
    let created = service.create(root, selected).await.unwrap();
    assert!(created.error.is_none(), "{:?}", created.error);
    assert_eq!(created.outcome, StashMutationOutcome::Created);
    assert_eq!(git(root, &["show", ":other.txt"]).await, "keep staged\n");
    assert_eq!(
        std::fs::read_to_string(root.join("other.txt")).unwrap(),
        "keep working\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "base\n"
    );
    let detail = service.detail(root, selection(root).await).await.unwrap();
    assert_eq!(
        detail
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["tracked.txt"]
    );
    // Native stash apply --index requires a compatible index/worktree. Verify
    // round-tripping after finishing the unrelated edits preserved above.
    git(
        root,
        &[
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--",
            "other.txt",
        ],
    )
    .await;
    let applied = service.apply(root, selection(root).await).await.unwrap();
    assert!(applied.error.is_none(), "{:?}", applied.error);
    assert_eq!(
        git(root, &["show", ":tracked.txt"]).await,
        "selected staged\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "selected working\n"
    );
    assert_eq!(git(root, &["show", ":other.txt"]).await, "base other\n");
}

#[tokio::test]
async fn partial_stash_handles_untracked_literal_names_without_saving_other_files() {
    let repo = fixture(true).await;
    let root = repo.path();
    for path in ["[draft] 中文.txt", "keep new.txt"] {
        std::fs::write(root.join(path), path).unwrap();
    }
    std::fs::write(root.join("tracked.txt"), "keep tracked\n").unwrap();
    let service = StashService::default();
    let mut selected = request(None, true);
    selected.paths = Some(vec!["[draft] 中文.txt".into()]);
    let created = service.create(root, selected).await.unwrap();
    assert!(created.error.is_none(), "{:?}", created.error);
    assert!(!root.join("[draft] 中文.txt").exists());
    assert!(root.join("keep new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "keep tracked\n"
    );
    let detail = service.detail(root, selection(root).await).await.unwrap();
    assert_eq!(detail.files.len(), 1);
    assert_eq!(detail.files[0].path, "[draft] 中文.txt");
    assert!(detail.files[0].untracked);
    let restored = service.pop(root, selection(root).await).await.unwrap();
    assert!(restored.error.is_none(), "{:?}", restored.error);
    assert_eq!(
        std::fs::read_to_string(root.join("[draft] 中文.txt")).unwrap(),
        "[draft] 中文.txt"
    );
}

#[tokio::test]
async fn partial_stash_includes_both_sides_of_a_selected_rename() {
    let repo = fixture(true).await;
    let root = repo.path();
    git(root, &["mv", "tracked.txt", "renamed.txt"]).await;
    let mut selected = request(None, false);
    selected.paths = Some(vec!["renamed.txt".into()]);
    let service = StashService::default();
    let created = service.create(root, selected).await.unwrap();
    assert!(created.error.is_none(), "{:?}", created.error);
    assert!(root.join("tracked.txt").exists());
    assert!(!root.join("renamed.txt").exists());
    assert!(git(root, &["status", "--porcelain"]).await.is_empty());
    let restored = service.pop(root, selection(root).await).await.unwrap();
    assert!(restored.error.is_none(), "{:?}", restored.error);
    assert!(!root.join("tracked.txt").exists());
    assert!(root.join("renamed.txt").exists());
    assert!(
        git(root, &["status", "--porcelain"])
            .await
            .starts_with("R ")
    );
}

#[tokio::test]
async fn partial_stash_rejects_empty_stale_and_untracked_without_opt_in() {
    let repo = fixture(true).await;
    let root = repo.path();
    std::fs::write(root.join("tracked.txt"), "keep staged\n").unwrap();
    git(root, &["add", "tracked.txt"]).await;
    std::fs::write(root.join("new.txt"), "keep new\n").unwrap();
    let service = StashService::default();
    for paths in [
        vec![],
        vec!["missing.txt"],
        vec!["new.txt"],
        vec!["."],
        vec!["../outside"],
    ] {
        let mut selected = request(None, false);
        selected.paths = Some(paths.into_iter().map(str::to_owned).collect());
        assert_eq!(
            service.create(root, selected).await.unwrap_err().code,
            ErrorCode::InvalidPath
        );
        assert!(service.snapshot(root).await.unwrap().entries.is_empty());
        assert_eq!(git(root, &["show", ":tracked.txt"]).await, "keep staged\n");
        assert_eq!(
            std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "keep staged\n"
        );
        assert!(root.join("new.txt").exists());
    }
}

#[tokio::test]
async fn snapshot_accepts_unborn_repository_and_lists_metadata() {
    let empty = fixture(false).await;
    let service = StashService::default();
    assert!(
        service
            .snapshot(empty.path())
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    let repo = fixture(true).await;
    for message in ["first", "\u{68c0}\u{67e5} checkpoint"] {
        std::fs::write(repo.path().join("tracked.txt"), message).unwrap();
        service
            .create(repo.path(), request(Some(message), false))
            .await
            .unwrap();
    }
    let entries = service.snapshot(repo.path()).await.unwrap().entries;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].selector, "stash@{0}");
    assert_eq!(entries[1].selector, "stash@{1}");
    assert_eq!(entries[0].branch.as_deref(), Some("main"));
    assert_eq!(entries[0].description, "\u{68c0}\u{67e5} checkpoint");
    assert!(entries[0].timestamp.contains('T'));
    assert_eq!(entries[0].object_id.len(), 40);
    assert_ne!(entries[0].object_id, entries[1].object_id);
}

#[tokio::test]
async fn create_trims_message_preserves_untracked_and_restores_index_on_apply() {
    let repo = fixture(true).await;
    let root = repo.path();
    std::fs::write(root.join("tracked.txt"), "staged\n").unwrap();
    git(root, &["add", "tracked.txt"]).await;
    std::fs::write(root.join("tracked.txt"), "unstaged\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "unsaved\n").unwrap();
    let service = StashService::default();
    let created = service
        .create(root, request(Some("  checkpoint  "), false))
        .await
        .unwrap();
    assert_eq!(created.outcome, StashMutationOutcome::Created);
    assert!(created.error.is_none());
    assert_eq!(created.stashes.entries[0].description, "checkpoint");
    assert!(root.join("untracked.txt").exists());
    assert_eq!(created.workspace.changes.staged_count, 0);
    let applied = service.apply(root, selection(root).await).await.unwrap();
    assert_eq!(applied.outcome, StashMutationOutcome::Applied);
    assert_eq!(applied.stashes.entries.len(), 1);
    assert_eq!(git(root, &["show", ":tracked.txt"]).await, "staged\n");
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "unstaged\n"
    );
}

#[tokio::test]
async fn create_no_changes_does_not_create_entry_and_include_untracked_is_opt_in() {
    let repo = fixture(true).await;
    let root = repo.path();
    let service = StashService::default();
    std::fs::write(root.join("new file.txt"), "saved\n").unwrap();
    let unchanged = service.create(root, request(None, false)).await.unwrap();
    assert_eq!(unchanged.outcome, StashMutationOutcome::NoChanges);
    assert!(unchanged.stashes.entries.is_empty());
    let created = service
        .create(root, request(Some("  "), true))
        .await
        .unwrap();
    assert_eq!(created.outcome, StashMutationOutcome::Created);
    assert!(!root.join("new file.txt").exists());
    let popped = service.pop(root, selection(root).await).await.unwrap();
    assert_eq!(popped.outcome, StashMutationOutcome::Removed);
    assert!(popped.stashes.entries.is_empty());
    assert_eq!(
        std::fs::read_to_string(root.join("new file.txt")).unwrap(),
        "saved\n"
    );
}

#[tokio::test]
async fn detail_and_diff_cover_rename_binary_unicode_and_third_parent_files() {
    let repo = fixture(true).await;
    let root = repo.path();
    std::fs::write(root.join("old name.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(root.join("binary.dat"), [0, 1, 2]).unwrap();
    git(root, &["add", "."]).await;
    git(root, &["commit", "-m", "extra files"]).await;
    git(root, &["mv", "old name.txt", "new name.txt"]).await;
    std::fs::write(root.join("new name.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    std::fs::write(root.join("binary.dat"), [0, 3, 4]).unwrap();
    let unicode_path = "\u{672a}\u{8ddf}\u{8e2a} file.txt";
    std::fs::write(root.join(unicode_path), "untracked content\n").unwrap();
    let service = StashService::default();
    service.create(root, request(None, true)).await.unwrap();
    let selected = selection(root).await;
    let detail = service.detail(root, selected.clone()).await.unwrap();
    assert_eq!(detail.files.len(), 3);
    let renamed = detail
        .files
        .iter()
        .find(|file| file.path == "new name.txt")
        .unwrap();
    assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
    assert_eq!(renamed.additions, Some(1));
    assert_eq!(renamed.deletions, Some(0));
    assert!(!renamed.untracked);
    let binary = detail
        .files
        .iter()
        .find(|file| file.path == "binary.dat")
        .unwrap();
    assert!(binary.binary);
    assert_eq!(binary.additions, None);
    let untracked = detail
        .files
        .iter()
        .find(|file| file.path == unicode_path)
        .unwrap();
    assert!(untracked.untracked);
    assert_eq!(untracked.additions, Some(1));
    let diff = service
        .file_diff(root, selected.clone(), unicode_path, true)
        .await
        .unwrap();
    assert!(
        diff.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.content == "untracked content")
    );
    assert!(
        service
            .file_diff(root, selected.clone(), "binary.dat", false)
            .await
            .unwrap()
            .binary
    );
    let rename_diff = service
        .file_diff(root, selected, "new name.txt", false)
        .await
        .unwrap();
    assert!(
        rename_diff
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .any(|line| line.content == "four")
    );
}

#[tokio::test]
async fn selections_reject_revision_expressions_and_stale_push_or_drop() {
    let repo = fixture(true).await;
    let root = repo.path();
    let service = StashService::default();
    std::fs::write(root.join("tracked.txt"), "first\n").unwrap();
    service.create(root, request(None, false)).await.unwrap();
    let first = selection(root).await;
    for selector in ["HEAD", "stash@{0}^1", "stash@{-1}", "stash@{01}", "--all"] {
        let invalid = StashSelection {
            selector: selector.into(),
            ..first.clone()
        };
        assert_eq!(
            service.detail(root, invalid).await.unwrap_err().code,
            ErrorCode::InvalidReference
        );
    }
    std::fs::write(root.join("tracked.txt"), "second\n").unwrap();
    service.create(root, request(None, false)).await.unwrap();
    assert_eq!(
        service.apply(root, first.clone()).await.unwrap_err().code,
        ErrorCode::StaleStash
    );
    assert_eq!(
        service
            .file_diff(root, first, "tracked.txt", false)
            .await
            .unwrap_err()
            .code,
        ErrorCode::StaleStash
    );
    let newest = selection(root).await;
    git(root, &["stash", "drop", "stash@{0}"]).await;
    assert_eq!(
        service.pop(root, newest).await.unwrap_err().code,
        ErrorCode::StaleStash
    );
    assert_eq!(service.snapshot(root).await.unwrap().entries.len(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "base\n"
    );
}

#[tokio::test]
async fn conflicts_retain_stash_without_fabricating_abort_and_block_later_writes() {
    for pop in [false, true] {
        let repo = fixture(true).await;
        let root = repo.path();
        let service = StashService::default();
        std::fs::write(root.join("tracked.txt"), "stashed\n").unwrap();
        service.create(root, request(None, false)).await.unwrap();
        let selected = selection(root).await;
        std::fs::write(root.join("tracked.txt"), "committed\n").unwrap();
        git(root, &["commit", "-am", "diverge"]).await;
        let result = if pop {
            service.pop(root, selected.clone()).await
        } else {
            service.apply(root, selected.clone()).await
        }
        .unwrap();
        assert_eq!(result.outcome, StashMutationOutcome::Retained);
        assert!(result.error.is_some());
        assert_eq!(result.stashes.entries.len(), 1);
        assert_eq!(result.operation_state.kind, RepositoryOperationKind::None);
        assert_eq!(result.operation_state.abort_action, None);
        assert_eq!(result.operation_state.conflicts.len(), 1);
        assert_eq!(
            service
                .create(root, request(None, false))
                .await
                .unwrap_err()
                .code,
            ErrorCode::GitOperationInProgress
        );
        assert_eq!(
            service
                .apply(root, selected.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::GitOperationInProgress
        );
        assert_eq!(
            service.pop(root, selected).await.unwrap_err().code,
            ErrorCode::GitOperationInProgress
        );
    }
}

#[tokio::test]
async fn non_conflict_failure_returns_refreshed_state_and_retains_stash() {
    let repo = fixture(true).await;
    let root = repo.path();
    let service = StashService::default();
    std::fs::write(root.join("saved.txt"), "stashed\n").unwrap();
    service.create(root, request(None, true)).await.unwrap();
    std::fs::write(root.join("saved.txt"), "external work\n").unwrap();
    let result = service.pop(root, selection(root).await).await.unwrap();
    assert_eq!(result.outcome, StashMutationOutcome::Retained);
    assert!(result.error.as_ref().unwrap().diagnostics.is_some());
    assert_eq!(result.stashes.entries.len(), 1);
    assert!(result.operation_state.conflicts.is_empty());
    assert_eq!(result.operation_state.abort_action, None);
    assert!(!result.workspace.repository.is_clean);
    assert_eq!(
        std::fs::read_to_string(root.join("saved.txt")).unwrap(),
        "external work\n"
    );
}

#[tokio::test]
async fn active_operation_without_conflicts_blocks_every_stash_write() {
    let repo = fixture(true).await;
    let root = repo.path();
    let service = StashService::default();
    std::fs::write(root.join("tracked.txt"), "saved\n").unwrap();
    service.create(root, request(None, false)).await.unwrap();
    let selected = selection(root).await;
    let head = git(root, &["rev-parse", "HEAD"]).await;
    std::fs::write(root.join(".git/MERGE_HEAD"), head).unwrap();
    assert_eq!(
        service
            .create(root, request(None, true))
            .await
            .unwrap_err()
            .code,
        ErrorCode::GitOperationInProgress
    );
    assert_eq!(
        service
            .apply(root, selected.clone())
            .await
            .unwrap_err()
            .code,
        ErrorCode::GitOperationInProgress
    );
    assert_eq!(
        service.pop(root, selected).await.unwrap_err().code,
        ErrorCode::GitOperationInProgress
    );
    assert_eq!(service.snapshot(root).await.unwrap().entries.len(), 1);
}

#[tokio::test]
async fn file_diff_treats_globs_literally_and_rejects_paths_outside_selection() {
    let repo = fixture(true).await;
    let root = repo.path();
    let service = StashService::default();
    std::fs::write(root.join("[ab].txt"), "literal only\n").unwrap();
    std::fs::write(root.join("a.txt"), "must not include\n").unwrap();
    service.create(root, request(None, true)).await.unwrap();
    let selected = selection(root).await;
    let diff = service
        .file_diff(root, selected.clone(), "[ab].txt", true)
        .await
        .unwrap();
    let lines: Vec<_> = diff
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .map(|line| line.content.as_str())
        .collect();
    assert!(lines.contains(&"literal only"));
    assert!(!lines.contains(&"must not include"));
    for path in ["../tracked.txt", "*.txt", "missing.txt"] {
        assert_eq!(
            service
                .file_diff(root, selected.clone(), path, true)
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidPath
        );
    }
    let stale = StashSelection {
        expected_object_id: "0".repeat(40),
        ..selected
    };
    assert_eq!(
        service.detail(root, stale).await.unwrap_err().code,
        ErrorCode::StaleStash
    );
}

#[tokio::test]
async fn create_failure_in_unborn_repository_returns_the_workspace() {
    let repo = fixture(false).await;
    let root = repo.path();
    std::fs::write(root.join("first.txt"), "keep\n").unwrap();
    let result = StashService::default()
        .create(root, request(None, true))
        .await
        .unwrap();
    assert_eq!(result.outcome, StashMutationOutcome::Retained);
    assert!(result.error.is_some());
    assert!(result.stashes.entries.is_empty());
    assert!(!result.workspace.repository.is_clean);
    assert!(root.join("first.txt").exists());
}

#[test]
fn create_request_defaults_to_excluding_untracked_files() {
    let request: StashCreateRequest = serde_json::from_str("{}").unwrap();
    assert!(!request.include_untracked);
    assert_eq!(request.message, None);
}

#[tokio::test]
async fn creating_identical_contents_reports_saved_even_when_git_reuses_the_object() {
    let repo = fixture(true).await;
    let root = repo.path();
    let service = StashService::default();
    for _ in 0..2 {
        std::fs::write(root.join("tracked.txt"), "same contents\n").unwrap();
        let result = service
            .create(root, request(Some("same message"), false))
            .await
            .unwrap();
        assert_eq!(result.outcome, StashMutationOutcome::Created);
        assert!(!result.stashes.entries.is_empty());
        assert!(result.workspace.repository.is_clean);
    }
}

#[tokio::test]
async fn create_in_clean_unborn_repository_has_no_changes() {
    let repo = fixture(false).await;
    let result = StashService::default()
        .create(repo.path(), request(None, true))
        .await
        .unwrap();
    assert_eq!(result.outcome, StashMutationOutcome::NoChanges);
    assert!(result.error.is_none());
    assert!(result.stashes.entries.is_empty());
}

#[tokio::test]
async fn pop_removes_only_selected_entry_when_another_entry_has_the_same_object() {
    let repo = fixture(true).await;
    let root = repo.path();
    let service = StashService::default();
    std::fs::write(root.join("tracked.txt"), "first\n").unwrap();
    service
        .create(root, request(Some("first"), false))
        .await
        .unwrap();
    let original = selection(root).await;
    std::fs::write(root.join("tracked.txt"), "second\n").unwrap();
    service
        .create(root, request(Some("second"), false))
        .await
        .unwrap();
    git(
        root,
        &[
            "stash",
            "store",
            "--message",
            "duplicate",
            &original.expected_object_id,
        ],
    )
    .await;
    let selected = selection(root).await;
    assert_eq!(selected.expected_object_id, original.expected_object_id);
    let result = service.pop(root, selected).await.unwrap();
    assert_eq!(result.outcome, StashMutationOutcome::Removed);
    assert!(result.error.is_none());
    assert_eq!(result.stashes.entries.len(), 2);
    assert_eq!(
        result.stashes.entries[1].object_id,
        original.expected_object_id
    );
}

#[tokio::test]
async fn tracked_and_untracked_copies_at_the_same_path_remain_inspectable() {
    for pop in [false, true] {
        let repo = fixture(true).await;
        let root = repo.path();
        let service = StashService::default();
        git(root, &["rm", "--cached", "tracked.txt"]).await;
        std::fs::write(root.join("tracked.txt"), "untracked preserved\n").unwrap();
        service.create(root, request(None, true)).await.unwrap();
        let selected = selection(root).await;
        let detail = service.detail(root, selected.clone()).await.unwrap();
        assert_eq!(detail.files.len(), 2);
        assert!(
            detail
                .files
                .iter()
                .any(|file| file.path == "tracked.txt" && file.status == "M" && !file.untracked)
        );
        assert!(
            detail
                .files
                .iter()
                .any(|file| file.path == "tracked.txt" && file.status == "A" && file.untracked)
        );
        let deletion = service
            .file_diff(root, selected.clone(), "tracked.txt", false)
            .await
            .unwrap();
        assert!(
            deletion
                .hunks
                .iter()
                .flat_map(|hunk| &hunk.lines)
                .any(
                    |line| line.kind == hq_git_lib::domain::changes::DiffLineKind::Deletion
                        && line.content == "base"
                )
        );
        let untracked = service
            .file_diff(root, selected.clone(), "tracked.txt", true)
            .await
            .unwrap();
        assert!(
            untracked
                .hunks
                .iter()
                .flat_map(|hunk| &hunk.lines)
                .any(
                    |line| line.kind == hq_git_lib::domain::changes::DiffLineKind::Addition
                        && line.content == "untracked preserved"
                )
        );
        let result = if pop {
            service.pop(root, selected).await
        } else {
            service.apply(root, selected).await
        }
        .unwrap();
        // Git refuses to check out the third-parent copy over the restored
        // worktree copy. A nonzero exit must preserve the stash, including Pop.
        assert_eq!(result.outcome, StashMutationOutcome::Retained);
        assert!(result.error.is_some());
        assert!(result.operation_state.conflicts.is_empty());
        assert_eq!(result.stashes.entries.len(), 1);
        assert_eq!(
            std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "untracked preserved\n"
        );
        assert!(git(root, &["ls-files", "tracked.txt"]).await.is_empty());
    }
}
