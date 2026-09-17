use crate::{
    application::activity_service::{ActivityReply, ActivitySummary},
    commands::AppState,
    domain::{
        error::{BackendError, ErrorCode},
        operation::MutationWorkspace,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::path::Path;
use tauri::State;

#[tauri::command]
pub async fn activity_record_external(
    state: State<'_, AppState>,
    path: String,
    title: String,
    success: bool,
    message: String,
) -> Result<(), BackendError> {
    if title.chars().count() > 120 || message.len() > 4096 {
        return Err(invalid());
    }
    state
        .activity
        .record_external(Path::new(&path), &title, success, &message)
        .await
}
#[tauri::command]
pub async fn activity_list(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<ActivitySummary>, BackendError> {
    state.activity.list(Path::new(&path)).await
}
#[tauri::command]
pub async fn activity_rollback(
    state: State<'_, AppState>,
    path: String,
    id: String,
) -> Result<MutationWorkspace, BackendError> {
    state.activity.rollback(Path::new(&path), &id).await
}
#[tauri::command]
pub async fn activity_execute(
    state: State<'_, AppState>,
    action: String,
    args: Value,
) -> Result<ActivityReply, BackendError> {
    execute_recorded(state.inner(), action, args).await
}

async fn execute_recorded(
    state: &AppState,
    action: String,
    args: Value,
) -> Result<ActivityReply, BackendError> {
    let path: String = field(&args, "path")?;
    let title = action_title(&action).ok_or_else(invalid)?;
    let details = ["relativePath", "relativePaths", "name", "target", "commit"]
        .iter()
        .find_map(|key| args.get(key))
        .or_else(|| {
            args.get("request").and_then(|r| {
                r.get("name")
                    .or_else(|| r.get("commit"))
                    .or_else(|| r.get("target"))
            })
        });
    let title = if let Some(details) = details {
        format!(
            "{title} · {}",
            details.to_string().chars().take(240).collect::<String>()
        )
    } else {
        title.into()
    };
    state
        .activity
        .execute(
            Path::new(&path),
            &action,
            &title,
            dispatch(state, Path::new(&path), &action, &args),
        )
        .await
}
fn field<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, BackendError> {
    serde_json::from_value(args.get(key).cloned().ok_or_else(invalid)?).map_err(|_| invalid())
}
fn value<T: Serialize>(result: Result<T, BackendError>) -> Result<Value, BackendError> {
    serde_json::to_value(result?).map_err(|_| invalid())
}
fn invalid() -> BackendError {
    BackendError::new(ErrorCode::InvalidPath, "操作历史请求参数无效。")
}

pub fn action_title(action: &str) -> Option<&'static str> {
    Some(match action {
        "changes_stage_file" => "暂存文件",
        "changes_stage_files" => "批量暂存",
        "changes_unstage_file" => "取消暂存文件",
        "changes_unstage_files" => "批量取消暂存",
        "changes_stage_hunk" => "暂存代码块",
        "changes_unstage_hunk" => "取消暂存代码块",
        "changes_stage_lines" => "暂存代码行",
        "changes_unstage_lines" => "取消暂存代码行",
        "changes_discard_file" => "丢弃文件修改",
        "changes_restore_noise" => "恢复无效变更",
        "changes_commit" => "创建提交",
        "refs_create_branch" => "创建分支",
        "refs_switch_branch" => "切换分支",
        "refs_delete_branch" => "删除分支",
        "refs_merge" => "合并分支",
        "refs_rebase" => "变基",
        "refs_abort" => "中止 Git 操作",
        "history_checkout" => "检出提交",
        "history_revert" => "Revert 提交",
        "history_cherry_pick" => "Cherry-pick 提交",
        "history_reset" => "Reset 提交",
        "stash_create" => "贮藏修改",
        "stash_apply" => "应用贮藏",
        "stash_pop" => "弹出贮藏",
        "conflicts_resolve" => "解决冲突",
        "conflicts_continue" => "继续 Git 操作",
        "files_execute" => "文件操作",
        "task_branches_create" => "创建任务分支",
        "task_branches_run" => "提交并移植",
        "task_branches_unlink" => "解除任务分支关联",
        _ => return None,
    })
}
fn dispatch<'a>(
    state: &'a AppState,
    root: &'a Path,
    action: &str,
    args: &'a Value,
) -> futures_util::future::BoxFuture<'a, Result<Value, BackendError>> {
    // Keep each operation in its own future. A single async match combines all
    // service call paths into one poll function and overflows the Windows 1 MiB
    // stack when invoked through activity_execute, even for a simple branch.
    macro_rules! operation {
        ($result:expr $(,)?) => {
            Box::pin(async move { value($result) })
        };
    }
    match action {
        "changes_stage_file" => operation!(
            state
                .changes
                .stage_file(root, &field::<String>(args, "relativePath")?)
                .await,
        ),
        "changes_unstage_file" => operation!(
            state
                .changes
                .unstage_file(root, &field::<String>(args, "relativePath")?)
                .await,
        ),
        "changes_stage_files" => operation!(
            state
                .changes
                .stage_files(root, &field::<Vec<String>>(args, "relativePaths")?)
                .await,
        ),
        "changes_unstage_files" => operation!(
            state
                .changes
                .unstage_files(root, &field::<Vec<String>>(args, "relativePaths")?)
                .await,
        ),
        "changes_stage_hunk" => operation!(
            state
                .changes
                .stage_hunk(
                    root,
                    &field::<String>(args, "relativePath")?,
                    field(args, "hunkIndex")?,
                )
                .await,
        ),
        "changes_unstage_hunk" => operation!(
            state
                .changes
                .unstage_hunk(
                    root,
                    &field::<String>(args, "relativePath")?,
                    field(args, "hunkIndex")?,
                )
                .await,
        ),
        "changes_stage_lines" => operation!(
            state
                .changes
                .stage_lines(
                    root,
                    &field::<String>(args, "relativePath")?,
                    field(args, "startLine")?,
                    field(args, "endLine")?,
                )
                .await,
        ),
        "changes_unstage_lines" => operation!(
            state
                .changes
                .unstage_lines(
                    root,
                    &field::<String>(args, "relativePath")?,
                    field(args, "startLine")?,
                    field(args, "endLine")?,
                )
                .await,
        ),
        "changes_discard_file" => operation!(
            state
                .changes
                .discard_file(root, &field::<String>(args, "relativePath")?)
                .await,
        ),
        "changes_restore_noise" => operation!(
            state
                .changes
                .restore_noise(root, field(args, "selected")?)
                .await,
        ),
        "changes_commit" => operation!(
            state
                .changes
                .commit(root, &field::<String>(args, "message")?)
                .await,
        ),
        "refs_create_branch" => operation!(
            state
                .refs
                .create_branch(root, field(args, "request")?)
                .await,
        ),
        "refs_switch_branch" => operation!(
            state
                .refs
                .switch_branch(root, &field::<String>(args, "name")?)
                .await,
        ),
        "refs_delete_branch" => operation!(
            state
                .refs
                .delete_branch(root, field(args, "request")?)
                .await,
        ),
        "refs_merge" => operation!(
            state
                .refs
                .merge(root, &field::<String>(args, "target")?)
                .await,
        ),
        "refs_rebase" => operation!(
            state
                .refs
                .rebase(root, &field::<String>(args, "target")?)
                .await,
        ),
        "refs_abort" => operation!(state.refs.abort(root, field(args, "action")?).await),
        "history_checkout" => operation!(
            state
                .history
                .checkout(root, &field::<String>(args, "commit")?)
                .await,
        ),
        "history_revert" => operation!(state.history.revert(root, field(args, "request")?).await),
        "history_cherry_pick" => operation!(
            state
                .history
                .cherry_pick(root, field(args, "request")?)
                .await,
        ),
        "history_reset" => operation!(state.history.reset(root, field(args, "request")?).await),
        "stash_create" => operation!(state.stashes.create(root, field(args, "request")?).await),
        "stash_apply" => operation!(state.stashes.apply(root, field(args, "selection")?).await),
        "stash_pop" => operation!(state.stashes.pop(root, field(args, "selection")?).await),
        "conflicts_resolve" => {
            operation!(state.conflicts.resolve(root, field(args, "request")?).await)
        }
        "conflicts_continue" => operation!(
            state
                .conflicts
                .continue_operation(root, &field::<String>(args, "operationToken")?)
                .await,
        ),
        "files_execute" => operation!(state.files.execute(root, field(args, "request")?).await),
        "task_branches_create" => operation!(Ok(state
            .task_branches
            .create(root, field(args, "request")?)
            .await)),
        "task_branches_run" => operation!(Ok(state
            .task_branches
            .run(root, field(args, "request")?)
            .await)),
        "task_branches_unlink" => operation!(
            state
                .task_branches
                .unlink(root, &field::<String>(args, "id")?)
                .await,
        ),
        _ => Box::pin(async { Err(invalid()) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commands::build_app_state, infrastructure::settings_repository::SettingsPaths};

    #[test]
    fn create_branch_through_activity_on_windows_sized_stack() {
        const CHILD: &str = "HQ_GIT_ACTIVITY_STACK_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // Stack overflow aborts the process, so isolate the native command path.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "commands::activity::tests::create_branch_through_activity_on_windows_sized_stack", "--nocapture"])
                .env(CHILD, "1")
                .output().unwrap();
            assert!(
                output.status.success(),
                "activity command crashed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        std::thread::Builder::new().stack_size(1024 * 1024).spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async {
                let directory = tempfile::tempdir().unwrap();
                let root = directory.path();
                let state = build_app_state(SettingsPaths {
                    current: root.join("settings.json"), legacy_candidates: vec![], migration_marker: root.join("migration.complete"),
                });
                let runner = crate::infrastructure::git_runner::GitCommandRunner::default();
                for args in [vec!["init", "-b", "main"], vec!["-c", "user.name=Stack Test", "-c", "user.email=stack@example.test", "commit", "--allow-empty", "-m", "initial"]] {
                    runner.run(Some(root), args).await.unwrap();
                }
                let args = serde_json::json!({"path": root, "request": {"name": "feature/stack-regression", "startPoint": "HEAD", "switch": true}});
                let reply = execute_recorded(&state, "refs_create_branch".into(), args.clone()).await.unwrap();
                assert!(reply.error.is_none());
                assert_eq!(runner.run(Some(root), ["branch", "--show-current"]).await.unwrap().stdout.trim(), "feature/stack-regression");
                assert_eq!(state.activity.list(root).await.unwrap()[0].status, "success");
                let duplicate = execute_recorded(&state, "refs_create_branch".into(), args).await.unwrap();
                assert!(duplicate.error.is_some());
                let records = state.activity.list(root).await.unwrap();
                assert_eq!(records.len(), 2);
                assert!(records.iter().any(|record| record.status == "failed"));
                assert!(execute_recorded(&state, "refs_create_branch".into(), serde_json::json!({})).await.is_err());

                // Exercise both quick-create modes through the same command boundary.
                runner.run(Some(root), ["branch", "master"]).await.unwrap();
                runner.run(Some(root), ["remote", "add", "origin", "."]).await.unwrap();
                let head = runner.run(Some(root), ["rev-parse", "HEAD"]).await.unwrap().stdout.trim().to_owned();
                for (mode, ticket, source) in [
                    ("current", "R202609170001", "feature/stack-regression"),
                    ("remoteMaster", "R202609170002", "feature/R202609170001-quick-create"),
                ] {
                    let reply = execute_recorded(&state, "task_branches_create".into(), serde_json::json!({
                        "path": root, "request": {"kind": "feature", "ticket": ticket, "slug": "quick-create", "description": "测试创建分支", "mode": mode, "remote": "origin", "sourceBranch": source, "expectedHead": head},
                    })).await.unwrap();
                    assert!(reply.error.is_none());
                    assert!(reply.value.as_ref().unwrap()["error"].is_null(), "{:?}", reply.value);
                    let target = format!("feature/{ticket}-quick-create");
                    runner.run(Some(root), ["show-ref", "--verify", &format!("refs/heads/{target}")]).await.unwrap();
                    let expected = if mode == "current" { target.as_str() } else { source };
                    assert_eq!(runner.run(Some(root), ["branch", "--show-current"]).await.unwrap().stdout.trim(), expected);
                }
                assert_eq!(state.activity.list(root).await.unwrap().iter().filter(|record| record.status == "success").count(), 3);
            });
        }).unwrap().join().unwrap();
    }
}
