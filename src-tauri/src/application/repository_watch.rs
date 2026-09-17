use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::application::repository_service::RepositoryService;
use crate::domain::error::{BackendError, ErrorCode};
use crate::infrastructure::git_runner::GitCommandRunner;

#[path = "repository_watch_fingerprint.rs"]
mod fingerprint;

const RECHECK_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryWatchSnapshot {
    pub watch_id: String,
    pub worktree_version: u64,
    pub metadata_version: u64,
}

#[derive(Default)]
struct Changes {
    worktree: AtomicU64,
    metadata: AtomicU64,
    failed: AtomicBool,
}
struct ActiveWatch {
    requested_path: PathBuf,
    root: PathBuf,
    git_roots: Vec<PathBuf>,
    id: String,
    changes: Arc<Changes>,
    watcher: Option<RecommendedWatcher>,
    fingerprint: fingerprint::Fingerprint,
    observed: (u64, u64),
    worktree_version: u64,
    metadata_version: u64,
    last_check: Instant,
    last_retry: Instant,
}
impl ActiveWatch {
    fn snapshot(&self) -> RepositoryWatchSnapshot {
        RepositoryWatchSnapshot {
            watch_id: self.id.clone(),
            worktree_version: self.worktree_version,
            metadata_version: self.metadata_version,
        }
    }

    async fn check(&mut self) -> Result<(), BackendError> {
        let unavailable = self.watcher.is_none() || self.changes.failed.load(Ordering::Relaxed);
        if unavailable && self.last_retry.elapsed() >= RETRY_INTERVAL {
            self.watcher = None;
            self.changes.failed.store(false, Ordering::Relaxed);
            self.watcher = register(&self.root, &self.git_roots, self.changes.clone()).ok();
            self.last_retry = Instant::now();
        }
        // Capture counters BEFORE reading: events during capture must be checked
        // again, rather than accidentally consumed by the preceding snapshot.
        let observed = (
            self.changes.worktree.load(Ordering::Relaxed),
            self.changes.metadata.load(Ordering::Relaxed),
        );
        let interval = if unavailable {
            RETRY_INTERVAL
        } else {
            RECHECK_INTERVAL
        };
        if observed == self.observed && self.last_check.elapsed() < interval {
            return Ok(());
        }
        let next = fingerprint::capture(&self.root, &self.git_roots[0]).await?;
        if next.worktree != self.fingerprint.worktree {
            self.worktree_version += 1;
        }
        if next.metadata != self.fingerprint.metadata {
            self.metadata_version += 1;
        }
        self.fingerprint = next;
        self.observed = observed;
        self.last_check = Instant::now();
        Ok(())
    }
}

/// One native watcher for the active repository. Unchanged polls only read
/// counters. Events and periodic recovery checks
/// compare actual Git/content fingerprints before advancing UI versions.
#[derive(Clone, Default)]
pub struct RepositoryWatchService {
    active: Arc<Mutex<Option<ActiveWatch>>>,
}
impl RepositoryWatchService {
    pub async fn snapshot(&self, path: &Path) -> Result<RepositoryWatchSnapshot, BackendError> {
        let mut active = self.active.lock().await;
        if let Some(watch) = active.as_mut()
            && watch.requested_path == path
        {
            watch.check().await?;
            return Ok(watch.snapshot());
        }
        // Drop handles from a previously selected repository before registering.
        *active = None;
        let root = RepositoryService::default().resolve_root(path).await?;
        let runner = GitCommandRunner::default();
        let git_dir = runner
            .run(Some(&root), ["rev-parse", "--absolute-git-dir"])
            .await?;
        let common_dir = runner
            .run(Some(&root), ["rev-parse", "--git-common-dir"])
            .await?;
        let git_dir = root.join(git_dir.stdout.trim()).canonicalize()?;
        let common_dir = root.join(common_dir.stdout.trim()).canonicalize()?;
        let root = root.canonicalize()?;
        let changes = Arc::new(Changes::default());
        let git_roots = vec![git_dir.clone(), common_dir.clone()];
        let watcher = register(&root, &git_roots, changes.clone()).ok();
        let observed = (
            changes.worktree.load(Ordering::Relaxed),
            changes.metadata.load(Ordering::Relaxed),
        );
        let fingerprint = fingerprint::capture(&root, &git_dir).await?;
        let watch = ActiveWatch {
            requested_path: path.to_path_buf(),
            root,
            git_roots,
            id: uuid::Uuid::new_v4().to_string(),
            changes,
            watcher,
            fingerprint,
            observed,
            worktree_version: 0,
            metadata_version: 0,
            last_check: Instant::now(),
            last_retry: Instant::now(),
        };
        let snapshot = watch.snapshot();
        *active = Some(watch);
        Ok(snapshot)
    }

    pub async fn stop(&self) {
        *self.active.lock().await = None;
    }
}

fn register(
    root: &Path,
    git_roots: &[PathBuf],
    captured: Arc<Changes>,
) -> Result<RecommendedWatcher, BackendError> {
    let event_roots = git_roots.to_vec();
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<Event>| match event {
            Ok(event) => {
                let (worktree, metadata) = classify(&event, &event_roots);
                if worktree {
                    captured.worktree.fetch_add(1, Ordering::Relaxed);
                }
                if metadata {
                    captured.metadata.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(_) => {
                captured.failed.store(true, Ordering::Relaxed);
            }
        })
        .map_err(watch_error)?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(watch_error)?;
    // Linked worktrees and separate Git directories keep metadata outside
    // the worktree. Watch the common directory (refs) and private HEAD/index.
    let mut watched = vec![root.to_path_buf()];
    for directory in git_roots.iter().rev() {
        if !watched.iter().any(|parent| directory.starts_with(parent)) {
            watcher
                .watch(&directory, RecursiveMode::Recursive)
                .map_err(watch_error)?;
            watched.push(directory.clone());
        }
    }
    Ok(watcher)
}

fn classify(event: &Event, git_roots: &[PathBuf]) -> (bool, bool) {
    if matches!(event.kind, EventKind::Access(_)) {
        return (false, false);
    }
    if event.paths.is_empty() {
        return (true, true);
    }
    let mut worktree = false;
    let mut metadata = false;
    for path in &event.paths {
        if let Some(relative) = git_roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())
        {
            let first = relative
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy());
            if matches!(first.as_deref(), Some("objects" | "logs" | "hq-git-operations"))
                || path.extension().is_some_and(|ext| ext == "lock")
            {
                continue;
            }
            // Index changes affect the staged list; all other Git state may
            // alter HEAD, refs, operation state, remotes or ignore configuration.
            worktree = true;
            if relative != Path::new("index") {
                metadata = true;
            }
        } else {
            worktree = true;
        }
    }
    (worktree, metadata)
}
fn watch_error(error: notify::Error) -> BackendError {
    BackendError::new(
        ErrorCode::Io,
        "仓库文件监听暂不可用，将自动重试并检查 Git 是否变化。",
    )
    .with_diagnostics(error.to_string())
}
