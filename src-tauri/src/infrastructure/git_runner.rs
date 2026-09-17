use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::domain::error::{BackendError, ErrorCode, MAX_DIAGNOSTIC_BYTES, sanitize_git_output};

const DEFAULT_GIT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_PROGRESS_LINE_BYTES: usize = 512;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitProgressPhase {
    Enumerating,
    Counting,
    Compressing,
    Receiving,
    Resolving,
    Writing,
    Updating,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitProgressLine {
    pub phase: GitProgressPhase,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitInvocation {
    args: Vec<OsString>,
    disable_optional_locks: bool,
    index_file: Option<PathBuf>,
}

impl GitInvocation {
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut args: Vec<OsString> = args
            .into_iter()
            .map(|argument| argument.as_ref().to_os_string())
            .collect();
        // App-owned read queries put this global option first. Transport its
        // intent through the equivalent environment variable for older Git.
        // Never strip a similarly named path or subcommand argument.
        let disable_optional_locks = args.first().is_some_and(|arg| arg == "--no-optional-locks");
        if disable_optional_locks {
            args.remove(0);
        }
        Self {
            args,
            disable_optional_locks,
            index_file: None,
        }
    }

    pub fn program(&self) -> &'static str {
        "git"
    }

    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    fn configure_command(&self, command: &mut Command) {
        command.args(self.args());
        if let Some(index) = &self.index_file {
            command.env("GIT_INDEX_FILE", index);
        }
        if self.disable_optional_locks {
            command.env("GIT_OPTIONAL_LOCKS", "0");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
    pub status_code: Option<i32>,
}

impl GitOutput {
    pub fn is_success(&self) -> bool {
        self.status_code == Some(0)
    }

    pub fn into_result(self) -> Result<Self, BackendError> {
        if self.is_success() {
            return Ok(self);
        }
        let diagnostics = combined_diagnostics(&self);
        let code = classify_git_failure(&diagnostics);
        Err(BackendError::new(code, error_message(code)).with_diagnostics(diagnostics))
    }
}

#[derive(Debug, Clone)]
pub struct GitCommandRunner {
    timeout: Duration,
}

impl Default for GitCommandRunner {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_GIT_TIMEOUT,
        }
    }
}

impl GitCommandRunner {
    pub async fn run_bytes<I, S>(
        &self,
        working_directory: Option<&Path>,
        args: I,
        max_stdout_bytes: usize,
    ) -> Result<Vec<u8>, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let invocation = GitInvocation::new(args);
        let mut command = Command::new(invocation.program());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        invocation.configure_command(&mut command);
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(directory) = working_directory {
            command.current_dir(directory);
        }
        let mut child = command.spawn().map_err(map_spawn_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BackendError::new(ErrorCode::Unexpected, "无法读取 Git 标准输出。"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BackendError::new(ErrorCode::Unexpected, "无法读取 Git 错误输出。"))?;
        let ((bytes, exceeded), (diagnostics, _), status) = timeout(self.timeout, async {
            tokio::try_join!(
                read_bounded_bytes(stdout, max_stdout_bytes),
                read_bounded_bytes(stderr, MAX_DIAGNOSTIC_BYTES),
                child.wait()
            )
        })
        .await
        .map_err(|_| BackendError::new(ErrorCode::Cancelled, "Git 命令执行超时。"))?
        .map_err(BackendError::from)?;
        if exceeded {
            return Err(BackendError::new(
                ErrorCode::GitCommandFailed,
                format!("Git 输出超过 {max_stdout_bytes} 字节限制。"),
            ));
        }
        GitOutput {
            stdout: String::new(),
            stderr: String::from_utf8_lossy(&diagnostics).into_owned(),
            status_code: status.code(),
        }
        .into_result()?;
        Ok(bytes)
    }

    pub async fn run<I, S>(
        &self,
        working_directory: Option<&Path>,
        args: I,
    ) -> Result<GitOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let invocation = GitInvocation::new(args);
        self.execute(working_directory, invocation, None, false)
            .await?
            .into_result()
    }

    pub async fn run_allowing_failure<I, S>(
        &self,
        working_directory: Option<&Path>,
        args: I,
    ) -> Result<GitOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let invocation = GitInvocation::new(args);
        self.execute(working_directory, invocation, None, false)
            .await
    }

    pub async fn run_without_editor<I, S>(
        &self,
        working_directory: Option<&Path>,
        args: I,
    ) -> Result<GitOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.execute(working_directory, GitInvocation::new(args), None, true)
            .await
    }

    pub async fn run_with_input<I, S>(
        &self,
        working_directory: Option<&Path>,
        args: I,
        input: impl Into<Vec<u8>>,
    ) -> Result<GitOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let invocation = GitInvocation::new(args);
        self.execute(working_directory, invocation, Some(input.into()), false)
            .await?
            .into_result()
    }

    /// Use a private index without changing process-wide environment or the
    /// repository index. Preserve the normal timeout and hidden-window behavior.
    pub(crate) async fn run_with_index<I, S>(
        &self,
        root: &Path,
        index: &Path,
        args: I,
        input: Option<Vec<u8>>,
    ) -> Result<GitOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut invocation = GitInvocation::new(args);
        invocation.index_file = Some(index.to_path_buf());
        self.execute(Some(root), invocation, input, false)
            .await?
            .into_result()
    }

    pub async fn run_streaming<F>(
        &self,
        working_directory: Option<&Path>,
        invocation: GitInvocation,
        cancellation: CancellationToken,
        mut on_progress: F,
    ) -> Result<GitOutput, BackendError>
    where
        F: FnMut(GitProgressLine) + Send,
    {
        let mut command = Command::new(invocation.program());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        invocation.configure_command(&mut command);
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(directory) = working_directory {
            command.current_dir(directory);
        }

        let mut child = command.spawn().map_err(map_spawn_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BackendError::new(ErrorCode::Unexpected, "无法读取 Git 标准输出。"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BackendError::new(ErrorCode::Unexpected, "无法读取 Git 错误输出。"))?;
        let (line_tx, mut line_rx) = mpsc::unbounded_channel();
        let stdout_task = tokio::spawn(read_stream(stdout, line_tx.clone()));
        let stderr_task = tokio::spawn(read_stream(stderr, line_tx.clone()));
        drop(line_tx);
        let deadline = tokio::time::sleep(self.timeout);
        tokio::pin!(deadline);

        let status = loop {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    stdout_task.abort();
                    stderr_task.abort();
                    return Err(cancelled_error("Git 操作已取消。"));
                }
                _ = &mut deadline => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    stdout_task.abort();
                    stderr_task.abort();
                    return Err(cancelled_error("Git 命令执行超时。"));
                }
                Some(line) = line_rx.recv() => {
                    if let Some(progress) = parse_progress_line(&line) {
                        on_progress(progress);
                    }
                }
                result = child.wait() => {
                    break result.map_err(BackendError::from)?;
                }
            }
        };

        let stdout = stdout_task.await.map_err(join_error)?;
        let stderr = stderr_task.await.map_err(join_error)?;
        while let Ok(line) = line_rx.try_recv() {
            if let Some(progress) = parse_progress_line(&line) {
                on_progress(progress);
            }
        }
        GitOutput {
            stdout,
            stderr,
            status_code: status.code(),
        }
        .into_result()
    }

    async fn execute(
        &self,
        working_directory: Option<&Path>,
        invocation: GitInvocation,
        input: Option<Vec<u8>>,
        disable_editors: bool,
    ) -> Result<GitOutput, BackendError> {
        let mut command = Command::new(invocation.program());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        invocation.configure_command(&mut command);
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if disable_editors {
            command
                .env("GIT_EDITOR", "true")
                .env("GIT_SEQUENCE_EDITOR", "true");
        }
        if let Some(directory) = working_directory {
            command.current_dir(directory);
        }

        let process_output = match input {
            Some(input) => {
                let mut child = command.spawn().map_err(map_spawn_error)?;
                let mut stdin = child.stdin.take().ok_or_else(|| {
                    BackendError::new(ErrorCode::Unexpected, "无法写入 Git 命令输入。")
                })?;
                timeout(self.timeout, async move {
                    stdin.write_all(&input).await?;
                    stdin.shutdown().await?;
                    drop(stdin);
                    child.wait_with_output().await
                })
                .await
                .map_err(|_| BackendError::new(ErrorCode::Cancelled, "Git 命令执行超时。"))?
                .map_err(BackendError::from)?
            }
            None => timeout(self.timeout, command.output())
                .await
                .map_err(|_| BackendError::new(ErrorCode::Cancelled, "Git 命令执行超时。"))?
                .map_err(map_spawn_error)?,
        };

        let output = GitOutput {
            stdout: String::from_utf8_lossy(&process_output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&process_output.stderr).into_owned(),
            status_code: process_output.status.code(),
        };

        Ok(output)
    }
}

async fn read_stream<R>(mut reader: R, line_tx: mpsc::UnboundedSender<String>) -> String
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0_u8; 2048];
    let mut line = Vec::new();
    let mut output = String::new();
    loop {
        let count = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        for byte in &chunk[..count] {
            if matches!(*byte, b'\r' | b'\n') {
                finish_stream_line(&mut line, &mut output, &line_tx);
            } else if line.len() < MAX_DIAGNOSTIC_BYTES {
                line.push(*byte);
            }
        }
    }
    finish_stream_line(&mut line, &mut output, &line_tx);
    output
}

async fn read_bounded_bytes<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let retained = count.min(limit.saturating_sub(captured.len()));
        captured.extend_from_slice(&buffer[..retained]);
        exceeded |= retained != count;
        // Drain both pipes even after reaching the cap so the child cannot deadlock.
    }
    Ok((captured, exceeded))
}

fn finish_stream_line(
    line: &mut Vec<u8>,
    output: &mut String,
    line_tx: &mpsc::UnboundedSender<String>,
) {
    if line.is_empty() {
        return;
    }
    let safe = sanitize_git_output(&String::from_utf8_lossy(line));
    line.clear();
    append_bounded(output, &safe);
    let _ = line_tx.send(safe);
}

fn append_bounded(output: &mut String, line: &str) {
    if output.len() >= MAX_DIAGNOSTIC_BYTES {
        return;
    }
    if !output.is_empty() {
        output.push('\n');
    }
    let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(output.len());
    let mut end = remaining.min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&line[..end]);
}

fn parse_progress_line(line: &str) -> Option<GitProgressLine> {
    let safe = clip_utf8(&sanitize_git_output(line), MAX_PROGRESS_LINE_BYTES);
    let normalized = safe
        .strip_prefix("remote: ")
        .unwrap_or(&safe)
        .trim_start()
        .to_ascii_lowercase();
    let phase = [
        ("enumerating", GitProgressPhase::Enumerating),
        ("counting", GitProgressPhase::Counting),
        ("compressing", GitProgressPhase::Compressing),
        ("receiving", GitProgressPhase::Receiving),
        ("resolving", GitProgressPhase::Resolving),
        ("writing", GitProgressPhase::Writing),
        ("updating", GitProgressPhase::Updating),
    ]
    .into_iter()
    .find_map(|(prefix, phase)| normalized.starts_with(prefix).then_some(phase))?;
    Some(GitProgressLine { phase, text: safe })
}

fn clip_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn join_error(error: tokio::task::JoinError) -> BackendError {
    BackendError::new(ErrorCode::Unexpected, "Git 输出读取任务异常结束。")
        .with_diagnostics(error.to_string())
}

fn cancelled_error(message: &str) -> BackendError {
    BackendError::new(ErrorCode::Cancelled, message)
}

fn map_spawn_error(error: std::io::Error) -> BackendError {
    if error.kind() == ErrorKind::NotFound {
        BackendError::new(ErrorCode::GitNotFound, "未检测到 Git。")
            .with_diagnostics(error.to_string())
    } else {
        BackendError::from(error)
    }
}

fn combined_diagnostics(output: &GitOutput) -> String {
    match (output.stderr.trim(), output.stdout.trim()) {
        ("", stdout) => stdout.to_owned(),
        (stderr, "") => stderr.to_owned(),
        (stderr, stdout) => format!("{stderr}\n{stdout}"),
    }
}

fn classify_git_failure(diagnostics: &str) -> ErrorCode {
    let diagnostics = diagnostics.to_ascii_lowercase();

    if contains_any(
        &diagnostics,
        &[
            "authentication failed",
            "could not read username",
            "terminal prompts disabled",
            "permission denied (publickey)",
            "invalid username or password",
            "http basic: access denied",
        ],
    ) {
        ErrorCode::GitAuthentication
    } else if contains_any(
        &diagnostics,
        &[
            "could not resolve host",
            "failed to connect",
            "connection timed out",
            "connection reset",
            "network is unreachable",
            "unable to access",
            "ssl certificate problem",
        ],
    ) {
        ErrorCode::GitNetwork
    } else if contains_any(
        &diagnostics,
        &[
            "index.lock",
            "another git process seems to be running",
            "cannot lock ref",
        ],
    ) {
        ErrorCode::GitLocked
    } else if contains_any(
        &diagnostics,
        &[
            "merge conflict",
            "resolve your current index",
            "unmerged files",
            "needs merge",
            "automatic merge failed",
        ],
    ) {
        ErrorCode::GitConflict
    } else {
        ErrorCode::GitCommandFailed
    }
}

fn contains_any(value: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| value.contains(candidate))
}

fn error_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::GitAuthentication => "Git 身份验证失败。",
        ErrorCode::GitNetwork => "Git 网络请求失败。",
        ErrorCode::GitConflict => "Git 操作遇到冲突。",
        ErrorCode::GitLocked => "Git 仓库正被其他操作占用。",
        _ => "Git 命令执行失败。",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::domain::error::ErrorCode;

    #[test]
    fn invocation_passes_arguments_without_shell_interpolation() {
        let invocation = GitInvocation::new(["rev-parse", "--show-toplevel"]);

        assert_eq!(invocation.program(), "git");
        assert_eq!(
            invocation.args(),
            ["rev-parse", "--show-toplevel"].map(std::ffi::OsString::from)
        );
    }

    #[tokio::test]
    async fn runner_executes_git_in_the_requested_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let runner = GitCommandRunner::default();

        runner
            .run(Some(directory.path()), ["init", "-b", "main"])
            .await
            .unwrap();
        let output = runner
            .run(Some(directory.path()), ["rev-parse", "--show-toplevel"])
            .await
            .unwrap();

        assert_eq!(output.status_code, Some(0));
        assert_eq!(
            std::path::PathBuf::from(output.stdout.trim())
                .canonicalize()
                .unwrap(),
            directory.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn optional_locks_are_not_passed_as_an_unsupported_global_option() {
        let invocation = GitInvocation::new(["--no-optional-locks", "status", "--porcelain=v1"]);
        assert_eq!(
            invocation.args(),
            ["status", "--porcelain=v1"].map(OsString::from)
        );

        // A filename with the same spelling is data and must not be removed.
        let invocation = GitInvocation::new(["diff", "--", "--no-optional-locks"]);
        assert_eq!(
            invocation.args(),
            ["diff", "--", "--no-optional-locks"].map(OsString::from)
        );
    }

    #[test]
    fn optional_locks_environment_is_scoped_to_opted_in_queries() {
        for (args, expected) in [
            (vec!["--no-optional-locks", "status"], Some(Some("0"))),
            (vec!["status"], None),
            (vec!["add", "--", "--no-optional-locks"], None),
        ] {
            let invocation = GitInvocation::new(args);
            let mut command = Command::new(invocation.program());
            invocation.configure_command(&mut command);
            let value = command
                .as_std()
                .get_envs()
                .find(|(name, _)| *name == "GIT_OPTIONAL_LOCKS")
                .map(|(_, value)| value.and_then(OsStr::to_str));
            assert_eq!(value, expected);
        }
    }

    #[tokio::test]
    async fn optional_locks_environment_reaches_every_execution_path() {
        let directory = tempfile::tempdir().unwrap();
        let runner = GitCommandRunner::default();
        runner.run(Some(directory.path()), ["init"]).await.unwrap();
        // Git's shell alias reports only this fixed, non-secret variable.
        let args = [
            "--no-optional-locks",
            "-c",
            "alias.hq-lock-probe=!printf '%s' \"$GIT_OPTIONAL_LOCKS\"",
            "hq-lock-probe",
        ];
        let root = Some(directory.path());
        assert_eq!(runner.run(root, args).await.unwrap().stdout, "0");
        assert_eq!(runner.run_bytes(root, args, 16).await.unwrap(), b"0");
        assert_eq!(
            runner
                .run_with_input(root, args, Vec::new())
                .await
                .unwrap()
                .stdout,
            "0"
        );
        let no_editor = runner.run_without_editor(root, args).await.unwrap();
        assert!(no_editor.is_success());
        assert_eq!(no_editor.stdout, "0");
        let streaming = runner
            .run_streaming(
                root,
                GitInvocation::new(args),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(streaming.stdout, "0");
    }

    #[tokio::test]
    async fn runner_maps_an_unknown_non_zero_exit_to_git_command_failed() {
        let directory = tempfile::tempdir().unwrap();
        let runner = GitCommandRunner::default();

        let error = runner
            .run(Some(directory.path()), ["rev-parse", "--show-toplevel"])
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::GitCommandFailed);
        assert!(error.diagnostics.is_some());
    }

    #[tokio::test]
    async fn byte_output_preserves_invalid_utf8_and_nul() {
        let directory = tempfile::tempdir().unwrap();
        let runner = GitCommandRunner::default();
        runner.run(Some(directory.path()), ["init"]).await.unwrap();
        let bytes = vec![0, 0xff, 0xfe, b'\r', b'\n', 42];
        let object = runner
            .run_with_input(
                Some(directory.path()),
                ["hash-object", "-w", "--stdin"],
                bytes.clone(),
            )
            .await
            .unwrap();
        let actual = runner
            .run_bytes(
                Some(directory.path()),
                ["cat-file", "blob", object.stdout.trim()],
                bytes.len(),
            )
            .await
            .unwrap();
        assert_eq!(actual, bytes);
    }

    #[tokio::test]
    async fn byte_output_rejects_limits_and_git_failures() {
        let directory = tempfile::tempdir().unwrap();
        let runner = GitCommandRunner::default();
        runner.run(Some(directory.path()), ["init"]).await.unwrap();
        let object = runner
            .run_with_input(
                Some(directory.path()),
                ["hash-object", "-w", "--stdin"],
                vec![42; 100_000],
            )
            .await
            .unwrap();
        let error = runner
            .run_bytes(
                Some(directory.path()),
                ["cat-file", "blob", object.stdout.trim()],
                32,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::GitCommandFailed);
        assert!(error.message.contains("32"));
        assert!(
            runner
                .run_bytes(Some(directory.path()), ["cat-file", "blob", "missing"], 32)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn allowing_failure_preserves_non_zero_git_output() {
        let directory = tempfile::tempdir().unwrap();
        let output = GitCommandRunner::default()
            .run_allowing_failure(Some(directory.path()), ["rev-parse", "--verify", "missing"])
            .await
            .unwrap();

        assert!(!output.is_success());
        assert_ne!(output.status_code, Some(0));
        assert!(!output.stderr.is_empty());
    }

    #[tokio::test]
    async fn runner_applies_patch_from_stdin() {
        let directory = tempfile::tempdir().unwrap();
        let runner = GitCommandRunner::default();
        runner
            .run(Some(directory.path()), ["init", "-b", "main"])
            .await
            .unwrap();
        runner
            .run(Some(directory.path()), ["config", "user.name", "HQ Test"])
            .await
            .unwrap();
        runner
            .run(
                Some(directory.path()),
                ["config", "user.email", "hq@example.test"],
            )
            .await
            .unwrap();
        std::fs::write(directory.path().join("note.txt"), "one\ntwo\n").unwrap();
        runner
            .run(Some(directory.path()), ["add", "note.txt"])
            .await
            .unwrap();
        runner
            .run(Some(directory.path()), ["commit", "-m", "base"])
            .await
            .unwrap();
        std::fs::write(directory.path().join("note.txt"), "one\nchanged\n").unwrap();
        let patch = runner
            .run(Some(directory.path()), ["diff", "--", "note.txt"])
            .await
            .unwrap()
            .stdout;

        runner
            .run_with_input(
                Some(directory.path()),
                ["apply", "--cached", "-"],
                patch.into_bytes(),
            )
            .await
            .unwrap();

        let cached = runner
            .run(
                Some(directory.path()),
                ["diff", "--cached", "--", "note.txt"],
            )
            .await
            .unwrap();
        assert!(cached.stdout.contains("+changed"));
    }

    #[test]
    fn failure_classifier_recognizes_actionable_git_errors() {
        let cases = [
            ("fatal: Authentication failed", ErrorCode::GitAuthentication),
            (
                "fatal: unable to access host: Could not resolve host",
                ErrorCode::GitNetwork,
            ),
            (
                "fatal: Unable to create '.git/index.lock': File exists",
                ErrorCode::GitLocked,
            ),
            (
                "error: you need to resolve your current index first",
                ErrorCode::GitConflict,
            ),
            ("fatal: ambiguous argument", ErrorCode::GitCommandFailed),
        ];

        for (diagnostics, expected) in cases {
            assert_eq!(classify_git_failure(diagnostics), expected);
        }
    }

    #[tokio::test]
    async fn streaming_runner_honors_pre_cancelled_token() {
        let directory = tempfile::tempdir().unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();

        let error = GitCommandRunner::default()
            .run_streaming(
                Some(directory.path()),
                GitInvocation::new(["status"]),
                cancellation,
                move |progress| captured.lock().unwrap().push(progress),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn progress_parser_exposes_only_normalized_git_phases() {
        let progress = [
            "remote: Enumerating objects: 10, done.",
            "Receiving objects:  50% (5/10)",
            "Resolving deltas: 100% (3/3), done.",
            "fatal: https://user:secret@example.test/repo.git?token=secret",
        ]
        .into_iter()
        .filter_map(parse_progress_line)
        .collect::<Vec<_>>();

        assert_eq!(
            progress.iter().map(|line| line.phase).collect::<Vec<_>>(),
            [
                GitProgressPhase::Enumerating,
                GitProgressPhase::Receiving,
                GitProgressPhase::Resolving,
            ]
        );
        assert!(progress.iter().all(|line| !line.text.contains("secret")));
    }

    #[test]
    fn remote_diagnostics_remove_credentials_and_bound_output() {
        let raw =
            "https://user:secret@example.test/repo.git?token=secret&name=safe\n".repeat(5_000);

        let safe = sanitize_git_output(&raw);

        assert!(!safe.contains("user:secret"));
        assert!(!safe.contains("token=secret"));
        assert!(safe.len() <= MAX_DIAGNOSTIC_BYTES);
    }

    #[test]
    fn continuation_runner_overrides_inherited_editor() {
        const CHILD_FLAG: &str = "HQ_GIT_EDITOR_TEST_CHILD";
        if std::env::var_os(CHILD_FLAG).is_some() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let result = GitCommandRunner::default()
                    .run_without_editor(
                        None,
                        ["-c", "core.editor=configured-editor", "var", "GIT_EDITOR"],
                    )
                    .await
                    .unwrap();
                assert!(result.is_success());
                assert_eq!(result.stdout.trim(), "true");
            });
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "infrastructure::git_runner::tests::continuation_runner_overrides_inherited_editor",
                "--nocapture",
            ])
            .env(CHILD_FLAG, "1")
            .env("GIT_EDITOR", "inherited-editor-must-not-launch")
            .env(
                "GIT_SEQUENCE_EDITOR",
                "inherited-sequence-editor-must-not-launch",
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
