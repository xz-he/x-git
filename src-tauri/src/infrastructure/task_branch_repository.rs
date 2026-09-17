use crate::domain::{
    error::{BackendError, ErrorCode},
    task_branch::TaskBranchBinding,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

// All repositories share one versioned file. Serialize read/modify/replace across roots.
static STORE_LOCK: Mutex<()> = Mutex::new(());
#[derive(Debug, Clone)]
pub struct TaskBranchRepository {
    path: PathBuf,
}
#[derive(Serialize, Deserialize)]
struct Store {
    version: u32,
    bindings: Vec<TaskBranchBinding>,
}
impl TaskBranchRepository {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn snapshot(&self, root: &Path) -> Result<Vec<TaskBranchBinding>, BackendError> {
        let _guard = STORE_LOCK.lock().map_err(|_| invalid_store())?;
        let key = root.to_string_lossy();
        Ok(self
            .read()?
            .bindings
            .into_iter()
            .filter(|b| b.root_path == key)
            .collect())
    }
    pub fn save(&self, binding: &TaskBranchBinding) -> Result<(), BackendError> {
        let _guard = STORE_LOCK.lock().map_err(|_| invalid_store())?;
        let mut store = self.read()?;
        if let Some(existing) = store
            .bindings
            .iter_mut()
            .find(|b| b.id == binding.id && b.root_path == binding.root_path)
        {
            *existing = binding.clone();
        } else {
            store.bindings.push(binding.clone());
        }
        self.write(&store)
    }
    pub fn unlink(&self, root: &Path, id: &str) -> Result<Vec<TaskBranchBinding>, BackendError> {
        let _guard = STORE_LOCK.lock().map_err(|_| invalid_store())?;
        let key = root.to_string_lossy();
        let mut store = self.read()?;
        let previous_len = store.bindings.len();
        store
            .bindings
            .retain(|b| !(b.root_path == key && b.id == id));
        if store.bindings.len() != previous_len {
            self.write(&store)?;
        }
        Ok(store
            .bindings
            .into_iter()
            .filter(|b| b.root_path == key)
            .collect())
    }
    fn write(&self, store: &Store) -> Result<(), BackendError> {
        let parent = self.path.parent().ok_or_else(invalid_store)?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        let bytes = serde_json::to_vec_pretty(&store).map_err(|_| invalid_store())?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(&self.path)
            .map_err(|e| BackendError::from(e.error))?;
        Ok(())
    }
    fn read(&self) -> Result<Store, BackendError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Store {
                    version: 1,
                    bindings: vec![],
                });
            }
            Err(error) => return Err(error.into()),
        };
        let store: Store = serde_json::from_slice(&bytes).map_err(|_| invalid_store())?;
        if store.version != 1 {
            return Err(invalid_store());
        }
        Ok(store)
    }
}
fn invalid_store() -> BackendError {
    BackendError::new(ErrorCode::Io, "任务分支状态文件无效，未覆盖原文件。")
}
