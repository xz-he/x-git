//! Small, bounded, read-only Git transport. All argument vectors are application-owned.
use crate::domain::error::{BackendError, ErrorCode};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

pub(crate) fn command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command.env_clear();
    for name in ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .current_dir(root)
        .args([
            "--no-pager",
            "--no-replace-objects",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "core.attributesFile=",
            "-c",
            "credential.helper=",
            "-c",
            "protocol.allow=never",
            "-c",
            "diff.external=",
            "-c",
            "core.quotePath=true",
        ])
        .arg("-c")
        .arg(format!("safe.directory={}", root.display()))
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

pub(crate) async fn bytes(
    root: &Path,
    args: &[&str],
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, BackendError> {
    let (success, output) = optional_bytes(root, args, limit, cancel).await?;
    if !success {
        return Err(BackendError::new(
            ErrorCode::GitCommandFailed,
            "无法读取冻结的 Git 对象；对象可能缺失，请检查本地仓库后重试。",
        ));
    }
    Ok(output)
}

pub(crate) async fn optional_bytes(
    root: &Path,
    args: &[&str],
    limit: usize,
    cancel: &CancellationToken,
) -> Result<(bool, Vec<u8>), BackendError> {
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    let mut child = command(root, args)
        .spawn()
        .map_err(|_| BackendError::new(ErrorCode::GitNotFound, "无法启动 Git。"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BackendError::new(ErrorCode::Unexpected, "无法读取 Git 输出。"))?;
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(cancelled()),
        _ = tokio::time::sleep(Duration::from_secs(60)) => Err(BackendError::new(ErrorCode::AiTimeout, "读取 Git 对象超时。")),
        result = async {
            let mut output = Vec::new();
            stdout.take((limit + 1) as u64).read_to_end(&mut output).await.map_err(|_| BackendError::new(ErrorCode::Io, "无法读取 Git 对象。"))?;
            if output.len() > limit { return Err(BackendError::new(ErrorCode::AiContextTooLarge, "Git 上下文超过读取限制。")); }
            let status = child.wait().await.map_err(|_| BackendError::new(ErrorCode::Io, "无法等待 Git 读取结束。"))?;
            Ok((status.success(), output))
        } => result,
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

pub(crate) fn cancelled() -> BackendError {
    BackendError::new(ErrorCode::Cancelled, "AI 审查已取消。")
}
