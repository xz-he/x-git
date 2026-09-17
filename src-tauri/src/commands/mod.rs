pub mod activity;
pub mod ai;
pub mod changes;
pub mod conflicts;
pub mod console;
pub mod terminal;
pub mod files;
pub mod history;
pub mod refs;
pub mod remotes;
pub mod repository;
pub mod settings;
pub mod stash;
pub mod task_branches;

use crate::application::ai_context::AiContextBuilder;
use crate::application::ai_service::AiService;
use crate::application::changes_service::ChangesService;
use crate::application::conflict_service::ConflictService;
use crate::application::console_service::ConsoleService;
use crate::application::terminal_service::TerminalService;
use crate::application::file_service::FileService;
use crate::application::history_service::HistoryService;
use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::application::refs_service::RefsService;
use crate::application::remote_service::RemoteService;
use crate::application::repository_service::RepositoryService;
use crate::application::settings_service::SettingsService;
use crate::application::stash_service::StashService;
use crate::application::task_branch_service::TaskBranchService;
use crate::infrastructure::ai_client::AiHttpClient;
use crate::infrastructure::git_runner::GitCommandRunner;
use crate::infrastructure::settings_repository::{SettingsPaths, SettingsRepository};
use crate::infrastructure::task_branch_repository::TaskBranchRepository;

pub struct AppState {
    pub activity: crate::application::activity_service::ActivityService,
    pub repository_watch: crate::application::repository_watch::RepositoryWatchService,
    pub ai: AiService,
    pub changes: ChangesService,
    pub conflicts: ConflictService,
    pub console: ConsoleService,
    pub terminal: TerminalService,
    pub files: FileService,
    pub history: HistoryService,
    pub refs: RefsService,
    pub remotes: RemoteService,
    pub repositories: RepositoryService,
    pub settings: SettingsService,
    pub stashes: StashService,
    pub task_branches: TaskBranchService,
}

pub fn build_app_state(settings_paths: SettingsPaths) -> AppState {
    let coordinator = RepositoryMutationCoordinator::default();
    let runner = GitCommandRunner::default();
    let changes = ChangesService::new(runner.clone(), coordinator.clone());
    let conflicts = ConflictService::new(runner.clone(), coordinator.clone());
    let history = HistoryService::new(runner.clone(), coordinator.clone());
    let refs = RefsService::new(runner.clone(), coordinator.clone());
    let remotes = RemoteService::new(runner.clone(), coordinator.clone());
    let stashes = StashService::new(runner.clone(), coordinator.clone());
    AppState {
        activity: crate::application::activity_service::ActivityService::new(runner.clone(), coordinator.clone()),
        repository_watch: Default::default(),
        task_branches: TaskBranchService::new(
            runner.clone(),
            coordinator.clone(),
            TaskBranchRepository::new(settings_paths.current.with_file_name("task-branches.json")),
        ),
        ai: AiService::new(
            AiContextBuilder::new(changes.clone()),
            AiHttpClient::default(),
        ),
        changes,
        conflicts,
        console: ConsoleService::new(coordinator.clone()),
        terminal: TerminalService::new(coordinator.clone()),
        files: FileService::new(runner.clone(), coordinator.clone()),
        history,
        refs,
        remotes,
        stashes,
        repositories: RepositoryService::with_coordinator(runner, coordinator),
        settings: SettingsService::new(SettingsRepository::new(settings_paths)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::AiRunAccepted;
    use crate::domain::error::{BackendError, ErrorCode};
    use crate::domain::remotes::GitRunAccepted;
    use crate::domain::repository::RepositorySnapshot;
    use crate::infrastructure::settings_repository::SettingsPaths;

    fn assert_command_result<T>(_: Result<T, BackendError>) {}

    #[test]
    fn app_state_construction_does_not_touch_injected_paths() {
        let directory = tempfile::tempdir().unwrap();
        let paths = SettingsPaths {
            current: directory.path().join("settings.json"),
            legacy_candidates: vec![directory.path().join("legacy.json")],
            migration_marker: directory.path().join("migration.complete"),
        };

        let _state = build_app_state(paths);

        assert!(directory.path().read_dir().unwrap().next().is_none());
    }

    #[test]
    fn command_contracts_use_structured_errors() {
        assert_command_result::<RepositorySnapshot>(Err(BackendError::new(
            ErrorCode::InvalidRepository,
            "测试",
        )));
        assert_command_result::<AiRunAccepted>(Err(BackendError::new(
            ErrorCode::AiConfiguration,
            "测试",
        )));
        assert_command_result::<GitRunAccepted>(Err(BackendError::new(
            ErrorCode::GitOperationInProgress,
            "测试",
        )));
    }
}
