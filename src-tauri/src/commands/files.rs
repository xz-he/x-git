use crate::{
    commands::AppState,
    domain::{error::BackendError, files::*},
};
use std::path::Path;
use tauri::State;

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
