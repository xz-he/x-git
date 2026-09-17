//! Dedicated console runner: fixed executable, bounded separate pipes, no shell.
use super::console_output::ConsoleOutput;
use crate::domain::{
    console::{ConsoleOutcome, ConsoleStream},
    error::{BackendError, ErrorCode},
};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::mpsc,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub const STREAM_LIMIT: usize = 512 * 1024;
pub const READ_CHUNK: usize = 8 * 1024;
const QUEUE_BLOCKS: usize = 32;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug)]
pub struct ConsoleRunResult {
    pub outcome: ConsoleOutcome,
    pub exit_code: Option<i32>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub error: Option<BackendError>,
    pub captured_stdout: Vec<u8>,
}
impl ConsoleRunResult {
    pub fn failed(error: BackendError) -> Self {
        Self {
            outcome: ConsoleOutcome::Failed,
            exit_code: None,
            stdout_truncated: false,
            stderr_truncated: false,
            error: Some(error),
            captured_stdout: Vec::new(),
        }
    }
    pub fn interrupted(timed_out: bool) -> Self {
        let mut result = Self::failed(BackendError::new(
            ErrorCode::Cancelled,
            if timed_out {
                "Git 查询已超时。"
            } else {
                "Git 查询已取消。"
            },
        ));
        result.outcome = if timed_out {
            ConsoleOutcome::TimedOut
        } else {
            ConsoleOutcome::Cancelled
        };
        result
    }
}

pub(crate) fn command(root: &Path, args: &[String]) -> Command {
    let mut command = Command::new("git");
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command.args(["--no-pager", "--literal-pathspecs"]);
    for config in [
        "color.ui=false",
        "color.status=false",
        "color.branch=false",
        "color.diff=false",
        "color.decorate=false",
        "color.pager=false",
        "core.pager=",
        "core.fsmonitor=false",
        "maintenance.auto=false",
        "gc.auto=0",
        "log.showSignature=false",
        "format.pretty=medium",
        "diff.submodule=short",
    ] {
        command.args(["-c", config]);
    }
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
    ] {
        command.env_remove(name);
    }
    for (name, value) in [
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_OPTIONAL_LOCKS", "0"),
        ("GIT_NO_LAZY_FETCH", "1"),
        ("GIT_PAGER", ""),
        ("PAGER", ""),
        ("GIT_EDITOR", ""),
        ("GIT_SEQUENCE_EDITOR", ""),
        ("GIT_LITERAL_PATHSPECS", "1"),
    ] {
        command.env(name, value);
    }
    command
}

pub(super) async fn read_pipe<R: AsyncRead + Unpin>(
    mut reader: R,
    stream: ConsoleStream,
    tx: mpsc::Sender<(ConsoleStream, Vec<u8>)>,
    truncated: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let mut buffer = [0u8; READ_CHUNK];
    let mut retained = 0;
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        let keep = count.min(STREAM_LIMIT.saturating_sub(retained));
        retained += keep;
        if count > keep {
            truncated.store(true, Ordering::Relaxed);
        }
        if keep > 0 && tx.send((stream, buffer[..keep].to_vec())).await.is_err() {
            return Ok(());
        }
        // Continue draining after the cap; never wait for a newline to enforce it.
    }
}

pub async fn run<F: FnMut(ConsoleStream, String)>(
    root: &Path,
    args: &[String],
    cancellation: &CancellationToken,
    deadline: Instant,
    capture_stdout: bool,
    mut emit: F,
) -> ConsoleRunResult {
    if cancellation.is_cancelled() {
        return ConsoleRunResult::interrupted(false);
    }
    if Instant::now() >= deadline {
        return ConsoleRunResult::interrupted(true);
    }
    let mut child = match command(root, args).spawn() {
        Ok(child) => child,
        Err(error) => {
            return ConsoleRunResult::failed(if error.kind() == std::io::ErrorKind::NotFound {
                BackendError::new(ErrorCode::GitNotFound, "未检测到 Git。")
            } else {
                BackendError::from(error)
            });
        }
    };
    let (tx, mut rx) = mpsc::channel(QUEUE_BLOCKS);
    let stdout_truncated = Arc::new(AtomicBool::new(false));
    let stderr_truncated = Arc::new(AtomicBool::new(false));
    let stdout_task = tokio::spawn(read_pipe(
        child.stdout.take().expect("piped stdout"),
        ConsoleStream::Stdout,
        tx.clone(),
        stdout_truncated.clone(),
    ));
    let stderr_task = tokio::spawn(read_pipe(
        child.stderr.take().expect("piped stderr"),
        ConsoleStream::Stderr,
        tx,
        stderr_truncated.clone(),
    ));
    let mut stdout = ConsoleOutput::default();
    let mut stderr = ConsoleOutput::default();
    let mut captured_stdout = Vec::new();
    let mut status = None;
    let mut eof = false;
    let mut interrupted = None;
    let mut io_error = None;
    while status.is_none() || !eof {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => { interrupted = Some(false); break; }
            _ = tokio::time::sleep_until(deadline) => { interrupted = Some(true); break; }
            result = child.wait(), if status.is_none() => match result { Ok(value) => status = Some(value), Err(error) => { io_error = Some(BackendError::from(error)); break; } },
            block = rx.recv(), if !eof => match block {
                Some((stream, bytes)) => {
                    if capture_stdout && stream == ConsoleStream::Stdout { captured_stdout.extend_from_slice(&bytes); }
                    let text = if stream == ConsoleStream::Stdout { stdout.push(&bytes) } else { stderr.push(&bytes) };
                    emit_text(stream, text, &mut emit);
                }
                None => eof = true,
            }
        }
    }
    if interrupted.is_some() || io_error.is_some() {
        let _ = child.start_kill();
        // Reap explicitly; kill_on_drop is an additional guard, not our cleanup protocol.
        let _ = child.wait().await;
        stdout_task.abort();
        stderr_task.abort();
        let _ = tokio::join!(stdout_task, stderr_task);
    } else {
        for joined in [stdout_task.await, stderr_task.await] {
            match joined {
                Ok(Ok(())) => {}
                Ok(Err(error)) => io_error = Some(error.into()),
                Err(_) => io_error = Some(BackendError::new(ErrorCode::Io, "无法读取 Git 输出。")),
            }
        }
        emit_text(
            ConsoleStream::Stdout,
            stdout.finish(stdout_truncated.load(Ordering::Relaxed)),
            &mut emit,
        );
        emit_text(
            ConsoleStream::Stderr,
            stderr.finish(stderr_truncated.load(Ordering::Relaxed)),
            &mut emit,
        );
    }
    let exit_code = status.and_then(|value| value.code());
    let mut result = if let Some(timed_out) = interrupted {
        ConsoleRunResult::interrupted(timed_out)
    } else if let Some(error) = io_error {
        ConsoleRunResult::failed(error)
    } else if exit_code == Some(0) {
        ConsoleRunResult {
            outcome: ConsoleOutcome::Completed,
            exit_code,
            stdout_truncated: false,
            stderr_truncated: false,
            error: None,
            captured_stdout: Vec::new(),
        }
    } else {
        ConsoleRunResult::failed(BackendError::new(
            ErrorCode::GitCommandFailed,
            "Git 查询失败，请查看输出。",
        ))
    };
    result.exit_code = exit_code;
    result.stdout_truncated = stdout_truncated.load(Ordering::Relaxed);
    result.stderr_truncated = stderr_truncated.load(Ordering::Relaxed);
    result.captured_stdout = captured_stdout;
    result
}

fn emit_text<F: FnMut(ConsoleStream, String)>(stream: ConsoleStream, text: String, emit: &mut F) {
    let mut remaining = text.as_str();
    while !remaining.is_empty() {
        let mut end = remaining.len().min(READ_CHUNK);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        emit(stream, remaining[..end].to_owned());
        remaining = &remaining[end..];
    }
}
