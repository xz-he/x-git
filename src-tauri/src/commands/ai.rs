use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::application::ai_service::AiEventSink;
use crate::commands::AppState;
use crate::domain::ai::{AiConnectionConfig, AiConnectionTestResult, AiRunAccepted, AiRunEvent};
use crate::domain::error::{BackendError, ErrorCode};

#[derive(Clone)]
struct TauriAiEventSink(AppHandle);

impl AiEventSink for TauriAiEventSink {
    fn emit(&self, event: &AiRunEvent) -> Result<(), BackendError> {
        self.0
            .emit("ai://run-event", event)
            .map_err(|_| BackendError::new(ErrorCode::Unexpected, "无法发送 AI 任务事件。"))
    }
}

#[tauri::command]
pub async fn ai_start_review(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    source: Option<crate::domain::ai::ReviewSource>,
) -> Result<AiRunAccepted, BackendError> {
    let settings = state.settings.load()?.settings;
    state
        .ai
        .start_review_source(
            &run_id,
            Path::new(&path),
            &settings,
            source.unwrap_or_default(),
            Arc::new(TauriAiEventSink(app)),
        )
        .await
}

#[tauri::command]
pub async fn ai_review_skill_status(
    state: State<'_, AppState>,
    path: String,
) -> Result<crate::domain::ai::ReviewSkillStatus, BackendError> {
    let settings = state.settings.load()?.settings;
    tokio::task::spawn_blocking(move || {
        crate::application::review_skill::ReviewSkillLoader::status(
            Path::new(&path),
            &settings.review_skill_directory,
        )
    })
    .await
    .map_err(|_| BackendError::new(ErrorCode::Unexpected, "无法读取审查技能状态。"))
}

#[tauri::command]
pub async fn ai_start_commit_message(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
) -> Result<AiRunAccepted, BackendError> {
    let settings = state.settings.load()?.settings;
    state
        .ai
        .start_commit_message(
            &run_id,
            Path::new(&path),
            &settings,
            Arc::new(TauriAiEventSink(app)),
        )
        .await
}

#[tauri::command]
pub async fn ai_start_conflict_suggestion(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    run_id: String,
    relative_path: String,
    token: String,
) -> Result<AiRunAccepted, BackendError> {
    let settings = state.settings.load()?.settings;
    state
        .ai
        .start_conflict_suggestion(
            &run_id,
            Path::new(&path),
            &settings,
            &state.conflicts,
            &relative_path,
            &token,
            Arc::new(TauriAiEventSink(app)),
        )
        .await
}

#[tauri::command]
pub async fn ai_chat(
    state: State<'_, AppState>,
    run_id: String,
    messages: Vec<crate::application::ai_chat::ChatMessage>,
) -> Result<String, BackendError> {
    let settings = state.settings.load()?.settings;
    state.ai.chat(&run_id, &messages, &settings).await
}

#[tauri::command]
pub async fn ai_cancel(state: State<'_, AppState>, run_id: String) -> Result<(), BackendError> {
    state.ai.cancel(&run_id).await
}

#[tauri::command]
pub async fn ai_test_connection(
    state: State<'_, AppState>,
    config: AiConnectionConfig,
) -> Result<AiConnectionTestResult, BackendError> {
    state.ai.test_connection(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_commands_are_exposed() {
        let _ = ai_start_review;
        let _ = ai_start_commit_message;
        let _ = ai_start_conflict_suggestion;
        let _ = ai_cancel;
        let _ = ai_test_connection;
    }
}
