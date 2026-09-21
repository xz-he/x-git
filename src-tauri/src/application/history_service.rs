use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::application::diff_parser::parse_unified_diff;
use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::application::operation_state::{
    MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
};
use crate::application::repository_service::RepositoryService;
use crate::domain::changes::{DiffScope, FileDiff};
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::history::{
    CherryPickRequest, CommitDetail, CommitFileSummary, CommitSummary, CommitTopology,
    HistoryMutationResult, HistoryPage, HistoryQuery, ResetMode, ResetRequest, RevertRequest,
    TopologyParent,
};
use crate::domain::operation::{
    MutationWorkspace, RepositoryOperationKind, RepositoryOperationState,
};
use crate::infrastructure::git_runner::GitCommandRunner;

pub const HISTORY_PAGE_SIZE: usize = 200;
const MAX_CURSOR_BYTES: usize = 16 * 1024;
const MAX_TOPOLOGY_LANES: usize = 32;
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const FIELD_SEPARATOR: char = '\u{1f}';
const RECORD_SEPARATOR: char = '\u{1e}';
const DECORATION_SEPARATOR: char = '\u{1d}';
const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%P%x1f%s%x1f%an%x1f%ae%x1f%aI%x1f%(decorate:prefix=,suffix=,separator=%x1d,tag=tag: )%x1e";

#[derive(Debug, Clone)]
pub struct HistoryService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryCursor {
    version: u8,
    reference_oid: String,
    next_offset: usize,
    query_fingerprint: String,
    lanes: Vec<String>,
}

#[derive(Debug, Clone)]
struct ParsedCommit {
    summary: CommitSummary,
}

impl Default for HistoryService {
    fn default() -> Self {
        Self::new(
            GitCommandRunner::default(),
            RepositoryMutationCoordinator::default(),
        )
    }
}

impl HistoryService {
    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
        }
    }

    pub async fn page(
        &self,
        requested_root: &Path,
        query: HistoryQuery,
    ) -> Result<HistoryPage, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        self.page_at_root(&root, query).await
    }

    async fn page_at_root(
        &self,
        root: &Path,
        query: HistoryQuery,
    ) -> Result<HistoryPage, BackendError> {
        let reference = normalize_reference(query.reference.as_deref())?;
        let search = query.search.trim().to_owned();
        let fingerprint = query_fingerprint(&reference, &search);
        let Some(reference_oid) = self.resolve_reference(root, &reference).await? else {
            return Ok(HistoryPage {
                commits: Vec::new(),
                next_cursor: None,
                query_fingerprint: fingerprint,
                continuation_lanes: Vec::new(),
            });
        };
        let cursor = query.cursor.as_deref().map(decode_cursor).transpose()?;
        let (offset, lanes) = match cursor {
            Some(cursor)
                if cursor.reference_oid == reference_oid
                    && cursor.query_fingerprint == fingerprint =>
            {
                (cursor.next_offset, cursor.lanes)
            }
            Some(_) => return Err(invalid_cursor()),
            None => (0, Vec::new()),
        };

        let (mut page_commits, has_more): (Vec<CommitSummary>, bool) = if search.is_empty() {
            let mut commits = self.log_page(root, &reference_oid, offset).await?;
            let has_more = commits.len() > HISTORY_PAGE_SIZE;
            commits.truncate(HISTORY_PAGE_SIZE);
            (
                commits.into_iter().map(|commit| commit.summary).collect(),
                has_more,
            )
        } else {
            let commits = self.search_commits(root, &reference_oid, &search).await?;
            if offset > commits.len() {
                return Err(invalid_cursor());
            }
            let end = (offset + HISTORY_PAGE_SIZE).min(commits.len());
            (
                commits[offset..end]
                    .iter()
                    .map(|commit| commit.summary.clone())
                    .collect(),
                end < commits.len(),
            )
        };
        let continuation_lanes = apply_topology(&mut page_commits, lanes);
        let next_offset = offset + page_commits.len();
        let next_cursor = has_more
            .then(|| {
                encode_cursor(&HistoryCursor {
                    version: 1,
                    reference_oid,
                    next_offset,
                    query_fingerprint: fingerprint.clone(),
                    lanes: continuation_lanes.clone(),
                })
            })
            .transpose()?;

        Ok(HistoryPage {
            commits: page_commits,
            next_cursor,
            query_fingerprint: fingerprint,
            continuation_lanes,
        })
    }

    pub async fn checkout(
        &self,
        requested_root: &Path,
        commit: &str,
    ) -> Result<HistoryMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Checkout)
            .await?;
        let hash = self.resolve_commit(&root, commit).await?;
        self.runner
            .run(Some(&root), ["switch", "--detach", "--", hash.as_str()])
            .await?;
        self.refreshed_mutation_result(&root).await
    }

    pub async fn reset(
        &self,
        requested_root: &Path,
        request: ResetRequest,
    ) -> Result<HistoryMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Reset)
            .await?;
        let hash = self.resolve_commit(&root, &request.target).await?;
        let mode = match request.mode {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => {
                if request.confirmation.as_deref() != Some(&hash[..7]) {
                    return Err(BackendError::new(
                        ErrorCode::InvalidReference,
                        "请输入目标提交的七位短哈希以确认硬重置。",
                    ));
                }
                "--hard"
            }
        };
        self.runner
            .run(Some(&root), ["reset", mode, hash.as_str()])
            .await?;
        self.refreshed_mutation_result(&root).await
    }

    pub async fn cherry_pick(
        &self,
        requested_root: &Path,
        request: CherryPickRequest,
    ) -> Result<HistoryMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::CherryPick)
            .await?;
        let hash = self.resolve_commit(&root, &request.commit).await?;
        let source_branch = self.current_branch(&root).await?;
        let target_branch = match request.target_branch {
            Some(target) => self.resolve_local_branch(&root, &target).await?,
            None => source_branch.clone(),
        };
        let switched = target_branch != source_branch;
        let saved = super::cherry_pick_worktree::CherryPickWorktree::save(
            &root,
            &self.runner,
            &format!("refs/heads/{target_branch}"),
            &hash,
        )
        .await?;
        let outcome = async {
            if switched {
                self.runner
                    .run(
                        Some(&root),
                        [
                            "-c",
                            "submodule.recurse=false",
                            "switch",
                            "--",
                            target_branch.as_str(),
                        ],
                    )
                    .await?;
            }

            let output = self
                .runner
                .run_allowing_failure(
                    Some(&root),
                    [
                        "-c",
                        "submodule.recurse=false",
                        "cherry-pick",
                        "--",
                        hash.as_str(),
                    ],
                )
                .await?;
            if !output.is_success() {
                if is_recoverable_cherry_pick(&read_operation_state(&root, &self.runner).await?) {
                    return Ok(());
                }
                output.into_result()?;
            }
            if switched && request.return_after_success {
                super::cherry_pick_worktree::CherryPickWorktree::ensure_safe_return(
                    &root,
                    &self.runner,
                    &source_branch,
                )
                .await?;
                self.runner
                    .run(
                        Some(&root),
                        [
                            "-c",
                            "submodule.recurse=false",
                            "switch",
                            "--",
                            source_branch.as_str(),
                        ],
                    )
                    .await?;
            }
            Ok(())
        }
        .await;
        let outcome = saved.finish(&root, &self.runner, outcome).await;
        let mut result = self.refreshed_mutation_result(&root).await?;
        result.error = outcome.err();
        Ok(result)
    }

    pub async fn revert(
        &self,
        requested_root: &Path,
        request: RevertRequest,
    ) -> Result<HistoryMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Revert)
            .await?;
        let hash = self.resolve_commit(&root, &request.commit).await?;
        let branch = self
            .runner
            .run_allowing_failure(Some(&root), ["symbolic-ref", "--quiet", "HEAD"])
            .await?;
        if !branch.is_success() {
            return Err(BackendError::new(
                ErrorCode::BranchUnavailable,
                "请先切换到要生成回滚提交的本地分支。",
            ));
        }
        let status = self
            .runner
            .run(
                Some(&root),
                ["status", "--porcelain=v1", "--untracked-files=normal"],
            )
            .await?;
        if !status.stdout.trim().is_empty() {
            return Err(BackendError::new(
                ErrorCode::DirtyWorktree,
                "请先提交或贮藏工作区修改，再执行 Revert。",
            ));
        }
        let parents = self
            .runner
            .run(
                Some(&root),
                ["rev-list", "--parents", "-n", "1", hash.as_str()],
            )
            .await?;
        let parent_count = parents.stdout.split_whitespace().count().saturating_sub(1);
        if (parent_count > 1
            && !request
                .mainline
                .is_some_and(|line| line > 0 && line as usize <= parent_count))
            || (parent_count <= 1 && request.mainline.is_some())
        {
            return Err(BackendError::new(
                ErrorCode::InvalidReference,
                "合并提交必须选择有效的主线父提交；普通提交不需要主线。",
            ));
        }
        let mut args = vec!["revert".to_owned(), "--no-edit".to_owned()];
        if let Some(mainline) = request.mainline {
            args.extend(["--mainline".to_owned(), mainline.to_string()]);
        }
        args.extend(["--".to_owned(), hash]);
        let output = self.runner.run_without_editor(Some(&root), args).await?;
        let result = self.refreshed_mutation_result(&root).await?;
        if !output.is_success() && result.operation_state.kind != RepositoryOperationKind::Revert {
            output.into_result()?;
        }
        Ok(result)
    }

    pub async fn detail(
        &self,
        requested_root: &Path,
        commit: &str,
    ) -> Result<CommitDetail, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        let hash = self.resolve_commit(&root, commit).await?;
        let output = self
            .runner
            .run(
                Some(&root),
                [
                    "show",
                    "--no-patch",
                    "--date=iso-strict",
                    "--decorate=full",
                    "--format=%H%x1f%h%x1f%P%x1f%B%x1f%an%x1f%ae%x1f%aI%x1f%cn%x1f%ce%x1f%cI%x1f%(decorate:prefix=,suffix=,separator=%x1d,tag=tag: )%x1e",
                    &hash,
                ],
            )
            .await?;
        let fields = single_record(&output.stdout, 11)?;
        let files = self.commit_files(&root, &hash).await?;

        Ok(CommitDetail {
            hash: fields[0].to_owned(),
            short_hash: fields[1].to_owned(),
            parent_hashes: split_words(fields[2]),
            message: fields[3].trim_end().to_owned(),
            author_name: fields[4].to_owned(),
            author_email: fields[5].to_owned(),
            authored_at: fields[6].to_owned(),
            committer_name: fields[7].to_owned(),
            committer_email: fields[8].to_owned(),
            committed_at: fields[9].to_owned(),
            references: split_decorations(fields[10]),
            files,
        })
    }

    pub async fn file_diff(
        &self,
        requested_root: &Path,
        commit: &str,
        path: &str,
    ) -> Result<FileDiff, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let relative_path = validate_relative_path(path)?;
        let _guard = self.coordinator.read(&root).await;
        let hash = self.resolve_commit(&root, commit).await?;
        let parents = self.parents(&root, &hash).await?;
        let base = parents.first().map(String::as_str).unwrap_or(EMPTY_TREE);
        let args = vec![
            OsString::from("diff"),
            OsString::from("--no-color"),
            OsString::from("--no-ext-diff"),
            OsString::from("--unified=3"),
            OsString::from("-M20%"),
            OsString::from(base),
            OsString::from(&hash),
            OsString::from("--"),
            relative_path.as_os_str().to_os_string(),
        ];
        let output = self.runner.run(Some(&root), args).await?;
        let binary =
            output.stdout.contains("Binary files ") || output.stdout.contains("GIT binary patch");
        Ok(FileDiff {
            path: path.replace('\\', "/"),
            scope: DiffScope::Commit,
            binary,
            hunks: if binary {
                Vec::new()
            } else {
                parse_unified_diff(&output.stdout)
            },
        })
    }

    async fn resolve_reference(
        &self,
        root: &Path,
        reference: &str,
    ) -> Result<Option<String>, BackendError> {
        match self
            .runner
            .run(
                Some(root),
                ["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
            )
            .await
        {
            Ok(output) => Ok(Some(output.stdout.trim().to_owned())),
            Err(error) if reference == "HEAD" && is_unborn_error(&error) => Ok(None),
            Err(error) => Err(BackendError::new(
                ErrorCode::BranchUnavailable,
                "所选引用不存在或不再指向提交。",
            )
            .with_diagnostics(error.diagnostics.unwrap_or_default())),
        }
    }

    async fn log_page(
        &self,
        root: &Path,
        reference_oid: &str,
        offset: usize,
    ) -> Result<Vec<ParsedCommit>, BackendError> {
        let args = vec![
            OsString::from("log"),
            OsString::from("--date=iso-strict"),
            OsString::from("--decorate=full"),
            OsString::from(LOG_FORMAT),
            OsString::from(format!("--skip={offset}")),
            OsString::from(format!("--max-count={}", HISTORY_PAGE_SIZE + 1)),
            OsString::from(reference_oid),
        ];
        let output = self.runner.run(Some(root), args).await?;
        parse_log(&output.stdout)
    }

    async fn search_commits(
        &self,
        root: &Path,
        reference_oid: &str,
        search: &str,
    ) -> Result<Vec<ParsedCommit>, BackendError> {
        let mut matches = HashSet::new();
        for filter in [format!("--grep={search}"), format!("--author={search}")] {
            let output = self
                .runner
                .run(
                    Some(root),
                    [
                        OsString::from("log"),
                        OsString::from("--format=%H"),
                        OsString::from("--regexp-ignore-case"),
                        OsString::from(filter),
                        OsString::from(reference_oid),
                    ],
                )
                .await?;
            matches.extend(output.stdout.lines().map(str::to_owned));
        }
        if is_hash_prefix(search)
            && let Ok(output) = self
                .runner
                .run(
                    Some(root),
                    ["rev-parse", "--verify", &format!("{search}^{{commit}}")],
                )
                .await
        {
            matches.insert(output.stdout.trim().to_owned());
        }
        if matches.is_empty() {
            return Ok(Vec::new());
        }
        let output = self
            .runner
            .run(
                Some(root),
                [
                    "log",
                    "--date=iso-strict",
                    "--decorate=full",
                    LOG_FORMAT,
                    reference_oid,
                ],
            )
            .await?;
        let mut commits = parse_log(&output.stdout)?;
        commits.retain(|commit| matches.contains(&commit.summary.hash));
        Ok(commits)
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

    async fn current_branch(&self, root: &Path) -> Result<String, BackendError> {
        self.runner
            .run(Some(root), ["symbolic-ref", "--quiet", "--short", "HEAD"])
            .await
            .map(|output| output.stdout.trim().to_owned())
            .map_err(|error| {
                BackendError::new(
                    ErrorCode::BranchUnavailable,
                    "当前处于分离 HEAD，无法执行 Cherry-pick 分支工作流。",
                )
                .with_diagnostics(error.diagnostics.unwrap_or_default())
            })
    }

    async fn resolve_local_branch(
        &self,
        root: &Path,
        branch: &str,
    ) -> Result<String, BackendError> {
        let branch = normalize_object_name(branch)?;
        let branch = self
            .runner
            .run(
                Some(root),
                ["check-ref-format", "--branch", branch.as_str()],
            )
            .await
            .map(|output| output.stdout.trim().to_owned())
            .map_err(|error| {
                BackendError::new(ErrorCode::InvalidReference, "本地分支名称无效。")
                    .with_diagnostics(error.diagnostics.unwrap_or_default())
            })?;
        let reference = format!("refs/heads/{branch}^{{commit}}");
        self.runner
            .run(Some(root), ["rev-parse", "--verify", reference.as_str()])
            .await
            .map_err(|error| {
                BackendError::new(ErrorCode::BranchUnavailable, "目标本地分支不存在。")
                    .with_diagnostics(error.diagnostics.unwrap_or_default())
            })?;
        Ok(branch)
    }

    async fn refreshed_mutation_result(
        &self,
        root: &Path,
    ) -> Result<HistoryMutationResult, BackendError> {
        let (workspace, history) = tokio::join!(
            refresh_mutation_workspace(root, &self.runner),
            self.page_at_root(root, HistoryQuery::default()),
        );
        let MutationWorkspace {
            workspace,
            operation_state,
        } = workspace?;
        Ok(HistoryMutationResult {
            workspace,
            history: history?,
            operation_state,
            error: None,
        })
    }

    async fn resolve_commit(&self, root: &Path, commit: &str) -> Result<String, BackendError> {
        let commit = normalize_object_name(commit)?;
        self.runner
            .run(
                Some(root),
                ["rev-parse", "--verify", &format!("{commit}^{{commit}}")],
            )
            .await
            .map(|output| output.stdout.trim().to_owned())
            .map_err(|error| {
                BackendError::new(ErrorCode::InvalidReference, "提交不存在或已失效。")
                    .with_diagnostics(error.diagnostics.unwrap_or_default())
            })
    }

    async fn parents(&self, root: &Path, hash: &str) -> Result<Vec<String>, BackendError> {
        let output = self
            .runner
            .run(Some(root), ["show", "--no-patch", "--format=%P", hash])
            .await?;
        Ok(split_words(output.stdout.trim()))
    }

    async fn commit_files(
        &self,
        root: &Path,
        hash: &str,
    ) -> Result<Vec<CommitFileSummary>, BackendError> {
        // Collect both summaries for the whole commit. Spawning a separate
        // diff-tree process for every path makes opening large commits linear
        // in process startup cost and loses rename context in numstat.
        let parents = self.parents(root, hash).await?;
        let base = parents.first().map(String::as_str).unwrap_or(EMPTY_TREE);
        let output = self
            .runner
            .run(
                Some(root),
                [
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "-r",
                    "-M20%",
                    "--name-status",
                    "-z",
                    base,
                    hash,
                ],
            )
            .await?;
        let statistics = self
            .runner
            .run(
                Some(root),
                [
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "-r",
                    "-M20%",
                    "--numstat",
                    "-z",
                    base,
                    hash,
                ],
            )
            .await?;
        let mut statistics = parse_file_stats(&statistics.stdout)?;
        let mut tokens = output.stdout.split('\0').filter(|token| !token.is_empty());
        let mut files = Vec::new();
        while let Some(status) = tokens.next() {
            let first_path = tokens.next().ok_or_else(history_parse_error)?;
            let (old_path, path) = if status.starts_with('R') || status.starts_with('C') {
                let current = tokens.next().ok_or_else(history_parse_error)?;
                (Some(first_path.to_owned()), current.to_owned())
            } else {
                (None, first_path.to_owned())
            };
            let (additions, deletions) = statistics
                .remove(path.as_str())
                .ok_or_else(history_parse_error)?;
            files.push(CommitFileSummary {
                status: status.to_owned(),
                path,
                old_path,
                additions,
                deletions,
            });
        }
        if !statistics.is_empty() {
            return Err(history_parse_error());
        }
        Ok(files)
    }
}

type LineStats = (Option<u64>, Option<u64>);

fn parse_file_stats(output: &str) -> Result<HashMap<&str, LineStats>, BackendError> {
    let mut statistics = HashMap::new();
    let mut records = output.split_terminator('\0');
    while let Some(record) = records.next() {
        // Only the first two tabs are separators: filenames may contain tabs.
        let mut fields = record.splitn(3, '\t');
        let additions = parse_stat(fields.next())?;
        let deletions = parse_stat(fields.next())?;
        let path = fields.next().ok_or_else(history_parse_error)?;
        let path = if path.is_empty() {
            // With -z, renames/copies use: counts TAB NUL old NUL new NUL.
            let old = records.next().ok_or_else(history_parse_error)?;
            if old.is_empty() {
                return Err(history_parse_error());
            }
            records.next().ok_or_else(history_parse_error)?
        } else {
            path
        };
        if path.is_empty() || statistics.insert(path, (additions, deletions)).is_some() {
            return Err(history_parse_error());
        }
    }
    Ok(statistics)
}

fn is_recoverable_cherry_pick(state: &RepositoryOperationState) -> bool {
    state.kind == RepositoryOperationKind::CherryPick && !state.conflicts.is_empty()
}

fn parse_log(output: &str) -> Result<Vec<ParsedCommit>, BackendError> {
    output
        .split(RECORD_SEPARATOR)
        .filter(|record| !record.trim().is_empty())
        .map(|record| {
            let fields = record
                .trim_start_matches(['\r', '\n'])
                .split(FIELD_SEPARATOR)
                .collect::<Vec<_>>();
            if fields.len() != 8 {
                return Err(history_parse_error());
            }
            Ok(ParsedCommit {
                summary: CommitSummary {
                    hash: fields[0].to_owned(),
                    short_hash: fields[1].to_owned(),
                    parent_hashes: split_words(fields[2]),
                    subject: fields[3].to_owned(),
                    author_name: fields[4].to_owned(),
                    author_email: fields[5].to_owned(),
                    authored_at: fields[6].to_owned(),
                    references: split_decorations(fields[7]),
                    topology: CommitTopology {
                        lane: 0,
                        parents: Vec::new(),
                    },
                },
            })
        })
        .collect()
}

fn single_record(output: &str, expected: usize) -> Result<Vec<&str>, BackendError> {
    let record = output
        .split(RECORD_SEPARATOR)
        .find(|record| !record.trim().is_empty())
        .ok_or_else(history_parse_error)?;
    let fields = record
        .trim_start_matches(['\r', '\n'])
        .split(FIELD_SEPARATOR)
        .collect::<Vec<_>>();
    (fields.len() == expected)
        .then_some(fields)
        .ok_or_else(history_parse_error)
}

fn apply_topology(commits: &mut [CommitSummary], mut lanes: Vec<String>) -> Vec<String> {
    for commit in commits {
        let lane = lanes
            .iter()
            .position(|hash| hash == &commit.hash)
            .unwrap_or_else(|| {
                if lanes.len() == MAX_TOPOLOGY_LANES {
                    return MAX_TOPOLOGY_LANES - 1;
                }
                lanes.push(commit.hash.clone());
                lanes.len() - 1
            });
        if lane < lanes.len() {
            lanes.remove(lane);
        }
        if let Some(first_parent) = commit.parent_hashes.first()
            && !lanes.contains(first_parent)
        {
            lanes.insert(lane.min(lanes.len()), first_parent.clone());
        }
        for parent in commit.parent_hashes.iter().skip(1) {
            if lanes.len() < MAX_TOPOLOGY_LANES && !lanes.contains(parent) {
                lanes.push(parent.clone());
            }
        }
        let parents = commit
            .parent_hashes
            .iter()
            .filter_map(|hash| {
                lanes
                    .iter()
                    .position(|lane_hash| lane_hash == hash)
                    .map(|lane| TopologyParent {
                        hash: hash.clone(),
                        lane,
                    })
            })
            .collect();
        commit.topology = CommitTopology { lane, parents };
    }
    lanes
}

fn normalize_reference(reference: Option<&str>) -> Result<String, BackendError> {
    let reference = reference.unwrap_or("HEAD").trim();
    if reference.is_empty() {
        return Ok("HEAD".to_owned());
    }
    normalize_object_name(reference)
}

fn normalize_object_name(value: &str) -> Result<String, BackendError> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(BackendError::new(
            ErrorCode::InvalidReference,
            "引用名称无效。",
        ));
    }
    Ok(value.to_owned())
}

fn validate_relative_path(path: &str) -> Result<PathBuf, BackendError> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "只能查看当前仓库内的文件。",
        ));
    }
    Ok(path)
}

fn query_fingerprint(reference: &str, search: &str) -> String {
    let normalized = format!("{}\0{}", reference, search.to_lowercase());
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

fn encode_cursor(cursor: &HistoryCursor) -> Result<String, BackendError> {
    serde_json::to_vec(cursor)
        .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
        .map_err(|error| invalid_cursor().with_diagnostics(error.to_string()))
}

fn decode_cursor(cursor: &str) -> Result<HistoryCursor, BackendError> {
    if cursor.len() > MAX_CURSOR_BYTES {
        return Err(invalid_cursor());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|error| invalid_cursor().with_diagnostics(error.to_string()))?;
    let cursor: HistoryCursor = serde_json::from_slice(&bytes)
        .map_err(|error| invalid_cursor().with_diagnostics(error.to_string()))?;
    if cursor.version != 1 || cursor.lanes.len() > MAX_TOPOLOGY_LANES {
        return Err(invalid_cursor());
    }
    Ok(cursor)
}

fn split_words(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}

fn split_decorations(value: &str) -> Vec<String> {
    value
        .split(DECORATION_SEPARATOR)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_stat(value: Option<&str>) -> Result<Option<u64>, BackendError> {
    match value {
        Some("-") => Ok(None),
        Some(value) => value.parse().map(Some).map_err(|_| history_parse_error()),
        None => Err(history_parse_error()),
    }
}

fn is_hash_prefix(value: &str) -> bool {
    (4..=40).contains(&value.len()) && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn is_unborn_error(error: &BackendError) -> bool {
    error.diagnostics.as_deref().is_some_and(|diagnostics| {
        let diagnostics = diagnostics.to_ascii_lowercase();
        diagnostics.contains("needed a single revision")
            || diagnostics.contains("unknown revision")
            || diagnostics.contains("ambiguous argument")
    })
}

fn invalid_cursor() -> BackendError {
    BackendError::new(
        ErrorCode::InvalidHistoryCursor,
        "历史记录已变化，正在从第一页重新加载。",
    )
}

fn history_parse_error() -> BackendError {
    BackendError::new(ErrorCode::GitCommandFailed, "无法解析 Git 历史记录。")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_stats_preserve_special_paths_and_rename_destinations() {
        let stats = parse_file_stats(concat!(
            "2\t1\t文档\t说明\n.md\0",
            "-\t-\timage.bin\0",
            "0\t3\tdeleted.md\0",
            "2\t1\t\0old {name}.md\0new => name.md\0",
        ))
        .unwrap();
        assert_eq!(stats.len(), 4);
        assert_eq!(stats["文档\t说明\n.md"], (Some(2), Some(1)));
        assert_eq!(stats["image.bin"], (None, None));
        assert_eq!(stats["deleted.md"], (Some(0), Some(3)));
        assert_eq!(stats["new => name.md"], (Some(2), Some(1)));
        assert!(parse_file_stats("").unwrap().is_empty());
    }

    #[test]
    fn batch_stats_reject_malformed_records() {
        for output in [
            "bad\t1\tfile\0",
            "1\tfile\0",
            "1\t0\t\0old\0",
            "1\t0\t\0\0new\0",
            "1\t0\tfile\x001\t0\tfile\0",
        ] {
            assert!(parse_file_stats(output).is_err(), "{output:?}");
        }
    }

    #[test]
    fn parser_preserves_decorations_with_commas() {
        let output = "abc\u{1f}abc\u{1f}\u{1f}subject\u{1f}author\u{1f}mail\u{1f}2026-09-08T10:00:00+08:00\u{1f}refs/heads/with,comma\u{1d}tag: refs/tags/v1\u{1e}";
        let parsed = parse_log(output).unwrap();
        assert_eq!(
            parsed[0].summary.references,
            ["refs/heads/with,comma", "tag: refs/tags/v1"]
        );
    }

    #[test]
    fn topology_carries_parent_lanes_to_the_next_page() {
        let mut commits = vec![CommitSummary {
            hash: "merge".into(),
            short_hash: "merge".into(),
            parent_hashes: vec!["left".into(), "right".into()],
            subject: "merge".into(),
            author_name: "author".into(),
            author_email: "mail".into(),
            authored_at: "time".into(),
            references: Vec::new(),
            topology: CommitTopology {
                lane: 0,
                parents: Vec::new(),
            },
        }];
        let lanes = apply_topology(&mut commits, Vec::new());
        assert_eq!(lanes, ["left", "right"]);
        assert_eq!(commits[0].topology.parents.len(), 2);
    }
}
