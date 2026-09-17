use crate::application::{
    console_policy::parse_command, mutation_coordinator::RepositoryMutationCoordinator,
};
use crate::domain::{
    console::{
        ConsoleAccepted, ConsoleEvent, ConsoleEventKind, ConsoleOutcome, ConsoleQuery,
        ConsoleStream,
    },
    error::{BackendError, ErrorCode},
};
use crate::infrastructure::console_runner::{self, ConsoleRunResult};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::oneshot, time::Instant};
use tokio_util::sync::CancellationToken;

const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
pub trait ConsoleEventSink: Clone + Send + Sync + 'static {
    fn emit(&self, event: ConsoleEvent);
}
#[derive(Clone)]
pub struct ConsoleService {
    coordinator: RepositoryMutationCoordinator,
    registry: Arc<Mutex<Registry>>,
    timeout: Duration,
}
#[derive(Default)]
struct Registry {
    active: Option<Arc<Run>>,
    known: HashMap<String, KnownRun>,
    shutting_down: bool,
}
struct KnownRun {
    root: PathBuf,
    canonical_root: Option<PathBuf>,
    cancelled: bool,
}
struct Run {
    id: String,
    root: PathBuf,
    cancellation: CancellationToken,
    done: CancellationToken,
}
struct Publisher<S> {
    sink: S,
    id: String,
    root: PathBuf,
    sequence: u64,
}
impl<S: ConsoleEventSink> Publisher<S> {
    fn emit(&mut self, event: ConsoleEventKind) {
        self.sequence += 1;
        self.sink.emit(ConsoleEvent {
            run_id: self.id.clone(),
            root_path: self.root.clone(),
            sequence: self.sequence,
            event,
        });
    }
    fn output(&mut self, stream: ConsoleStream, text: String) {
        self.emit(ConsoleEventKind::Output { stream, text });
    }
}
impl Default for ConsoleService {
    fn default() -> Self {
        Self::new(RepositoryMutationCoordinator::default())
    }
}
impl ConsoleService {
    pub fn new(coordinator: RepositoryMutationCoordinator) -> Self {
        Self::with_timeout(coordinator, QUERY_TIMEOUT)
    }
    pub fn with_timeout(coordinator: RepositoryMutationCoordinator, timeout: Duration) -> Self {
        Self {
            coordinator,
            registry: Arc::new(Mutex::new(Registry::default())),
            timeout: timeout.min(QUERY_TIMEOUT),
        }
    }
    pub async fn start<S: ConsoleEventSink>(
        &self,
        root: &Path,
        id: &str,
        command: &str,
        sink: S,
    ) -> Result<ConsoleAccepted, BackendError> {
        let started = Instant::now();
        // Pure policy runs before any filesystem lookup or subprocess.
        let query = parse_command(command)?;
        validate_id(id)?;
        if !root.is_absolute() {
            return Err(BackendError::new(
                ErrorCode::InvalidRepository,
                "请提供仓库根的绝对路径。",
            ));
        }
        let run = Arc::new(Run {
            id: id.to_owned(),
            root: root.to_path_buf(),
            cancellation: CancellationToken::new(),
            done: CancellationToken::new(),
        });
        {
            // No await before registration: cancel can always find a pending start.
            let mut registry = self.registry.lock().unwrap();
            if registry.shutting_down {
                return Err(BackendError::new(ErrorCode::Cancelled, "应用正在退出。"));
            }
            if let Some(known) = registry.known.get(id) {
                return Err(BackendError::new(
                    if known.cancelled && same_owner(&known.root, root) {
                        ErrorCode::Cancelled
                    } else {
                        ErrorCode::InvalidConsoleCommand
                    },
                    "runId 已使用或已取消，请使用新的 runId。",
                ));
            }
            if registry.active.is_some() {
                return Err(BackendError::new(
                    ErrorCode::GitOperationInProgress,
                    "已有 Git 查询正在执行或结束，请稍后重试。",
                ));
            }
            registry.known.insert(
                id.to_owned(),
                KnownRun {
                    root: root.to_path_buf(),
                    canonical_root: None,
                    cancelled: false,
                },
            );
            registry.active = Some(run.clone());
        }
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let service = self.clone();
        tokio::spawn(async move {
            service
                .execute(run, query, sink, started, accepted_tx)
                .await;
        });
        accepted_rx.await.unwrap_or_else(|_| {
            Err(BackendError::new(
                ErrorCode::Unexpected,
                "Git 查询任务异常结束。",
            ))
        })
    }
    pub async fn cancel(&self, root: &Path, id: &str) -> Result<(), BackendError> {
        validate_id(id)?;
        let active = {
            let mut registry = self.registry.lock().unwrap();
            if let Some(known) = registry.known.get_mut(id) {
                if !same_owner(&known.root, root)
                    && !known
                        .canonical_root
                        .as_ref()
                        .is_some_and(|canonical| same_owner(canonical, root))
                {
                    return Err(BackendError::new(
                        ErrorCode::InvalidRepository,
                        "runId 不属于此仓库。",
                    ));
                }
                known.cancelled = true;
            } else {
                // A tombstone prevents a start that arrives after its cancellation.
                registry.known.insert(
                    id.to_owned(),
                    KnownRun {
                        root: root.to_path_buf(),
                        canonical_root: None,
                        cancelled: true,
                    },
                );
            }
            registry.active.as_ref().filter(|run| run.id == id).cloned()
        };
        if let Some(run) = active {
            run.cancellation.cancel();
            run.done.cancelled().await;
        }
        Ok(())
    }
    pub async fn shutdown(&self) {
        let active = {
            let mut registry = self.registry.lock().unwrap();
            registry.shutting_down = true;
            registry.active.clone()
        };
        if let Some(run) = active {
            run.cancellation.cancel();
            run.done.cancelled().await;
        }
    }
    async fn execute<S: ConsoleEventSink>(
        &self,
        run: Arc<Run>,
        query: ConsoleQuery,
        sink: S,
        started: Instant,
        accepted: oneshot::Sender<Result<ConsoleAccepted, BackendError>>,
    ) {
        let deadline = started + self.timeout;
        let mut publisher = Publisher {
            sink,
            id: run.id.clone(),
            root: run.root.clone(),
            sequence: 0,
        };
        let canonical = tokio::select! {
            biased;
            _ = run.cancellation.cancelled() => Err(ConsoleRunResult::interrupted(false)),
            _ = tokio::time::sleep_until(deadline) => Err(ConsoleRunResult::interrupted(true)),
            value = tokio::fs::canonicalize(&run.root) => value.map_err(|error| ConsoleRunResult::failed(BackendError::new(ErrorCode::InvalidRepository, "无法读取仓库目录。").with_diagnostics(error.to_string()))),
        };
        let result = match canonical {
            Ok(root) => {
                publisher.root = root.clone();
                if let Some(known) = self.registry.lock().unwrap().known.get_mut(&run.id) {
                    known.canonical_root = Some(root.clone());
                }
                // Canonical root is published before acceptance; terminal may precede the IPC response.
                publisher.emit(ConsoleEventKind::Started);
                let _ = accepted.send(Ok(ConsoleAccepted {
                    run_id: run.id.clone(),
                    root_path: root.clone(),
                }));
                self.execute_at_root(&root, &query, &run.cancellation, deadline, &mut publisher)
                    .await
            }
            Err(result) => {
                let _ = accepted.send(Err(result.error.clone().unwrap()));
                result
            }
        };
        // The worker has reaped its process and dropped its repository guard now.
        // Clear the slot before terminal delivery so a terminal subscriber can start anew.
        {
            let mut registry = self.registry.lock().unwrap();
            registry.active = None;
        }
        publisher.emit(ConsoleEventKind::Terminal {
            outcome: result.outcome,
            exit_code: result.exit_code,
            duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
            error: result.error,
        });
        run.done.cancel();
    }
    async fn execute_at_root<S: ConsoleEventSink>(
        &self,
        root: &Path,
        query: &ConsoleQuery,
        cancellation: &CancellationToken,
        deadline: Instant,
        publisher: &mut Publisher<S>,
    ) -> ConsoleRunResult {
        let _guard = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return ConsoleRunResult::interrupted(false),
            _ = tokio::time::sleep_until(deadline) => return ConsoleRunResult::interrupted(true),
            guard = self.coordinator.read(root) => guard,
        };
        let validation = console_runner::run(
            root,
            &["rev-parse".into(), "--show-toplevel".into()],
            cancellation,
            deadline,
            true,
            |stream, text| {
                if stream == ConsoleStream::Stderr {
                    publisher.output(stream, text);
                }
            },
        )
        .await;
        if validation.outcome != ConsoleOutcome::Completed {
            return validation;
        }
        let git_root = match std::str::from_utf8(&validation.captured_stdout) {
            Ok(value) => PathBuf::from(value.trim_end_matches(['\r', '\n'])),
            Err(_) => {
                return ConsoleRunResult::failed(BackendError::new(
                    ErrorCode::InvalidRepository,
                    "仓库根路径编码无效。",
                ));
            }
        };
        let verified_root = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return ConsoleRunResult::interrupted(false),
            _ = tokio::time::sleep_until(deadline) => return ConsoleRunResult::interrupted(true),
            value = tokio::fs::canonicalize(git_root) => value,
        };
        if !verified_root.is_ok_and(|verified| same_owner(&verified, root)) {
            return ConsoleRunResult::failed(BackendError::new(
                ErrorCode::InvalidRepository,
                "请求路径必须为当前仓库根。",
            ));
        }
        let mut oid = None;
        if let Some(revision) = query.revision() {
            let resolved = console_runner::run(
                root,
                &[
                    "rev-parse".into(),
                    "--verify".into(),
                    "--end-of-options".into(),
                    format!("{revision}^{{commit}}"),
                ],
                cancellation,
                deadline,
                true,
                |stream, text| {
                    if stream == ConsoleStream::Stderr {
                        publisher.output(stream, text);
                    }
                },
            )
            .await;
            if resolved.outcome != ConsoleOutcome::Completed {
                return resolved;
            }
            let value = String::from_utf8_lossy(&resolved.captured_stdout)
                .trim()
                .to_owned();
            if !matches!(value.len(), 40 | 64) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
                return ConsoleRunResult::failed(BackendError::new(
                    ErrorCode::InvalidReference,
                    "Git 未返回完整提交 ID。",
                ));
            }
            oid = Some(value);
        }
        console_runner::run(
            root,
            &query.argv(oid.as_deref()),
            cancellation,
            deadline,
            false,
            |stream, text| publisher.output(stream, text),
        )
        .await
    }
}
fn validate_id(id: &str) -> Result<(), BackendError> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        Err(BackendError::new(
            ErrorCode::InvalidConsoleCommand,
            "runId 必须为 1–128 字节且不含控制字符。",
        ))
    } else {
        Ok(())
    }
}
fn same_owner(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        let normalize = |path: &Path| {
            path.to_string_lossy()
                .replace('/', "\\")
                .trim_start_matches("\\\\?\\")
                .trim_end_matches('\\')
                .to_ascii_lowercase()
        };
        normalize(a) == normalize(b)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}
