use crate::{
    application::{terminal_completion, terminal_service::TerminalEventSink},
    commands::AppState,
    domain::{error::BackendError, terminal::*},
};
use std::path::Path;
use tauri::{AppHandle, Emitter, State};
impl TerminalEventSink for AppHandle {
    fn emit(&self, event: TerminalEvent) {
        let _ = Emitter::emit(self, "git://terminal-event", event);
    }
}
#[tauri::command]
pub async fn terminal_start(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    command: String,
    cols: u16,
    rows: u16,
) -> Result<TerminalAccepted, BackendError> {
    state
        .terminal
        .start(Path::new(&path), &run_id, &command, cols, rows, app)
        .await
}
#[tauri::command]
pub async fn terminal_write(
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    data: String,
) -> Result<(), BackendError> {
    state.terminal.write(Path::new(&path), &run_id, &data).await
}
#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), BackendError> {
    state
        .terminal
        .resize(Path::new(&path), &run_id, cols, rows)
        .await
}
#[tauri::command]
pub async fn terminal_terminate(
    state: State<'_, AppState>,
    path: String,
    run_id: String,
) -> Result<(), BackendError> {
    state.terminal.terminate(Path::new(&path), &run_id).await
}
#[tauri::command]
pub fn terminal_ack(
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    sequence: u64,
) -> Result<(), BackendError> {
    state.terminal.ack(Path::new(&path), &run_id, sequence)
}
#[tauri::command]
pub async fn terminal_complete(
    path: String,
    command: String,
    cursor: usize,
) -> Result<TerminalCompletion, BackendError> {
    terminal_completion::complete(Path::new(&path), &command, cursor).await
}
