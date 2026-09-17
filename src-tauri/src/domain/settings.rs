use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::ai::{AiApiFormat, AiProvider};
use crate::domain::error::{BackendError, ErrorCode};

const SETTINGS_SCHEMA_VERSION: u32 = 1;
const MAX_RECENT_REPOSITORIES: usize = 20;
const MAX_FONT_NAME_LENGTH: usize = 128;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    pub schema_version: u32,
    pub theme: ThemePreference,
    pub font_family: String,
    pub code_font_family: String,
    pub launch_at_login: bool,
    pub check_updates_on_startup: bool,
    pub last_repo_path: Option<String>,
    pub recent_repo_paths: Vec<String>,
    pub review_rule_files: Vec<String>,
    pub review_skill_directory: String,
    pub use_review_rule_files_in_review: bool,
    pub ai_drawer_open: bool,
    pub ai_drawer_width: u16,
    pub ai_provider: AiProvider,
    pub ai_api_format: AiApiFormat,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme: ThemePreference::System,
            font_family: String::new(),
            code_font_family: String::new(),
            launch_at_login: false,
            check_updates_on_startup: true,
            last_repo_path: None,
            recent_repo_paths: Vec::new(),
            review_rule_files: Vec::new(),
            review_skill_directory: String::new(),
            use_review_rule_files_in_review: false,
            ai_drawer_open: false,
            ai_drawer_width: 360,
            ai_provider: AiProvider::OpenAi,
            ai_api_format: AiApiFormat::default(),
            api_key: String::new(),
            base_url: "https://api.openai.com/v1/chat/completions".to_owned(),
            model: "gpt-4o-mini".to_owned(),
        }
    }
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.schema_version = SETTINGS_SCHEMA_VERSION;
        self.font_family = normalize_font_family(&self.font_family);
        self.code_font_family = normalize_font_family(&self.code_font_family);
        self.ai_drawer_width = self.ai_drawer_width.clamp(300, 560);
        deduplicate_recent_paths(&mut self.recent_repo_paths);
        self.recent_repo_paths.truncate(MAX_RECENT_REPOSITORIES);
    }

    pub fn record_recent_repository(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.recent_repo_paths
            .retain(|existing| !existing.eq_ignore_ascii_case(&path));
        self.recent_repo_paths.insert(0, path.clone());
        self.recent_repo_paths.truncate(MAX_RECENT_REPOSITORIES);
        self.last_repo_path = Some(path);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLoadResult {
    pub settings: AppSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migration_warning: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacySettings {
    theme: Option<ThemePreference>,
    launch_at_login: Option<bool>,
    last_repo_path: Option<String>,
    recent_repo_paths: Option<Vec<String>>,
    review_rule_files: Option<Vec<String>>,
    use_review_rule_files_in_review: Option<bool>,
    ai_provider: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

pub fn migrate_legacy_value(value: Value) -> Result<AppSettings, BackendError> {
    let legacy: LegacySettings = serde_json::from_value(value).map_err(|error| {
        BackendError::new(ErrorCode::SettingsMigration, "旧版设置格式无法识别。")
            .with_diagnostics(error.to_string())
    })?;
    let mut settings = AppSettings::default();

    if let Some(value) = legacy.theme {
        settings.theme = value;
    }
    if let Some(value) = legacy.launch_at_login {
        settings.launch_at_login = value;
    }
    settings.last_repo_path = legacy.last_repo_path;
    if let Some(value) = legacy.recent_repo_paths {
        settings.recent_repo_paths = value;
    }
    if let Some(value) = legacy.review_rule_files {
        settings.review_rule_files = value;
    }
    if let Some(value) = legacy.use_review_rule_files_in_review {
        settings.use_review_rule_files_in_review = value;
    }
    if let Some(value) = legacy.ai_provider {
        settings.ai_provider = AiProvider::from_stored(&value);
    }
    if let Some(value) = legacy.api_key {
        settings.api_key = value;
    }
    if let Some(value) = legacy.base_url {
        settings.base_url = value;
    }
    if let Some(value) = legacy.model {
        settings.model = value;
    }
    settings.normalize();
    Ok(settings)
}

impl std::fmt::Debug for AppSettings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppSettings")
            .field("schema_version", &self.schema_version)
            .field("theme", &self.theme)
            .field("launch_at_login", &self.launch_at_login)
            .field("last_repo_path", &self.last_repo_path)
            .field("recent_repo_paths", &self.recent_repo_paths)
            .field("review_rule_files", &self.review_rule_files)
            .field(
                "use_review_rule_files_in_review",
                &self.use_review_rule_files_in_review,
            )
            .field("ai_drawer_open", &self.ai_drawer_open)
            .field("ai_drawer_width", &self.ai_drawer_width)
            .field("ai_provider", &self.ai_provider)
            .field("ai_api_format", &self.ai_api_format)
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

fn normalize_font_family(name: &str) -> String {
    if name.chars().any(char::is_control) {
        return String::new();
    }
    name.trim().chars().take(MAX_FONT_NAME_LENGTH).collect()
}

fn deduplicate_recent_paths(paths: &mut Vec<String>) {
    let mut unique = Vec::with_capacity(paths.len());
    for path in paths.drain(..) {
        if !unique
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&path))
        {
            unique.push(path);
        }
    }
    *paths = unique;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::AiProvider;

    #[test]
    fn defaults_use_system_theme_and_closed_ai_drawer() {
        let settings = AppSettings::default();

        assert_eq!(settings.theme, ThemePreference::System);
        assert!(!settings.ai_drawer_open);
        assert_eq!(settings.ai_drawer_width, 360);
    }

    #[test]
    fn responses_format_round_trips_and_old_settings_default_to_chat() {
        let old: AppSettings =
            serde_json::from_value(serde_json::json!({ "schemaVersion": 1 })).unwrap();
        assert_eq!(
            serde_json::to_value(old).unwrap()["aiApiFormat"],
            "chatCompletions"
        );
        let selected: AppSettings =
            serde_json::from_value(serde_json::json!({ "aiApiFormat": "responses" })).unwrap();
        assert_eq!(
            serde_json::to_value(selected).unwrap()["aiApiFormat"],
            "responses"
        );
    }

    #[test]
    fn font_settings_defaults_and_serialization_are_backward_compatible() {
        let old: AppSettings =
            serde_json::from_value(serde_json::json!({"schemaVersion": 1, "theme": "dark"}))
                .unwrap();
        let value = serde_json::to_value(old).unwrap();
        assert_eq!(value["fontFamily"], "");
        assert_eq!(value["codeFontFamily"], "");
        let mut custom: AppSettings = serde_json::from_value(
            serde_json::json!({"fontFamily": " 微软雅黑 ", "codeFontFamily": "Cascadia Code"}),
        )
        .unwrap();
        custom.normalize();
        let value = serde_json::to_value(custom).unwrap();
        assert_eq!(value["fontFamily"], "微软雅黑");
        assert_eq!(value["codeFontFamily"], "Cascadia Code");
    }

    #[test]
    fn migration_imports_supported_fields_and_drops_obsolete_fields() {
        let legacy = serde_json::json!({
            "aiProvider": "qwen",
            "apiKey": "plain-secret",
            "baseUrl": "https://example.test/v1/chat/completions",
            "model": "qwen-plus",
            "lastRepoPath": "D:\\work\\repo",
            "recentRepoPaths": ["D:\\work\\repo"],
            "launchAtLogin": true,
            "cacheDirectory": "D:\\old-cache",
            "commitOriginClassificationPrompt": "obsolete"
        });

        let migrated = migrate_legacy_value(legacy).unwrap();

        assert_eq!(migrated.ai_provider, AiProvider::Qwen);
        assert_eq!(migrated.api_key, "plain-secret");
        assert_eq!(migrated.last_repo_path.as_deref(), Some("D:\\work\\repo"));
        let encoded = serde_json::to_value(migrated).unwrap();
        assert!(encoded.get("cacheDirectory").is_none());
        assert!(encoded.get("commitOriginClassificationPrompt").is_none());
    }

    #[test]
    fn legacy_openai_provider_maps_to_typed_open_ai() {
        let migrated = migrate_legacy_value(serde_json::json!({
            "aiProvider": "openai"
        }))
        .unwrap();

        assert_eq!(migrated.ai_provider, AiProvider::OpenAi);
    }

    #[test]
    fn settings_debug_never_exposes_api_key() {
        let settings = AppSettings {
            api_key: "secret-key".to_owned(),
            ..AppSettings::default()
        };

        let debug = format!("{settings:?}");

        assert!(!debug.contains("secret-key"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn recent_repositories_are_deduplicated_and_capped_at_twenty() {
        let mut settings = AppSettings::default();
        for index in 0..25 {
            settings.record_recent_repository(format!("D:\\repo-{index}"));
        }
        settings.record_recent_repository("D:\\repo-10");

        assert_eq!(settings.recent_repo_paths.len(), 20);
        assert_eq!(settings.recent_repo_paths[0], "D:\\repo-10");
        assert_eq!(
            settings
                .recent_repo_paths
                .iter()
                .filter(|path| path.as_str() == "D:\\repo-10")
                .count(),
            1
        );
    }
}
