use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::domain::error::{sanitize_git_output, BackendError, ErrorCode};

const DEFAULT_SVN_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvnOutput {
    pub stdout: String,
    pub stderr: String,
    pub status_code: Option<i32>,
}

impl SvnOutput {
    pub fn is_success(&self) -> bool {
        self.status_code == Some(0)
    }

    fn into_result(self) -> Result<Self, BackendError> {
        if self.is_success() {
            return Ok(self);
        }
        let diagnostics = sanitize_git_output(&format!("{}\n{}", self.stdout, self.stderr));
        Err(BackendError::new(ErrorCode::GitCommandFailed, "SVN 命令执行失败。")
            .with_diagnostics(diagnostics))
    }
}

#[derive(Debug, Clone)]
pub struct SvnCommandRunner {
    timeout: Duration,
}

impl Default for SvnCommandRunner {
    fn default() -> Self {
        Self { timeout: DEFAULT_SVN_TIMEOUT }
    }
}

impl SvnCommandRunner {
    pub async fn run<I, S>(&self, working_directory: Option<&Path>, args: I) -> Result<SvnOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.execute(working_directory, args).await?.into_result()
    }

    pub async fn run_allowing_failure<I, S>(
        &self,
        working_directory: Option<&Path>,
        args: I,
    ) -> Result<SvnOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.execute(working_directory, args).await
    }

    async fn execute<I, S>(&self, working_directory: Option<&Path>, args: I) -> Result<SvnOutput, BackendError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new("svn");
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        if let Some(directory) = working_directory {
            command.current_dir(directory);
        }
        let output = timeout(self.timeout, command.output())
            .await
            .map_err(|_| BackendError::new(ErrorCode::Cancelled, "SVN 命令执行超时。"))?
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    BackendError::new(ErrorCode::GitNotFound, "未检测到 SVN 命令。")
                } else {
                    BackendError::from(error)
                }
            })?;
        Ok(SvnOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status_code: output.status.code(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_output_is_classified_as_command_failure() {
        let error = SvnOutput { stdout: String::new(), stderr: "not a working copy".into(), status_code: Some(1) }
            .into_result()
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::GitCommandFailed);
        assert!(error.diagnostics.unwrap().contains("not a working copy"));
    }
}
