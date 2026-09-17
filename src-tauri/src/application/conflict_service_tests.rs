use std::path::Path;
use std::process::{Command, Output};

use super::conflict_service::ConflictService;
use crate::domain::conflicts::{ConflictLineEnding, ConflictVersionKind};
use crate::domain::conflicts::{ConflictResolution, ResolveConflictRequest};
use crate::domain::error::ErrorCode;
use crate::domain::operation::RepositoryOperationKind;

fn git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn ok(root: &Path, args: &[&str]) {
    let output = git(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn conflict(path: &str, base: &[u8], ours: &[u8], theirs: &[u8]) -> tempfile::TempDir {
    conflict_versions(path, Some(base), Some(ours), Some(theirs))
}

pub(super) fn conflict_versions(
    path: &str,
    base: Option<&[u8]>,
    ours: Option<&[u8]>,
    theirs: Option<&[u8]>,
) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    ok(root, &["init", "-b", "main"]);
    ok(root, &["config", "user.name", "Conflict Test"]);
    ok(root, &["config", "user.email", "test@example.invalid"]);
    ok(root, &["config", "core.autocrlf", "false"]);
    ok(root, &["config", "commit.gpgsign", "false"]);
    if let Some(bytes) = base {
        std::fs::write(root.join(path), bytes).unwrap();
    }
    ok(root, &["add", "--all"]);
    ok(root, &["commit", "--allow-empty", "-m", "base"]);
    ok(root, &["switch", "-c", "incoming"]);
    if let Some(bytes) = theirs {
        std::fs::write(root.join(path), bytes).unwrap();
    } else {
        std::fs::remove_file(root.join(path)).unwrap();
    }
    ok(root, &["add", "--all"]);
    ok(root, &["commit", "-m", "incoming"]);
    ok(root, &["switch", "main"]);
    if let Some(bytes) = ours {
        std::fs::write(root.join(path), bytes).unwrap();
    } else {
        std::fs::remove_file(root.join(path)).unwrap();
    }
    ok(root, &["add", "--all"]);
    ok(root, &["commit", "-m", "ours"]);
    assert!(!git(root, &["merge", "incoming"]).status.success());
    temp
}

#[tokio::test]
async fn ai_conflict_capture_freezes_four_versions_and_does_not_touch_index() {
    let temp = conflict("中文.txt", b"base\n", b"ours\n", b"theirs\n");
    std::fs::write(temp.path().join("中文.txt"), b"\xef\xbb\xbfworking\r\n").unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "中文.txt").await.unwrap();
    let before = std::fs::read(temp.path().join(".git/index")).unwrap();
    let captured = service
        .ai_detail(temp.path(), "中文.txt", &detail.token)
        .await
        .unwrap();
    assert_eq!(captured.base.text.as_deref(), Some("base\n"));
    assert_eq!(captured.ours.text.as_deref(), Some("ours\n"));
    assert_eq!(captured.theirs.text.as_deref(), Some("theirs\n"));
    assert_eq!(captured.working.text.as_deref(), Some("working\n"));
    assert!(captured.working.bom);
    assert_eq!(captured.working.line_ending, ConflictLineEnding::Crlf);
    assert_eq!(
        std::fs::read(temp.path().join(".git/index")).unwrap(),
        before
    );
    std::fs::write(temp.path().join("中文.txt"), b"changed").unwrap();
    assert_eq!(
        service
            .ai_detail(temp.path(), "中文.txt", &detail.token)
            .await
            .unwrap_err()
            .code,
        ErrorCode::StaleConflict
    );
    assert_eq!(captured.working.text.as_deref(), Some("working\n"));
}

#[tokio::test]
async fn ai_conflict_capture_missing_is_distinct_from_empty_and_checks_limits() {
    let temp = conflict_versions("file.txt", None, Some(b"ours\n"), Some(b"theirs\n"));
    std::fs::write(temp.path().join("file.txt"), b"").unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let captured = service
        .ai_detail(temp.path(), "file.txt", &detail.token)
        .await
        .unwrap();
    assert!(!captured.base.exists);
    assert_eq!(captured.base.text, None);
    assert_eq!(captured.working.text.as_deref(), Some(""));
    for (content, code) in [
        (vec![b'a'; 2 * 1024 * 1024 + 1], ErrorCode::AiContextTooLarge),
        (b"a\0b".to_vec(), ErrorCode::UnsupportedConflict),
        (vec![0xff], ErrorCode::UnsupportedConflict),
        (b"a\r\nb\n".to_vec(), ErrorCode::UnsupportedConflict),
    ] {
        std::fs::write(temp.path().join("file.txt"), content).unwrap();
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        assert_eq!(
            service
                .ai_detail(temp.path(), "file.txt", &detail.token)
                .await
                .unwrap_err()
                .code,
            code
        );
    }
}

#[tokio::test]
async fn ai_conflict_capture_rejects_oversized_or_unsupported_stage_and_path_types() {
    for (ours, expected) in [
        (vec![b'a'; 2 * 1024 * 1024 + 1], ErrorCode::AiContextTooLarge),
        (b"x\0y".to_vec(), ErrorCode::UnsupportedConflict),
        (vec![0xff], ErrorCode::UnsupportedConflict),
        (b"x\r\ny\n".to_vec(), ErrorCode::UnsupportedConflict),
    ] {
        let temp = conflict("file.txt", b"base\n", &ours, b"theirs\n");
        std::fs::write(temp.path().join("file.txt"), b"small working text").unwrap();
        let service = ConflictService::default();
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        assert_eq!(
            service
                .ai_detail(temp.path(), "file.txt", &detail.token)
                .await
                .unwrap_err()
                .code,
            expected
        );
    }
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let token = service.detail(temp.path(), "file.txt").await.unwrap().token;
    std::fs::remove_file(temp.path().join("file.txt")).unwrap();
    std::fs::create_dir(temp.path().join("file.txt")).unwrap();
    assert_eq!(
        service
            .ai_detail(temp.path(), "file.txt", &token)
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedConflict
    );
}

#[tokio::test]
async fn ai_conflict_capture_accepts_exact_aggregate_limit_and_missing_sides() {
    let temp = conflict(
        "file.txt",
        &vec![b'b'; 64 * 1024],
        &vec![b'o'; 64 * 1024],
        &vec![b't'; 64 * 1024],
    );
    std::fs::write(temp.path().join("file.txt"), vec![b'w'; 64 * 1024]).unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let captured = service
        .ai_detail(temp.path(), "file.txt", &detail.token)
        .await
        .unwrap();
    assert_eq!(
        [
            &captured.base,
            &captured.ours,
            &captured.theirs,
            &captured.working
        ]
        .iter()
        .map(|version| version.byte_length)
        .sum::<u64>(),
        256 * 1024
    );
    for (ours, theirs) in [(None, Some(&b"theirs\n"[..])), (Some(&b"ours\n"[..]), None)] {
        let temp = conflict_versions("file.txt", Some(b"base\n"), ours, theirs);
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        let captured = service
            .ai_detail(temp.path(), "file.txt", &detail.token)
            .await
            .unwrap();
        assert_eq!(captured.ours.exists, ours.is_some());
        assert_eq!(captured.theirs.exists, theirs.is_some());
    }
}

#[tokio::test]
async fn ai_conflict_capture_rejects_link_and_submodule_index_modes() {
    use std::io::Write;
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let oid = service
        .detail(temp.path(), "file.txt")
        .await
        .unwrap()
        .ours
        .oid
        .unwrap();
    for mode in ["120000", "160000"] {
        let input =
            format!("0 {oid}\tfile.txt\n{mode} {oid} 2\tfile.txt\n100644 {oid} 3\tfile.txt\n");
        let mut child = Command::new("git")
            .current_dir(temp.path())
            .args(["update-index", "--index-info"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        assert_eq!(
            service
                .ai_detail(temp.path(), "file.txt", &detail.token)
                .await
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedConflict
        );
    }
}

#[tokio::test]
async fn ai_conflict_capture_actual_rebase_cherry_pick_and_index_versions() {
    for command in ["rebase", "cherry-pick", "index"] {
        let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
        if command == "index" {
            std::fs::remove_file(temp.path().join(".git/MERGE_HEAD")).unwrap();
        } else {
            ok(temp.path(), &["merge", "--abort"]);
            assert!(!git(temp.path(), &[command, "incoming"]).status.success());
        }
        let service = ConflictService::default();
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        let frozen =
            super::ai_conflict_context::capture(&service, temp.path(), "file.txt", &detail.token)
                .await
                .unwrap();
        assert_eq!(frozen.context.base_oid, detail.base.oid);
        assert_eq!(frozen.context.ours_oid, detail.ours.oid);
        assert_eq!(frozen.context.theirs_oid, detail.theirs.oid);
        let input: serde_json::Value = serde_json::from_str(&frozen.prompts[0].user).unwrap();
        if command == "rebase" {
            assert_eq!(input["labels"]["ours"], "变基目标 + 已重放提交");
            assert_eq!(input["labels"]["theirs"], "正在重放的提交");
            assert_eq!(input["versions"]["ours"]["text"], "theirs\n");
            assert_eq!(input["versions"]["theirs"]["text"], "ours\n");
        }
        let input_before = frozen.prompts[0].user.clone();
        std::fs::write(temp.path().join("file.txt"), b"later outside edit").unwrap();
        assert_eq!(frozen.prompts[0].user, input_before);
        assert_eq!(
            frozen.context.operation_kind,
            match command {
                "rebase" => RepositoryOperationKind::Rebase,
                "cherry-pick" => RepositoryOperationKind::CherryPick,
                _ => RepositoryOperationKind::None,
            }
        );
    }
}

#[tokio::test]
async fn merge_detail_save_recovery_and_explicit_continue() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let snapshot = service.snapshot(temp.path()).await.unwrap();
    assert_eq!(
        snapshot.operation_state.kind,
        RepositoryOperationKind::Merge
    );
    assert_eq!(snapshot.files.len(), 1);
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    assert_eq!(detail.base.text.as_deref(), Some("base\n"));
    assert_eq!(detail.ours.text.as_deref(), Some("ours\n"));
    assert!(detail.editable);
    let original = std::fs::read(temp.path().join("file.txt")).unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "resolved".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert!(result.resolved);
    assert_eq!(
        std::fs::read(temp.path().join("file.txt")).unwrap(),
        b"resolved"
    );
    assert_eq!(
        std::fs::read(Path::new(result.recovery_path.as_ref().unwrap()).join("original")).unwrap(),
        original
    );
    assert_eq!(result.operation_state.kind, RepositoryOperationKind::Merge);
    let continued = service
        .continue_operation(temp.path(), &result.conflicts.operation_token)
        .await
        .unwrap();
    assert!(continued.error.is_none(), "{:?}", continued.error);
    assert_eq!(
        continued.operation_state.kind,
        RepositoryOperationKind::None
    );
    assert!(continued.refs.is_some());
}

#[tokio::test]
async fn rejects_stale_file_index_head_and_unsafe_paths() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    for mutation in ["file", "index", "head"] {
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        match mutation {
            "file" => std::fs::write(temp.path().join("file.txt"), b"external\n").unwrap(),
            "index" => {
                std::fs::write(temp.path().join("unrelated.txt"), b"new").unwrap();
                ok(temp.path(), &["add", "unrelated.txt"]);
            }
            _ => ok(temp.path(), &["update-ref", "HEAD", "incoming"]),
        }
        let error = service
            .resolve(
                temp.path(),
                ResolveConflictRequest {
                    relative_path: "file.txt".into(),
                    token: detail.token,
                    resolution: ConflictResolution::Ours,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleConflict);
    }
    for path in [
        "../outside",
        ".git/config",
        "file.txt:stream",
        "/absolute",
        "sub/../file.txt",
    ] {
        assert!(service.detail(temp.path(), path).await.is_err(), "{path}");
    }
}

#[tokio::test]
async fn index_lock_keeps_saved_file_recovery_and_unresolved_state() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    std::fs::write(temp.path().join(".git/index.lock"), b"held").unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "saved".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(result.error.unwrap().code, ErrorCode::GitLocked);
    assert!(!result.resolved);
    assert!(result.recovery_path.is_some());
    assert_eq!(
        std::fs::read(temp.path().join("file.txt")).unwrap(),
        b"saved"
    );
    assert_eq!(result.conflicts.files.len(), 1);
}

#[tokio::test]
async fn preserves_bom_crlf_and_requires_marker_acknowledgement() {
    let temp = conflict(
        "file.txt",
        b"\xef\xbb\xbfbase\r\n",
        b"\xef\xbb\xbfours\r\n",
        b"\xef\xbb\xbftheirs\r\n",
    );
    // Git may emit LF markers; simulate the user's CRLF editor buffer on disk.
    std::fs::write(
        temp.path().join("file.txt"),
        b"\xef\xbb\xbf<<<<<<< ours\r\n=======\r\n>>>>>>> theirs\r\n",
    )
    .unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    assert!(detail.working.bom);
    assert_eq!(detail.working.line_ending, ConflictLineEnding::Crlf);
    let request = ResolveConflictRequest {
        relative_path: "file.txt".into(),
        token: detail.token.clone(),
        resolution: ConflictResolution::Text {
            text: "<<<<<<< literal\nkept".into(),
            acknowledge_markers: false,
        },
    };
    assert_eq!(
        service
            .resolve(temp.path(), request)
            .await
            .unwrap_err()
            .code,
        ErrorCode::UnsupportedConflict
    );
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "resolved\nlast".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap();
    assert!(result.resolved);
    assert_eq!(
        std::fs::read(temp.path().join("file.txt")).unwrap(),
        b"\xef\xbb\xbfresolved\r\nlast"
    );
}

#[tokio::test]
async fn whole_side_adoption_is_exact_for_binary_invalid_utf8_and_large_files() {
    for ours in [
        b"ours\0\xff".to_vec(),
        b"ours\xff".to_vec(),
        vec![b'x'; 2 * 1024 * 1024 + 1],
    ] {
        let temp = conflict("file.bin", b"base\0", &ours, b"theirs\0");
        let service = ConflictService::default();
        let detail = service.detail(temp.path(), "file.bin").await.unwrap();
        assert!(!detail.editable);
        assert!(detail.ours.text.is_none());
        assert!(detail.can_choose_ours);
        let result = service
            .resolve(
                temp.path(),
                ResolveConflictRequest {
                    relative_path: "file.bin".into(),
                    token: detail.token,
                    resolution: ConflictResolution::Ours,
                },
            )
            .await
            .unwrap();
        assert!(result.resolved, "{:?}", result.error);
        assert_eq!(std::fs::read(temp.path().join("file.bin")).unwrap(), ours);
    }
}

#[tokio::test]
async fn deletion_and_literal_path_stage_only_selected_path() {
    let name = "literal [1] file.txt";
    let temp = conflict(name, b"base\n", b"ours\n", b"theirs\n");
    std::fs::write(temp.path().join("literal 1 file.txt"), b"unrelated").unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), name).await.unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: name.into(),
                token: detail.token,
                resolution: ConflictResolution::Delete,
            },
        )
        .await
        .unwrap();
    assert!(result.resolved);
    assert!(!temp.path().join(name).exists());
    assert!(
        !result
            .conflicts
            .staged_files
            .contains(&"literal 1 file.txt".to_owned())
    );
}

#[tokio::test]
async fn mixed_line_endings_are_read_only_and_recovery_failure_aborts_write() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    std::fs::write(temp.path().join("file.txt"), b"a\r\nb\n").unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    assert_eq!(detail.working.kind, ConflictVersionKind::MixedLineEndings);
    assert!(!detail.editable);
    std::fs::write(temp.path().join(".git/hq-git-recovery"), b"blocked").unwrap();
    assert!(
        service
            .resolve(
                temp.path(),
                ResolveConflictRequest {
                    relative_path: "file.txt".into(),
                    token: detail.token,
                    resolution: ConflictResolution::Ours
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(temp.path().join("file.txt")).unwrap(),
        b"a\r\nb\n"
    );
}

#[tokio::test]
async fn add_add_and_modify_delete_represent_missing_versions() {
    for (base, ours, theirs) in [
        (None, Some(&b"ours\n"[..]), Some(&b"theirs\n"[..])),
        (Some(&b"base\n"[..]), None, Some(&b"theirs\n"[..])),
    ] {
        let temp = conflict_versions("file.txt", base, ours, theirs);
        let service = ConflictService::default();
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        assert_eq!(detail.base.exists, base.is_some());
        assert_eq!(detail.ours.exists, ours.is_some());
        assert_eq!(detail.can_choose_ours, ours.is_some());
        let result = service
            .resolve(
                temp.path(),
                ResolveConflictRequest {
                    relative_path: "file.txt".into(),
                    token: detail.token,
                    resolution: ConflictResolution::Theirs,
                },
            )
            .await
            .unwrap();
        assert!(result.resolved, "{:?}", result.error);
        assert_eq!(
            std::fs::read(temp.path().join("file.txt")).unwrap(),
            b"theirs\n"
        );
    }
}

#[tokio::test]
async fn rebase_and_cherry_pick_continue_refresh_refs() {
    for kind in [
        RepositoryOperationKind::Rebase,
        RepositoryOperationKind::CherryPick,
    ] {
        let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
        ok(temp.path(), &["merge", "--abort"]);
        let command = if kind == RepositoryOperationKind::Rebase {
            "rebase"
        } else {
            "cherry-pick"
        };
        assert!(!git(temp.path(), &[command, "incoming"]).status.success());
        let service = ConflictService::default();
        let detail = service.detail(temp.path(), "file.txt").await.unwrap();
        assert_eq!(detail.operation_kind, kind);
        let result = service
            .resolve(
                temp.path(),
                ResolveConflictRequest {
                    relative_path: "file.txt".into(),
                    token: detail.token,
                    resolution: ConflictResolution::Text {
                        text: "resolved\n".into(),
                        acknowledge_markers: false,
                    },
                },
            )
            .await
            .unwrap();
        let continued = service
            .continue_operation(temp.path(), &result.conflicts.operation_token)
            .await
            .unwrap();
        assert!(continued.error.is_none(), "{:?}", continued.error);
        assert_eq!(
            continued.operation_state.kind,
            RepositoryOperationKind::None
        );
        assert!(continued.refs.is_some());
    }
}

#[tokio::test]
async fn stash_conflict_has_no_continue_and_retains_stash() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    ok(temp.path(), &["merge", "--abort"]);
    std::fs::write(temp.path().join("file.txt"), b"stashed\n").unwrap();
    ok(temp.path(), &["stash", "push"]);
    std::fs::write(temp.path().join("file.txt"), b"current\n").unwrap();
    ok(temp.path(), &["commit", "-am", "current"]);
    assert!(!git(temp.path(), &["stash", "apply"]).status.success());
    let service = ConflictService::default();
    let snapshot = service.snapshot(temp.path()).await.unwrap();
    assert_eq!(snapshot.operation_state.kind, RepositoryOperationKind::None);
    assert!(snapshot.continue_action.is_none());
    assert!(snapshot.operation_state.abort_action.is_none());
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Theirs,
            },
        )
        .await
        .unwrap();
    assert!(result.resolved);
    assert!(result.conflicts.continue_action.is_none());
    assert!(!git(temp.path(), &["stash", "list"]).stdout.is_empty());
}

#[tokio::test]
async fn continuation_rejects_stale_index_and_returns_empty_commit_failure() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    ok(temp.path(), &["merge", "--abort"]);
    assert!(
        !git(temp.path(), &["cherry-pick", "incoming"])
            .status
            .success()
    );
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Ours,
            },
        )
        .await
        .unwrap();
    let failed = service
        .continue_operation(temp.path(), &result.conflicts.operation_token)
        .await
        .unwrap();
    assert!(failed.error.is_some());
    assert_eq!(
        failed.operation_state.kind,
        RepositoryOperationKind::CherryPick
    );
    assert!(failed.conflicts.files.is_empty());
    assert!(failed.refs.is_some());
    let token = failed.conflicts.operation_token;
    std::fs::write(temp.path().join("extra.txt"), b"extra").unwrap();
    ok(temp.path(), &["add", "extra.txt"]);
    assert_eq!(
        service
            .continue_operation(temp.path(), &token)
            .await
            .unwrap_err()
            .code,
        ErrorCode::StaleConflict
    );
}

#[tokio::test]
async fn continuation_can_encounter_another_rebase_conflict() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    ok(temp.path(), &["merge", "--abort"]);
    std::fs::write(temp.path().join("file.txt"), b"ours again\n").unwrap();
    ok(temp.path(), &["commit", "-am", "second replay"]);
    assert!(!git(temp.path(), &["rebase", "incoming"]).status.success());
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "combined\n".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap();
    let continued = service
        .continue_operation(temp.path(), &result.conflicts.operation_token)
        .await
        .unwrap();
    assert!(continued.error.is_some());
    assert_eq!(
        continued.operation_state.kind,
        RepositoryOperationKind::Rebase
    );
    assert_eq!(continued.conflicts.files.len(), 1);
    assert!(continued.refs.is_some());
}

#[tokio::test]
async fn symlink_stages_are_display_only() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let oid = detail.ours.oid.unwrap();
    let index = format!("0 {oid}\tfile.txt\n120000 {oid} 2\tfile.txt\n100644 {oid} 3\tfile.txt\n");
    let mut child = Command::new("git")
        .current_dir(temp.path())
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(index.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    assert!(!detail.editable);
    assert!(!detail.can_choose_ours);
    assert!(!detail.can_choose_theirs);
    assert!(!detail.can_delete);
    assert!(
        service
            .resolve(
                temp.path(),
                ResolveConflictRequest {
                    relative_path: "file.txt".into(),
                    token: detail.token,
                    resolution: ConflictResolution::Theirs
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn snapshot_supports_an_unborn_repository() {
    let temp = tempfile::tempdir().unwrap();
    ok(temp.path(), &["init", "-b", "main"]);
    let snapshot = ConflictService::default()
        .snapshot(temp.path())
        .await
        .unwrap();
    assert!(snapshot.files.is_empty());
    assert!(snapshot.continue_action.is_none());
    assert_eq!(snapshot.operation_state.kind, RepositoryOperationKind::None);
    assert!(!snapshot.operation_token.is_empty());
}

#[tokio::test]
async fn aborting_and_restarting_same_merge_invalidates_old_detail() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    ok(temp.path(), &["merge", "--abort"]);
    assert!(!git(temp.path(), &["merge", "incoming"]).status.success());
    let error = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Ours,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleConflict);
}

#[tokio::test]
async fn staging_failure_explicitly_reports_that_working_file_was_saved() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    std::fs::write(temp.path().join(".git/index.lock"), b"held").unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "saved".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap();
    assert!(
        result
            .error
            .unwrap()
            .message
            .contains("working file was saved")
    );
}

#[tokio::test]
async fn unicode_paths_and_checkout_line_ending_filters_are_preserved() {
    let name = "\u{51b2}\u{7a81} [draft].txt";
    let temp = conflict(name, b"base\n", b"ours\n", b"theirs\n");
    std::fs::write(
        temp.path().join(".git/info/attributes"),
        b"*.txt text eol=crlf\n",
    )
    .unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), name).await.unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: name.into(),
                token: detail.token,
                resolution: ConflictResolution::Theirs,
            },
        )
        .await
        .unwrap();
    assert!(result.resolved, "{:?}", result.error);
    assert_eq!(
        std::fs::read(temp.path().join(name)).unwrap(),
        b"theirs\r\n"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn failed_atomic_replace_preserves_original_and_returns_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let original = std::fs::read(temp.path().join("file.txt")).unwrap();
    const SHARE_READ_AND_WRITE: u32 = 3;
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_AND_WRITE)
        .open(temp.path().join("file.txt"))
        .unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "replacement".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap();
    assert!(result.error.is_some());
    assert!(!result.resolved);
    assert!(result.recovery_path.is_some());
    assert_eq!(
        std::fs::read(temp.path().join("file.txt")).unwrap(),
        original
    );
    drop(handle);
}

#[tokio::test]
async fn parent_link_cannot_read_or_write_outside_the_repository() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("file.txt"), b"outside private content").unwrap();
    #[cfg(windows)]
    assert!(
        Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(temp.path().join("linked"))
            .arg(outside.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), temp.path().join("linked")).unwrap();
    let service = ConflictService::default();
    let source = service.detail(temp.path(), "file.txt").await.unwrap();
    let oid = source.ours.oid.unwrap();
    let index = format!(
        "100644 {oid} 1\tlinked/file.txt\n100644 {oid} 2\tlinked/file.txt\n100644 {oid} 3\tlinked/file.txt\n"
    );
    let mut child = Command::new("git")
        .current_dir(temp.path())
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(index.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    let detail = service
        .detail(temp.path(), "linked/file.txt")
        .await
        .unwrap();
    assert!(!detail.can_delete);
    assert!(!detail.can_choose_ours);
    assert!(detail.working.text.is_none());
    assert!(
        service
            .resolve(
                temp.path(),
                ResolveConflictRequest {
                    relative_path: "linked/file.txt".into(),
                    token: detail.token,
                    resolution: ConflictResolution::Ours
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(outside.path().join("file.txt")).unwrap(),
        b"outside private content"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn manual_resolution_preserves_executable_mode() {
    use std::os::unix::fs::PermissionsExt;
    let temp = conflict("file.sh", b"base\n", b"ours\n", b"theirs\n");
    let file = temp.path().join("file.sh");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.sh").await.unwrap();
    let result = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.sh".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "resolved\n".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap();
    assert!(result.resolved);
    assert_ne!(
        std::fs::metadata(file).unwrap().permissions().mode() & 0o111,
        0
    );
}

#[tokio::test]
async fn externally_committed_cherry_pick_keeps_pending_sequencer_continue() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    ok(temp.path(), &["merge", "--abort"]);
    ok(temp.path(), &["switch", "incoming"]);
    std::fs::write(temp.path().join("second.txt"), b"second commit\n").unwrap();
    ok(temp.path(), &["add", "second.txt"]);
    ok(temp.path(), &["commit", "-m", "second incoming"]);
    ok(temp.path(), &["switch", "main"]);
    assert!(
        !git(temp.path(), &["cherry-pick", "incoming~1", "incoming"])
            .status
            .success()
    );
    std::fs::write(temp.path().join("file.txt"), b"external resolution\n").unwrap();
    ok(temp.path(), &["add", "file.txt"]);
    ok(temp.path(), &["commit", "--no-edit"]);
    assert!(!temp.path().join(".git/CHERRY_PICK_HEAD").exists());
    assert!(temp.path().join(".git/sequencer/todo").exists());
    let service = ConflictService::default();
    let snapshot = service.snapshot(temp.path()).await.unwrap();
    assert_eq!(
        snapshot.operation_state.kind,
        RepositoryOperationKind::CherryPick
    );
    assert_eq!(
        snapshot.continue_action,
        Some(crate::domain::operation::AbortAction::CherryPick)
    );
    let result = service
        .continue_operation(temp.path(), &snapshot.operation_token)
        .await
        .unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.operation_state.kind, RepositoryOperationKind::None);
    assert_eq!(
        std::fs::read(temp.path().join("second.txt")).unwrap(),
        b"second commit\n"
    );
}

#[tokio::test]
async fn two_sources_renamed_to_one_destination_are_display_only() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    ok(temp.path(), &["merge", "--abort"]);
    std::fs::write(temp.path().join("a.txt"), b"source a\n").unwrap();
    std::fs::write(temp.path().join("b.txt"), b"source b\n").unwrap();
    ok(temp.path(), &["add", "--all"]);
    ok(temp.path(), &["commit", "-m", "rename base"]);
    ok(temp.path(), &["switch", "-c", "rename-incoming"]);
    ok(temp.path(), &["mv", "b.txt", "dest.txt"]);
    ok(temp.path(), &["commit", "-m", "rename b"]);
    ok(temp.path(), &["switch", "main"]);
    ok(temp.path(), &["mv", "a.txt", "dest.txt"]);
    ok(temp.path(), &["commit", "-m", "rename a"]);
    assert!(
        !git(temp.path(), &["merge", "rename-incoming"])
            .status
            .success()
    );
    let service = ConflictService::default();
    let snapshot = service.snapshot(temp.path()).await.unwrap();
    assert!(
        snapshot.files.iter().all(|file| !file.supported),
        "{:?}",
        snapshot.files
    );
    for file in snapshot.files {
        let detail = service.detail(temp.path(), &file.path).await.unwrap();
        assert!(!detail.can_choose_ours && !detail.can_choose_theirs && !detail.can_delete);
    }
}

#[tokio::test]
async fn failed_post_save_refresh_retains_recovery_location() {
    let temp = conflict("file.txt", b"base\n", b"ours\n", b"theirs\n");
    let service = ConflictService::default();
    let detail = service.detail(temp.path(), "file.txt").await.unwrap();
    let hook = temp.path().join(".git/hooks/post-index-change");
    std::fs::write(
        &hook,
        b"#!/bin/sh\ngit config core.repositoryformatversion 999\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let error = service
        .resolve(
            temp.path(),
            ResolveConflictRequest {
                relative_path: "file.txt".into(),
                token: detail.token,
                resolution: ConflictResolution::Text {
                    text: "saved\n".into(),
                    acknowledge_markers: false,
                },
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        std::fs::read(temp.path().join("file.txt")).unwrap(),
        b"saved\n"
    );
    assert!(error.message.contains("state refresh failed"), "{error:?}");
    assert!(
        error
            .diagnostics
            .as_deref()
            .unwrap_or("")
            .contains("hq-git-recovery"),
        "{error:?}"
    );
}
