use crate::commands::AppState;
use crate::domain::{
    error::BackendError,
    task_branch::{
        CreateTaskBranchRequest, TaskBranchBinding, TaskBranchResult, TaskBranchRunRequest,
    },
};
use std::path::Path;
use tauri::State;

#[tauri::command]
pub async fn task_branches_snapshot(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<TaskBranchBinding>, BackendError> {
    state.task_branches.snapshot(Path::new(&path)).await
}
#[tauri::command]
pub async fn task_branches_unlink(
    state: State<'_, AppState>,
    path: String,
    id: String,
) -> Result<Vec<TaskBranchBinding>, BackendError> {
    state.task_branches.unlink(Path::new(&path), &id).await
}
#[tauri::command]
pub async fn task_branches_create(
    state: State<'_, AppState>,
    path: String,
    request: CreateTaskBranchRequest,
) -> Result<TaskBranchResult, BackendError> {
    Ok(state.task_branches.create(Path::new(&path), request).await)
}
#[tauri::command]
pub async fn task_branches_run(
    state: State<'_, AppState>,
    path: String,
    request: TaskBranchRunRequest,
) -> Result<TaskBranchResult, BackendError> {
    Ok(state.task_branches.run(Path::new(&path), request).await)
}
