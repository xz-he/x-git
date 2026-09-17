use super::terminal_job::Job;
use crate::domain::error::{BackendError, ErrorCode};
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
pub const CHUNK_SIZE: usize = 8192;
pub const LOCATION_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
    "GIT_IMPLICIT_WORK_TREE",
];
pub struct Controls {
    pub master: Mutex<Box<dyn MasterPty + Send>>,
    pub writer: Mutex<Box<dyn Write + Send>>,
}
pub struct PtyResult {
    pub exit_code: Option<i32>,
    pub error: Option<BackendError>,
}
pub fn failure(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(ErrorCode::GitCommandFailed, "Git 终端执行失败。")
        .with_diagnostics(error.to_string())
}
pub fn size(cols: u16, rows: u16) -> Result<PtySize, BackendError> {
    if !(2..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
        return Err(BackendError::new(
            ErrorCode::InvalidConsoleCommand,
            "终端尺寸超出范围。",
        ));
    }
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}
pub fn git_executable() -> Result<PathBuf, BackendError> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path).filter(|path| path.is_absolute()) {
        let candidate = directory.join(if cfg!(windows) { "git.exe" } else { "git" });
        if candidate.is_file() {
            return candidate.canonicalize().map_err(BackendError::from);
        }
    }
    Err(BackendError::new(
        ErrorCode::GitNotFound,
        "未在 PATH 中找到 Git。",
    ))
}
pub fn run(
    root: &Path,
    args: &[String],
    dimensions: PtySize,
    cancellation: CancellationToken,
    controls_slot: Arc<Mutex<Option<Arc<Controls>>>>,
    output: mpsc::Sender<Vec<u8>>,
) -> PtyResult {
    let execute = || -> Result<Option<i32>, BackendError> {
        if cancellation.is_cancelled() {
            return Ok(None);
        }
        let pair = portable_pty::native_pty_system()
            .openpty(dimensions)
            .map_err(failure)?;
        let mut reader = pair.master.try_clone_reader().map_err(failure)?;
        let writer = pair.master.take_writer().map_err(failure)?;
        let job = Arc::new(Job::new().map_err(failure)?);
        let mut command = CommandBuilder::new(git_executable()?);
        command.args(args);
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        // portable-pty consults the registry on Windows; explicitly restore the
        // app environment so editor, pager, helper and Git config choices survive.
        for (name, value) in std::env::vars_os() {
            command.env(name, value);
        }
        command.env("TERM", "xterm-256color");
        for name in LOCATION_ENV {
            command.env_remove(name);
        }
        job.configure(&mut command);
        let mut child = pair.slave.spawn_command(command).map_err(failure)?;
        drop(pair.slave);
        let controls = Arc::new(Controls {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
        });
        *controls_slot.lock().unwrap() = Some(controls.clone());
        // Even if the event receiver is cancelled, keep draining until ConPTY
        // closes: ClosePseudoConsole can block if its output pipe is not drained.
        let read_thread = std::thread::spawn(move || {
            let mut buffer = [0u8; CHUNK_SIZE];
            let mut deliver = true;
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => return Ok(()),
                    Ok(count) => {
                        if deliver && output.blocking_send(buffer[..count].to_vec()).is_err() {
                            deliver = false;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => return Ok(()),
                    Err(error) => return Err(failure(error)),
                }
            }
        });
        let mut wait_error = None;
        let exit_code = loop {
            if cancellation.is_cancelled() {
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status.exit_code() as i32),
                Ok(None) => {}
                Err(error) => {
                    wait_error = Some(failure(error));
                    break None;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // A successful Git exit may leave background helpers behind. Always
        // reap the job tree before releasing the repository mutation guard.
        let mut cleanup_error = job.terminate().err().map(failure);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match job.active() {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) => {
                    cleanup_error = Some(failure(error));
                }
            }
            if Instant::now() >= deadline {
                cleanup_error = Some(failure("等待 Git 进程树退出超时；仓库仍被终端占用。"));
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // On an exceptional OS cleanup failure, do not falsely release the
        // mutation lock. The worker keeps ownership while retrying containment.
        if cleanup_error.is_some() {
            while job.active().unwrap_or(1) != 0 {
                let _ = job.terminate();
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let _ = child.wait();
        controls_slot.lock().unwrap().take();
        drop(controls);
        match read_thread.join() {
            Ok(Err(error)) => wait_error = Some(error),
            Err(_) => wait_error = Some(failure("终端输出读取线程异常结束。")),
            Ok(Ok(())) => {}
        }
        if let Some(error) = cleanup_error.or(wait_error) {
            return Err(error);
        }
        Ok(exit_code)
    };
    match execute() {
        Ok(exit_code) => PtyResult {
            exit_code,
            error: None,
        },
        Err(error) => PtyResult {
            exit_code: None,
            error: Some(error),
        },
    }
}
