use hq_git_lib::{
    application::{file_service::FileService, mutation_coordinator::RepositoryMutationCoordinator},
    domain::{error::ErrorCode, files::*},
    infrastructure::git_runner::GitCommandRunner,
};
use std::{fs, path::Path};
use tempfile::TempDir;

async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
}
async fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]).await;
    git(dir.path(), &["config", "user.name", "Files Test"]).await;
    git(dir.path(), &["config", "user.email", "files@example.test"]).await;
    git(dir.path(), &["config", "core.autocrlf", "false"]).await;
    fs::write(dir.path().join("tracked.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "."]).await;
    git(dir.path(), &["commit", "-m", "base"]).await;
    dir
}
fn create(name: &str) -> FileOperationIntent {
    FileOperationIntent::CreateFile {
        parent_dir: String::new(),
        name: name.into(),
    }
}
fn delete(path: &str) -> FileOperationIntent {
    FileOperationIntent::Delete {
        relative_path: path.into(),
    }
}
fn rename(path: &str, name: &str) -> FileOperationIntent {
    FileOperationIntent::Rename {
        relative_path: path.into(),
        new_name: name.into(),
    }
}
async fn execute(
    service: &FileService,
    root: &Path,
    intent: FileOperationIntent,
) -> FileMutationResult {
    let prepared = service.prepare(root, intent.clone()).await.unwrap();
    service
        .execute(
            root,
            ExecuteFileOperationRequest {
                intent,
                token: prepared.token,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn lists_direct_children_including_ignored_and_unicode_and_sorts_directories_first() {
    let repo = fixture().await;
    fs::create_dir(repo.path().join("z-dir")).unwrap();
    fs::write(repo.path().join("z-dir/deep.txt"), "deep").unwrap();
    for name in [
        ".hidden",
        "中文.txt",
        "__proto__",
        "constructor",
        "-option",
        "[literal]",
        "ignored.txt",
    ] {
        fs::write(repo.path().join(name), "x").unwrap();
    }
    fs::write(repo.path().join(".gitignore"), "ignored.txt\n").unwrap();
    let page = FileService::default()
        .list(repo.path(), "", None)
        .await
        .unwrap();
    assert_eq!(page.entries[0].name, "z-dir");
    assert_eq!(page.entries[0].kind, FileEntryKind::Directory);
    assert!(
        !page
            .entries
            .iter()
            .any(|e| e.name == ".git" || e.name == "deep.txt")
    );
    assert!(page.entries.iter().any(|e| e.name == "ignored.txt"));
    assert!(page.entries.iter().any(|e| e.name == "中文.txt"));
    assert_eq!(
        page.entries
            .iter()
            .find(|e| e.name == "ignored.txt")
            .unwrap()
            .git_status
            .as_deref(),
        Some("!!")
    );
}

#[tokio::test]
async fn pages_are_snapshot_bound_and_never_silently_truncated() {
    let repo = fixture().await;
    for i in 0..510 {
        fs::write(repo.path().join(format!("file-{i:04}")), "").unwrap();
    }
    let service = FileService::default();
    let first = service.list(repo.path(), "", None).await.unwrap();
    assert_eq!(first.entries.len(), 500);
    assert_eq!(first.total_entries, 511);
    let cursor = first.next_cursor.unwrap();
    let second = service.list(repo.path(), "", Some(&cursor)).await.unwrap();
    assert_eq!(second.entries.len(), 11);
    assert_eq!(second.token, first.token);
    assert!(second.next_cursor.is_none());
    fs::write(repo.path().join("new"), "").unwrap();
    assert_eq!(
        service
            .list(repo.path(), "", Some(&cursor))
            .await
            .unwrap_err()
            .code,
        ErrorCode::StaleFileOperation
    );
}

#[tokio::test]
async fn previews_strict_text_bom_newlines_and_metadata_only_fallbacks() {
    let repo = fixture().await;
    let service = FileService::default();
    fs::write(repo.path().join("text"), b"\xef\xbb\xbfhello\r\nworld\r\n").unwrap();
    let text = service.preview(repo.path(), "text").await.unwrap();
    assert_eq!(text.kind, FilePreviewKind::Text);
    assert!(text.bom);
    assert_eq!(text.line_ending, FileLineEnding::Crlf);
    assert_eq!(text.text.as_deref(), Some("hello\r\nworld\r\n"));
    for (name, bytes, kind) in [
        ("binary", vec![0, 1, 2], FilePreviewKind::Binary),
        (
            "invalid",
            vec![0xff, 0xfe],
            FilePreviewKind::UnsupportedEncoding,
        ),
        (
            "large",
            vec![b'x'; 2 * 1024 * 1024 + 1],
            FilePreviewKind::TooLarge,
        ),
    ] {
        fs::write(repo.path().join(name), bytes).unwrap();
        let preview = service.preview(repo.path(), name).await.unwrap();
        assert_eq!(preview.kind, kind);
        assert!(preview.text.is_none());
    }
}

#[tokio::test]
async fn prepare_is_read_only_and_create_is_exclusive_and_tokens_cannot_replay() {
    let repo = fixture().await;
    let service = FileService::default();
    let before = fs::read(repo.path().join(".git/index")).unwrap();
    let intent = create("中文 space [x].txt");
    let prepared = service.prepare(repo.path(), intent.clone()).await.unwrap();
    assert!(!repo.path().join("中文 space [x].txt").exists());
    assert!(!repo.path().join(".git/hq-git-file-recovery").exists());
    assert_eq!(fs::read(repo.path().join(".git/index")).unwrap(), before);
    let request = ExecuteFileOperationRequest {
        intent,
        token: prepared.token,
    };
    let result = service.execute(repo.path(), request.clone()).await.unwrap();
    assert!(result.applied);
    assert_eq!(result.selected_path.as_deref(), Some("中文 space [x].txt"));
    assert_eq!(fs::read(repo.path().join(".git/index")).unwrap(), before);
    assert_eq!(
        service
            .execute(repo.path(), request)
            .await
            .unwrap_err()
            .code,
        ErrorCode::StaleFileOperation
    );
    assert_eq!(
        service
            .prepare(repo.path(), create("tracked.txt"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::FileAlreadyExists
    );
}

#[tokio::test]
async fn directories_rename_and_delete_preserve_all_content_and_index() {
    let repo = fixture().await;
    let service = FileService::default();
    let before = fs::read(repo.path().join(".git/index")).unwrap();
    let result = execute(
        &service,
        repo.path(),
        FileOperationIntent::CreateDirectory {
            parent_dir: "".into(),
            name: "folder".into(),
        },
    )
    .await;
    assert!(result.applied);
    fs::create_dir(repo.path().join("folder/empty")).unwrap();
    fs::write(repo.path().join("folder/data"), "untracked data").unwrap();
    assert!(
        execute(&service, repo.path(), rename("folder", "renamed"))
            .await
            .applied
    );
    let intent = delete("renamed");
    let prepared = service.prepare(repo.path(), intent.clone()).await.unwrap();
    assert_eq!(
        (
            prepared.node_count,
            prepared.file_count,
            prepared.directory_count,
            prepared.total_bytes
        ),
        (3, 1, 2, 14)
    );
    let result = service
        .execute(
            repo.path(),
            ExecuteFileOperationRequest {
                intent,
                token: prepared.token,
            },
        )
        .await
        .unwrap();
    assert!(result.applied);
    assert!(!repo.path().join("renamed").exists());
    let payload = std::path::PathBuf::from(result.recovery_path.unwrap());
    assert_eq!(
        fs::read_to_string(payload.join("data")).unwrap(),
        "untracked data"
    );
    assert!(payload.join("empty").is_dir());
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(payload.parent().unwrap().join("metadata.json")).unwrap())
            .unwrap();
    assert_eq!(metadata["originalPath"], "renamed");
    assert!(metadata["fingerprint"].as_str().unwrap().len() >= 64);
    assert_eq!(fs::read(repo.path().join(".git/index")).unwrap(), before);
}

#[tokio::test]
async fn rejects_unsafe_paths_names_and_nested_repositories() {
    let repo = fixture().await;
    let service = FileService::default();
    for path in [
        "",
        ".",
        "..",
        "../out",
        "/absolute",
        "C:/root",
        "\\\\server\\share",
        "x:y",
        "a//b",
        ".git/config",
        "GIT~1/config",
        "a\0b",
        "a.",
        "a ",
        "CON",
        "lpt1.txt",
        "a<b",
        "a>b",
        "a|b",
        "a?b",
        "a*b",
        "a\"b",
    ] {
        assert!(
            service.prepare(repo.path(), delete(path)).await.is_err(),
            "{path:?}"
        );
    }
    for name in ["a/b", "a\\b", ".git", "git~1", "", ".."] {
        assert!(service.prepare(repo.path(), create(name)).await.is_err());
    }
    fs::create_dir(repo.path().join("nested")).unwrap();
    git(&repo.path().join("nested"), &["init"]).await;
    let page = service.list(repo.path(), "", None).await.unwrap();
    assert_eq!(
        page.entries
            .iter()
            .find(|e| e.name == "nested")
            .unwrap()
            .kind,
        FileEntryKind::Restricted
    );
    assert!(service.list(repo.path(), "nested", None).await.is_err());
    assert!(
        service
            .prepare(repo.path(), delete("nested"))
            .await
            .is_err()
    );
    assert!(
        service
            .prepare(
                repo.path(),
                FileOperationIntent::CreateFile {
                    parent_dir: "nested".into(),
                    name: "x".into()
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn changed_sources_destinations_intents_head_index_and_operation_are_stale() {
    let repo = fixture().await;
    let service = FileService::default();
    for change in [
        "source",
        "destination",
        "intent",
        "head",
        "index",
        "operation",
        "membership",
        "parent",
    ] {
        fs::create_dir_all(repo.path().join("parent")).unwrap();
        fs::write(repo.path().join("parent/source"), "original").unwrap();
        let mut intent = rename("parent/source", "destination");
        let prepared = service.prepare(repo.path(), intent.clone()).await.unwrap();
        match change {
            "source" => fs::write(repo.path().join("parent/source"), "changed!").unwrap(),
            "destination" => fs::write(repo.path().join("parent/destination"), "keep").unwrap(),
            "intent" => intent = delete("parent/source"),
            "head" => {
                git(repo.path(), &["commit", "--allow-empty", "-m", "new head"]).await;
            }
            "index" => {
                git(repo.path(), &["add", "parent/source"]).await;
            }
            "operation" => fs::write(
                repo.path().join(".git/MERGE_HEAD"),
                git(repo.path(), &["rev-parse", "HEAD"]).await,
            )
            .unwrap(),
            "membership" => fs::write(repo.path().join("parent/new-child"), "new").unwrap(),
            "parent" => {
                fs::rename(repo.path().join("parent"), repo.path().join("old-parent")).unwrap();
                fs::create_dir(repo.path().join("parent")).unwrap();
                fs::write(repo.path().join("parent/source"), "original").unwrap();
            }
            _ => unreachable!(),
        }
        let error = service
            .execute(
                repo.path(),
                ExecuteFileOperationRequest {
                    intent,
                    token: prepared.token,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleFileOperation, "{change}");
        assert!(repo.path().join("parent/source").exists());
        if change == "operation" {
            fs::remove_file(repo.path().join(".git/MERGE_HEAD")).unwrap();
        }
        for name in ["parent/destination", "parent/new-child"] {
            if repo.path().join(name).exists() {
                fs::remove_file(repo.path().join(name)).unwrap();
            }
        }
    }
}

#[tokio::test]
async fn recovery_creation_failure_leaves_source_and_index_intact() {
    let repo = fixture().await;
    let service = FileService::default();
    fs::write(repo.path().join(".git/hq-git-file-recovery"), "occupied").unwrap();
    let prepared = service
        .prepare(repo.path(), delete("tracked.txt"))
        .await
        .unwrap();
    assert!(
        service
            .execute(
                repo.path(),
                ExecuteFileOperationRequest {
                    intent: delete("tracked.txt"),
                    token: prepared.token
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
}

#[tokio::test]
async fn read_operations_wait_for_shared_write_lease() {
    let repo = fixture().await;
    let coordinator = RepositoryMutationCoordinator::default();
    let service = FileService::new(GitCommandRunner::default(), coordinator.clone());
    let lease = coordinator.write(repo.path()).await;
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            service.list(repo.path(), "", None)
        )
        .await
        .is_err()
    );
    drop(lease);
    assert!(service.list(repo.path(), "", None).await.is_ok());
}

#[tokio::test]
async fn execute_waits_for_shared_read_lease_without_consuming_confirmation() {
    let repo = fixture().await;
    let coordinator = RepositoryMutationCoordinator::default();
    let service = FileService::new(GitCommandRunner::default(), coordinator.clone());
    let prepared = service
        .prepare(repo.path(), create("after-read"))
        .await
        .unwrap();
    let request = ExecuteFileOperationRequest {
        intent: create("after-read"),
        token: prepared.token,
    };
    let lease = coordinator.read(repo.path()).await;
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            service.execute(repo.path(), request.clone())
        )
        .await
        .is_err()
    );
    assert!(!repo.path().join("after-read").exists());
    drop(lease);
    assert!(service.execute(repo.path(), request).await.unwrap().applied);
}

#[test]
fn intent_contract_has_camel_case_variant_fields_and_nullable_metadata() {
    let value = serde_json::to_value(rename("old", "new")).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"kind":"rename", "relativePath":"old", "newName":"new"})
    );
}

#[test]
fn windows_console_and_superscript_device_names_are_rejected_lexically() {
    use hq_git_lib::infrastructure::repository_paths::validate_name;
    for name in ["COM¹", "com².txt", "LPT³", "CONIN$", "CONOUT$", "NUL .txt"] {
        assert_eq!(
            validate_name(name).unwrap_err().code,
            ErrorCode::InvalidPath,
            "{name}"
        );
    }
}

#[tokio::test]
async fn unborn_repository_and_nested_parent_creation_work() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]).await;
    fs::create_dir(repo.path().join("child")).unwrap();
    let service = FileService::default();
    let result = execute(
        &service,
        repo.path(),
        FileOperationIntent::CreateFile {
            parent_dir: "child".into(),
            name: "new".into(),
        },
    )
    .await;
    assert!(result.applied);
    assert!(repo.path().join("child/new").is_file());
}

#[tokio::test]
async fn source_directory_membership_and_nested_metadata_block_whole_directory_mutation() {
    let repo = fixture().await;
    let service = FileService::default();
    fs::create_dir_all(repo.path().join("parent/child")).unwrap();
    fs::write(repo.path().join("parent/child/old"), "old").unwrap();
    let prepared = service
        .prepare(repo.path(), delete("parent"))
        .await
        .unwrap();
    fs::write(repo.path().join("parent/child/new"), "new").unwrap();
    assert_eq!(
        service
            .execute(
                repo.path(),
                ExecuteFileOperationRequest {
                    intent: delete("parent"),
                    token: prepared.token
                }
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::StaleFileOperation
    );
    fs::create_dir(repo.path().join("parent/child/.git")).unwrap();
    assert_eq!(
        service
            .prepare(repo.path(), rename("parent", "renamed"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFileOperation
    );
}

#[tokio::test]
async fn source_byte_and_depth_limits_fail_without_mutations() {
    let repo = fixture().await;
    let service = FileService::default();
    let file = fs::File::create(repo.path().join("oversize")).unwrap();
    file.set_len(512 * 1024 * 1024 + 1).unwrap();
    drop(file);
    assert_eq!(
        service
            .prepare(repo.path(), delete("oversize"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFileOperation
    );
    fs::create_dir(repo.path().join("deep")).unwrap();
    let mut path = repo.path().join("deep");
    for _ in 0..65 {
        path.push("d");
        fs::create_dir(&path).unwrap();
    }
    assert_eq!(
        service
            .prepare(repo.path(), delete("deep"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFileOperation
    );
    assert!(repo.path().join("oversize").is_file());
    assert!(repo.path().join("deep").is_dir());
}

#[tokio::test]
async fn submodule_entries_are_restricted_even_without_a_git_marker() {
    let repo = fixture().await;
    let service = FileService::default();
    fs::create_dir(repo.path().join("module")).unwrap();
    let oid = git(repo.path(), &["rev-parse", "HEAD"]).await;
    git(
        repo.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},module", oid.trim()),
        ],
    )
    .await;
    let entry = service
        .list(repo.path(), "", None)
        .await
        .unwrap()
        .entries
        .into_iter()
        .find(|e| e.name == "module")
        .unwrap();
    assert_eq!(entry.kind, FileEntryKind::Restricted);
    assert!(service.list(repo.path(), "module", None).await.is_err());
    #[cfg(windows)]
    {
        assert!(service.list(repo.path(), "MODULE", None).await.is_err());
        assert!(
            service
                .prepare(
                    repo.path(),
                    FileOperationIntent::CreateFile {
                        parent_dir: "MODULE".into(),
                        name: "new".into()
                    }
                )
                .await
                .is_err()
        );
    }
    assert!(
        service
            .prepare(repo.path(), delete("module"))
            .await
            .is_err()
    );
}

#[cfg(windows)]
#[tokio::test]
async fn windows_locked_file_fails_safely_and_case_only_rename_is_unsupported() {
    use std::os::windows::fs::OpenOptionsExt;
    let repo = fixture().await;
    let service = FileService::default();
    fs::write(repo.path().join("Ä.txt"), "unicode").unwrap();
    assert_eq!(
        service
            .prepare(repo.path(), rename("Ä.txt", "ä.txt"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFileOperation
    );
    assert_eq!(
        service
            .prepare(repo.path(), rename("tracked.txt", "TRACKED.TXT"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFileOperation
    );
    let prepared = service
        .prepare(repo.path(), delete("tracked.txt"))
        .await
        .unwrap();
    let _locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(repo.path().join("tracked.txt"))
        .unwrap();
    assert!(
        service
            .execute(
                repo.path(),
                ExecuteFileOperationRequest {
                    intent: delete("tracked.txt"),
                    token: prepared.token
                }
            )
            .await
            .is_err()
    );
    assert!(repo.path().join("tracked.txt").exists());
}

#[cfg(windows)]
#[tokio::test]
async fn windows_junction_is_restricted_and_cannot_redirect_mutations() {
    let repo = fixture().await;
    let external = tempfile::tempdir().unwrap();
    let service = FileService::default();
    fs::write(external.path().join("keep"), "external").unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(repo.path().join("junction"))
        .arg(external.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let entries = service.list(repo.path(), "", None).await.unwrap().entries;
    assert_eq!(
        entries.iter().find(|e| e.name == "junction").unwrap().kind,
        FileEntryKind::Restricted
    );
    assert!(service.list(repo.path(), "junction", None).await.is_err());
    assert!(
        service
            .prepare(repo.path(), delete("junction"))
            .await
            .is_err()
    );
    assert!(
        service
            .prepare(
                repo.path(),
                FileOperationIntent::CreateFile {
                    parent_dir: "junction".into(),
                    name: "new".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(external.path().join("keep")).unwrap(),
        "external"
    );
    fs::remove_dir(repo.path().join("junction")).unwrap();
}

#[tokio::test]
async fn git_private_directory_in_worktree_is_hidden_and_rejected() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("repo");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("container")).unwrap();
    let private = root.join("container/private-storage");
    git(
        &root,
        &["init", "--separate-git-dir", private.to_str().unwrap()],
    )
    .await;
    let service = FileService::default();
    assert!(
        !service
            .list(&root, "", None)
            .await
            .unwrap()
            .entries
            .iter()
            .any(|e| e.name == "private-storage")
    );
    assert!(
        service
            .list(&root, "container/private-storage", None)
            .await
            .is_err()
    );
    assert!(
        service
            .list(&root, "container", None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    #[cfg(windows)]
    {
        assert!(
            service
                .list(&root, "CONTAINER/PRIVATE-STORAGE", None)
                .await
                .is_err()
        );
        assert!(
            service
                .list(&root, "CONTAINER", None)
                .await
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(
            service
                .preview(&root, "CONTAINER/PRIVATE-STORAGE/config")
                .await
                .is_err()
        );
    }
    assert!(
        service
            .prepare(&root, delete("container/private-storage"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn linked_worktree_deletes_into_its_git_resolved_private_recovery() {
    let repo = fixture().await;
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("linked");
    git(
        repo.path(),
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    )
    .await;
    let service = FileService::default();
    let result = execute(&service, &linked, delete("tracked.txt")).await;
    assert!(result.applied);
    let payload = std::path::PathBuf::from(result.recovery_path.unwrap());
    assert_eq!(fs::read_to_string(payload).unwrap(), "base\n");
    assert!(repo.path().join("tracked.txt").is_file());
}

#[tokio::test]
async fn node_and_immediate_child_limits_are_explicit_errors() {
    let repo = fixture().await;
    let service = FileService::default();
    fs::create_dir(repo.path().join("many")).unwrap();
    for index in 0..10_000 {
        fs::write(repo.path().join(format!("many/{index}")), "").unwrap();
    }
    assert_eq!(
        service
            .prepare(repo.path(), delete("many"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFileOperation
    );
    fs::write(repo.path().join("many/overflow"), "").unwrap();
    assert_eq!(
        service
            .list(repo.path(), "many", None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedFileOperation
    );
}

#[tokio::test]
async fn nested_bare_repositories_are_restricted_including_inside_directory_operations() {
    let repo = fixture().await;
    let service = FileService::default();
    fs::create_dir(repo.path().join("container")).unwrap();
    git(repo.path(), &["init", "--bare", "container/archive.git"]).await;
    let entry = service
        .list(repo.path(), "container", None)
        .await
        .unwrap()
        .entries
        .remove(0);
    assert_eq!(entry.kind, FileEntryKind::Restricted);
    assert!(
        service
            .list(repo.path(), "container/archive.git", None)
            .await
            .is_err()
    );
    assert!(
        service
            .prepare(
                repo.path(),
                FileOperationIntent::CreateFile {
                    parent_dir: "container/archive.git".into(),
                    name: "new".into()
                }
            )
            .await
            .is_err()
    );
    assert!(
        service
            .prepare(repo.path(), delete("container"))
            .await
            .is_err()
    );
}
