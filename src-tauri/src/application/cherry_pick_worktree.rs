use std::path::Path;

use crate::application::operation_state::read_operation_state;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::operation::RepositoryOperationKind;
use crate::infrastructure::git_runner::GitCommandRunner;

#[path = "cherry_pick_residuals.rs"]
mod residuals;

/// Caller holds the repository mutation lock. Backups are ordinary, durable Git
/// stashes, so they remain recoverable even if the app exits during a pick.
pub(super) struct CherryPickWorktree {
    stash: Option<String>,
    source: String,
}

impl CherryPickWorktree {
    pub async fn save(
        root: &Path,
        runner: &GitCommandRunner,
        target: &str,
        commit: &str,
    ) -> Result<Self, BackendError> {
        let staged = runner
            .run_allowing_failure(
                Some(root),
                [
                    "diff",
                    "--cached",
                    "--quiet",
                    "--exit-code",
                    "--ignore-submodules=none",
                ],
            )
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
        let status = residuals::inspect(root, runner, target, Some(commit))
            .await
            .map_err(|error| saved.save_failed(error))?;
        // No stash is needed when the only changes are independent repositories
        // that the switch and pick will not touch.
        if status.ordinary.is_empty() {
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
                [
                    "-c",
                    "submodule.recurse=false",
                    "stash",
                    "push",
                    "--include-untracked",
                    "-m",
                    &label,
                ],
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
            return Err(saved.save_failed(error));
        }
        let status = residuals::inspect(root, runner, target, Some(commit))
            .await
            .map_err(|error| saved.save_failed(error))?;
        if saved.stash.is_none() || !status.ordinary.is_empty() {
            let reason = if saved.stash.is_none() {
                "未确认到本次自动贮藏备份，已停止移植。"
            } else {
                "未能完整贮藏本地变更，工作区仍有残留，请查看详情中的文件列表。"
            };
            return Err(saved.save_failed(
                BackendError::new(ErrorCode::DirtyWorktree, reason).with_diagnostics(format!(
                    "贮藏后的剩余 Git 状态（前两列为文件状态）：\n{}\n\nGit stash 输出：\n{}\n{}\n\n普通 stash 不会收走嵌套 Git 仓库或子模块内部的修改；持续写入的文件、换行转换或过滤规则也可能导致残留。请根据上方路径检查，勿直接删除残留文件。",
                    if status.ordinary.is_empty() { "（无）".to_owned() } else { status.ordinary.join("\n") },
                    output.stdout.trim_end(),
                    output.stderr.trim_end(),
                )),
            ));
        }
        Ok(saved)
    }

    pub async fn ensure_safe_return(
        root: &Path,
        runner: &GitCommandRunner,
        target: &str,
    ) -> Result<(), BackendError> {
        let status =
            residuals::inspect(root, runner, &format!("refs/heads/{target}"), None).await?;
        if !status.ordinary.is_empty() {
            return Err(BackendError::new(
                ErrorCode::DirtyWorktree,
                "已保留提交；请处理剩余未暂存或未跟踪变更，再返回开发分支。",
            )
            .with_diagnostics(status.ordinary.join("\n")));
        }
        Ok(())
    }

    fn save_failed(&self, mut error: BackendError) -> BackendError {
        error.message.push_str(" 本次尚未切换分支或执行 Cherry-pick；如果使用了“提交并移植”，前面的本地提交可能已完成，请核对任务状态。");
        if let Some(stash) = &self.stash {
            error.message.push_str(&format!(
                " 未暂存变更的自动贮藏备份仍保留（{}，原分支 {}）。请先检查残留文件与贮藏内容，再决定恢复备份或继续移植。",
                stash, self.source,
            ));
        }
        error
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
            .run(
                Some(root),
                [
                    "-c",
                    "submodule.recurse=false",
                    "stash",
                    "apply",
                    stash.as_str(),
                ],
            )
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn git(root: &Path, args: &[&str]) -> String {
        GitCommandRunner::default()
            .run(Some(root), args)
            .await
            .unwrap()
            .stdout
    }

    async fn init(root: &Path) {
        git(root, &["init", "-b", "main"]).await;
        git(root, &["config", "user.name", "Auto stash test"]).await;
        git(root, &["config", "user.email", "test@example.test"]).await;
        git(root, &["config", "core.autocrlf", "false"]).await;
        std::fs::write(root.join("tracked.txt"), "base\n").unwrap();
        git(root, &["add", "tracked.txt"]).await;
        git(root, &["commit", "-m", "base"]).await;
    }

    #[tokio::test]
    async fn auto_stash_preserves_unrelated_nested_work_and_restores_ordinary_files() {
        for submodule in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path();
            init(root).await;
            let nested = root.join("nested-repository");
            std::fs::create_dir(&nested).unwrap();
            init(&nested).await;
            if submodule {
                let commit = git(&nested, &["rev-parse", "HEAD"]).await;
                git(
                    root,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("160000,{},nested-repository", commit.trim()),
                    ],
                )
                .await;
                git(root, &["commit", "-m", "track nested repository"]).await;
            }
            git(root, &["commit", "--allow-empty", "-m", "unrelated pick"]).await;
            std::fs::write(nested.join("tracked.txt"), "nested local work\n").unwrap();
            std::fs::write(root.join("tracked.txt"), "parent local work\n").unwrap();
            let head = git(root, &["rev-parse", "HEAD"]).await;
            let runner = GitCommandRunner::default();
            let saved = CherryPickWorktree::save(root, &runner, "HEAD", "HEAD")
                .await
                .unwrap();
            assert!(saved.stash.is_some());
            assert_eq!(
                git(root, &["show", "stash@{0}:tracked.txt"]).await,
                "parent local work\n"
            );
            assert_eq!(
                std::fs::read(nested.join("tracked.txt")).unwrap(),
                b"nested local work\n"
            );
            assert_eq!(git(root, &["rev-parse", "HEAD"]).await, head);
            saved.finish(root, &runner, Ok(())).await.unwrap();
            assert_eq!(
                std::fs::read(root.join("tracked.txt")).unwrap(),
                b"parent local work\n"
            );
            assert_eq!(git(root, &["stash", "list"]).await, "");
        }
    }

    #[tokio::test]
    async fn changed_gitlink_is_not_hidden_by_ignore_submodules_config() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        init(root).await;
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        init(&nested).await;
        let base = git(&nested, &["rev-parse", "HEAD"]).await;
        git(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},nested", base.trim()),
            ],
        )
        .await;
        git(root, &["commit", "-m", "submodule base"]).await;
        git(root, &["branch", "source"]).await;
        git(
            &nested,
            &["commit", "--allow-empty", "-m", "next nested commit"],
        )
        .await;
        let next = git(&nested, &["rev-parse", "HEAD"]).await;
        git(
            root,
            &[
                "update-index",
                "--cacheinfo",
                &format!("160000,{},nested", next.trim()),
            ],
        )
        .await;
        git(root, &["commit", "-m", "move gitlink"]).await;
        let overlapping = git(root, &["rev-parse", "HEAD"]).await;
        git(root, &["switch", "source"]).await;
        git(root, &["commit", "--allow-empty", "-m", "unrelated"]).await;
        git(root, &["config", "diff.ignoreSubmodules", "all"]).await;
        git(root, &["config", "submodule.nested.ignore", "all"]).await;
        std::fs::write(nested.join("tracked.txt"), "nested local\n").unwrap();
        for (target, commit) in [(overlapping.trim(), "HEAD"), ("HEAD", overlapping.trim())] {
            let error =
                match CherryPickWorktree::save(root, &GitCommandRunner::default(), target, commit)
                    .await
                {
                    Err(error) => error,
                    Ok(_) => {
                        panic!("changed gitlink must block even when Git hides submodule diffs")
                    }
                };
            assert_eq!(error.code, ErrorCode::DirtyWorktree);
            assert!(error.diagnostics.unwrap().contains("nested"));
            assert_eq!(git(root, &["stash", "list"]).await, "");
            assert_eq!(
                std::fs::read(nested.join("tracked.txt")).unwrap(),
                b"nested local\n"
            );
        }
    }

    #[tokio::test]
    async fn nested_only_skips_stash_and_overlap_blocks_before_saving() {
        for overlap_in_commit in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path();
            init(root).await;
            git(root, &["branch", "source"]).await;
            std::fs::create_dir(root.join("嵌套 repo")).unwrap();
            std::fs::write(root.join("嵌套 repo/tracked.txt"), "target\n").unwrap();
            git(root, &["add", "."]).await;
            git(root, &["commit", "-m", "overlapping target"]).await;
            let overlapping = git(root, &["rev-parse", "HEAD"]).await;
            git(root, &["switch", "source"]).await;
            git(root, &["commit", "--allow-empty", "-m", "unrelated"]).await;
            std::fs::create_dir_all(root.join("嵌套 repo")).unwrap();
            init(&root.join("嵌套 repo")).await;
            std::fs::write(root.join("嵌套 repo/tracked.txt"), "nested local\n").unwrap();
            std::fs::write(root.join("tracked.txt"), "old user stash\n").unwrap();
            git(root, &["stash", "push", "-m", "user stash"]).await;
            let stash = git(root, &["stash", "list", "--format=%H"]).await;
            let runner = GitCommandRunner::default();
            let saved = CherryPickWorktree::save(root, &runner, "HEAD", "HEAD")
                .await
                .unwrap();
            assert!(saved.stash.is_none());
            saved.finish(root, &runner, Ok(())).await.unwrap();
            std::fs::write(root.join("tracked.txt"), "parent local\n").unwrap();
            let head = git(root, &["rev-parse", "HEAD"]).await;
            let index = git(root, &["write-tree"]).await;
            let (target, commit) = if overlap_in_commit {
                ("HEAD", overlapping.trim())
            } else {
                (overlapping.trim(), "HEAD")
            };
            let error = match CherryPickWorktree::save(root, &runner, target, commit).await {
                Err(error) => error,
                Ok(_) => panic!("overlapping nested repository must be rejected"),
            };
            assert_eq!(error.code, ErrorCode::DirtyWorktree);
            assert!(error.diagnostics.unwrap().contains("嵌套 repo"));
            assert_eq!(git(root, &["rev-parse", "HEAD"]).await, head);
            assert_eq!(git(root, &["write-tree"]).await, index);
            assert_eq!(git(root, &["stash", "list", "--format=%H"]).await, stash);
            assert_eq!(
                std::fs::read(root.join("tracked.txt")).unwrap(),
                b"parent local\n"
            );
            assert_eq!(
                std::fs::read(root.join("嵌套 repo/tracked.txt")).unwrap(),
                b"nested local\n"
            );
        }
    }
}
