use super::*;
use crate::domain::history::{SquashCommit, SquashPreview, SquashRequest};

struct Plan {
    preview: SquashPreview,
    base: Option<String>,
    // Original order, oldest first; includes unselected commits through HEAD.
    rewritten: Vec<String>,
}

fn invalid(message: &str) -> BackendError {
    BackendError::new(ErrorCode::InvalidReference, message)
}

impl HistoryService {
    pub async fn squash_preview(
        &self,
        requested_root: &Path,
        commits: Vec<String>,
    ) -> Result<SquashPreview, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        Ok(self.squash_plan(&root, &commits).await?.preview)
    }

    async fn squash_plan(&self, root: &Path, commits: &[String]) -> Result<Plan, BackendError> {
        self.ensure_operation_allows(root, MutationIntent::Rebase)
            .await?;
        let branch = self
            .runner
            .run_allowing_failure(Some(root), ["symbolic-ref", "--quiet", "--short", "HEAD"])
            .await?;
        if !branch.is_success() {
            return Err(invalid("请先切换到要合并提交的本地分支。"));
        }
        let head = self.resolve_commit(root, "HEAD").await?;
        let selected: HashSet<_> = commits.iter().cloned().collect();
        if selected.len() < 2
            || selected.len() != commits.len()
            || commits.iter().any(|hash| {
                !matches!(hash.len(), 40 | 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Err(invalid("请至少选择两个不同的有效提交。"));
        }
        let status = self
            .runner
            .run(
                Some(root),
                [
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=normal",
                    "--ignore-submodules=none",
                ],
            )
            .await?;
        if !status.stdout.trim().is_empty() {
            return Err(BackendError::new(
                ErrorCode::DirtyWorktree,
                "合并提交前请先提交或贮藏本地修改（包括未跟踪文件和子模块修改）。",
            ));
        }
        let log = self
            .runner
            .run(
                Some(root),
                ["rev-list", "--first-parent", "--parents", &head],
            )
            .await?;
        let mut remaining = selected.clone();
        let mut rewritten = Vec::new();
        let mut base = None;
        for line in log.stdout.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            let Some(hash) = fields.first() else { continue };
            if fields.len() > 2 {
                return Err(invalid(
                    "所选提交到 HEAD 之间包含 Merge 提交，暂不支持跨合并节点改写；请选择同一条线性历史中的提交。",
                ));
            }
            rewritten.push((*hash).to_owned());
            remaining.remove(*hash);
            if remaining.is_empty() {
                base = fields.get(1).map(|s| (*s).to_owned());
                break;
            }
        }
        if !remaining.is_empty() {
            return Err(invalid("部分提交不属于当前分支的可合并历史，请重新选择。"));
        }
        rewritten.reverse();
        self.ensure_squash_ignored_files_safe(root, &rewritten)
            .await?;
        let mut ordered = Vec::new();
        for hash in rewritten.iter().filter(|hash| selected.contains(*hash)) {
            let subject = self
                .runner
                .run(Some(root), ["show", "--no-patch", "--format=%s", hash])
                .await?;
            ordered.push(SquashCommit {
                hash: hash.clone(),
                subject: subject.stdout.trim_end().into(),
            });
        }
        Ok(Plan {
            preview: SquashPreview {
                branch: branch.stdout.trim().into(),
                head,
                commits: ordered,
                rewritten_count: rewritten.len(),
            },
            base,
            rewritten,
        })
    }

    pub async fn squash(
        &self,
        requested_root: &Path,
        request: SquashRequest,
    ) -> Result<HistoryMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        if request.message.trim().is_empty()
            || request.message.contains('\0')
            || request.message.len() > 64 * 1024
        {
            return Err(invalid("请填写有效的合并提交说明（最多 64 KiB）。"));
        }
        let plan = self.squash_plan(&root, &request.commits).await?;
        if plan.preview.head != request.expected_head
            || plan.preview.branch != request.expected_branch
        {
            return Err(invalid("分支或 HEAD 已变化，请关闭对话框并重新预览合并。"));
        }
        let first = &plan.preview.commits[0].hash;
        let tree = self
            .runner
            .run(Some(&root), ["rev-parse", &format!("{first}^{{tree}}")])
            .await?;
        // A replacement for the first selected commit supplies the chosen message
        // without a message editor, exec instruction, or shell interpolation.
        let mut create = vec!["commit-tree".to_owned(), tree.stdout.trim().into()];
        if let Some(base) = &plan.base {
            create.extend(["-p".into(), base.clone()]);
        }
        let replacement = self
            .runner
            .run_with_input(Some(&root), create, request.message.trim().as_bytes())
            .await?;
        let mut todo = format!("pick {}\n", replacement.stdout.trim());
        for commit in plan.preview.commits.iter().skip(1) {
            todo.push_str(&format!("fixup {}\n", commit.hash));
        }
        let selected: HashSet<_> = request.commits.iter().collect();
        for hash in &plan.rewritten {
            if !selected.contains(hash) {
                todo.push_str(&format!("pick {hash}\n"));
            }
        }
        let backup = format!(
            "hq-git-backup/squash-{}-{}",
            &plan.preview.head[..7],
            uuid::Uuid::new_v4().simple()
        );
        self.runner
            .run(
                Some(&root),
                [
                    "update-ref",
                    "-m",
                    "HQ Git before squash",
                    &format!("refs/heads/{backup}"),
                    &plan.preview.head,
                    "",
                ],
            )
            .await?;
        let mut args = vec![
            "-c",
            "rebase.missingCommitsCheck=ignore",
            "-c",
            "rebase.updateRefs=false",
            "-c",
            "core.abbrev=40",
            "-c",
            "submodule.recurse=false",
            "rebase",
            "--interactive",
            "--force-rebase",
            "--no-autosquash",
            "--no-autostash",
            "--no-rebase-merges",
            "--no-fork-point",
            "--keep-empty",
            "--empty=keep",
            "--reapply-cherry-picks",
            "--no-gpg-sign",
        ];
        if let Some(base) = &plan.base {
            args.push(base);
        } else {
            args.push("--root");
        }
        let outcome = self
            .runner
            .run_with_sequence(&root, args, todo)
            .await
            .and_then(|output| output.into_result());
        let backup_notice = format!("原历史已备份到分支 {backup}。");
        let mut result = self
            .refreshed_mutation_result(&root)
            .await
            .map_err(|mut error| {
                error.message.push_str(&backup_notice);
                error
            })?;
        result.notice = Some(
            if result.operation_state.kind == RepositoryOperationKind::Rebase {
                format!(
                    "合并提交已暂停，请在冲突工作台解决后继续，或中止 Rebase 恢复原历史。{backup_notice}"
                )
            } else if outcome.is_ok() {
                format!(
                    "已将 {} 个提交合并为一个提交。{backup_notice}",
                    plan.preview.commits.len()
                )
            } else {
                backup_notice.clone()
            },
        );
        if let Err(mut error) = outcome {
            error.message.push_str(&backup_notice);
            result.error = Some(error);
        }
        Ok(result)
    }

    // Rebase resets through historical trees. A file ignored at HEAD can have
    // been tracked earlier, so a clean status alone does not protect local data.
    async fn ensure_squash_ignored_files_safe(
        &self,
        root: &Path,
        rewritten: &[String],
    ) -> Result<(), BackendError> {
        let ignored = self
            .runner
            .run(
                Some(root),
                [
                    "ls-files",
                    "--others",
                    "--ignored",
                    "--exclude-standard",
                    "--directory",
                    "-z",
                ],
            )
            .await?;
        if ignored.stdout.is_empty() {
            return Ok(());
        }
        let changed = self
            .runner
            .run_with_input(
                Some(root),
                [
                    "diff-tree",
                    "--stdin",
                    "--root",
                    "--no-commit-id",
                    "--name-only",
                    "--no-renames",
                    "-r",
                    "-z",
                ],
                format!("{}\n", rewritten.join("\n")),
            )
            .await?;
        let normalize = |path: &str| {
            let path = path.trim_end_matches('/');
            if cfg!(windows) {
                path.to_lowercase()
            } else {
                path.to_owned()
            }
        };
        let paths: Vec<_> = changed
            .stdout
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(normalize)
            .collect();
        for path in ignored.stdout.split('\0').filter(|path| !path.is_empty()) {
            let key = normalize(path);
            if paths.iter().any(|changed| {
                changed == &key
                    || changed.starts_with(&format!("{key}/"))
                    || key.starts_with(&format!("{changed}/"))
            }) {
                return Err(BackendError::new(ErrorCode::DirtyWorktree,
                    "历史提交涉及当前被忽略的本地文件，已停止合并以避免覆盖；请先备份并移走这些文件。")
                    .with_diagnostics(path));
            }
        }
        Ok(())
    }
}
