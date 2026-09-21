use crate::application::{
    mutation_coordinator::RepositoryMutationCoordinator,
    operation_state::{
        MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
    },
    refs_service::RefsService,
    repository_service::RepositoryService,
};
use crate::domain::{
    error::{BackendError, ErrorCode},
    operation::RepositoryOperationKind,
    refs::RefsMutationResult,
    task_branch::*,
};
use crate::infrastructure::{
    git_runner::GitCommandRunner, task_branch_repository::TaskBranchRepository,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TaskBranchService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
    repository: TaskBranchRepository,
}
impl TaskBranchService {
    pub fn new(
        runner: GitCommandRunner,
        coordinator: RepositoryMutationCoordinator,
        repository: TaskBranchRepository,
    ) -> Self {
        Self {
            runner,
            coordinator,
            repository,
        }
    }
    async fn root(&self, path: &Path) -> Result<PathBuf, BackendError> {
        let root =
            RepositoryService::with_coordinator(self.runner.clone(), self.coordinator.clone())
                .resolve_root(path)
                .await?;
        Ok(root.canonicalize()?)
    }
    pub async fn snapshot(&self, path: &Path) -> Result<Vec<TaskBranchBinding>, BackendError> {
        let root = self.root(path).await?;
        let _guard = self.coordinator.read(&root).await;
        self.repository.snapshot(&root)
    }
    pub async fn unlink(
        &self,
        path: &Path,
        id: &str,
    ) -> Result<Vec<TaskBranchBinding>, BackendError> {
        let root = self.root(path).await?;
        // Serialize with create/run so an in-flight task cannot save the binding back.
        let _guard = self.coordinator.write(&root).await;
        self.repository.unlink(&root, id)
    }
    pub async fn create(&self, path: &Path, request: CreateTaskBranchRequest) -> TaskBranchResult {
        let root = match self.root(path).await {
            Ok(root) => root,
            Err(error) => return failed(error),
        };
        let _guard = self.coordinator.write(&root).await;
        let mut binding = None;
        let error = self.create_inner(&root, request, &mut binding).await.err();
        self.finish(&root, binding, error).await
    }
    async fn create_inner(
        &self,
        root: &Path,
        request: CreateTaskBranchRequest,
        binding: &mut Option<TaskBranchBinding>,
    ) -> Result<(), BackendError> {
        let (prefix, ticket_prefix) = match request.kind {
            TaskBranchKind::Feature => ("feature", 'R'),
            TaskBranchKind::Hotfix => ("hotfix", 'B'),
        };
        if !request.ticket.starts_with(ticket_prefix)
            || request.ticket.len() < 2
            || !request.ticket[1..].bytes().all(|c| c.is_ascii_digit())
        {
            return Err(invalid("单号必须为 R/B 加完整数字，且与任务类型一致。"));
        }
        if request.description.trim().is_empty()
            || request.description.chars().any(char::is_control)
        {
            return Err(invalid("请填写任务中文说明。"));
        }
        if request.slug.split('-').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }) {
            return Err(invalid("英文简述必须为小写字母、数字组成的 kebab-case。"));
        }
        let target = format!("{prefix}/{}-{}", request.ticket, request.slug);
        self.git(root, &["check-ref-format", "--branch", &target])
            .await?;
        self.ensure_idle(root, MutationIntent::CreateBranch).await?;
        self.ensure_head(root, &request.expected_head).await?;
        self.ensure_branch(root, &request.source_branch).await?;
        if self
            .optional_oid(root, &format!("refs/heads/{target}"))
            .await?
            .is_some()
        {
            return Err(invalid("同名本地分支已存在。"));
        }
        let previous = self
            .repository
            .snapshot(root)?
            .into_iter()
            .find(|b| b.target_branch == target);
        let remote = if request.mode == TaskBranchMode::RemoteMaster {
            let remote = request
                .remote
                .as_deref()
                .filter(|r| !r.is_empty() && !r.starts_with('-'))
                .ok_or_else(|| invalid("请选择已有远端。"))?;
            if !self
                .git(root, &["remote"])
                .await?
                .lines()
                .any(|r| r == remote)
            {
                return Err(invalid("所选远端不存在。"));
            }
            Some(remote.to_owned())
        } else {
            None
        };
        let source_head = self.oid(root, "HEAD").await?;
        if let Some(previous) = previous {
            if !previous.creation_pending
                || previous.source_branch != request.source_branch
                || previous.mode != request.mode
                || previous.ticket != request.ticket
                || previous.description != request.description.trim()
                || previous.create_source_head.as_deref() != Some(source_head.as_str())
                || previous.create_remote != remote
            {
                return Err(invalid("该任务已有不同创建上下文，请核对原任务状态。"));
            }
            *binding = Some(previous);
        } else {
            *binding = Some(TaskBranchBinding {
                id: uuid::Uuid::new_v4().to_string(),
                root_path: root.to_string_lossy().into_owned(),
                source_branch: request.source_branch,
                target_branch: target,
                ticket: request.ticket,
                description: request.description.trim().to_owned(),
                mode: request.mode,
                phase: TaskBranchPhase::NeedsAttention,
                source_commit: None,
                target_commit: None,
                return_after_success: true,
                message: Some("正在创建任务分支。".into()),
                create_start: None,
                creation_pending: true,
                create_source_head: Some(source_head),
                create_remote: remote.clone(),
                commit_base: None,
                commit_tree: None,
                commit_message: None,
                pick_base: None,
            });
        }
        let b = binding.as_mut().unwrap();
        self.repository.save(b)?;
        let start = if let Some(remote) = remote {
            let refspec = format!("+refs/heads/master:refs/remotes/{remote}/master");
            self.git(root, &["fetch", "--no-tags", "--", &remote, &refspec])
                .await?;
            self.oid(root, &format!("refs/remotes/{remote}/master"))
                .await?
        } else {
            self.oid(root, "HEAD").await?
        };
        b.create_start = Some(start.clone());
        self.repository.save(b)?;
        self.ensure_head(root, &request.expected_head).await?;
        self.ensure_branch(root, &b.source_branch).await?;
        if request.mode == TaskBranchMode::Current {
            self.git(root, &["switch", "-c", &b.target_branch, &start])
                .await?;
        } else {
            self.git(root, &["branch", "--", &b.target_branch, &start])
                .await?;
        }
        b.phase = TaskBranchPhase::Ready;
        b.creation_pending = false;
        b.message = None;
        self.repository.save(b)
    }
    pub async fn run(&self, path: &Path, request: TaskBranchRunRequest) -> TaskBranchResult {
        let root = match self.root(path).await {
            Ok(root) => root,
            Err(error) => return failed(error),
        };
        let _guard = self.coordinator.write(&root).await;
        let mut binding = match self.repository.snapshot(&root) {
            Ok(bindings) => match bindings.into_iter().find(|b| b.id == request.id) {
                Some(b) => b,
                None => {
                    return self
                        .finish(&root, None, Some(invalid("任务绑定不存在。")))
                        .await;
                }
            },
            Err(error) => return failed(error),
        };
        let error = self.run_inner(&root, &mut binding, request).await.err();
        self.finish(&root, Some(binding), error).await
    }
    async fn run_inner(
        &self,
        root: &Path,
        b: &mut TaskBranchBinding,
        request: TaskBranchRunRequest,
    ) -> Result<(), BackendError> {
        // A repeated request from the pre-commit UI cannot start a second commit.
        let expected = self.expected_oid(root, &request.expected_head).await?;
        if request.action == TaskBranchAction::Commit
            && b.commit_base.as_deref() == Some(expected.as_str())
            && b.source_commit.is_some()
        {
            return Ok(());
        }
        self.ensure_head(root, &request.expected_head).await?;
        if request.action == TaskBranchAction::Reconcile {
            return self.reconcile(root, b).await;
        }
        if b.mode != TaskBranchMode::RemoteMaster {
            return Err(invalid("当前分支模式使用普通提交即可。"));
        }
        match request.action {
            TaskBranchAction::Commit => {
                if !matches!(b.phase, TaskBranchPhase::Ready | TaskBranchPhase::Completed) {
                    return Err(invalid("该任务仍有未完成的移植，请先继续或核对状态。"));
                }
                self.ensure_idle(root, MutationIntent::Commit).await?;
                self.ensure_branch(root, &b.source_branch).await?;
                let message = request
                    .message
                    .as_deref()
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .ok_or_else(|| invalid("提交说明不能为空。"))?;
                let staged = self
                    .runner
                    .run_allowing_failure(
                        Some(root),
                        ["diff", "--cached", "--quiet", "--exit-code"],
                    )
                    .await?;
                if staged.status_code == Some(0) {
                    return Err(BackendError::new(
                        ErrorCode::AiNoStagedChanges,
                        "没有暂存变更可提交。",
                    ));
                }
                if staged.status_code != Some(1) {
                    staged.into_result()?;
                }
                // Validate target before committing, so known missing branches never consume staged work.
                let target_base = self.target_oid(root, b).await?;
                b.commit_base = Some(expected);
                b.commit_tree = Some(self.git(root, &["write-tree"]).await?);
                b.commit_message = Some(message.to_owned());
                b.source_commit = None;
                b.target_commit = None;
                b.pick_base = Some(target_base);
                b.return_after_success = request.return_after_success;
                b.phase = TaskBranchPhase::Committing;
                b.message = Some("正在提交已暂存变更。".into());
                self.repository.save(b)?;
                self.git(root, &["commit", "-m", message]).await?;
                b.source_commit = Some(self.oid(root, "HEAD").await?);
                b.phase = TaskBranchPhase::PendingPick;
                b.message = None;
                self.repository.save(b)?;
                self.pick(root, b).await
            }
            TaskBranchAction::Pick => {
                if matches!(
                    b.phase,
                    TaskBranchPhase::Completed | TaskBranchPhase::PendingReturn
                ) {
                    return Ok(());
                }
                if b.phase != TaskBranchPhase::PendingPick {
                    return Err(invalid("请先核对任务状态，不能重复移植。"));
                }
                b.return_after_success = request.return_after_success;
                self.repository.save(b)?;
                self.pick(root, b).await
            }
            TaskBranchAction::Return => {
                if b.phase == TaskBranchPhase::Completed {
                    return Ok(());
                }
                if b.phase != TaskBranchPhase::PendingReturn {
                    return Err(invalid("任务尚未完成移植，不能执行返回。"));
                }
                self.return_to_source(root, b).await
            }
            TaskBranchAction::Reconcile => unreachable!(),
        }
    }
    async fn pick(&self, root: &Path, b: &mut TaskBranchBinding) -> Result<(), BackendError> {
        self.ensure_idle(root, MutationIntent::CherryPick).await?;
        let source = b
            .source_commit
            .clone()
            .ok_or_else(|| invalid("没有可移植的已保存提交。"))?;
        if !is_full_oid(&source) {
            return Err(invalid("已保存的提交标识无效。"));
        }
        if self
            .oid(root, &format!("refs/heads/{}", b.source_branch))
            .await?
            != source
        {
            return Err(invalid("开发分支已发生其他变更，请核对状态。"));
        }
        let target = self.target_oid(root, b).await?;
        if b.pick_base.as_ref().is_some_and(|old| old != &target) {
            return Err(invalid("目标分支已发生其他变更，请核对状态。"));
        }
        let current = self.git(root, &["branch", "--show-current"]).await?;
        if current != b.source_branch && current != b.target_branch {
            return Err(invalid("当前不在任务的开发或目标分支。"));
        }
        let mut saved = super::cherry_pick_worktree::CherryPickWorktree::save(
            root,
            &self.runner,
            &target,
            &source,
        )
        .await?;
        let result = async {
            b.pick_base = Some(target);
            b.phase = TaskBranchPhase::Picking;
            b.message = Some("正在切换并移植已保存提交。".into());
            self.repository.save(b)?;
            if current != b.target_branch {
                self.git(
                    root,
                    &[
                        "-c",
                        "submodule.recurse=false",
                        "switch",
                        "--no-overwrite-ignore",
                        "--",
                        &b.target_branch,
                    ],
                )
                .await?;
                saved
                    .save_exposed_files(root, &self.runner, &source)
                    .await?;
            }
            // -x provides durable source identity for recovery after a successful Git write.
            let result = self
                .git(
                    root,
                    &[
                        "-c",
                        "submodule.recurse=false",
                        "cherry-pick",
                        "-x",
                        &source,
                    ],
                )
                .await;
            if let Err(error) = result {
                if read_operation_state(root, &self.runner).await?.kind
                    == RepositoryOperationKind::CherryPick
                {
                    b.phase = TaskBranchPhase::Conflict;
                    b.message = Some("请在冲突工作台继续或中止，然后核对任务状态。".into());
                    self.repository.save(b)?;
                }
                return Err(error);
            }
            b.target_commit = Some(self.oid(root, "HEAD").await?);
            self.picked(b)?;
            if b.return_after_success {
                self.return_to_source(root, b).await?;
            }
            Ok(())
        }
        .await;
        match saved.finish(root, &self.runner, result).await {
            Ok(notice) => {
                b.message = notice;
                self.repository.save(b)
            }
            Err(error) => {
                b.message = Some(error.message.clone());
                self.repository.save(b)?;
                Err(error)
            }
        }
    }
    fn picked(&self, b: &mut TaskBranchBinding) -> Result<(), BackendError> {
        b.phase = if b.return_after_success {
            TaskBranchPhase::PendingReturn
        } else {
            TaskBranchPhase::Completed
        };
        b.message = None;
        self.repository.save(b)
    }
    async fn return_to_source(
        &self,
        root: &Path,
        b: &mut TaskBranchBinding,
    ) -> Result<(), BackendError> {
        self.ensure_idle(root, MutationIntent::SwitchBranch).await?;
        super::cherry_pick_worktree::CherryPickWorktree::ensure_safe_return(
            root,
            &self.runner,
            &b.source_branch,
        )
        .await?;
        if self
            .oid(root, &format!("refs/heads/{}", b.source_branch))
            .await?
            .as_str()
            != b.source_commit.as_deref().unwrap_or("")
        {
            return Err(invalid("开发分支已变化，请核对后手动返回。"));
        }
        self.ensure_branch(root, &b.target_branch).await?;
        if Some(self.oid(root, "HEAD").await?) != b.target_commit {
            return Err(invalid("目标分支提交已变化，请核对状态。"));
        }
        b.phase = TaskBranchPhase::PendingReturn;
        self.repository.save(b)?;
        self.git(
            root,
            &[
                "-c",
                "submodule.recurse=false",
                "switch",
                "--no-overwrite-ignore",
                "--",
                &b.source_branch,
            ],
        )
        .await?;
        b.phase = TaskBranchPhase::Completed;
        b.message = None;
        self.repository.save(b)
    }
    async fn reconcile(&self, root: &Path, b: &mut TaskBranchBinding) -> Result<(), BackendError> {
        let head = self.oid(root, "HEAD").await?;
        let branch = self.git(root, &["branch", "--show-current"]).await?;
        let state = read_operation_state(root, &self.runner).await?;
        if b.creation_pending {
            let target = self
                .optional_oid(root, &format!("refs/heads/{}", b.target_branch))
                .await?;
            if b.create_start.is_some() && target == b.create_start {
                b.creation_pending = false;
                b.phase = TaskBranchPhase::Ready;
                b.message = None;
            } else {
                b.phase = TaskBranchPhase::NeedsAttention;
                b.message = Some("分支创建未完成或已被外部修改，请核对本地分支。".into());
            }
            return self.repository.save(b);
        }
        if matches!(b.phase, TaskBranchPhase::Ready | TaskBranchPhase::Completed) {
            return Ok(());
        }
        if b.source_commit.is_none() && b.commit_base.is_some() {
            if state.kind != RepositoryOperationKind::None || branch != b.source_branch {
                return self.attention(b);
            }
            if b.commit_base.as_deref() == Some(head.as_str()) {
                b.phase = TaskBranchPhase::Ready;
                b.message = Some("提交尚未生成，可检查暂存内容后重试。".into());
            } else if self
                .git(root, &["show", "-s", "--format=%P", &head])
                .await?
                == b.commit_base.as_deref().unwrap_or("")
                && self
                    .git(root, &["show", "-s", "--format=%T", &head])
                    .await?
                    == b.commit_tree.as_deref().unwrap_or("")
                && self
                    .git(root, &["show", "-s", "--format=%B", &head])
                    .await?
                    == b.commit_message.as_deref().unwrap_or("")
            {
                b.source_commit = Some(head);
                b.phase = TaskBranchPhase::PendingPick;
                b.message = None;
            } else {
                return self.attention(b);
            }
            return self.repository.save(b);
        }
        if b.target_commit.is_some() {
            // Returning to the source is not proof that the picked commit survived.
            if self
                .optional_oid(root, &format!("refs/heads/{}", b.target_branch))
                .await?
                != b.target_commit
            {
                return self.attention(b);
            }
            if state.kind != RepositoryOperationKind::None {
                return self.attention(b);
            }
            if branch == b.source_branch && Some(head.clone()) == b.source_commit {
                b.phase = TaskBranchPhase::Completed;
                b.message = None;
            } else if branch == b.target_branch && Some(head) == b.target_commit {
                self.picked(b)?;
            } else {
                return self.attention(b);
            }
            return self.repository.save(b);
        }
        if let Some(base) = b.pick_base.as_deref() {
            let target = self.target_oid(root, b).await?;
            if state.kind == RepositoryOperationKind::CherryPick && branch == b.target_branch {
                let pending = self.optional_oid(root, "CHERRY_PICK_HEAD").await?;
                if pending == b.source_commit {
                    b.phase = TaskBranchPhase::Conflict;
                    b.message = Some("请在冲突工作台继续或中止。".into());
                    return self.repository.save(b);
                }
                return self.attention(b);
            }
            if state.kind != RepositoryOperationKind::None {
                return self.attention(b);
            }
            if target == base
                && (branch == b.target_branch
                    || (branch == b.source_branch && Some(head) == b.source_commit))
            {
                b.phase = TaskBranchPhase::PendingPick;
                b.message = Some("移植未完成或已中止，可继续移植。".into());
                return self.repository.save(b);
            }
            let parent = self
                .git(root, &["show", "-s", "--format=%P", &target])
                .await?;
            let message = self
                .git(root, &["show", "-s", "--format=%B", &target])
                .await?;
            let marker = format!(
                "(cherry picked from commit {})",
                b.source_commit.as_deref().unwrap_or("")
            );
            if branch == b.target_branch
                && parent == base
                && message.lines().any(|line| line == marker)
            {
                b.target_commit = Some(target);
                return self.picked(b);
            }
            return self.attention(b);
        }
        if b.phase == TaskBranchPhase::PendingPick
            && branch == b.source_branch
            && Some(head) == b.source_commit
        {
            return Ok(());
        }
        self.attention(b)
    }
    fn attention(&self, b: &mut TaskBranchBinding) -> Result<(), BackendError> {
        b.phase = TaskBranchPhase::NeedsAttention;
        b.message = Some("检测到无法确认的外部变化；已保留提交，请人工核对，未自动重试。".into());
        self.repository.save(b)
    }
    async fn ensure_idle(&self, root: &Path, intent: MutationIntent) -> Result<(), BackendError> {
        ensure_mutation_allowed(&read_operation_state(root, &self.runner).await?, intent)
    }
    async fn ensure_branch(&self, root: &Path, expected: &str) -> Result<(), BackendError> {
        if expected.is_empty() || self.git(root, &["branch", "--show-current"]).await? != expected {
            return Err(invalid("当前分支已变化，请刷新后重试。"));
        }
        Ok(())
    }
    async fn expected_oid(&self, root: &Path, expected: &str) -> Result<String, BackendError> {
        if expected.is_empty() || !expected.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid("当前提交校验值无效，请刷新后重试。"));
        }
        self.oid(root, expected).await
    }
    async fn ensure_head(&self, root: &Path, expected: &str) -> Result<(), BackendError> {
        if self.expected_oid(root, expected).await? != self.oid(root, "HEAD").await? {
            return Err(invalid("当前提交已变化，请刷新后重试。"));
        }
        Ok(())
    }
    async fn target_oid(&self, root: &Path, b: &TaskBranchBinding) -> Result<String, BackendError> {
        self.optional_oid(root, &format!("refs/heads/{}", b.target_branch))
            .await?
            .ok_or_else(|| {
                BackendError::new(ErrorCode::BranchUnavailable, "任务目标分支已被删除。")
            })
    }
    async fn optional_oid(&self, root: &Path, value: &str) -> Result<Option<String>, BackendError> {
        let output = self
            .runner
            .run_allowing_failure(
                Some(root),
                [
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{value}^{{commit}}"),
                ],
            )
            .await?;
        if output.is_success() {
            Ok(Some(output.stdout.trim().to_owned()))
        } else if output.status_code == Some(1) {
            Ok(None)
        } else {
            output.into_result()?;
            unreachable!()
        }
    }
    async fn oid(&self, root: &Path, value: &str) -> Result<String, BackendError> {
        self.git(
            root,
            &["rev-parse", "--verify", &format!("{value}^{{commit}}")],
        )
        .await
    }
    async fn git(&self, root: &Path, args: &[&str]) -> Result<String, BackendError> {
        Ok(self
            .runner
            .run(Some(root), args)
            .await?
            .stdout
            .trim()
            .to_owned())
    }
    async fn finish(
        &self,
        root: &Path,
        binding: Option<TaskBranchBinding>,
        mut error: Option<BackendError>,
    ) -> TaskBranchResult {
        let mut bindings = match self.repository.snapshot(root) {
            Ok(bindings) => bindings,
            Err(e) => {
                if error.is_none() {
                    error = Some(e)
                }
                vec![]
            }
        };
        if let Some(mut b) = binding {
            if let Some(e) = error.as_ref() {
                b.message = Some(e.message.clone());
            }
            if let Some(existing) = bindings.iter_mut().find(|old| old.id == b.id) {
                *existing = b;
            } else {
                bindings.push(b);
            }
        }
        let workspace = match self.refresh(root).await {
            Ok(workspace) => Some(workspace),
            Err(e) => {
                if error.is_none() {
                    error = Some(e)
                }
                None
            }
        };
        TaskBranchResult {
            bindings,
            workspace,
            error,
        }
    }
    async fn refresh(&self, root: &Path) -> Result<RefsMutationResult, BackendError> {
        let workspace = refresh_mutation_workspace(root, &self.runner).await?;
        let refs = RefsService::new(self.runner.clone(), self.coordinator.clone())
            .snapshot_at_root(root)
            .await?;
        Ok(RefsMutationResult {
            workspace: workspace.workspace,
            operation_state: workspace.operation_state,
            refs,
        })
    }
}
fn invalid(message: &str) -> BackendError {
    BackendError::new(ErrorCode::InvalidReference, message)
}
fn is_full_oid(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn failed(error: BackendError) -> TaskBranchResult {
    TaskBranchResult {
        bindings: vec![],
        workspace: None,
        error: Some(error),
    }
}
