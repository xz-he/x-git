use crate::commands::AppState;
use crate::domain::changes::FileDiff;
use crate::domain::error::BackendError;
use crate::domain::stash::{
    StashCreateRequest, StashDetail, StashMutationResult, StashSelection, StashSnapshot,
};
use std::path::Path;
use tauri::State;

#[tauri::command]
pub async fn stash_snapshot(
    state: State<'_, AppState>,
    path: String,
) -> Result<StashSnapshot, BackendError> {
    state.stashes.snapshot(Path::new(&path)).await
}

#[tauri::command]
pub async fn stash_detail(
    state: State<'_, AppState>,
    path: String,
    selection: StashSelection,
) -> Result<StashDetail, BackendError> {
    state.stashes.detail(Path::new(&path), selection).await
}

#[tauri::command]
pub async fn stash_file_diff(
    state: State<'_, AppState>,
    path: String,
    selection: StashSelection,
    relative_path: String,
    untracked: Option<bool>,
) -> Result<FileDiff, BackendError> {
    state
        .stashes
        .file_diff(
            Path::new(&path),
            selection,
            &relative_path,
            untracked.unwrap_or(false),
        )
        .await
}

#[tauri::command]
pub async fn stash_create(
    state: State<'_, AppState>,
    path: String,
    request: StashCreateRequest,
) -> Result<StashMutationResult, BackendError> {
    state.stashes.create(Path::new(&path), request).await
}

#[tauri::command]
pub async fn stash_apply(
    state: State<'_, AppState>,
    path: String,
    selection: StashSelection,
) -> Result<StashMutationResult, BackendError> {
    state.stashes.apply(Path::new(&path), selection).await
}

#[tauri::command]
pub async fn stash_pop(
    state: State<'_, AppState>,
    path: String,
    selection: StashSelection,
) -> Result<StashMutationResult, BackendError> {
    state.stashes.pop(Path::new(&path), selection).await
}
