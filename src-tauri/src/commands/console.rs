use crate::{
    application::console_service::ConsoleEventSink,
    commands::AppState,
    domain::{
        console::{ConsoleAccepted, ConsoleEvent},
        error::BackendError,
    },
};
use std::path::Path;
use tauri::{AppHandle, Emitter, State};

impl ConsoleEventSink for AppHandle {
    fn emit(&self, event: ConsoleEvent) {
        let _ = Emitter::emit(self, "git://console-event", event);
    }
}
#[tauri::command]
pub async fn console_start(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    command: String,
) -> Result<ConsoleAccepted, BackendError> {
    state
        .console
        .start(Path::new(&path), &run_id, &command, app)
        .await
}
#[tauri::command]
pub async fn console_cancel(
    state: State<'_, AppState>,
    path: String,
    run_id: String,
) -> Result<(), BackendError> {
    state.console.cancel(Path::new(&path), &run_id).await
}
