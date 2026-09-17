use hq_git_lib::application::changes_service::ChangesService;
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use std::path::Path;
use tempfile::TempDir;

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
}
async fn fixture(commit: bool) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]).await;
    git(repo.path(), &["config", "user.name", "Batch Test"]).await;
    git(repo.path(), &["config", "user.email", "batch@example.test"]).await;
    git(repo.path(), &["config", "core.autocrlf", "false"]).await;
    if commit {
        for path in ["a.txt", "b.txt", "[x].txt", "x.txt", "gone.txt"] {
            std::fs::write(repo.path().join(path), "base\n").unwrap();
        }
        git(repo.path(), &["add", "."]).await;
        git(repo.path(), &["commit", "-m", "base"]).await;
    }
    repo
}
fn paths(values: &[&str]) -> Vec<String> {
    values.iter().map(|path| path.to_string()).collect()
}

#[tokio::test]
async fn batch_stage_and_unstage_only_selected_literal_paths() {
    let repo = fixture(true).await;
    let root = repo.path();
    for path in ["a.txt", "b.txt", "[x].txt", "x.txt", "新 文件.txt"] {
        std::fs::write(root.join(path), "working\n").unwrap();
    }
    std::fs::remove_file(root.join("gone.txt")).unwrap();
    let service = ChangesService::default();
    service
        .stage_files(
            root,
            &paths(&["a.txt", "[x].txt", "gone.txt", "新 文件.txt", "a.txt"]),
        )
        .await
        .unwrap();
    let staged = git(root, &["diff", "--cached", "--name-only", "-z"]).await;
    assert_eq!(
        staged.split('\0').filter(|path| !path.is_empty()).count(),
        4
    );
    assert!(
        !staged
            .split('\0')
            .any(|path| path == "b.txt" || path == "x.txt")
    );
    service
        .unstage_files(root, &paths(&["a.txt", "[x].txt", "gone.txt"]))
        .await
        .unwrap();
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only", "-z"]).await,
        "新 文件.txt\0"
    );
    for path in ["a.txt", "b.txt", "[x].txt", "x.txt", "新 文件.txt"] {
        assert_eq!(
            std::fs::read_to_string(root.join(path)).unwrap(),
            "working\n"
        );
    }
    assert!(!root.join("gone.txt").exists());
}

#[tokio::test]
async fn batch_unstage_rename_includes_source_without_touching_other_staged_files() {
    let repo = fixture(true).await;
    let root = repo.path();
    git(root, &["mv", "a.txt", "renamed.txt"]).await;
    std::fs::write(root.join("b.txt"), "keep staged\n").unwrap();
    git(root, &["add", "b.txt"]).await;
    ChangesService::default()
        .unstage_files(root, &paths(&["renamed.txt"]))
        .await
        .unwrap();
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only", "-z"]).await,
        "b.txt\0"
    );
    assert!(!root.join("a.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("renamed.txt")).unwrap(),
        "base\n"
    );
}

#[tokio::test]
async fn batch_unstage_unborn_keeps_latest_working_contents() {
    let repo = fixture(false).await;
    let root = repo.path();
    for path in ["first.txt", "second.txt", "keep.txt"] {
        std::fs::write(root.join(path), "staged\n").unwrap();
    }
    git(root, &["add", "."]).await;
    std::fs::write(root.join("first.txt"), "newer working\n").unwrap();
    ChangesService::default()
        .unstage_files(root, &paths(&["first.txt", "second.txt"]))
        .await
        .unwrap();
    assert_eq!(git(root, &["ls-files", "-z"]).await, "keep.txt\0");
    assert_eq!(
        std::fs::read_to_string(root.join("first.txt")).unwrap(),
        "newer working\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("second.txt")).unwrap(),
        "staged\n"
    );
}

#[tokio::test]
async fn invalid_batch_is_rejected_before_any_index_change() {
    let repo = fixture(true).await;
    let root = repo.path();
    std::fs::write(root.join("a.txt"), "keep working\n").unwrap();
    let service = ChangesService::default();
    for selected in [
        paths(&[]),
        paths(&["a.txt", "b.txt"]),
        paths(&["a.txt", "../outside"]),
        paths(&["."]),
    ] {
        assert!(service.stage_files(root, &selected).await.is_err());
        assert!(
            git(root, &["diff", "--cached", "--name-only"])
                .await
                .is_empty()
        );
    }
    service.stage_files(root, &paths(&["a.txt"])).await.unwrap();
    assert!(
        service
            .unstage_files(root, &paths(&["a.txt", "b.txt"]))
            .await
            .is_err()
    );
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only", "-z"]).await,
        "a.txt\0"
    );
}
