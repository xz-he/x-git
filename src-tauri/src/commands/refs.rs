use std::path::Path;

use tauri::State;

use crate::commands::AppState;
use crate::domain::error::BackendError;
use crate::domain::operation::AbortAction;
use crate::domain::refs::{
    CreateBranchRequest, DeleteBranchRequest, RefsMutationResult, RefsSnapshot,
};

#[tauri::command]
pub async fn refs_snapshot(
    state: State<'_, AppState>,
    path: String,
) -> Result<RefsSnapshot, BackendError> {
    state.refs.snapshot(Path::new(&path)).await
}

#[tauri::command]
pub async fn refs_create_branch(
    state: State<'_, AppState>,
    path: String,
    request: CreateBranchRequest,
) -> Result<RefsMutationResult, BackendError> {
    state.refs.create_branch(Path::new(&path), request).await
}

#[tauri::command]
pub async fn refs_switch_branch(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<RefsMutationResult, BackendError> {
    state.refs.switch_branch(Path::new(&path), &name).await
}

#[tauri::command]
pub async fn refs_delete_branch(
    state: State<'_, AppState>,
    path: String,
    request: DeleteBranchRequest,
) -> Result<RefsMutationResult, BackendError> {
    state.refs.delete_branch(Path::new(&path), request).await
}

#[tauri::command]
pub async fn refs_merge(
    state: State<'_, AppState>,
    path: String,
    target: String,
) -> Result<RefsMutationResult, BackendError> {
    state.refs.merge(Path::new(&path), &target).await
}

#[tauri::command]
pub async fn refs_rebase(
    state: State<'_, AppState>,
    path: String,
    target: String,
) -> Result<RefsMutationResult, BackendError> {
    state.refs.rebase(Path::new(&path), &target).await
}

#[tauri::command]
pub async fn refs_abort(
    state: State<'_, AppState>,
    path: String,
    action: AbortAction,
) -> Result<RefsMutationResult, BackendError> {
    state.refs.abort(Path::new(&path), action).await
}
