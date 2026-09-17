use std::path::Path;

use tauri::State;

use crate::application::repository_watch::RepositoryWatchSnapshot;
use crate::commands::AppState;
use crate::domain::error::BackendError;
use crate::domain::repository::RepositorySnapshot;

#[tauri::command]
pub async fn repository_watch_snapshot(
    state: State<'_, AppState>,
    path: String,
) -> Result<RepositoryWatchSnapshot, BackendError> {
    state.repository_watch.snapshot(Path::new(&path)).await
}

#[tauri::command]
pub async fn repository_watch_stop(state: State<'_, AppState>) -> Result<(), BackendError> {
    state.repository_watch.stop().await;
    Ok(())
}

#[tauri::command]
pub async fn repository_open(
    state: State<'_, AppState>,
    path: String,
) -> Result<RepositorySnapshot, BackendError> {
    state.repositories.open_repository(Path::new(&path)).await
}

#[tauri::command]
pub async fn repository_init(
    state: State<'_, AppState>,
    path: String,
) -> Result<RepositorySnapshot, BackendError> {
    state.repositories.init_repository(Path::new(&path)).await
}

#[tauri::command]
pub async fn repository_clone(
    state: State<'_, AppState>,
    url: String,
    path: String,
) -> Result<RepositorySnapshot, BackendError> {
    state
        .repositories
        .clone_repository(&url, Path::new(&path))
        .await
}

#[tauri::command]
pub async fn repository_refresh(
    state: State<'_, AppState>,
    path: String,
) -> Result<RepositorySnapshot, BackendError> {
    state.repositories.open_repository(Path::new(&path)).await
}
