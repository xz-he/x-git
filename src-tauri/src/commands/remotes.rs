use std::path::Path;

use tauri::{AppHandle, Emitter, State};

use crate::application::remote_service::GitRunEventSink;
use crate::commands::AppState;
use crate::domain::error::BackendError;
use crate::domain::remotes::{
    FetchRequest, GitRunAccepted, GitRunEvent, PullRequest, PushRequest, RemoteSnapshot,
};

impl GitRunEventSink for AppHandle {
    fn emit(&self, event: GitRunEvent) {
        let _ = Emitter::emit(self, "git://run-event", event);
    }
}

#[tauri::command]
pub async fn remotes_snapshot(
    state: State<'_, AppState>,
    path: String,
) -> Result<RemoteSnapshot, BackendError> {
    state.remotes.snapshot(Path::new(&path)).await
}

#[tauri::command]
pub async fn remote_start_fetch(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    request: FetchRequest,
) -> Result<GitRunAccepted, BackendError> {
    state
        .remotes
        .start_fetch(Path::new(&path), &run_id, request, app)
        .await
}

#[tauri::command]
pub async fn remote_start_pull(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    request: PullRequest,
) -> Result<GitRunAccepted, BackendError> {
    state
        .remotes
        .start_pull(Path::new(&path), &run_id, request, app)
        .await
}

#[tauri::command]
pub async fn remote_start_push(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    request: PushRequest,
) -> Result<GitRunAccepted, BackendError> {
    state
        .remotes
        .start_push(Path::new(&path), &run_id, request, app)
        .await
}

#[tauri::command]
pub async fn git_run_cancel(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<(), BackendError> {
    state.remotes.cancel(&run_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_commands_are_exposed() {
        let _ = remotes_snapshot;
        let _ = remote_start_fetch;
        let _ = remote_start_pull;
        let _ = remote_start_push;
        let _ = git_run_cancel;
    }
}
