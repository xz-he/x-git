use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::application::operation_state::{
    MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
};
use crate::application::refs_service::RefsService;
use crate::application::repository_service::RepositoryService;
use crate::domain::error::{BackendError, ErrorCode, sanitize_git_output};
use crate::domain::operation::MutationWorkspace;
use crate::domain::remotes::{
    FetchRequest, GitRunAccepted, GitRunEvent, GitRunEventKind, GitRunOperation,
    GitRunProgressPhase, PullRequest, PushRequest, RemoteBranchSummary, RemoteDetail,
    RemoteOperationResult, RemoteSnapshot,
};
use crate::infrastructure::git_runner::{
    GitCommandRunner, GitInvocation, GitProgressLine, GitProgressPhase,
};

const FIELD_SEPARATOR: char = '\u{1f}';
const RECORD_SEPARATOR: char = '\u{1e}';
const MAX_RUN_PROGRESS_BYTES: usize = 512;

pub trait GitRunEventSink: Clone + Send + Sync + 'static {
    fn emit(&self, event: GitRunEvent);
}

#[derive(Debug, Clone)]
pub struct RemoteService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
    active_runs: Arc<AsyncMutex<HashMap<String, CancellationToken>>>,
}

impl Default for RemoteService {
    fn default() -> Self {
        Self::new(
            GitCommandRunner::default(),
            RepositoryMutationCoordinator::default(),
        )
    }
}

impl RemoteService {
    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
            active_runs: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    pub async fn cancel(&self, run_id: &str) -> Result<(), BackendError> {
        if let Some(cancellation) = self.active_runs.lock().await.get(run_id).cloned() {
            cancellation.cancel();
        }
        Ok(())
    }

    pub async fn start_fetch<S>(
        &self,
        requested_root: &Path,
        run_id: &str,
        request: FetchRequest,
        sink: S,
    ) -> Result<GitRunAccepted, BackendError>
    where
        S: GitRunEventSink,
    {
        let root = self.repository_root(requested_root).await?;
        let service = self.clone();
        self.start_run(
            run_id,
            GitRunOperation::Fetch,
            sink,
            move |cancellation, publisher| async move {
                service
                    .execute_sync(
                        root,
                        GitRunOperation::Fetch,
                        request.remote,
                        None,
                        None,
                        cancellation,
                        publisher,
                    )
                    .await
            },
        )
        .await
    }

    pub async fn start_pull<S>(
        &self,
        requested_root: &Path,
        run_id: &str,
        request: PullRequest,
        sink: S,
    ) -> Result<GitRunAccepted, BackendError>
    where
        S: GitRunEventSink,
    {
        let root = self.repository_root(requested_root).await?;
        let service = self.clone();
        self.start_run(
            run_id,
            GitRunOperation::Pull,
            sink,
            move |cancellation, publisher| async move {
                service
                    .execute_sync(
                        root,
                        GitRunOperation::Pull,
                        request.remote,
                        Some(request.remote_branch),
                        request.local_branch,
                        cancellation,
                        publisher,
                    )
                    .await
            },
        )
        .await
    }

    pub async fn start_push<S>(
        &self,
        requested_root: &Path,
        run_id: &str,
        request: PushRequest,
        sink: S,
    ) -> Result<GitRunAccepted, BackendError>
    where
        S: GitRunEventSink,
    {
        let root = self.repository_root(requested_root).await?;
        let service = self.clone();
        self.start_run(
            run_id,
            GitRunOperation::Push,
            sink,
            move |cancellation, publisher| async move {
                service
                    .execute_push(root, request, cancellation, publisher)
                    .await
            },
        )
        .await
    }

    pub async fn snapshot(&self, requested_root: &Path) -> Result<RemoteSnapshot, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        self.snapshot_at_root(&root).await
    }

    pub(crate) async fn snapshot_at_root(
        &self,
        root: &Path,
    ) -> Result<RemoteSnapshot, BackendError> {
        let remote_names = self
            .runner
            .run(Some(root), ["remote"])
            .await?
            .stdout
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if remote_names.is_empty() {
            return Ok(RemoteSnapshot::default());
        }

        let remote_refs = self
            .runner
            .run(
                Some(root),
                [
                    OsString::from("for-each-ref"),
                    OsString::from("--sort=refname"),
                    OsString::from("--format=%(refname)%1f%(objectname)%1f%(symref)%1e"),
                    OsString::from("refs/remotes"),
                ],
            )
            .await?;
        let local_tracking = self
            .runner
            .run(
                Some(root),
                [
                    OsString::from("for-each-ref"),
                    OsString::from("--sort=refname"),
                    OsString::from("--format=%(refname)%1f%(upstream)%1e"),
                    OsString::from("refs/heads"),
                ],
            )
            .await?;
        let parsed_refs = parse_remote_refs(&remote_refs.stdout)?;
        let tracking = parse_tracking_refs(&local_tracking.stdout)?;
        let mut remotes = Vec::with_capacity(remote_names.len());

        for name in remote_names {
            let fetch_url = self.remote_url(root, &name, false).await?;
            let push_url = self.remote_url(root, &name, true).await?;
            let prefix = format!("refs/remotes/{name}/");
            let mut branches = Vec::new();
            for parsed in parsed_refs
                .iter()
                .filter(|reference| reference.full_name.starts_with(&prefix))
            {
                let tracking_local = tracking.get(&parsed.full_name).cloned();
                let (ahead, behind) = if let Some(local) = tracking_local.as_deref() {
                    let local_ref = format!("refs/heads/{local}");
                    let (ahead, behind) =
                        self.divergence(root, &local_ref, &parsed.full_name).await?;
                    (Some(ahead), Some(behind))
                } else {
                    (None, None)
                };
                branches.push(RemoteBranchSummary {
                    name: parsed.full_name[prefix.len()..].to_owned(),
                    full_name: parsed.full_name.clone(),
                    object_id: parsed.object_id.clone(),
                    tracking_local,
                    ahead,
                    behind,
                });
            }
            remotes.push(RemoteDetail {
                name,
                fetch_url,
                push_url,
                branches,
            });
        }

        Ok(RemoteSnapshot { remotes })
    }

    async fn repository_root(&self, requested_root: &Path) -> Result<PathBuf, BackendError> {
        RepositoryService::with_coordinator(self.runner.clone(), self.coordinator.clone())
            .resolve_root(requested_root)
            .await
    }

    async fn remote_url(
        &self,
        root: &Path,
        name: &str,
        push: bool,
    ) -> Result<String, BackendError> {
        let mut args = vec![OsString::from("remote"), OsString::from("get-url")];
        if push {
            args.push(OsString::from("--push"));
        }
        args.push(OsString::from("--"));
        args.push(OsString::from(name));
        Ok(sanitize_git_output(
            self.runner.run(Some(root), args).await?.stdout.trim(),
        ))
    }

    async fn divergence(
        &self,
        root: &Path,
        local: &str,
        remote: &str,
    ) -> Result<(u32, u32), BackendError> {
        let range = format!("{local}...{remote}");
        let output = self
            .runner
            .run(Some(root), ["rev-list", "--left-right", "--count", &range])
            .await?;
        let mut counts = output.stdout.split_whitespace();
        let ahead = parse_count(counts.next())?;
        let behind = parse_count(counts.next())?;
        if counts.next().is_some() {
            return Err(parse_error("Git 返回了多余的远程分歧字段。"));
        }
        Ok((ahead, behind))
    }

    async fn start_run<S, F, Fut>(
        &self,
        run_id: &str,
        operation: GitRunOperation,
        sink: S,
        execute: F,
    ) -> Result<GitRunAccepted, BackendError>
    where
        S: GitRunEventSink,
        F: FnOnce(CancellationToken, GitRunEventPublisher<S>) -> Fut + Send + 'static,
        Fut: Future<Output = RunTerminal> + Send + 'static,
    {
        let cancellation = self.register_run(run_id).await?;
        let accepted = GitRunAccepted {
            run_id: run_id.to_owned(),
            operation,
        };
        let publisher = GitRunEventPublisher::new(run_id, sink);
        publisher.started(operation);

        let active_runs = self.active_runs.clone();
        let owned_run_id = run_id.to_owned();
        tokio::spawn(async move {
            let terminal = execute(cancellation, publisher.clone()).await;
            publisher.terminal(terminal);
            active_runs.lock().await.remove(&owned_run_id);
        });

        Ok(accepted)
    }

    async fn register_run(&self, run_id: &str) -> Result<CancellationToken, BackendError> {
        let mut active_runs = self.active_runs.lock().await;
        if active_runs.contains_key(run_id) {
            return Err(BackendError::new(
                ErrorCode::GitOperationInProgress,
                "该 Git 任务标识已在使用。",
            ));
        }
        let cancellation = CancellationToken::new();
        active_runs.insert(run_id.to_owned(), cancellation.clone());
        Ok(cancellation)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_sync<S>(
        &self,
        root: PathBuf,
        operation: GitRunOperation,
        remote: String,
        remote_branch: Option<String>,
        local_branch: Option<String>,
        cancellation: CancellationToken,
        publisher: GitRunEventPublisher<S>,
    ) -> RunTerminal
    where
        S: GitRunEventSink,
    {
        let _guard = self.coordinator.write(&root).await;
        if let Err(error) = self
            .validate_sync_target(&root, &remote, remote_branch.as_deref())
            .await
        {
            return self.failed_with_refresh(&root, error).await;
        }

        if cancellation.is_cancelled() {
            return finish_sync(
                operation,
                Err(BackendError::new(ErrorCode::Cancelled, "操作已取消。")),
                self.refreshed_operation_result(&root).await,
            );
        }
        if let Some(local_branch) = local_branch {
            let refs = RefsService::new(self.runner.clone(), self.coordinator.clone());
            if let Err(error) = refs
                .checkout_integration_destination(&root, &local_branch)
                .await
            {
                return self.failed_with_refresh(&root, error).await;
            }
            publisher.progress(
                GitRunProgressPhase::Updating,
                format!("正在拉取到本地分支 {local_branch}。"),
            );
        }
        let invocation = match remote_branch.as_deref() {
            Some(branch) => GitInvocation::new([
                OsString::from("pull"),
                OsString::from("--progress"),
                OsString::from("--"),
                OsString::from(&remote),
                OsString::from(format!("refs/heads/{branch}")),
            ]),
            None => GitInvocation::new([
                OsString::from("fetch"),
                OsString::from("--progress"),
                OsString::from("--"),
                OsString::from(&remote),
            ]),
        };
        let progress_publisher = publisher.clone();
        let execution = self
            .runner
            .run_streaming(Some(&root), invocation, cancellation, move |progress| {
                publish_progress(&progress_publisher, progress);
            })
            .await;
        let refreshed = self.refreshed_operation_result(&root).await;
        finish_sync(operation, execution, refreshed)
    }

    async fn execute_push<S>(
        &self,
        root: PathBuf,
        request: PushRequest,
        cancellation: CancellationToken,
        publisher: GitRunEventPublisher<S>,
    ) -> RunTerminal
    where
        S: GitRunEventSink,
    {
        let _guard = self.coordinator.write(&root).await;
        let (local_ref, lease_argument) = match self.validate_push_request(&root, &request).await {
            Ok(validated) => validated,
            Err(error) => return self.failed_with_refresh(&root, error).await,
        };
        let mut args = vec![OsString::from("push"), OsString::from("--progress")];
        if request.establish_upstream {
            args.push(OsString::from("--set-upstream"));
        }
        if let Some(lease_argument) = lease_argument {
            args.push(OsString::from(lease_argument));
        }
        args.extend([
            OsString::from("--"),
            OsString::from(&request.remote),
            OsString::from(format!("{local_ref}:refs/heads/{}", request.remote_branch)),
        ]);

        let progress_publisher = publisher.clone();
        let force_with_lease = request.force_with_lease.is_some();
        let execution = self
            .runner
            .run_streaming(
                Some(&root),
                GitInvocation::new(args),
                cancellation,
                move |progress| publish_progress(&progress_publisher, progress),
            )
            .await
            .map_err(|error| map_push_error(error, force_with_lease));
        let refreshed = self.refreshed_operation_result(&root).await;
        finish_sync(GitRunOperation::Push, execution, refreshed)
    }

    async fn validate_sync_target(
        &self,
        root: &Path,
        remote_name: &str,
        remote_branch: Option<&str>,
    ) -> Result<(), BackendError> {
        let state = read_operation_state(root, &self.runner).await?;
        ensure_mutation_allowed(&state, MutationIntent::RemoteSync)?;
        let remotes = self.snapshot_at_root(root).await?;
        let _remote = remotes
            .remotes
            .iter()
            .find(|remote| remote.name == remote_name)
            .ok_or_else(|| unavailable_target("所选远程仓库已不存在，请刷新后重试。"))?;
        if let Some(branch) = remote_branch {
            validate_remote_branch_name(root, &self.runner, branch).await?;
        }
        Ok(())
    }

    async fn validate_push_request(
        &self,
        root: &Path,
        request: &PushRequest,
    ) -> Result<(String, Option<String>), BackendError> {
        let state = read_operation_state(root, &self.runner).await?;
        ensure_mutation_allowed(&state, MutationIntent::RemoteSync)?;
        let remotes = self.snapshot_at_root(root).await?;
        let remote = remotes
            .remotes
            .iter()
            .find(|remote| remote.name == request.remote)
            .ok_or_else(|| unavailable_target("所选远程仓库已不存在，请刷新后重试。"))?;

        validate_remote_branch_name(root, &self.runner, &request.remote_branch).await?;
        let refs = RefsService::new(self.runner.clone(), self.coordinator.clone())
            .snapshot_at_root(root)
            .await?;
        let local = refs
            .local_branches
            .iter()
            .find(|branch| branch.name == request.local_branch)
            .ok_or_else(|| unavailable_target("所选本地分支已不存在，请刷新后重试。"))?;
        let resolved_oid = self
            .runner
            .run(
                Some(root),
                [
                    "rev-parse",
                    "--verify",
                    &format!("{}^{{commit}}", local.full_name),
                ],
            )
            .await?
            .stdout
            .trim()
            .to_owned();
        if resolved_oid != local.tip.full_hash {
            return Err(unavailable_target("所选本地分支已变化，请刷新后重试。"));
        }

        let lease_argument = request
            .force_with_lease
            .as_ref()
            .map(|lease| {
                let remote_branch = remote
                    .branches
                    .iter()
                    .find(|branch| branch.name == request.remote_branch)
                    .ok_or_else(|| lease_rejected("Force With Lease 需要已知的远程分支对象。"))?;
                if lease.expected_remote_oid != remote_branch.object_id {
                    return Err(lease_rejected("远程分支快照已变化，请刷新后重试。"));
                }
                Ok(format!(
                    "--force-with-lease=refs/heads/{}:{}",
                    request.remote_branch, lease.expected_remote_oid
                ))
            })
            .transpose()?;

        Ok((local.full_name.clone(), lease_argument))
    }

    async fn refreshed_operation_result(
        &self,
        root: &Path,
    ) -> Result<RemoteOperationResult, BackendError> {
        let refs = RefsService::new(self.runner.clone(), self.coordinator.clone());
        let (workspace, refs, remotes) = tokio::join!(
            refresh_mutation_workspace(root, &self.runner),
            refs.snapshot_at_root(root),
            self.snapshot_at_root(root),
        );
        let MutationWorkspace {
            workspace,
            operation_state,
        } = workspace?;
        Ok(RemoteOperationResult {
            workspace,
            refs: refs?,
            remotes: remotes?,
            operation_state,
        })
    }

    async fn failed_with_refresh(&self, root: &Path, error: BackendError) -> RunTerminal {
        RunTerminal::Failed {
            error,
            result: self.refreshed_operation_result(root).await.ok(),
        }
    }

    #[cfg(test)]
    async fn start_test_run<S, F, Fut>(
        &self,
        run_id: &str,
        operation: GitRunOperation,
        sink: S,
        execute: F,
    ) -> Result<GitRunAccepted, BackendError>
    where
        S: GitRunEventSink,
        F: FnOnce(CancellationToken, GitRunEventPublisher<S>) -> Fut + Send + 'static,
        Fut: Future<Output = RunTerminal> + Send + 'static,
    {
        self.start_run(run_id, operation, sink, execute).await
    }
}

#[derive(Debug)]
struct RunEventState {
    sequence: u64,
    terminal_emitted: bool,
}

#[derive(Clone)]
struct GitRunEventPublisher<S> {
    run_id: String,
    sink: S,
    state: Arc<Mutex<RunEventState>>,
}

impl<S: GitRunEventSink> GitRunEventPublisher<S> {
    fn new(run_id: &str, sink: S) -> Self {
        Self {
            run_id: run_id.to_owned(),
            sink,
            state: Arc::new(Mutex::new(RunEventState {
                sequence: 0,
                terminal_emitted: false,
            })),
        }
    }

    fn started(&self, operation: GitRunOperation) {
        self.emit_non_terminal(GitRunEventKind::Started { operation });
    }

    fn progress(&self, phase: GitRunProgressPhase, text: impl AsRef<str>) {
        self.emit_non_terminal(GitRunEventKind::Progress {
            phase,
            text: sanitize_progress_text(text.as_ref()),
        });
    }

    fn emit_non_terminal(&self, event: GitRunEventKind) {
        let mut state = self.state.lock().unwrap();
        if state.terminal_emitted {
            return;
        }
        state.sequence += 1;
        self.sink.emit(GitRunEvent {
            run_id: self.run_id.clone(),
            sequence: state.sequence,
            event,
        });
    }

    fn terminal(&self, terminal: RunTerminal) {
        let mut state = self.state.lock().unwrap();
        if state.terminal_emitted {
            return;
        }
        state.terminal_emitted = true;
        state.sequence += 1;
        self.sink.emit(GitRunEvent {
            run_id: self.run_id.clone(),
            sequence: state.sequence,
            event: terminal.into_event(),
        });
    }
}

enum RunTerminal {
    Completed(RemoteOperationResult),
    Conflicted(RemoteOperationResult),
    Cancelled(RemoteOperationResult),
    Failed {
        error: BackendError,
        result: Option<RemoteOperationResult>,
    },
}

impl RunTerminal {
    fn into_event(self) -> GitRunEventKind {
        match self {
            Self::Completed(result) => GitRunEventKind::Completed {
                result: sanitize_remote_result(result),
            },
            Self::Conflicted(result) => GitRunEventKind::Conflicted {
                result: sanitize_remote_result(result),
            },
            Self::Cancelled(result) => GitRunEventKind::Cancelled {
                result: sanitize_remote_result(result),
            },
            Self::Failed { mut error, result } => {
                error.diagnostics = error.diagnostics.map(|value| sanitize_git_output(&value));
                GitRunEventKind::Failed {
                    error,
                    result: result.map(sanitize_remote_result),
                }
            }
        }
    }
}

fn sanitize_progress_text(text: &str) -> String {
    let sanitized = sanitize_git_output(text);
    if sanitized.len() <= MAX_RUN_PROGRESS_BYTES {
        return sanitized;
    }
    let mut end = MAX_RUN_PROGRESS_BYTES;
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_owned()
}

fn sanitize_remote_result(mut result: RemoteOperationResult) -> RemoteOperationResult {
    for remote in &mut result.workspace.repository.remotes {
        remote.fetch_url = sanitize_git_output(&remote.fetch_url);
    }
    for remote in &mut result.remotes.remotes {
        remote.fetch_url = sanitize_git_output(&remote.fetch_url);
        remote.push_url = sanitize_git_output(&remote.push_url);
    }
    result
}

fn publish_progress<S: GitRunEventSink>(
    publisher: &GitRunEventPublisher<S>,
    progress: GitProgressLine,
) {
    publisher.progress(progress.phase.into(), progress.text);
}

impl From<GitProgressPhase> for GitRunProgressPhase {
    fn from(value: GitProgressPhase) -> Self {
        match value {
            GitProgressPhase::Enumerating => Self::Enumerating,
            GitProgressPhase::Counting => Self::Counting,
            GitProgressPhase::Compressing => Self::Compressing,
            GitProgressPhase::Receiving => Self::Receiving,
            GitProgressPhase::Resolving => Self::Resolving,
            GitProgressPhase::Writing => Self::Writing,
            GitProgressPhase::Updating => Self::Updating,
        }
    }
}

fn finish_sync(
    operation: GitRunOperation,
    execution: Result<crate::infrastructure::git_runner::GitOutput, BackendError>,
    refreshed: Result<RemoteOperationResult, BackendError>,
) -> RunTerminal {
    match (execution, refreshed) {
        (Ok(_), Ok(result)) => RunTerminal::Completed(result),
        (Ok(_), Err(error)) => RunTerminal::Failed {
            error: BackendError::new(
                ErrorCode::GitRefreshFailed,
                "Git 同步已成功，但刷新本地仓库状态失败。请手动刷新，不要重复执行同步。",
            )
            .with_diagnostics(format!(
                "{}\n{}",
                error.message,
                error.diagnostics.unwrap_or_default()
            )),
            result: None,
        },
        (Err(error), Ok(result)) if error.code == ErrorCode::Cancelled => {
            RunTerminal::Cancelled(result)
        }
        (Err(_), Ok(mut result))
            if operation == GitRunOperation::Pull
                && !result.operation_state.conflicts.is_empty() =>
        {
            result.operation_state.abort_action = None;
            RunTerminal::Conflicted(result)
        }
        (Err(error), Ok(result)) => RunTerminal::Failed {
            error,
            result: Some(result),
        },
        (Err(error), Err(refresh_error)) => RunTerminal::Failed {
            error: if error.code == ErrorCode::Cancelled {
                refresh_error
            } else {
                error
            },
            result: None,
        },
    }
}

fn unavailable_target(message: &str) -> BackendError {
    BackendError::new(ErrorCode::BranchUnavailable, message)
}

async fn validate_remote_branch_name(
    root: &Path,
    runner: &GitCommandRunner,
    name: &str,
) -> Result<(), BackendError> {
    if name.is_empty() || name != name.trim() || name.starts_with('-') {
        return Err(unavailable_target("远程分支名称无效。"));
    }
    runner
        .run(Some(root), ["check-ref-format", "--branch", name])
        .await
        .map(|_| ())
        .map_err(|_| unavailable_target("远程分支名称无效。"))
}

fn map_push_error(mut error: BackendError, force_with_lease: bool) -> BackendError {
    let diagnostics = error
        .diagnostics
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if force_with_lease
        && (diagnostics.contains("stale info")
            || diagnostics.contains("stale-info")
            || diagnostics.contains("[rejected]"))
    {
        error.code = ErrorCode::LeaseRejected;
        error.message = "远程分支已变化，Force With Lease 已安全拒绝推送。".to_owned();
    } else if !force_with_lease
        && (diagnostics.contains("non-fast-forward")
            || diagnostics.contains("fetch first")
            || diagnostics.contains("[rejected]"))
    {
        error.code = ErrorCode::NonFastForward;
        error.message = "远程分支包含本地没有的提交，推送已被拒绝。".to_owned();
    }
    error
}

fn lease_rejected(message: &str) -> BackendError {
    BackendError::new(ErrorCode::LeaseRejected, message)
}

#[derive(Debug)]
struct ParsedRemoteRef {
    full_name: String,
    object_id: String,
}

fn parse_remote_refs(output: &str) -> Result<Vec<ParsedRemoteRef>, BackendError> {
    records(output)
        .filter_map(|record| {
            let fields = record.splitn(3, FIELD_SEPARATOR).collect::<Vec<_>>();
            if fields.len() != 3 {
                return Some(Err(parse_error("Git 远程分支记录字段数量不正确。")));
            }
            if !fields[2].is_empty() {
                return None;
            }
            Some(Ok(ParsedRemoteRef {
                full_name: fields[0].to_owned(),
                object_id: fields[1].to_owned(),
            }))
        })
        .collect()
}

fn parse_tracking_refs(output: &str) -> Result<HashMap<String, String>, BackendError> {
    records(output)
        .filter_map(|record| {
            let fields = record.splitn(2, FIELD_SEPARATOR).collect::<Vec<_>>();
            if fields.len() != 2 {
                return Some(Err(parse_error("Git 本地跟踪记录字段数量不正确。")));
            }
            if fields[1].is_empty() {
                return None;
            }
            let local = fields[0]
                .strip_prefix("refs/heads/")
                .unwrap_or(fields[0])
                .to_owned();
            Some(Ok((fields[1].to_owned(), local)))
        })
        .collect()
}

fn records(output: &str) -> impl Iterator<Item = &str> {
    output
        .split(RECORD_SEPARATOR)
        .map(|record| record.trim_matches(['\r', '\n']))
        .filter(|record| !record.is_empty())
}

fn parse_count(value: Option<&str>) -> Result<u32, BackendError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| parse_error("Git 返回了无效的远程分歧计数。"))
}

fn parse_error(message: &str) -> BackendError {
    BackendError::new(ErrorCode::Unexpected, message)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::Notify;
    use tokio::time::timeout;

    use super::*;
    use crate::domain::changes::{ChangesSnapshot, WorkingTreeSnapshot};
    use crate::domain::operation::RepositoryOperationState;
    use crate::domain::refs::RefsSnapshot;
    use crate::domain::remotes::{
        GitRunEvent, GitRunEventKind, GitRunOperation, GitRunProgressPhase, RemoteOperationResult,
    };
    use crate::domain::repository::RepositorySnapshot;

    #[derive(Clone, Default)]
    struct RecordingSink {
        events: Arc<Mutex<Vec<GitRunEvent>>>,
        changed: Arc<Notify>,
    }

    impl GitRunEventSink for RecordingSink {
        fn emit(&self, event: GitRunEvent) {
            self.events.lock().unwrap().push(event);
            self.changed.notify_waiters();
        }
    }

    impl RecordingSink {
        fn events(&self, run_id: &str) -> Vec<GitRunEvent> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.run_id == run_id)
                .cloned()
                .collect()
        }

        async fn wait_for_terminal(&self, run_id: &str) {
            timeout(Duration::from_secs(2), async {
                loop {
                    let notified = self.changed.notified();
                    if self
                        .events(run_id)
                        .iter()
                        .any(|event| event.event.is_terminal())
                    {
                        return;
                    }
                    notified.await;
                }
            })
            .await
            .unwrap();
        }
    }

    fn empty_result() -> RemoteOperationResult {
        RemoteOperationResult {
            workspace: WorkingTreeSnapshot {
                repository: RepositorySnapshot {
                    root_path: PathBuf::from("C:/repo"),
                    name: "repo".to_owned(),
                    current_branch: Some("main".to_owned()),
                    head_short_hash: Some("1234567".to_owned()),
                    is_clean: true,
                    changed_file_count: 0,
                    conflict_count: 0,
                    remotes: Vec::new(),
                    upstream: None,
                },
                changes: ChangesSnapshot {
                    files: Vec::new(),
                    staged_count: 0,
                    unstaged_count: 0,
                },
            },
            refs: RefsSnapshot::default(),
            remotes: RemoteSnapshot::default(),
            operation_state: RepositoryOperationState::default(),
        }
    }

    #[test]
    fn completed_git_command_with_failed_refresh_is_reported_distinctly() {
        let execution = Ok(crate::infrastructure::git_runner::GitOutput {
            stdout: String::new(),
            stderr: String::new(),
            status_code: Some(0),
        });
        let refresh = Err(BackendError::new(ErrorCode::Io, "无法读取仓库状态"));
        let RunTerminal::Failed { error, result } =
            finish_sync(GitRunOperation::Push, execution, refresh)
        else {
            panic!("expected refresh warning");
        };
        assert_eq!(error.code, ErrorCode::GitRefreshFailed);
        assert!(error.message.contains("已成功"));
        assert!(error.message.contains("不要重复执行"));
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn accepted_run_emits_started_progress_and_one_terminal_event() {
        let service = RemoteService::default();
        let sink = RecordingSink::default();
        let accepted = service
            .start_test_run(
                "run-1",
                GitRunOperation::Fetch,
                sink.clone(),
                |_cancellation, publisher| async move {
                    publisher.progress(GitRunProgressPhase::Receiving, "Receiving objects: 50%");
                    RunTerminal::Completed(empty_result())
                },
            )
            .await
            .unwrap();

        sink.wait_for_terminal(&accepted.run_id).await;
        let events = sink.events(&accepted.run_id);

        assert_eq!(accepted.operation, GitRunOperation::Fetch);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 1);
        assert!(matches!(events[0].event, GitRunEventKind::Started { .. }));
        assert!(matches!(events[1].event, GitRunEventKind::Progress { .. }));
        assert!(matches!(events[2].event, GitRunEventKind::Completed { .. }));
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event.is_terminal())
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn duplicate_run_id_is_rejected_until_terminal_cleanup() {
        let service = RemoteService::default();
        let sink = RecordingSink::default();
        let first = service
            .start_test_run(
                "same-run",
                GitRunOperation::Fetch,
                sink.clone(),
                |cancellation, _publisher| async move {
                    cancellation.cancelled().await;
                    RunTerminal::Cancelled(empty_result())
                },
            )
            .await
            .unwrap();

        let duplicate = service
            .start_test_run(
                "same-run",
                GitRunOperation::Pull,
                sink.clone(),
                |_cancellation, _publisher| async move { RunTerminal::Completed(empty_result()) },
            )
            .await
            .unwrap_err();
        assert_eq!(duplicate.code, ErrorCode::GitOperationInProgress);

        service.cancel(&first.run_id).await.unwrap();
        sink.wait_for_terminal(&first.run_id).await;

        let restarted = service
            .start_test_run(
                "same-run",
                GitRunOperation::Pull,
                sink.clone(),
                |_cancellation, _publisher| async move { RunTerminal::Completed(empty_result()) },
            )
            .await
            .unwrap();
        sink.wait_for_terminal(&restarted.run_id).await;
    }

    #[tokio::test]
    async fn cancel_routes_only_to_the_named_active_run() {
        let service = RemoteService::default();
        let sink = RecordingSink::default();
        for run_id in ["run-a", "run-b"] {
            service
                .start_test_run(
                    run_id,
                    GitRunOperation::Fetch,
                    sink.clone(),
                    |cancellation, _publisher| async move {
                        cancellation.cancelled().await;
                        RunTerminal::Cancelled(empty_result())
                    },
                )
                .await
                .unwrap();
        }

        service.cancel("run-a").await.unwrap();
        sink.wait_for_terminal("run-a").await;
        assert!(matches!(
            sink.events("run-a").last().unwrap().event,
            GitRunEventKind::Cancelled { .. }
        ));
        assert!(
            sink.events("run-b")
                .iter()
                .all(|event| !event.event.is_terminal())
        );

        service.cancel("run-b").await.unwrap();
        sink.wait_for_terminal("run-b").await;
    }

    #[test]
    fn event_boundary_redacts_credentials_and_bounds_progress() {
        let sink = RecordingSink::default();
        let publisher = GitRunEventPublisher::new("secure-run", sink.clone());
        publisher.started(GitRunOperation::Push);
        publisher.progress(
            GitRunProgressPhase::Writing,
            format!(
                "https://user:password@example.test/repo?token=secret {}",
                "x".repeat(700)
            ),
        );

        let mut result = empty_result();
        result
            .workspace
            .repository
            .remotes
            .push(crate::domain::repository::RemoteSummary {
                name: "origin".to_owned(),
                fetch_url: "https://user:password@example.test/repo?token=secret".to_owned(),
            });
        result.remotes.remotes.push(RemoteDetail {
            name: "origin".to_owned(),
            fetch_url: "https://user:password@example.test/repo?token=secret".to_owned(),
            push_url: "https://user:password@example.test/repo?key=secret".to_owned(),
            branches: Vec::new(),
        });

        for terminal in [
            RunTerminal::Completed(result.clone()),
            RunTerminal::Conflicted(result.clone()),
            RunTerminal::Cancelled(result.clone()),
            RunTerminal::Failed {
                error: BackendError::new(ErrorCode::GitCommandFailed, "失败")
                    .with_diagnostics("https://user:password@example.test/repo?token=secret"),
                result: Some(result),
            },
        ] {
            let event = terminal.into_event();
            assert!(event.is_terminal());
            let serialized = serde_json::to_string(&event).unwrap();
            assert!(!serialized.contains("password"));
            assert!(!serialized.contains("secret"));
        }

        let events = sink.events("secure-run");
        let GitRunEventKind::Progress { text, .. } = &events[1].event else {
            panic!("expected progress event");
        };
        assert!(text.len() <= MAX_RUN_PROGRESS_BYTES);
        assert!(!text.contains("password"));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn cancellation_with_a_failed_refresh_reports_the_refresh_error() {
        let terminal = finish_sync(
            GitRunOperation::Fetch,
            Err(BackendError::new(ErrorCode::Cancelled, "已取消")),
            Err(BackendError::new(ErrorCode::Io, "刷新失败")),
        );

        let RunTerminal::Failed { error, result } = terminal else {
            panic!("expected failed terminal");
        };
        assert_eq!(error.code, ErrorCode::Io);
        assert!(result.is_none());
    }
}
