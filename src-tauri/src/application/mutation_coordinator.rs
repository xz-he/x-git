use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use tokio::sync::{Mutex, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

#[derive(Debug, Clone, Default)]
pub struct RepositoryMutationCoordinator {
    locks: Arc<Mutex<HashMap<PathBuf, Weak<RwLock<()>>>>>,
}

impl RepositoryMutationCoordinator {
    async fn lock_for(&self, root: &Path) -> Arc<RwLock<()>> {
        let key = normalized_key(root);
        let mut locks = self.locks.lock().await;
        if let Some(existing) = locks.get(&key).and_then(Weak::upgrade) {
            return existing;
        }

        let created = Arc::new(RwLock::new(()));
        locks.insert(key, Arc::downgrade(&created));
        created
    }

    pub async fn read(&self, root: &Path) -> OwnedRwLockReadGuard<()> {
        self.lock_for(root).await.read_owned().await
    }

    pub async fn write(&self, root: &Path) -> OwnedRwLockWriteGuard<()> {
        self.lock_for(root).await.write_owned().await
    }
}

fn normalized_key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| {
        if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|current| current.join(root))
                .unwrap_or_else(|_| root.to_path_buf())
        }
    })
}
