use std::path::Path;

use crate::application::operation_state::read_operation_state;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::operation::RepositoryOperationKind;
use crate::infrastructure::git_runner::GitCommandRunner;

/// Caller holds the repository mutation lock. Backups are ordinary, durable Git
/// stashes, so they remain recoverable even if the app exits during a pick.
pub(super) struct CherryPickWorktree {
    stash: Option<String>,
    source: String,
}

impl CherryPickWorktree {
    pub async fn save(root: &Path, runner: &GitCommandRunner) -> Result<Self, BackendError> {
        let staged = runner
            .run_allowing_failure(Some(root), ["diff", "--cached", "--quiet", "--exit-code"])
            .await?;
        if !staged.is_success() {
            if staged.status_code != Some(1) {
                staged.into_result()?;
            }
            return Err(BackendError::new(
                ErrorCode::DirtyWorktree,
                "存在已暂存变更，请先提交或取消暂存；Cherry-pick 只自动保留未暂存变更。",
            ));
        }
        let branch = runner
            .run(Some(root), ["rev-parse", "--abbrev-ref", "HEAD"])
            .await?;
        let source = branch.stdout.trim().to_owned();
        let mut saved = Self {
            stash: None,
            source,
        };
        let status = runner
            .run(
                Some(root),
                ["status", "--porcelain=v1", "--untracked-files=all"],
            )
            .await?;
        if status.stdout.is_empty() {
            return Ok(saved);
        }
        let label = format!(
            "hq-git auto-save before cherry-pick [{}] {}",
            saved.source,
            uuid::Uuid::new_v4()
        );
        let output = runner
            .run_allowing_failure(
                Some(root),
                ["stash", "push", "--include-untracked", "-m", &label],
            )
            .await?;
        // Identify our own stash, never an older user stash (including when push
        // saved successfully but failed while cleaning the working directory).
        let top = runner
            .run_allowing_failure(Some(root), ["log", "-1", "--format=%H%n%s", "refs/stash"])
            .await?;
        if top.is_success() && top.stdout.lines().skip(1).any(|line| line.contains(&label)) {
            saved.stash = top.stdout.lines().next().map(str::to_owned);
        }
        if !output.is_success() {
            let error = output.into_result().unwrap_err();
            return Err(saved.retained(error));
        }
        let status = runner
            .run(
                Some(root),
                ["status", "--porcelain=v1", "--untracked-files=all"],
            )
            .await?;
        if saved.stash.is_none() || !status.stdout.is_empty() {
            return Err(saved.retained(BackendError::new(
                ErrorCode::DirtyWorktree,
                "未能完整贮藏本地变更，已停止移植；请检查工作区和贮藏列表。",
            )));
        }
        Ok(saved)
    }

    fn retained(&self, mut error: BackendError) -> BackendError {
        if let Some(stash) = &self.stash {
            error.message = format!(
                "{} 未暂存变更的自动贮藏备份仍保留（{}，原分支 {}）。请先处理当前冲突或失败，再在贮藏列表中恢复该备份；不要重复移植已成功的提交。",
                error.message, stash, self.source
            );
        }
        error
    }

    pub async fn finish<T>(
        self,
        root: &Path,
        runner: &GitCommandRunner,
        outcome: Result<T, BackendError>,
    ) -> Result<T, BackendError> {
        let Some(stash) = &self.stash else {
            return outcome;
        };
        let state = read_operation_state(root, runner)
            .await
            .map_err(|e| self.retained(e))?;
        if state.kind != RepositoryOperationKind::None || !state.conflicts.is_empty() {
            return Err(self.retained(outcome.err().unwrap_or_else(|| {
                BackendError::new(ErrorCode::GitConflict, "移植尚未结束，暂不恢复未暂存变更。")
            })));
        }
        let restored = runner
            .run(Some(root), ["stash", "apply", stash.as_str()])
            .await;
        if let Err(error) = restored {
            let message = match &outcome {
                Ok(_) => "移植操作已完成，但未暂存变更恢复失败。",
                Err(_) => "移植操作失败，且未暂存变更未能自动恢复。",
            };
            return Err(
                self.retained(
                    BackendError::new(error.code, message).with_diagnostics(format!(
                        "{}\n{}",
                        outcome.err().map(|e| e.to_string()).unwrap_or_default(),
                        error.diagnostics.unwrap_or(error.message)
                    )),
                ),
            );
        }
        // Drop only our own top entry after a successful apply. If an external
        // Git client added a newer stash, leave the backup instead of deleting it.
        let top = runner
            .run(Some(root), ["rev-parse", "refs/stash"])
            .await
            .map_err(|e| restored_backup_error(e, stash, outcome.as_ref().err()))?;
        if top.stdout.trim() == stash {
            runner
                .run(Some(root), ["stash", "drop", "stash@{0}"])
                .await
                .map_err(|e| restored_backup_error(e, stash, outcome.as_ref().err()))?;
        }
        outcome
    }
}

fn restored_backup_error(
    error: BackendError,
    stash: &str,
    operation_error: Option<&BackendError>,
) -> BackendError {
    BackendError::new(error.code, format!(
        "{}未暂存变更已恢复，但自动贮藏备份清理失败（{}）；请核对贮藏列表，不要重复应用该备份。",
        operation_error.map(|e| format!("{} ", e.message)).unwrap_or_else(|| "移植操作已完成。".into()), stash,
    )).with_diagnostics(error.diagnostics.unwrap_or(error.message))
}
