use std::path::Path;

use tauri::State;

use super::AppState;
use crate::domain::conflicts::{
    ConflictDetail, ConflictMutationResult, ConflictSnapshot, ResolveConflictRequest,
};
use crate::domain::error::BackendError;

#[tauri::command]
pub async fn conflicts_snapshot(
    state: State<'_, AppState>,
    path: String,
) -> Result<ConflictSnapshot, BackendError> {
    state.conflicts.snapshot(Path::new(&path)).await
}

#[tauri::command]
pub async fn conflicts_detail(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
) -> Result<ConflictDetail, BackendError> {
    state
        .conflicts
        .detail(Path::new(&path), &relative_path)
        .await
}

#[tauri::command]
pub async fn conflicts_resolve(
    state: State<'_, AppState>,
    path: String,
    request: ResolveConflictRequest,
) -> Result<ConflictMutationResult, BackendError> {
    state.conflicts.resolve(Path::new(&path), request).await
}

#[tauri::command]
pub async fn conflicts_continue(
    state: State<'_, AppState>,
    path: String,
    operation_token: String,
) -> Result<ConflictMutationResult, BackendError> {
    state
        .conflicts
        .continue_operation(Path::new(&path), &operation_token)
        .await
}
