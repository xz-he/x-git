use tauri::State;

use crate::commands::AppState;
use crate::domain::error::BackendError;
use crate::domain::settings::{AppSettings, SettingsLoadResult};

#[tauri::command]
pub async fn settings_load(state: State<'_, AppState>) -> Result<SettingsLoadResult, BackendError> {
    state.settings.load()
}

#[tauri::command]
pub async fn settings_save(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, BackendError> {
    state.settings.save(&settings)
}
