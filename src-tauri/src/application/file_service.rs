use super::{
    file_fingerprint::FileContext,
    file_listing,
    file_mutation::{self, PreparedRecord},
    mutation_coordinator::RepositoryMutationCoordinator,
    repository_service::RepositoryService,
};
use crate::{
    domain::{error::BackendError, files::*},
    infrastructure::git_runner::GitCommandRunner,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct FileService {
    pub(super) runner: GitCommandRunner,
    pub(super) coordinator: RepositoryMutationCoordinator,
    pub(super) prepared: Arc<Mutex<HashMap<String, PreparedRecord>>>,
}
impl Default for FileService {
    fn default() -> Self {
        Self::new(
            GitCommandRunner::default(),
            RepositoryMutationCoordinator::default(),
        )
    }
}
impl FileService {
    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
            prepared: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub(super) async fn root(&self, path: &Path) -> Result<PathBuf, BackendError> {
        RepositoryService::with_coordinator(self.runner.clone(), self.coordinator.clone())
            .resolve_root(path)
            .await
    }
    pub async fn list(
        &self,
        path: &Path,
        relative_dir: &str,
        cursor: Option<&str>,
    ) -> Result<FileDirectoryPage, BackendError> {
        let root = self.root(path).await?;
        let _lease = self.coordinator.read(&root).await;
        let context = FileContext::read(&root, &self.runner).await?;
        file_listing::list(&context, &self.runner, relative_dir, cursor).await
    }
    pub async fn preview(
        &self,
        path: &Path,
        relative_path: &str,
    ) -> Result<RepositoryFilePreview, BackendError> {
        let root = self.root(path).await?;
        let _lease = self.coordinator.read(&root).await;
        let context = FileContext::read(&root, &self.runner).await?;
        file_listing::preview(&context, relative_path)
    }
    pub async fn prepare(
        &self,
        path: &Path,
        intent: FileOperationIntent,
    ) -> Result<PreparedFileOperation, BackendError> {
        file_mutation::prepare(self, path, intent).await
    }
    pub async fn execute(
        &self,
        path: &Path,
        request: ExecuteFileOperationRequest,
    ) -> Result<FileMutationResult, BackendError> {
        file_mutation::execute(self, path, request).await
    }
}
