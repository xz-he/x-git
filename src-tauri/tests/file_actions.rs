use hq_git_lib::{
    application::file_service::FileService, domain::files::*,
    infrastructure::git_runner::GitCommandRunner,
};
use std::{fs, path::Path};

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
}
async fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]).await;
    git(dir.path(), &["config", "user.name", "File Actions Test"]).await;
    git(dir.path(), &["config", "user.email", "files@example.test"]).await;
    git(dir.path(), &["config", "core.autocrlf", "false"]).await;
    fs::write(dir.path().join("tracked.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "tracked.txt"]).await;
    git(dir.path(), &["commit", "-m", "base"]).await;
    dir
}
#[tokio::test]
async fn local_ignore_matches_literal_special_name_and_preserves_existing_rules() {
    let repo = fixture().await;
    fs::create_dir(repo.path().join("tests")).unwrap();
    fs::write(repo.path().join("tests/a [1].txt"), "new").unwrap();
    fs::write(repo.path().join("tests/a 1.txt"), "other").unwrap();
    let target = repo.path().join(".git/info/exclude");
    fs::write(&target, "# existing\r\nkeep").unwrap();
    let request = IgnoreRequest {
        relative_path: "tests/a [1].txt".into(),
        rule: IgnoreRule::Exact,
        scope: IgnoreScope::Local,
    };
    let service = FileService::default();
    let preview = service.ignore_preview(repo.path(), &request).await.unwrap();
    assert!(!preview.tracked);
    assert_eq!(fs::read_to_string(&target).unwrap(), "# existing\r\nkeep");
    service.ignore_file(repo.path(), &request).await.unwrap();
    service.ignore_file(repo.path(), &request).await.unwrap();
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "# existing\r\nkeep\r\n/tests/a\\ \\[1\\].txt\r\n"
    );
    assert!(
        git(repo.path(), &["check-ignore", "--", "tests/a [1].txt"])
            .await
            .contains("a [1].txt")
    );
    let other = GitCommandRunner::default()
        .run_allowing_failure(Some(repo.path()), ["check-ignore", "--", "tests/a 1.txt"])
        .await
        .unwrap();
    assert_eq!(other.status_code, Some(1));
    assert!(!repo.path().join(".gitignore").exists());
}
#[tokio::test]
async fn shared_extension_directory_and_custom_rules_are_recognized_by_git() {
    let repo = fixture().await;
    fs::create_dir(repo.path().join("logs")).unwrap();
    for file in ["logs/one.log", "logs/two.bin", "root.log", "cache.tmp"] {
        fs::write(repo.path().join(file), "new").unwrap();
    }
    let service = FileService::default();
    for (file, rule, matches) in [
        (
            "logs/one.log",
            IgnoreRule::Extension,
            vec!["logs/one.log", "root.log"],
        ),
        (
            "logs/two.bin",
            IgnoreRule::Directory {
                directory: "logs".into(),
            },
            vec!["logs/two.bin"],
        ),
        (
            "cache.tmp",
            IgnoreRule::Custom {
                pattern: "*.tmp".into(),
            },
            vec!["cache.tmp"],
        ),
    ] {
        let request = IgnoreRequest {
            relative_path: file.into(),
            rule,
            scope: IgnoreScope::Repository,
        };
        service.ignore_file(repo.path(), &request).await.unwrap();
        for path in matches {
            assert!(
                !git(repo.path(), &["check-ignore", "--", path])
                    .await
                    .is_empty()
            );
        }
    }
    assert_eq!(
        fs::read_to_string(repo.path().join(".gitignore")).unwrap(),
        "*.log\n/logs/\n*.tmp\n"
    );
}
#[tokio::test]
async fn ignore_does_not_untrack_and_untrack_preserves_modified_worktree_file() {
    let repo = fixture().await;
    let service = FileService::default();
    let request = IgnoreRequest {
        relative_path: "tracked.txt".into(),
        rule: IgnoreRule::Exact,
        scope: IgnoreScope::Local,
    };
    assert!(
        service
            .ignore_preview(repo.path(), &request)
            .await
            .unwrap()
            .tracked
    );
    service.ignore_file(repo.path(), &request).await.unwrap();
    assert_eq!(
        git(repo.path(), &["ls-files", "--", "tracked.txt"])
            .await
            .trim(),
        "tracked.txt"
    );
    fs::write(repo.path().join("tracked.txt"), "valuable edit\n").unwrap();
    service
        .untrack_file(repo.path(), "tracked.txt")
        .await
        .unwrap();
    assert!(
        git(repo.path(), &["ls-files", "--", "tracked.txt"])
            .await
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "valuable edit\n"
    );
}
#[tokio::test]
async fn untrack_refuses_to_erase_unique_staged_content() {
    let repo = fixture().await;
    fs::write(repo.path().join("tracked.txt"), "staged\n").unwrap();
    git(repo.path(), &["add", "tracked.txt"]).await;
    fs::write(repo.path().join("tracked.txt"), "working\n").unwrap();
    assert!(
        FileService::default()
            .untrack_file(repo.path(), "tracked.txt")
            .await
            .is_err()
    );
    assert_eq!(
        git(repo.path(), &["show", ":tracked.txt"]).await,
        "staged\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "working\n"
    );
}
#[tokio::test]
async fn history_blame_and_unsafe_ignore_requests() {
    let repo = fixture().await;
    let service = FileService::default();
    assert!(
        service
            .inspect_file(repo.path(), "tracked.txt", FileInspectionKind::History)
            .await
            .unwrap()
            .contains("base")
    );
    assert!(
        service
            .inspect_file(repo.path(), "tracked.txt", FileInspectionKind::Blame)
            .await
            .unwrap()
            .contains("base")
    );
    for (file, rule) in [
        ("../outside", IgnoreRule::Exact),
        (
            "tracked.txt",
            IgnoreRule::Directory {
                directory: "other".into(),
            },
        ),
        (
            "tracked.txt",
            IgnoreRule::Custom {
                pattern: "one\ntwo".into(),
            },
        ),
    ] {
        let request = IgnoreRequest {
            relative_path: file.into(),
            rule,
            scope: IgnoreScope::Repository,
        };
        assert!(service.ignore_file(repo.path(), &request).await.is_err());
        assert!(!repo.path().join(".gitignore").exists());
    }
    fs::write(repo.path().join(".gitignore"), [0xff, 0xfe, 0x61]).unwrap();
    let request = IgnoreRequest {
        relative_path: "tracked.txt".into(),
        rule: IgnoreRule::Exact,
        scope: IgnoreScope::Repository,
    };
    assert!(service.ignore_file(repo.path(), &request).await.is_err());
    assert_eq!(
        fs::read(repo.path().join(".gitignore")).unwrap(),
        [0xff, 0xfe, 0x61]
    );
}
