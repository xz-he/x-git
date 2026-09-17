use crate::domain::error::BackendError;
use crate::domain::settings::{AppSettings, SettingsLoadResult};
use crate::infrastructure::settings_repository::SettingsRepository;

#[derive(Debug, Clone)]
pub struct SettingsService {
    repository: SettingsRepository,
}

impl SettingsService {
    pub fn new(repository: SettingsRepository) -> Self {
        Self { repository }
    }

    pub fn load(&self) -> Result<SettingsLoadResult, BackendError> {
        self.repository.load()
    }

    pub fn save(&self, settings: &AppSettings) -> Result<AppSettings, BackendError> {
        self.repository.save(settings)
    }
}
