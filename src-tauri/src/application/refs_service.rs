use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::application::operation_state::{
    MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
};
use crate::application::repository_service::RepositoryService;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::operation::{
    AbortAction, MutationWorkspace, RepositoryOperationKind, RepositoryOperationState,
};
use crate::domain::refs::{
    BranchKind, BranchSummary, BranchTip, CreateBranchRequest, DeleteBranchRequest,
    RefsMutationResult, RefsSnapshot, TagSummary,
};
use crate::infrastructure::git_runner::{GitCommandRunner, GitOutput};

const FIELD_SEPARATOR: char = '\u{1f}';
const RECORD_SEPARATOR: char = '\u{1e}';
const BRANCH_FORMAT: &str = "%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream)%1f%(objectname)%1f%(objectname:short)%1f%(subject)%1f%(authorname)%1f%(authordate:iso-strict)%1f%(symref)%1e";
const TAG_FORMAT: &str = "%(refname)%1f%(objectname)%1f%(objecttype)%1f%(*objectname)%1f%(taggername)%1f%(taggerdate:iso-strict)%1f%(contents)%1f%(*subject)%1f%(subject)%1e";

#[derive(Debug, Clone)]
pub struct RefsService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
}

impl Default for RefsService {
    fn default() -> Self {
        Self::new(
            GitCommandRunner::default(),
            RepositoryMutationCoordinator::default(),
        )
    }
}

impl RefsService {
    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
        }
    }

    pub async fn snapshot(&self, requested_root: &Path) -> Result<RefsSnapshot, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        self.snapshot_at_root(&root).await
    }

    pub async fn create_branch(
        &self,
        requested_root: &Path,
        request: CreateBranchRequest,
    ) -> Result<RefsMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::CreateBranch)
            .await?;

        let name = self.validate_branch_name(&root, &request.name).await?;
        let refs = self.snapshot_at_root(&root).await?;
        if refs.local_branches.iter().any(|branch| branch.name == name) {
            return Err(BackendError::new(
                ErrorCode::InvalidReference,
                "同名本地分支已存在。",
            ));
        }
        let start_point = request.start_point.as_deref().unwrap_or("HEAD");
        let start_oid = self.resolve_commit(&root, start_point).await?;
        let args = if request.switch {
            vec![
                OsString::from("switch"),
                OsString::from("-c"),
                OsString::from(&name),
                OsString::from(start_oid),
            ]
        } else {
            vec![
                OsString::from("branch"),
                OsString::from("--"),
                OsString::from(&name),
                OsString::from(start_oid),
            ]
        };
        let result = self.runner.run(Some(&root), args).await;
        if request.switch {
            result.map_err(map_switch_error)?;
        } else {
            result?;
        }
        self.refreshed_mutation_result(&root).await
    }

    pub async fn switch_branch(
        &self,
        requested_root: &Path,
        name: &str,
    ) -> Result<RefsMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::SwitchBranch)
            .await?;

        let name = self.validate_branch_name(&root, name).await?;
        let refs = self.snapshot_at_root(&root).await?;
        ensure_local_branch_exists(&refs, &name)?;
        self.runner
            .run(Some(&root), ["switch", "--", &name])
            .await
            .map_err(map_switch_error)?;
        self.refreshed_mutation_result(&root).await
    }

    pub async fn delete_branch(
        &self,
        requested_root: &Path,
        request: DeleteBranchRequest,
    ) -> Result<RefsMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::DeleteBranch)
            .await?;

        let name = self.validate_branch_name(&root, &request.name).await?;
        let refs = self.snapshot_at_root(&root).await?;
        let branch = ensure_local_branch_exists(&refs, &name)?;
        if branch.current {
            return Err(BackendError::new(
                ErrorCode::CurrentBranchDeletion,
                "不能删除当前检出的分支。",
            ));
        }
        if request.force && request.confirmation.as_deref() != Some(name.as_str()) {
            return Err(BackendError::new(
                ErrorCode::InvalidReference,
                "强制删除确认文本必须与分支名完全一致。",
            ));
        }

        let mode = if request.force { "-D" } else { "-d" };
        self.runner
            .run(Some(&root), ["branch", mode, "--", &name])
            .await
            .map_err(|error| map_delete_error(error, request.force))?;
        self.refreshed_mutation_result(&root).await
    }

    pub async fn merge(
        &self,
        requested_root: &Path,
        target: &str,
    ) -> Result<RefsMutationResult, BackendError> {
        self.merge_into(requested_root, target, None).await
    }

    pub async fn merge_into(
        &self,
        requested_root: &Path,
        source: &str,
        destination: Option<&str>,
    ) -> Result<RefsMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Merge)
            .await?;
        let source = self.validate_branch_name(&root, source).await?;
        let refs = self.snapshot_at_root(&root).await?;
        let source_branch = ensure_local_branch_exists(&refs, &source)?;
        let target_oid = source_branch.tip.full_hash.clone();
        if destination.is_some_and(|name| name.trim() == source)
            || (destination.is_none() && source_branch.current)
        {
            return Err(BackendError::new(
                ErrorCode::InvalidReference,
                "合并源分支和目标分支不能相同。",
            ));
        }
        if let Some(destination) = destination {
            self.checkout_integration_destination(&root, destination)
                .await?;
        }
        let output = self
            .runner
            .run_allowing_failure(
                Some(&root),
                ["merge", "--no-edit", "--", target_oid.as_str()],
            )
            .await?;
        let result = self.refreshed_mutation_result(&root).await?;
        recover_integration_result(output, result, RepositoryOperationKind::Merge)
    }

    /// Caller holds the repository mutation lock for validation, switch and integration.
    pub(crate) async fn checkout_integration_destination(
        &self,
        root: &Path,
        name: &str,
    ) -> Result<(), BackendError> {
        let name = self.validate_branch_name(root, name).await?;
        let refs = self.snapshot_at_root(root).await?;
        if ensure_local_branch_exists(&refs, &name)?.current {
            return Ok(());
        }
        let status = self
            .runner
            .run(
                Some(root),
                ["status", "--porcelain", "--untracked-files=normal"],
            )
            .await?;
        if !status.stdout.trim().is_empty() {
            return Err(BackendError::new(
                ErrorCode::DirtyWorktree,
                "切换到目标分支前，请先提交或贮藏当前未提交修改。",
            ));
        }
        self.runner
            .run(Some(root), ["switch", "--", &name])
            .await
            .map_err(map_switch_error)?;
        Ok(())
    }

    pub async fn rebase(
        &self,
        requested_root: &Path,
        target: &str,
    ) -> Result<RefsMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        let target_oid = self
            .resolve_integration_target(&root, target, MutationIntent::Rebase)
            .await?;
        let output = self
            .runner
            .run_allowing_failure(Some(&root), ["rebase", "--", target_oid.as_str()])
            .await?;
        let result = self.refreshed_mutation_result(&root).await?;
        recover_integration_result(output, result, RepositoryOperationKind::Rebase)
    }

    pub async fn abort(
        &self,
        requested_root: &Path,
        action: AbortAction,
    ) -> Result<RefsMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        let state = read_operation_state(&root, &self.runner).await?;
        let (intent, args) = match action {
            AbortAction::Merge => (MutationIntent::AbortMerge, ["merge", "--abort"]),
            AbortAction::Rebase => (MutationIntent::AbortRebase, ["rebase", "--abort"]),
            AbortAction::CherryPick => {
                (MutationIntent::AbortCherryPick, ["cherry-pick", "--abort"])
            }
            AbortAction::Revert => (MutationIntent::AbortRevert, ["revert", "--abort"]),
        };
        ensure_mutation_allowed(&state, intent)?;
        self.runner.run(Some(&root), args).await?;
        self.refreshed_mutation_result(&root).await
    }

    pub(crate) async fn snapshot_at_root(&self, root: &Path) -> Result<RefsSnapshot, BackendError> {
        let branch_args = [
            OsString::from("for-each-ref"),
            OsString::from("--sort=refname"),
            OsString::from(format!("--format={BRANCH_FORMAT}")),
            OsString::from("refs/heads"),
            OsString::from("refs/remotes"),
        ];
        let tag_args = [
            OsString::from("for-each-ref"),
            OsString::from("--sort=refname"),
            OsString::from(format!("--format={TAG_FORMAT}")),
            OsString::from("refs/tags"),
        ];
        let (branches_output, tags_output) = tokio::join!(
            self.runner.run(Some(root), branch_args),
            self.runner.run(Some(root), tag_args),
        );
        let parsed_branches = parse_branches(&branches_output?.stdout)?;
        let tags = parse_tags(&tags_output?.stdout)?;
        let mut local_branches = Vec::new();
        let mut remote_branches = Vec::new();

        for parsed in parsed_branches {
            let mut branch = parsed.summary;
            if branch.kind == BranchKind::Local {
                if let Some(upstream_full_name) = parsed.upstream_full_name {
                    let (ahead, behind) = self
                        .divergence(root, &branch.full_name, &upstream_full_name)
                        .await?;
                    branch.ahead = Some(ahead);
                    branch.behind = Some(behind);
                }
                local_branches.push(branch);
            } else {
                remote_branches.push(branch);
            }
        }

        Ok(RefsSnapshot {
            local_branches,
            remote_branches,
            tags,
        })
    }

    async fn repository_root(&self, requested_root: &Path) -> Result<PathBuf, BackendError> {
        RepositoryService::with_coordinator(self.runner.clone(), self.coordinator.clone())
            .resolve_root(requested_root)
            .await
    }

    async fn ensure_operation_allows(
        &self,
        root: &Path,
        intent: MutationIntent,
    ) -> Result<(), BackendError> {
        let state = read_operation_state(root, &self.runner).await?;
        ensure_mutation_allowed(&state, intent)
    }

    async fn validate_branch_name(&self, root: &Path, name: &str) -> Result<String, BackendError> {
        if name.is_empty()
            || name != name.trim()
            || name.starts_with('-')
            || name.chars().any(char::is_control)
        {
            return Err(invalid_branch_name());
        }
        let output = self
            .runner
            .run(Some(root), ["check-ref-format", "--branch", name])
            .await
            .map_err(|error| {
                invalid_branch_name().with_diagnostics(error.diagnostics.unwrap_or_default())
            })?;
        if output.stdout.trim() != name {
            return Err(invalid_branch_name());
        }
        Ok(name.to_owned())
    }

    async fn resolve_commit(&self, root: &Path, value: &str) -> Result<String, BackendError> {
        let value = value.trim();
        if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
            return Err(invalid_start_point());
        }
        self.runner
            .run(
                Some(root),
                ["rev-parse", "--verify", &format!("{value}^{{commit}}")],
            )
            .await
            .map(|output| output.stdout.trim().to_owned())
            .map_err(|error| {
                invalid_start_point().with_diagnostics(error.diagnostics.unwrap_or_default())
            })
    }

    async fn resolve_integration_target(
        &self,
        root: &Path,
        target: &str,
        intent: MutationIntent,
    ) -> Result<String, BackendError> {
        self.ensure_operation_allows(root, intent).await?;
        let target = self.validate_branch_name(root, target).await?;
        let refs = self.snapshot_at_root(root).await?;
        let branch = ensure_local_branch_exists(&refs, &target)?;
        if branch.current {
            return Err(BackendError::new(
                ErrorCode::InvalidReference,
                "请选择非当前本地分支。",
            ));
        }
        Ok(branch.tip.full_hash.clone())
    }

    async fn refreshed_mutation_result(
        &self,
        root: &Path,
    ) -> Result<RefsMutationResult, BackendError> {
        let (workspace, refs) = tokio::join!(
            refresh_mutation_workspace(root, &self.runner),
            self.snapshot_at_root(root),
        );
        let MutationWorkspace {
            workspace,
            operation_state,
        } = workspace?;
        Ok(RefsMutationResult {
            workspace,
            refs: refs?,
            operation_state,
        })
    }

    async fn divergence(
        &self,
        root: &Path,
        local: &str,
        upstream: &str,
    ) -> Result<(u32, u32), BackendError> {
        let range = format!("{local}...{upstream}");
        let output = self
            .runner
            .run(
                Some(root),
                [
                    OsString::from("rev-list"),
                    OsString::from("--left-right"),
                    OsString::from("--count"),
                    OsString::from(range),
                ],
            )
            .await?;
        let mut counts = output.stdout.split_whitespace();
        let ahead = parse_count(counts.next())?;
        let behind = parse_count(counts.next())?;
        if counts.next().is_some() {
            return Err(parse_error("Git 返回了多余的分支差异字段。"));
        }
        Ok((ahead, behind))
    }
}

fn recover_integration_result(
    output: GitOutput,
    result: RefsMutationResult,
    expected: RepositoryOperationKind,
) -> Result<RefsMutationResult, BackendError> {
    if output.is_success() || is_recoverable_conflict(&result.operation_state, expected) {
        return Ok(result);
    }
    output.into_result()?;
    unreachable!("non-zero Git output must produce an error")
}

fn is_recoverable_conflict(
    state: &RepositoryOperationState,
    expected: RepositoryOperationKind,
) -> bool {
    state.kind == expected && !state.conflicts.is_empty()
}

fn ensure_local_branch_exists<'a>(
    refs: &'a RefsSnapshot,
    name: &str,
) -> Result<&'a BranchSummary, BackendError> {
    refs.local_branches
        .iter()
        .find(|branch| branch.name == name)
        .ok_or_else(|| {
            BackendError::new(
                ErrorCode::BranchUnavailable,
                "所选本地分支已不存在，请刷新后重试。",
            )
        })
}

fn invalid_branch_name() -> BackendError {
    BackendError::new(ErrorCode::InvalidReference, "分支名称无效。")
}

fn invalid_start_point() -> BackendError {
    BackendError::new(ErrorCode::InvalidReference, "分支起点不存在或已失效。")
}

fn map_switch_error(error: BackendError) -> BackendError {
    if error.code == ErrorCode::GitCommandFailed
        && error.diagnostics.as_deref().is_some_and(|diagnostics| {
            let diagnostics = diagnostics.to_ascii_lowercase();
            diagnostics.contains("would be overwritten by checkout")
                || diagnostics.contains("would be overwritten by switch")
                || diagnostics.contains("please commit your changes or stash them")
        })
    {
        return BackendError::new(ErrorCode::DirtyWorktree, "当前未提交变更会被目标分支覆盖。")
            .with_diagnostics(error.diagnostics.unwrap_or_default());
    }
    error
}

fn map_delete_error(error: BackendError, force: bool) -> BackendError {
    if !force
        && error.code == ErrorCode::GitCommandFailed
        && error.diagnostics.as_deref().is_some_and(|diagnostics| {
            diagnostics
                .to_ascii_lowercase()
                .contains("not fully merged")
        })
    {
        return BackendError::new(
            ErrorCode::UnmergedBranchDeletion,
            "该分支尚未完全合并，如确认删除请使用强制删除。",
        )
        .with_diagnostics(error.diagnostics.unwrap_or_default());
    }
    error
}

#[derive(Debug)]
struct ParsedBranch {
    summary: BranchSummary,
    upstream_full_name: Option<String>,
}

fn parse_branches(output: &str) -> Result<Vec<ParsedBranch>, BackendError> {
    records(output)
        .map(|record| {
            let fields = record.splitn(10, FIELD_SEPARATOR).collect::<Vec<_>>();
            if fields.len() != 10 {
                return Err(parse_error("Git 分支记录字段数量不正确。"));
            }

            let full_name = fields[0].to_owned();
            let kind = if full_name.starts_with("refs/heads/") {
                BranchKind::Local
            } else if full_name.starts_with("refs/remotes/") {
                BranchKind::Remote
            } else {
                return Err(parse_error("Git 返回了未知引用类型。"));
            };
            if kind == BranchKind::Remote && !fields[9].is_empty() {
                return Ok(None);
            }

            let upstream_full_name = non_empty(fields[3]);
            let upstream = upstream_full_name
                .as_deref()
                .map(short_remote_name)
                .map(str::to_owned);
            Ok(Some(ParsedBranch {
                summary: BranchSummary {
                    name: fields[1].to_owned(),
                    full_name,
                    kind,
                    current: fields[2].trim() == "*",
                    upstream,
                    ahead: None,
                    behind: None,
                    tip: BranchTip {
                        full_hash: fields[4].to_owned(),
                        short_hash: fields[5].to_owned(),
                        subject: fields[6].to_owned(),
                        author: fields[7].to_owned(),
                        authored_at: fields[8].to_owned(),
                    },
                },
                upstream_full_name,
            }))
        })
        .filter_map(Result::transpose)
        .collect()
}

fn parse_tags(output: &str) -> Result<Vec<TagSummary>, BackendError> {
    records(output)
        .map(|record| {
            let fields = record.splitn(9, FIELD_SEPARATOR).collect::<Vec<_>>();
            if fields.len() != 9 {
                return Err(parse_error("Git 标签记录字段数量不正确。"));
            }
            let annotated = fields[2] == "tag";
            let annotation = annotated
                .then(|| fields[6].trim_end_matches(['\r', '\n']).to_owned())
                .filter(|value| !value.is_empty());
            let peeled_commit_hash = if annotated && !fields[3].is_empty() {
                fields[3]
            } else {
                fields[1]
            };
            let commit_subject = if annotated && !fields[7].is_empty() {
                fields[7]
            } else {
                fields[8]
            };

            Ok(TagSummary {
                name: fields[0]
                    .strip_prefix("refs/tags/")
                    .unwrap_or(fields[0])
                    .to_owned(),
                object_hash: fields[1].to_owned(),
                peeled_commit_hash: peeled_commit_hash.to_owned(),
                annotated,
                tagger: annotated
                    .then(|| fields[4].to_owned())
                    .filter(|value| !value.is_empty()),
                tagged_at: annotated
                    .then(|| fields[5].to_owned())
                    .filter(|value| !value.is_empty()),
                annotation,
                commit_subject: commit_subject.to_owned(),
            })
        })
        .collect()
}

fn records(output: &str) -> impl Iterator<Item = &str> {
    output
        .split(RECORD_SEPARATOR)
        .map(|record| record.trim_matches(['\r', '\n']))
        .filter(|record| !record.is_empty())
}

fn short_remote_name(full_name: &str) -> &str {
    full_name.strip_prefix("refs/remotes/").unwrap_or(full_name)
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_count(value: Option<&str>) -> Result<u32, BackendError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| parse_error("Git 分支差异计数无效。"))
}

fn parse_error(message: &str) -> BackendError {
    BackendError::new(ErrorCode::Unexpected, message)
}
