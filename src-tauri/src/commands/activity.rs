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
            dispatch(&state, Path::new(&path), &action, &args),
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
async fn dispatch(
    state: &AppState,
    root: &Path,
    action: &str,
    args: &Value,
) -> Result<Value, BackendError> {
    match action {
        "changes_stage_file" => value(
            state
                .changes
                .stage_file(root, &field::<String>(args, "relativePath")?)
                .await,
        ),
        "changes_unstage_file" => value(
            state
                .changes
                .unstage_file(root, &field::<String>(args, "relativePath")?)
                .await,
        ),
        "changes_stage_files" => value(
            state
                .changes
                .stage_files(root, &field::<Vec<String>>(args, "relativePaths")?)
                .await,
        ),
        "changes_unstage_files" => value(
            state
                .changes
                .unstage_files(root, &field::<Vec<String>>(args, "relativePaths")?)
                .await,
        ),
        "changes_stage_hunk" => value(
            state
                .changes
                .stage_hunk(
                    root,
                    &field::<String>(args, "relativePath")?,
                    field(args, "hunkIndex")?,
                )
                .await,
        ),
        "changes_unstage_hunk" => value(
            state
                .changes
                .unstage_hunk(
                    root,
                    &field::<String>(args, "relativePath")?,
                    field(args, "hunkIndex")?,
                )
                .await,
        ),
        "changes_stage_lines" => value(
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
        "changes_unstage_lines" => value(
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
        "changes_discard_file" => value(
            state
                .changes
                .discard_file(root, &field::<String>(args, "relativePath")?)
                .await,
        ),
        "changes_restore_noise" => value(
            state
                .changes
                .restore_noise(root, field(args, "selected")?)
                .await,
        ),
        "changes_commit" => value(
            state
                .changes
                .commit(root, &field::<String>(args, "message")?)
                .await,
        ),
        "refs_create_branch" => value(
            state
                .refs
                .create_branch(root, field(args, "request")?)
                .await,
        ),
        "refs_switch_branch" => value(
            state
                .refs
                .switch_branch(root, &field::<String>(args, "name")?)
                .await,
        ),
        "refs_delete_branch" => value(
            state
                .refs
                .delete_branch(root, field(args, "request")?)
                .await,
        ),
        "refs_merge" => value(
            state
                .refs
                .merge(root, &field::<String>(args, "target")?)
                .await,
        ),
        "refs_rebase" => value(
            state
                .refs
                .rebase(root, &field::<String>(args, "target")?)
                .await,
        ),
        "refs_abort" => value(state.refs.abort(root, field(args, "action")?).await),
        "history_checkout" => value(
            state
                .history
                .checkout(root, &field::<String>(args, "commit")?)
                .await,
        ),
        "history_revert" => value(state.history.revert(root, field(args, "request")?).await),
        "history_cherry_pick" => value(
            state
                .history
                .cherry_pick(root, field(args, "request")?)
                .await,
        ),
        "history_reset" => value(state.history.reset(root, field(args, "request")?).await),
        "stash_create" => value(state.stashes.create(root, field(args, "request")?).await),
        "stash_apply" => value(state.stashes.apply(root, field(args, "selection")?).await),
        "stash_pop" => value(state.stashes.pop(root, field(args, "selection")?).await),
        "conflicts_resolve" => value(state.conflicts.resolve(root, field(args, "request")?).await),
        "conflicts_continue" => value(
            state
                .conflicts
                .continue_operation(root, &field::<String>(args, "operationToken")?)
                .await,
        ),
        "files_execute" => value(state.files.execute(root, field(args, "request")?).await),
        "task_branches_create" => value(Ok(state
            .task_branches
            .create(root, field(args, "request")?)
            .await)),
        "task_branches_run" => value(Ok(state
            .task_branches
            .run(root, field(args, "request")?)
            .await)),
        "task_branches_unlink" => value(
            state
                .task_branches
                .unlink(root, &field::<String>(args, "id")?)
                .await,
        ),
        _ => Err(invalid()),
    }
}
