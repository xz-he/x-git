use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::settings::{AppSettings, SettingsLoadResult, migrate_legacy_value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPaths {
    pub current: PathBuf,
    pub legacy_candidates: Vec<PathBuf>,
    pub migration_marker: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SettingsRepository {
    paths: SettingsPaths,
}

impl SettingsRepository {
    pub fn new(paths: SettingsPaths) -> Self {
        Self { paths }
    }

    pub fn load(&self) -> Result<SettingsLoadResult, BackendError> {
        if self.paths.current.exists() {
            let mut settings: AppSettings = read_json(&self.paths.current).map_err(|error| {
                BackendError::new(ErrorCode::SettingsMigration, "当前设置文件无法读取。")
                    .with_diagnostics(error.to_string())
            })?;
            settings.normalize();
            return Ok(SettingsLoadResult {
                settings,
                migration_warning: None,
            });
        }

        if self.paths.migration_marker.exists() {
            return Ok(default_load_result());
        }

        for candidate in &self.paths.legacy_candidates {
            if !candidate.exists() {
                continue;
            }

            let migrated = fs::read(candidate)
                .map_err(|error| error.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
                .and_then(|value| migrate_legacy_value(value).map_err(|error| error.to_string()));
            let settings = match migrated {
                Ok(settings) => settings,
                Err(diagnostics) => {
                    return Ok(SettingsLoadResult {
                        settings: AppSettings::default(),
                        migration_warning: Some(format!(
                            "旧版设置迁移失败，将在下次启动时重试：{diagnostics}"
                        )),
                    });
                }
            };

            self.save(&settings)?;
            self.write_migration_marker()?;
            return Ok(SettingsLoadResult {
                settings,
                migration_warning: None,
            });
        }

        self.write_migration_marker()?;
        Ok(default_load_result())
    }

    pub fn save(&self, settings: &AppSettings) -> Result<AppSettings, BackendError> {
        let mut settings = settings.clone();
        settings.normalize();
        let parent = self
            .paths
            .current
            .parent()
            .ok_or_else(|| BackendError::new(ErrorCode::InvalidPath, "设置文件路径无效。"))?;
        fs::create_dir_all(parent)?;

        let temporary = self.paths.current.with_extension("json.tmp");
        let encoded = serde_json::to_vec_pretty(&settings).map_err(|error| {
            BackendError::new(ErrorCode::Unexpected, "设置序列化失败。")
                .with_diagnostics(error.to_string())
        })?;
        let mut file = File::create(&temporary)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, &self.paths.current)?;
        Ok(settings)
    }

    fn write_migration_marker(&self) -> Result<(), BackendError> {
        let parent = self
            .paths
            .migration_marker
            .parent()
            .ok_or_else(|| BackendError::new(ErrorCode::InvalidPath, "迁移标记路径无效。"))?;
        fs::create_dir_all(parent)?;
        let mut marker = File::create(&self.paths.migration_marker)?;
        marker.write_all(b"complete\n")?;
        marker.sync_all()?;
        Ok(())
    }
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &PathBuf,
) -> Result<T, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn default_load_result() -> SettingsLoadResult {
    SettingsLoadResult {
        settings: AppSettings::default(),
        migration_warning: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::{AppSettings, ThemePreference};

    fn fixture_paths(directory: &tempfile::TempDir) -> SettingsPaths {
        SettingsPaths {
            current: directory.path().join("current").join("settings.json"),
            legacy_candidates: vec![directory.path().join("legacy").join("hq-git-settings.json")],
            migration_marker: directory
                .path()
                .join("current")
                .join("settings-migration-v1.complete"),
        }
    }

    #[test]
    fn current_settings_win_over_legacy_settings() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture_paths(&directory);
        let repository = SettingsRepository::new(paths.clone());
        let current = AppSettings {
            theme: ThemePreference::Dark,
            ..AppSettings::default()
        };
        repository.save(&current).unwrap();
        std::fs::create_dir_all(paths.legacy_candidates[0].parent().unwrap()).unwrap();
        std::fs::write(
            &paths.legacy_candidates[0],
            r#"{"theme":"light","apiKey":"legacy"}"#,
        )
        .unwrap();

        let loaded = repository.load().unwrap();

        assert_eq!(loaded.settings.theme, ThemePreference::Dark);
        assert!(loaded.migration_warning.is_none());
    }

    #[test]
    fn migration_never_modifies_the_legacy_file() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture_paths(&directory);
        std::fs::create_dir_all(paths.legacy_candidates[0].parent().unwrap()).unwrap();
        let legacy = br#"{"aiProvider":"qwen","apiKey":"plain-secret"}"#;
        std::fs::write(&paths.legacy_candidates[0], legacy).unwrap();
        let repository = SettingsRepository::new(paths.clone());

        let loaded = repository.load().unwrap();

        assert_eq!(loaded.settings.api_key, "plain-secret");
        assert_eq!(std::fs::read(&paths.legacy_candidates[0]).unwrap(), legacy);
        assert!(paths.current.exists());
        assert!(paths.migration_marker.exists());
    }

    #[test]
    fn corrupt_legacy_file_returns_defaults_and_retryable_warning() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture_paths(&directory);
        std::fs::create_dir_all(paths.legacy_candidates[0].parent().unwrap()).unwrap();
        std::fs::write(&paths.legacy_candidates[0], "{not-json").unwrap();
        let repository = SettingsRepository::new(paths.clone());

        let loaded = repository.load().unwrap();

        assert_eq!(loaded.settings, AppSettings::default());
        assert!(loaded.migration_warning.is_some());
        assert!(!paths.current.exists());
        assert!(!paths.migration_marker.exists());
    }

    #[test]
    fn save_uses_a_temporary_peer_and_leaves_valid_json() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture_paths(&directory);
        let repository = SettingsRepository::new(paths.clone());
        let settings = AppSettings {
            api_key: "plain-secret".to_owned(),
            ai_api_format: crate::domain::ai::AiApiFormat::Responses,
            ..AppSettings::default()
        };

        repository.save(&settings).unwrap();

        assert!(!paths.current.with_extension("json.tmp").exists());
        let persisted: AppSettings =
            serde_json::from_slice(&std::fs::read(&paths.current).unwrap()).unwrap();
        assert_eq!(persisted, settings);
    }

    #[test]
    fn save_replaces_an_existing_settings_file() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture_paths(&directory);
        let repository = SettingsRepository::new(paths.clone());
        repository.save(&AppSettings::default()).unwrap();
        let updated = AppSettings {
            theme: ThemePreference::Light,
            ..AppSettings::default()
        };

        repository.save(&updated).unwrap();

        let persisted: AppSettings =
            serde_json::from_slice(&std::fs::read(&paths.current).unwrap()).unwrap();
        assert_eq!(persisted, updated);
    }

    #[test]
    fn font_settings_survive_save_reload_and_default_restore() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture_paths(&directory);
        let repository = SettingsRepository::new(paths.clone());
        let custom = AppSettings {
            font_family: " 微软雅黑 ".into(),
            code_font_family: "Cascadia Code".into(),
            ..AppSettings::default()
        };
        repository.save(&custom).unwrap();
        let loaded = SettingsRepository::new(paths).load().unwrap().settings;
        assert_eq!(loaded.font_family, "微软雅黑");
        assert_eq!(loaded.code_font_family, "Cascadia Code");
        repository.save(&AppSettings::default()).unwrap();
        assert_eq!(repository.load().unwrap().settings.font_family, "");
    }

    #[test]
    fn completed_no_file_scan_creates_the_migration_marker() {
        let directory = tempfile::tempdir().unwrap();
        let paths = fixture_paths(&directory);
        let repository = SettingsRepository::new(paths.clone());

        let loaded = repository.load().unwrap();

        assert_eq!(loaded.settings, AppSettings::default());
        assert!(paths.migration_marker.exists());
    }
}
