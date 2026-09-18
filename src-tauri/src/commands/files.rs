use crate::{
    commands::AppState,
    domain::{error::BackendError, files::*},
};
use std::path::Path;
use tauri::State;

#[tauri::command]
pub async fn files_open(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
    reveal: bool,
) -> Result<(), BackendError> {
    state
        .files
        .open_file(Path::new(&path), &relative_path, reveal)
        .await
}
#[tauri::command]
pub async fn files_inspect(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
    kind: FileInspectionKind,
) -> Result<String, BackendError> {
    state
        .files
        .inspect_file(Path::new(&path), &relative_path, kind)
        .await
}
#[tauri::command]
pub async fn files_ignore_preview(
    state: State<'_, AppState>,
    path: String,
    request: IgnoreRequest,
) -> Result<IgnorePreview, BackendError> {
    state.files.ignore_preview(Path::new(&path), &request).await
}
#[tauri::command]
pub async fn files_ignore(
    state: State<'_, AppState>,
    path: String,
    request: IgnoreRequest,
) -> Result<crate::domain::operation::MutationWorkspace, BackendError> {
    state.files.ignore_file(Path::new(&path), &request).await
}
#[tauri::command]
pub async fn files_untrack(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
) -> Result<crate::domain::operation::MutationWorkspace, BackendError> {
    state
        .files
        .untrack_file(Path::new(&path), &relative_path)
        .await
}
#[tauri::command]
pub async fn files_lfs_track(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
) -> Result<crate::domain::operation::MutationWorkspace, BackendError> {
    state
        .files
        .track_lfs(Path::new(&path), &relative_path)
        .await
}

#[tauri::command]
pub async fn files_list(
    state: State<'_, AppState>,
    path: String,
    relative_dir: String,
    cursor: Option<String>,
) -> Result<FileDirectoryPage, BackendError> {
    state
        .files
        .list(Path::new(&path), &relative_dir, cursor.as_deref())
        .await
}
#[tauri::command]
pub async fn files_preview(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
) -> Result<RepositoryFilePreview, BackendError> {
    state.files.preview(Path::new(&path), &relative_path).await
}
#[tauri::command]
pub async fn files_prepare(
    state: State<'_, AppState>,
    path: String,
    intent: FileOperationIntent,
) -> Result<PreparedFileOperation, BackendError> {
    state.files.prepare(Path::new(&path), intent).await
}
#[tauri::command]
pub async fn files_execute(
    state: State<'_, AppState>,
    path: String,
    request: ExecuteFileOperationRequest,
) -> Result<FileMutationResult, BackendError> {
    state.files.execute(Path::new(&path), request).await
}
