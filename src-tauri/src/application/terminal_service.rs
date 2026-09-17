use crate::application::{mutation_coordinator::RepositoryMutationCoordinator, terminal_command};
use crate::domain::{
    error::{BackendError, ErrorCode},
    terminal::*,
};
use crate::infrastructure::terminal_pty::{self, Controls, PtyResult};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;
pub trait TerminalEventSink: Clone + Send + Sync + 'static {
    fn emit(&self, event: TerminalEvent);
}
const OUTPUT_CREDITS: usize = 32;
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(12);
#[derive(Clone)]
pub struct TerminalService {
    coordinator: RepositoryMutationCoordinator,
    registry: Arc<Mutex<Registry>>,
}
#[derive(Default)]
struct Registry {
    active: Option<Arc<Run>>,
    known: HashMap<String, PathBuf>,
    shutting_down: bool,
}
struct Run {
    id: String,
    requested: PathBuf,
    root: PathBuf,
    cancellation: CancellationToken,
    done: CancellationToken,
    controls: Arc<Mutex<Option<Arc<Controls>>>>,
    credits: Mutex<Credits>,
    credit_changed: Notify,
}
#[derive(Default)]
struct Credits {
    outstanding: BTreeSet<u64>,
    last_output: u64,
}
impl Default for TerminalService {
    fn default() -> Self {
        Self::new(RepositoryMutationCoordinator::default())
    }
}
impl TerminalService {
    pub fn new(coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            coordinator,
            registry: Arc::new(Mutex::new(Registry::default())),
        }
    }
    pub async fn start<S: TerminalEventSink>(
        &self,
        path: &Path,
        id: &str,
        command: &str,
        cols: u16,
        rows: u16,
        sink: S,
    ) -> Result<TerminalAccepted, BackendError> {
        validate_id(id)?;
        let args = terminal_command::parse(command)?;
        let dimensions = terminal_pty::size(cols, rows)?;
        if !path.is_absolute() {
            return Err(BackendError::new(
                ErrorCode::InvalidRepository,
                "请提供当前仓库根的绝对路径。",
            ));
        }
        // No await until the ID has been reserved. Canonicalization is a single
        // local filesystem lookup and makes all subsequent control ownership exact.
        let root = path.canonicalize().map_err(|error| {
            BackendError::new(ErrorCode::InvalidRepository, "无法读取仓库根目录。")
                .with_diagnostics(error.to_string())
        })?;
        let run = Arc::new(Run {
            id: id.to_owned(),
            requested: path.to_owned(),
            root: root.clone(),
            cancellation: CancellationToken::new(),
            done: CancellationToken::new(),
            controls: Arc::new(Mutex::new(None)),
            credits: Mutex::new(Credits::default()),
            credit_changed: Notify::new(),
        });
        {
            let mut registry = self.registry.lock().unwrap();
            if registry.shutting_down {
                return Err(BackendError::new(ErrorCode::Cancelled, "应用正在退出。"));
            }
            if registry.known.contains_key(id) {
                return Err(terminal_command::invalid(
                    "runId 已使用或已取消，请使用新的 runId。",
                ));
            }
            if registry.active.is_some() {
                return Err(BackendError::new(
                    ErrorCode::GitOperationInProgress,
                    "已有 Git 终端正在运行或清理进程。",
                ));
            }
            registry.known.insert(id.to_owned(), root.clone());
            registry.active = Some(run.clone());
        }
        let service = self.clone();
        tokio::spawn(async move {
            service.execute(run, args, dimensions, sink).await;
        });
        Ok(TerminalAccepted {
            run_id: id.to_owned(),
            root_path: root,
        })
    }
    fn active(&self, root: &Path, id: &str) -> Result<Arc<Run>, BackendError> {
        let registry = self.registry.lock().unwrap();
        let run = registry
            .active
            .as_ref()
            .filter(|run| run.id == id)
            .ok_or_else(|| terminal_command::invalid("Git 终端已结束或 runId 已过期。"))?;
        if !same_root(root, &run.root) && !same_root(root, &run.requested) {
            return Err(BackendError::new(
                ErrorCode::InvalidRepository,
                "runId 不属于此仓库。",
            ));
        }
        Ok(run.clone())
    }
    pub fn ack(&self, root: &Path, id: &str, sequence: u64) -> Result<(), BackendError> {
        let run = self.active(root, id)?;
        let mut credits = run.credits.lock().unwrap();
        if sequence == 0 || sequence > credits.last_output {
            return Err(terminal_command::invalid("无效的终端输出确认序号。"));
        }
        // Duplicate acknowledgments do not create credits; out-of-order ACKs are valid.
        credits.outstanding.remove(&sequence);
        drop(credits);
        run.credit_changed.notify_one();
        Ok(())
    }
    pub async fn write(&self, root: &Path, id: &str, data: &str) -> Result<(), BackendError> {
        if data.len() > 12 * 1024 {
            return Err(terminal_command::invalid("单次终端输入过大，请分块发送。"));
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| terminal_command::invalid("终端输入必须为 Base64。"))?;
        let run = self.active(root, id)?;
        let controls = run.controls.lock().unwrap().clone().ok_or_else(|| {
            BackendError::new(
                ErrorCode::GitOperationInProgress,
                "终端正在等待仓库锁或启动。",
            )
        })?;
        if run.cancellation.is_cancelled() {
            return Err(BackendError::new(ErrorCode::Cancelled, "终端正在结束。"));
        }
        tokio::task::spawn_blocking(move || {
            let mut writer = controls.writer.lock().unwrap();
            writer
                .write_all(&bytes)
                .and_then(|_| writer.flush())
                .map_err(BackendError::from)
        })
        .await
        .map_err(terminal_pty::failure)?
    }
    pub async fn resize(
        &self,
        root: &Path,
        id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), BackendError> {
        let dimensions = terminal_pty::size(cols, rows)?;
        let run = self.active(root, id)?;
        let controls = run.controls.lock().unwrap().clone().ok_or_else(|| {
            BackendError::new(
                ErrorCode::GitOperationInProgress,
                "终端正在等待仓库锁或启动。",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            controls
                .master
                .lock()
                .unwrap()
                .resize(dimensions)
                .map_err(terminal_pty::failure)
        })
        .await
        .map_err(terminal_pty::failure)?
    }
    pub async fn terminate(&self, root: &Path, id: &str) -> Result<(), BackendError> {
        validate_id(id)?;
        if !root.is_absolute() {
            return Err(BackendError::new(
                ErrorCode::InvalidRepository,
                "仓库路径必须为绝对路径。",
            ));
        }
        let run = {
            let mut registry = self.registry.lock().unwrap();
            if let Some(owner) = registry.known.get(id) {
                if !same_root(owner, root)
                    && !root
                        .canonicalize()
                        .is_ok_and(|path| same_root(owner, &path))
                {
                    return Err(BackendError::new(
                        ErrorCode::InvalidRepository,
                        "runId 不属于此仓库。",
                    ));
                }
            } else {
                // A cancellation arriving before start permanently consumes this ID.
                registry.known.insert(
                    id.to_owned(),
                    root.canonicalize().unwrap_or_else(|_| root.to_owned()),
                );
            }
            registry.active.as_ref().filter(|run| run.id == id).cloned()
        };
        if let Some(run) = run {
            run.cancellation.cancel();
            wait_cleanup(&run).await?;
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<(), BackendError> {
        let active = {
            let mut registry = self.registry.lock().unwrap();
            registry.shutting_down = true;
            registry.active.clone()
        };
        if let Some(run) = active {
            run.cancellation.cancel();
            wait_cleanup(&run).await?;
        }
        Ok(())
    }
    async fn execute<S: TerminalEventSink>(
        &self,
        run: Arc<Run>,
        args: Vec<String>,
        dimensions: portable_pty::PtySize,
        sink: S,
    ) {
        let started = Instant::now();
        let mut sequence = 0;
        let emit = |sequence: &mut u64, event| {
            *sequence += 1;
            sink.emit(TerminalEvent {
                run_id: run.id.clone(),
                root_path: run.root.clone(),
                sequence: *sequence,
                event,
            });
        };
        emit(&mut sequence, TerminalEventKind::Started);
        let result = self
            .execute_locked(&run, args, dimensions, &mut sequence, &sink)
            .await;
        // All acknowledged output and the complete process tree are gone before
        // the active slot and the repository write guard are released.
        self.registry.lock().unwrap().active = None;
        emit(
            &mut sequence,
            TerminalEventKind::Exited {
                exit_code: result.exit_code,
                duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                cancelled: run.cancellation.is_cancelled(),
                error: result.error,
            },
        );
        run.done.cancel();
    }
    async fn execute_locked<S: TerminalEventSink>(
        &self,
        run: &Arc<Run>,
        args: Vec<String>,
        dimensions: portable_pty::PtySize,
        sequence: &mut u64,
        sink: &S,
    ) -> PtyResult {
        let cancelled = || PtyResult {
            exit_code: None,
            error: None,
        };
        let _guard = tokio::select! { biased; _ = run.cancellation.cancelled() => return cancelled(), guard = self.coordinator.write(&run.root) => guard };
        let validation = tokio::select! { biased; _ = run.cancellation.cancelled() => return cancelled(), result = crate::application::terminal_completion::query(&run.root, &["rev-parse", "--show-toplevel"]) => result };
        let valid = validation
            .and_then(|value| {
                PathBuf::from(value.trim_end_matches(['\r', '\n']))
                    .canonicalize()
                    .ok()
            })
            .is_some_and(|root| same_root(&root, &run.root));
        if !valid {
            return PtyResult {
                exit_code: None,
                error: Some(BackendError::new(
                    ErrorCode::InvalidRepository,
                    "请求路径必须是有效 Git 仓库的根目录。",
                )),
            };
        }
        let (tx, mut rx) = mpsc::channel(2);
        let worker_run = run.clone();
        let worker = tokio::task::spawn_blocking(move || {
            terminal_pty::run(
                &worker_run.root,
                &args,
                dimensions,
                worker_run.cancellation.clone(),
                worker_run.controls.clone(),
                tx,
            )
        });
        loop {
            let chunk = tokio::select! { biased; _ = run.cancellation.cancelled() => break, chunk = rx.recv() => chunk };
            let Some(chunk) = chunk else {
                break;
            };
            loop {
                let notified = run.credit_changed.notified();
                if run.credits.lock().unwrap().outstanding.len() < OUTPUT_CREDITS {
                    break;
                }
                tokio::select! { biased; _ = run.cancellation.cancelled() => break, _ = notified => {} }
                if run.cancellation.is_cancelled() {
                    break;
                }
            }
            if run.cancellation.is_cancelled() {
                break;
            }
            *sequence += 1;
            {
                let mut credits = run.credits.lock().unwrap();
                credits.outstanding.insert(*sequence);
                credits.last_output = *sequence;
            }
            sink.emit(TerminalEvent {
                run_id: run.id.clone(),
                root_path: run.root.clone(),
                sequence: *sequence,
                event: TerminalEventKind::Output {
                    data: STANDARD.encode(chunk),
                },
            });
        }
        drop(rx); // Termination must unblock a reader waiting on the bounded queue.
        let result = worker.await.unwrap_or_else(|error| PtyResult {
            exit_code: None,
            error: Some(terminal_pty::failure(error)),
        });
        // Keep the run addressable until xterm has consumed every delivered byte.
        // Cancellation bypasses missing credits, including a destroyed webview.
        loop {
            let notified = run.credit_changed.notified();
            if run.cancellation.is_cancelled() || run.credits.lock().unwrap().outstanding.is_empty()
            {
                break;
            }
            tokio::select! { biased; _ = run.cancellation.cancelled() => break, _ = notified => {} }
        }
        result
    }
}
async fn wait_cleanup(run: &Run) -> Result<(), BackendError> {
    tokio::time::timeout(CLEANUP_TIMEOUT, run.done.cancelled())
        .await
        .map_err(|_| {
            BackendError::new(
                ErrorCode::GitOperationInProgress,
                "Git 进程清理尚未完成，仓库锁仍保留，请稍后重试。",
            )
        })
}
fn validate_id(id: &str) -> Result<(), BackendError> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        Err(terminal_command::invalid(
            "runId 必须为 1–128 字节且不能包含控制字符。",
        ))
    } else {
        Ok(())
    }
}
fn same_root(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        let normalize = |path: &Path| {
            path.to_string_lossy()
                .replace('/', "\\")
                .trim_start_matches("\\\\?\\")
                .trim_end_matches('\\')
                .to_lowercase()
        };
        normalize(a) == normalize(b)
    } else {
        a == b
    }
}
#[cfg(test)]
#[path = "terminal_service_tests.rs"]
mod tests;
