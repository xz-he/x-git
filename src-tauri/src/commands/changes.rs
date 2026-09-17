use std::path::Path;

use tauri::State;

use crate::commands::AppState;
use crate::domain::changes::{ChangeScope, ChangesSnapshot, CommitResult, FileDiff};
use crate::domain::error::BackendError;
use crate::domain::operation::MutationWorkspace;

#[tauri::command]
pub async fn changes_line_stats(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<crate::domain::changes::ChangeLineStat>, BackendError> {
    state.changes.line_stats(Path::new(&path)).await
}

#[tauri::command]
pub async fn changes_scan_noise(
    state: State<'_, AppState>,
    path: String,
) -> Result<crate::domain::changes::NoiseScan, BackendError> {
    state.changes.scan_noise(Path::new(&path)).await
}

#[tauri::command]
pub async fn changes_restore_noise(
    state: State<'_, AppState>,
    path: String,
    selected: Vec<crate::domain::changes::NoiseCandidate>,
) -> Result<crate::domain::changes::NoiseRestoreResult, BackendError> {
    state
        .changes
        .restore_noise(Path::new(&path), selected)
        .await
}

#[tauri::command]
pub async fn changes_snapshot(
    state: State<'_, AppState>,
    path: String,
) -> Result<ChangesSnapshot, BackendError> {
    state.changes.snapshot(Path::new(&path)).await
}

#[tauri::command]
pub async fn changes_file_diff(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
    scope: ChangeScope,
) -> Result<FileDiff, BackendError> {
    state
        .changes
        .file_diff(Path::new(&path), &relative_path, scope)
        .await
}

#[tauri::command]
pub async fn changes_stage_files(
    state: State<'_, AppState>,
    path: String,
    relative_paths: Vec<String>,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .stage_files(Path::new(&path), &relative_paths)
        .await
}

#[tauri::command]
pub async fn changes_unstage_files(
    state: State<'_, AppState>,
    path: String,
    relative_paths: Vec<String>,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .unstage_files(Path::new(&path), &relative_paths)
        .await
}

#[tauri::command]
pub async fn changes_stage_file(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .stage_file(Path::new(&path), &relative_path)
        .await
}

#[tauri::command]
pub async fn changes_unstage_file(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .unstage_file(Path::new(&path), &relative_path)
        .await
}

#[tauri::command]
pub async fn changes_stage_hunk(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
    hunk_index: usize,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .stage_hunk(Path::new(&path), &relative_path, hunk_index)
        .await
}

#[tauri::command]
pub async fn changes_unstage_hunk(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
    hunk_index: usize,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .unstage_hunk(Path::new(&path), &relative_path, hunk_index)
        .await
}

#[tauri::command]
pub async fn changes_stage_lines(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
    start_line: u32,
    end_line: u32,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .stage_lines(Path::new(&path), &relative_path, start_line, end_line)
        .await
}

#[tauri::command]
pub async fn changes_unstage_lines(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
    start_line: u32,
    end_line: u32,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .unstage_lines(Path::new(&path), &relative_path, start_line, end_line)
        .await
}

#[tauri::command]
pub async fn changes_discard_file(
    state: State<'_, AppState>,
    path: String,
    relative_path: String,
) -> Result<MutationWorkspace, BackendError> {
    state
        .changes
        .discard_file(Path::new(&path), &relative_path)
        .await
}

#[tauri::command]
pub async fn changes_commit(
    state: State<'_, AppState>,
    path: String,
    message: String,
) -> Result<CommitResult, BackendError> {
    state.changes.commit(Path::new(&path), &message).await
}
