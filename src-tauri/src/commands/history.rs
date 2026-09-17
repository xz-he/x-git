use std::path::Path;

use tauri::State;

use crate::commands::AppState;
use crate::domain::changes::FileDiff;
use crate::domain::error::BackendError;
use crate::domain::history::{
    CherryPickRequest, CommitDetail, HistoryMutationResult, HistoryPage, HistoryQuery, ResetRequest, RevertRequest,
};

#[tauri::command]
pub async fn history_page(
    state: State<'_, AppState>,
    path: String,
    query: HistoryQuery,
) -> Result<HistoryPage, BackendError> {
    state.history.page(Path::new(&path), query).await
}

#[tauri::command]
pub async fn history_detail(
    state: State<'_, AppState>,
    path: String,
    commit: String,
) -> Result<CommitDetail, BackendError> {
    state.history.detail(Path::new(&path), &commit).await
}

#[tauri::command]
pub async fn history_file_diff(
    state: State<'_, AppState>,
    path: String,
    commit: String,
    relative_path: String,
) -> Result<FileDiff, BackendError> {
    state
        .history
        .file_diff(Path::new(&path), &commit, &relative_path)
        .await
}

#[tauri::command]
pub async fn history_checkout(
    state: State<'_, AppState>,
    path: String,
    commit: String,
) -> Result<HistoryMutationResult, BackendError> {
    state.history.checkout(Path::new(&path), &commit).await
}

#[tauri::command]
pub async fn history_reset(
    state: State<'_, AppState>,
    path: String,
    request: ResetRequest,
) -> Result<HistoryMutationResult, BackendError> {
    state.history.reset(Path::new(&path), request).await
}

#[tauri::command]
pub async fn history_cherry_pick(
    state: State<'_, AppState>,
    path: String,
    request: CherryPickRequest,
) -> Result<HistoryMutationResult, BackendError> {
    state.history.cherry_pick(Path::new(&path), request).await
}

#[tauri::command]
pub async fn history_revert(state: State<'_, AppState>, path: String, request: RevertRequest) -> Result<HistoryMutationResult, BackendError> {
    state.history.revert(Path::new(&path), request).await
}
