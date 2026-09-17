use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::application::diff_parser::{parse_hunk_starts, parse_unified_diff};
use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::application::operation_state::{
    MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
};
use crate::domain::changes::{
    ChangeScope, ChangesSnapshot, CommitResult, DiffHunk, DiffLine, DiffLineKind, FileChange,
    FileDiff,
};
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::operation::MutationWorkspace;
use crate::infrastructure::git_runner::GitCommandRunner;

const MAX_DIFF_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[path = "changes_noise.rs"]
mod noise;
#[path = "changes_stats.rs"]
mod stats;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFilePatch {
    pub path: String,
    pub status: String,
    pub binary: bool,
    pub patch: String,
    pub changed_line_anchors: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPatchSnapshot {
    pub root: PathBuf,
    pub files: Vec<StagedFilePatch>,
}

#[derive(Debug, Clone)]
pub struct ChangesService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
}

impl Default for ChangesService {
    fn default() -> Self {
        Self::new(
            GitCommandRunner::default(),
            RepositoryMutationCoordinator::default(),
        )
    }
}

impl ChangesService {
    pub async fn review_snapshot(
        &self,
        root: &Path,
        source: crate::domain::ai::ReviewSource,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<super::review_snapshot::ReviewSnapshot, BackendError> {
        let _guard = tokio::select! { biased; _ = cancel.cancelled() => return Err(super::review_git::cancelled()), guard = self.coordinator.read(root) => guard };
        super::review_snapshot::ReviewSnapshot::capture(root, source, cancel).await
    }
    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
        }
    }

    pub async fn staged_patch_snapshot(
        &self,
        requested_root: &Path,
    ) -> Result<StagedPatchSnapshot, BackendError> {
        let root = repository_root(&self.runner, requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        let output = self
            .runner
            .run(
                Some(&root),
                ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )
            .await?;
        let status = parse_status(&output.stdout);
        let mut files = Vec::with_capacity(status.staged_count);

        for file in status.files.into_iter().filter(|file| file.staged) {
            let relative_path = validate_relative_path(&file.path)?;
            let patch = self
                .raw_diff(&root, &relative_path, ChangeScope::Staged, 3)
                .await?;
            let binary = patch.contains("Binary files ") || patch.contains("GIT binary patch");
            let changed_line_anchors = if binary {
                Vec::new()
            } else {
                parse_unified_diff(&patch)
                    .into_iter()
                    .flat_map(|hunk| hunk.lines)
                    .filter(|line| {
                        matches!(line.kind, DiffLineKind::Addition | DiffLineKind::Deletion)
                    })
                    .filter_map(|line| line.new_line.or(line.old_line))
                    .collect()
            };
            files.push(StagedFilePatch {
                path: file.path,
                status: file.index_status,
                binary,
                patch: if binary { String::new() } else { patch },
                changed_line_anchors,
            });
        }

        Ok(StagedPatchSnapshot { root, files })
    }

    pub async fn snapshot(&self, requested_root: &Path) -> Result<ChangesSnapshot, BackendError> {
        let root = repository_root(&self.runner, requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        self.snapshot_at_root(&root).await
    }

    pub(crate) async fn snapshot_at_root(
        &self,
        root: &Path,
    ) -> Result<ChangesSnapshot, BackendError> {
        let output = self
            .runner
            .run(
                Some(root),
                [
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--untracked-files=all",
                ],
            )
            .await?;
        Ok(parse_status(&output.stdout))
    }

    pub async fn file_diff(
        &self,
        requested_root: &Path,
        relative_path: &str,
        scope: ChangeScope,
    ) -> Result<FileDiff, BackendError> {
        let root = repository_root(&self.runner, requested_root).await?;
        let relative_path = validate_relative_path(relative_path)?;
        let _guard = self.coordinator.read(&root).await;
        let transport_path = path_for_transport(&relative_path);
        if scope == ChangeScope::Unstaged {
            let snapshot = self.snapshot_at_root(&root).await?;
            if snapshot
                .files
                .iter()
                .any(|file| file.path == transport_path && file.index_status == "?")
            {
                return read_untracked_diff(&root, &relative_path).await;
            }
        }
        let mut args = vec![
            OsString::from("diff"),
            OsString::from("--no-color"),
            OsString::from("--no-ext-diff"),
            OsString::from("--unified=3"),
        ];
        if scope == ChangeScope::Staged {
            args.push(OsString::from("--cached"));
        }
        args.push(OsString::from("--"));
        args.push(relative_path.as_os_str().to_os_string());
        let output = self.runner.run(Some(&root), args).await?;
        let binary =
            output.stdout.contains("Binary files ") || output.stdout.contains("GIT binary patch");

        Ok(FileDiff {
            path: transport_path,
            scope: scope.into(),
            binary,
            hunks: if binary {
                Vec::new()
            } else {
                parse_unified_diff(&output.stdout)
            },
        })
    }

    pub async fn stage_file(
        &self,
        requested_root: &Path,
        relative_path: &str,
    ) -> Result<MutationWorkspace, BackendError> {
        self.mutate_file(requested_root, relative_path, &["add"])
            .await
    }

    pub async fn unstage_file(
        &self,
        requested_root: &Path,
        relative_path: &str,
    ) -> Result<MutationWorkspace, BackendError> {
        let root = repository_root(&self.runner, requested_root).await?;
        let relative_path = validate_relative_path(relative_path)?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Changes)
            .await?;
        let restore = path_command(&["restore", "--staged"], &relative_path);
        if let Err(error) = self.runner.run(Some(&root), restore).await {
            if !is_missing_head_error(&error) {
                return Err(error);
            }
            let remove_cached = path_command(&["rm", "--cached"], &relative_path);
            self.runner.run(Some(&root), remove_cached).await?;
        }
        self.refreshed_workspace(&root).await
    }

    pub async fn stage_files(
        &self,
        root: &Path,
        paths: &[String],
    ) -> Result<MutationWorkspace, BackendError> {
        self.mutate_files(root, paths, ChangeScope::Unstaged).await
    }

    pub async fn unstage_files(
        &self,
        root: &Path,
        paths: &[String],
    ) -> Result<MutationWorkspace, BackendError> {
        self.mutate_files(root, paths, ChangeScope::Staged).await
    }

    async fn mutate_files(
        &self,
        requested_root: &Path,
        paths: &[String],
        scope: ChangeScope,
    ) -> Result<MutationWorkspace, BackendError> {
        if paths.is_empty() {
            return Err(BackendError::new(
                ErrorCode::InvalidPath,
                "请至少选择一个文件。",
            ));
        }
        let root = repository_root(&self.runner, requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Changes)
            .await?;
        let snapshot = self.snapshot_at_root(&root).await?;
        let mut selected = std::collections::BTreeSet::new();
        for path in paths {
            let path = path_for_transport(&validate_relative_path(path)?);
            let file = snapshot
                .files
                .iter()
                .find(|file| {
                    file.path == path
                        && !file.conflict
                        && if scope == ChangeScope::Staged {
                            file.staged
                        } else {
                            file.unstaged
                        }
                })
                .ok_or_else(|| {
                    BackendError::new(
                        ErrorCode::StaleFileOperation,
                        "所选文件状态已变化，请重新选择后重试。",
                    )
                })?;
            selected.insert(path);
            let status = if scope == ChangeScope::Staged {
                &file.index_status
            } else {
                &file.worktree_status
            };
            if status == "R" {
                if let Some(old_path) = &file.old_path {
                    selected.insert(path_for_transport(&validate_relative_path(old_path)?));
                }
            }
        }
        // One index transaction for the entire selection; literal NUL-separated
        // paths also handle whitespace/globs and avoid Windows command limits.
        let input = selected
            .iter()
            .flat_map(|path| path.as_bytes().iter().copied().chain([0]))
            .collect::<Vec<_>>();
        if scope == ChangeScope::Unstaged {
            self.runner
                .run_with_input(
                    Some(&root),
                    [
                        "--literal-pathspecs",
                        "add",
                        "--all",
                        "--pathspec-from-file=-",
                        "--pathspec-file-nul",
                    ],
                    input,
                )
                .await?;
        } else {
            let result = self
                .runner
                .run_with_input(
                    Some(&root),
                    [
                        "--literal-pathspecs",
                        "restore",
                        "--staged",
                        "--pathspec-from-file=-",
                        "--pathspec-file-nul",
                    ],
                    input.clone(),
                )
                .await;
            if let Err(error) = result {
                if !is_missing_head_error(&error) {
                    return Err(error);
                }
                self.runner
                    .run_with_input(
                        Some(&root),
                        [
                            "--literal-pathspecs",
                            "rm",
                            "--cached",
                            "--force",
                            "--pathspec-from-file=-",
                            "--pathspec-file-nul",
                        ],
                        input,
                    )
                    .await?;
            }
        }
        self.refreshed_workspace(&root).await
    }

    pub async fn discard_file(
        &self,
        requested_root: &Path,
        relative_path: &str,
    ) -> Result<MutationWorkspace, BackendError> {
        let root = repository_root(&self.runner, requested_root).await?;
        let relative_path = validate_relative_path(relative_path)?;
        let transport_path = path_for_transport(&relative_path);
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Changes)
            .await?;
        let snapshot = self.snapshot_at_root(&root).await?;
        let change = snapshot
            .files
            .iter()
            .find(|file| file.path == transport_path)
            .ok_or_else(|| {
                BackendError::new(
                    ErrorCode::InvalidPath,
                    "所选文件已不在变更列表中，请刷新后重试。",
                )
            })?;

        if change.index_status == "?" {
            remove_untracked_path(&root, &relative_path).await?;
        } else {
            let args = path_command(&["restore", "--worktree"], &relative_path);
            self.runner.run(Some(&root), args).await?;
        }
        self.refreshed_workspace(&root).await
    }

    pub async fn stage_hunk(
        &self,
        requested_root: &Path,
        relative_path: &str,
        hunk_index: usize,
    ) -> Result<MutationWorkspace, BackendError> {
        self.apply_hunk(
            requested_root,
            relative_path,
            hunk_index,
            ChangeScope::Unstaged,
            false,
        )
        .await
    }

    pub async fn unstage_hunk(
        &self,
        requested_root: &Path,
        relative_path: &str,
        hunk_index: usize,
    ) -> Result<MutationWorkspace, BackendError> {
        self.apply_hunk(
            requested_root,
            relative_path,
            hunk_index,
            ChangeScope::Staged,
            true,
        )
        .await
    }

    pub async fn stage_lines(
        &self,
        requested_root: &Path,
        relative_path: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<MutationWorkspace, BackendError> {
        self.apply_lines(
            requested_root,
            relative_path,
            start_line,
            end_line,
            ChangeScope::Unstaged,
            false,
        )
        .await
    }

    pub async fn unstage_lines(
        &self,
        requested_root: &Path,
        relative_path: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<MutationWorkspace, BackendError> {
        self.apply_lines(
            requested_root,
            relative_path,
            start_line,
            end_line,
            ChangeScope::Staged,
            true,
        )
        .await
    }

    pub async fn commit(
        &self,
        requested_root: &Path,
        message: &str,
    ) -> Result<CommitResult, BackendError> {
        let message = message.trim();
        if message.is_empty() {
            return Err(BackendError::new(
                ErrorCode::InvalidPath,
                "请输入提交说明。",
            ));
        }

        let root = repository_root(&self.runner, requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Commit)
            .await?;
        let staged = self
            .runner
            .run(Some(&root), ["diff", "--cached", "--name-only"])
            .await?;
        if staged.stdout.trim().is_empty() {
            return Err(BackendError::new(
                ErrorCode::DirtyWorktree,
                "没有可提交的已暂存变更。",
            ));
        }

        self.runner
            .run(
                Some(&root),
                [
                    OsString::from("commit"),
                    OsString::from("-m"),
                    OsString::from(message),
                ],
            )
            .await?;
        let hash = self
            .runner
            .run(Some(&root), ["rev-parse", "--short", "HEAD"])
            .await?
            .stdout
            .trim()
            .to_owned();
        let workspace = self.refreshed_workspace(&root).await?;

        Ok(CommitResult {
            short_hash: hash,
            subject: message.lines().next().unwrap_or_default().to_owned(),
            workspace,
        })
    }

    async fn apply_lines(
        &self,
        requested_root: &Path,
        relative_path: &str,
        start_line: u32,
        end_line: u32,
        scope: ChangeScope,
        reverse: bool,
    ) -> Result<MutationWorkspace, BackendError> {
        if start_line == 0 || end_line == 0 {
            return Err(BackendError::new(
                ErrorCode::InvalidPath,
                "所选行号必须为正整数。",
            ));
        }
        let root = repository_root(&self.runner, requested_root).await?;
        let relative_path = validate_relative_path(relative_path)?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Changes)
            .await?;
        if scope == ChangeScope::Unstaged {
            self.ensure_intent_to_add(&root, &relative_path).await?;
        }
        let raw_diff = self.raw_diff(&root, &relative_path, scope, 3).await?;
        let patch = select_line_patch(
            &raw_diff,
            start_line.min(end_line),
            start_line.max(end_line),
        )?;
        self.apply_cached_patch(&root, patch, reverse).await?;
        self.refreshed_workspace(&root).await
    }

    async fn apply_hunk(
        &self,
        requested_root: &Path,
        relative_path: &str,
        hunk_index: usize,
        scope: ChangeScope,
        reverse: bool,
    ) -> Result<MutationWorkspace, BackendError> {
        let root = repository_root(&self.runner, requested_root).await?;
        let relative_path = validate_relative_path(relative_path)?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Changes)
            .await?;
        if scope == ChangeScope::Unstaged {
            self.ensure_intent_to_add(&root, &relative_path).await?;
        }
        let raw_diff = self.raw_diff(&root, &relative_path, scope, 0).await?;
        let patch = select_hunk_patch(&raw_diff, hunk_index)?;
        self.apply_cached_patch(&root, patch, reverse).await?;
        self.refreshed_workspace(&root).await
    }

    async fn ensure_intent_to_add(
        &self,
        root: &Path,
        relative_path: &Path,
    ) -> Result<(), BackendError> {
        let transport_path = path_for_transport(relative_path);
        let snapshot = self.snapshot_at_root(root).await?;
        if snapshot
            .files
            .iter()
            .any(|file| file.path == transport_path && file.index_status == "?")
        {
            let args = path_command(&["add", "-N"], relative_path);
            self.runner.run(Some(root), args).await?;
        }
        Ok(())
    }

    async fn raw_diff(
        &self,
        root: &Path,
        relative_path: &Path,
        scope: ChangeScope,
        context: u8,
    ) -> Result<String, BackendError> {
        let mut args = vec![
            OsString::from("diff"),
            OsString::from("--no-color"),
            OsString::from("--no-ext-diff"),
            OsString::from(format!("--unified={context}")),
        ];
        if scope == ChangeScope::Staged {
            args.push(OsString::from("--cached"));
        }
        args.push(OsString::from("--"));
        args.push(relative_path.as_os_str().to_os_string());
        Ok(self.runner.run(Some(root), args).await?.stdout)
    }

    async fn apply_cached_patch(
        &self,
        root: &Path,
        patch: Vec<u8>,
        reverse: bool,
    ) -> Result<(), BackendError> {
        let mut args = vec!["apply", "--cached"];
        if reverse {
            args.push("--reverse");
        }
        args.extend(["--recount", "--unidiff-zero", "-"]);
        self.runner.run_with_input(Some(root), args, patch).await?;
        Ok(())
    }

    async fn mutate_file(
        &self,
        requested_root: &Path,
        relative_path: &str,
        command: &[&str],
    ) -> Result<MutationWorkspace, BackendError> {
        let root = repository_root(&self.runner, requested_root).await?;
        let relative_path = validate_relative_path(relative_path)?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_operation_allows(&root, MutationIntent::Changes)
            .await?;
        let args = path_command(command, &relative_path);
        self.runner.run(Some(&root), args).await?;
        self.refreshed_workspace(&root).await
    }

    async fn ensure_operation_allows(
        &self,
        root: &Path,
        intent: MutationIntent,
    ) -> Result<(), BackendError> {
        let state = read_operation_state(root, &self.runner).await?;
        ensure_mutation_allowed(&state, intent)
    }

    async fn refreshed_workspace(&self, root: &Path) -> Result<MutationWorkspace, BackendError> {
        refresh_mutation_workspace(root, &self.runner).await
    }
}

fn path_command(command: &[&str], relative_path: &Path) -> Vec<OsString> {
    let mut args = command.iter().map(OsString::from).collect::<Vec<_>>();
    args.push(OsString::from("--"));
    args.push(relative_path.as_os_str().to_os_string());
    args
}

fn is_missing_head_error(error: &BackendError) -> bool {
    if error.code != ErrorCode::GitCommandFailed {
        return false;
    }
    error.diagnostics.as_deref().is_some_and(|diagnostics| {
        let diagnostics = diagnostics.to_ascii_lowercase();
        diagnostics.contains("could not resolve head")
            || diagnostics.contains("needed a single revision")
            || diagnostics.contains("unknown revision")
            || diagnostics.contains("ambiguous argument 'head'")
    })
}

async fn remove_untracked_path(root: &Path, relative_path: &Path) -> Result<(), BackendError> {
    let target = root.join(relative_path).canonicalize().map_err(|error| {
        BackendError::new(ErrorCode::InvalidPath, "无法访问要丢弃的未跟踪文件。")
            .with_diagnostics(error.to_string())
    })?;
    if !target.starts_with(root) || target == root {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "只能丢弃当前仓库内的文件。",
        ));
    }
    let metadata = tokio::fs::metadata(&target).await?;
    if metadata.is_dir() {
        tokio::fs::remove_dir_all(target).await?;
    } else {
        tokio::fs::remove_file(target).await?;
    }
    Ok(())
}

fn select_hunk_patch(raw_diff: &str, hunk_index: usize) -> Result<Vec<u8>, BackendError> {
    let (preamble, hunks) = split_unified_patch(raw_diff);
    let hunk = hunks.get(hunk_index).ok_or_else(|| {
        BackendError::new(ErrorCode::InvalidPath, "所选 Diff 块已失效，请刷新后重试。")
    })?;
    if preamble.is_empty() {
        return Err(BackendError::new(
            ErrorCode::GitCommandFailed,
            "无法构建所选 Diff 块。",
        ));
    }
    Ok(format!("{preamble}{hunk}").into_bytes())
}

fn split_unified_patch(raw_diff: &str) -> (String, Vec<String>) {
    let mut preamble = String::new();
    let mut hunks = Vec::new();
    let mut current = String::new();

    for line in raw_diff.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            if !current.is_empty() {
                hunks.push(std::mem::take(&mut current));
            }
            current.push_str(line);
        } else if current.is_empty() {
            preamble.push_str(line);
        } else {
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        hunks.push(current);
    }
    (preamble, hunks)
}

fn select_line_patch(
    raw_diff: &str,
    start_line: u32,
    end_line: u32,
) -> Result<Vec<u8>, BackendError> {
    let (preamble, hunks) = split_unified_patch(raw_diff);
    let selected_hunks = hunks
        .iter()
        .filter_map(|hunk| filter_hunk_lines(hunk, start_line, end_line))
        .collect::<String>();
    if preamble.is_empty() || selected_hunks.is_empty() {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "所选行内没有可应用的变更，请刷新后重试。",
        ));
    }
    Ok(format!("{preamble}{selected_hunks}").into_bytes())
}

fn filter_hunk_lines(hunk: &str, start_line: u32, end_line: u32) -> Option<String> {
    let mut lines = hunk.split_inclusive('\n');
    let header = lines.next()?;
    let header_text = header.trim_end_matches(['\r', '\n']);
    let (_, mut new_line) = parse_hunk_starts(header_text)?;
    let mut body = String::new();
    let mut has_selection = false;
    let mut previous_included = true;

    for line in lines {
        let text = line.trim_end_matches(['\r', '\n']);
        let newline = if line.ends_with('\n') { "\n" } else { "" };
        if let Some(content) = text.strip_prefix('+') {
            let selected = (start_line..=end_line).contains(&new_line);
            new_line += 1;
            previous_included = selected;
            if selected {
                has_selection = true;
                body.push('+');
                body.push_str(content);
                body.push_str(newline);
            }
        } else if let Some(content) = text.strip_prefix('-') {
            let anchor = new_line.max(1);
            let selected = (start_line..=end_line).contains(&anchor);
            previous_included = true;
            if selected {
                has_selection = true;
                body.push('-');
            } else {
                body.push(' ');
            }
            body.push_str(content);
            body.push_str(newline);
        } else if let Some(content) = text.strip_prefix(' ') {
            new_line += 1;
            previous_included = true;
            body.push(' ');
            body.push_str(content);
            body.push_str(newline);
        } else if previous_included {
            body.push_str(text);
            body.push_str(newline);
        }
    }

    has_selection.then(|| format!("{header}{body}"))
}

async fn read_untracked_diff(root: &Path, relative_path: &Path) -> Result<FileDiff, BackendError> {
    let target = root.join(relative_path).canonicalize().map_err(|error| {
        BackendError::new(ErrorCode::InvalidPath, "无法读取未跟踪文件。")
            .with_diagnostics(error.to_string())
    })?;
    if !target.starts_with(root) {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "文件路径必须位于当前仓库内。",
        ));
    }
    let metadata = tokio::fs::metadata(&target).await?;
    if !metadata.is_file() || metadata.len() > MAX_DIFF_FILE_BYTES {
        return Ok(binary_file_diff(relative_path));
    }
    let bytes = tokio::fs::read(&target).await?;
    if bytes.contains(&0) {
        return Ok(binary_file_diff(relative_path));
    }
    let text = String::from_utf8(bytes).map_err(|error| {
        BackendError::new(
            ErrorCode::UnsupportedEncoding,
            "文件不是有效的 UTF-8 文本。",
        )
        .with_diagnostics(error.to_string())
    })?;
    let lines = text
        .split_terminator('\n')
        .enumerate()
        .map(|(index, content)| DiffLine {
            kind: DiffLineKind::Addition,
            old_line: None,
            new_line: Some(index as u32 + 1),
            content: content.strip_suffix('\r').unwrap_or(content).to_owned(),
        })
        .collect::<Vec<_>>();
    let hunks = if lines.is_empty() {
        Vec::new()
    } else {
        vec![DiffHunk {
            index: 0,
            header: format!("@@ -0,0 +1,{} @@", lines.len()),
            lines,
        }]
    };
    Ok(FileDiff {
        path: path_for_transport(relative_path),
        scope: ChangeScope::Unstaged.into(),
        binary: false,
        hunks,
    })
}

fn binary_file_diff(relative_path: &Path) -> FileDiff {
    FileDiff {
        path: path_for_transport(relative_path),
        scope: ChangeScope::Unstaged.into(),
        binary: true,
        hunks: Vec::new(),
    }
}

fn validate_relative_path(path: &str) -> Result<PathBuf, BackendError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "文件路径必须位于当前仓库内。",
        ));
    }
    Ok(path.to_path_buf())
}

fn path_for_transport(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) async fn repository_root(
    runner: &GitCommandRunner,
    requested_root: &Path,
) -> Result<PathBuf, BackendError> {
    let requested_root = requested_root.canonicalize().map_err(|error| {
        BackendError::new(ErrorCode::InvalidPath, "无法访问仓库路径。")
            .with_diagnostics(error.to_string())
    })?;
    let output = runner
        .run(Some(&requested_root), ["rev-parse", "--show-toplevel"])
        .await?;
    Path::new(output.stdout.trim())
        .canonicalize()
        .map_err(|error| {
            BackendError::new(ErrorCode::InvalidPath, "无法解析仓库根目录。")
                .with_diagnostics(error.to_string())
        })
}

fn parse_status(output: &str) -> ChangesSnapshot {
    let mut records = output.split('\0').filter(|record| !record.is_empty());
    let mut files = Vec::new();

    while let Some(record) = records.next() {
        let bytes = record.as_bytes();
        if bytes.len() < 3 {
            continue;
        }
        let index = bytes[0] as char;
        let worktree = bytes[1] as char;
        let path = record[3..].to_owned();
        let old_path = (matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C'))
            .then(|| records.next().map(str::to_owned))
            .flatten();
        let staged = index != ' ' && index != '?';
        let unstaged = worktree != ' ' || (index == '?' && worktree == '?');
        let pair = &record[..2];
        let conflict = matches!(pair, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU");

        files.push(FileChange {
            path,
            old_path,
            index_status: index.to_string(),
            worktree_status: worktree.to_string(),
            staged,
            unstaged,
            conflict,
        });
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    ChangesSnapshot {
        staged_count: files.iter().filter(|file| file.staged).count(),
        unstaged_count: files.iter().filter(|file| file.unstaged).count(),
        files,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::domain::changes::{ChangeScope, DiffLineKind};
    use crate::infrastructure::git_runner::GitCommandRunner;

    async fn run_git(directory: &std::path::Path, args: &[&str]) {
        GitCommandRunner::default()
            .run(Some(directory), args)
            .await
            .unwrap();
    }

    async fn changes_fixture() -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        run_git(directory.path(), &["init", "-b", "main"]).await;
        run_git(directory.path(), &["config", "user.name", "HQ Test"]).await;
        run_git(
            directory.path(),
            &["config", "user.email", "hq@example.test"],
        )
        .await;
        run_git(directory.path(), &["config", "core.autocrlf", "false"]).await;
        std::fs::write(directory.path().join("old.txt"), "old\n").unwrap();
        std::fs::write(directory.path().join("both.txt"), "base\n").unwrap();
        run_git(directory.path(), &["add", "."]).await;
        run_git(directory.path(), &["commit", "-m", "base"]).await;

        std::fs::rename(
            directory.path().join("old.txt"),
            directory.path().join("renamed.txt"),
        )
        .unwrap();
        run_git(directory.path(), &["add", "-A"]).await;
        std::fs::write(directory.path().join("both.txt"), "staged\n").unwrap();
        run_git(directory.path(), &["add", "both.txt"]).await;
        std::fs::write(directory.path().join("both.txt"), "unstaged\n").unwrap();
        std::fs::write(directory.path().join("new.txt"), "new\n").unwrap();
        directory
    }

    async fn modified_text_fixture() -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        run_git(directory.path(), &["init", "-b", "main"]).await;
        run_git(directory.path(), &["config", "user.name", "HQ Test"]).await;
        run_git(
            directory.path(),
            &["config", "user.email", "hq@example.test"],
        )
        .await;
        run_git(directory.path(), &["config", "core.autocrlf", "false"]).await;
        std::fs::write(directory.path().join("note.txt"), "base\n").unwrap();
        run_git(directory.path(), &["add", "note.txt"]).await;
        run_git(directory.path(), &["commit", "-m", "base"]).await;
        std::fs::write(directory.path().join("note.txt"), "changed\n").unwrap();
        directory
    }

    async fn multi_hunk_fixture() -> TempDir {
        let directory = modified_text_fixture().await;
        let base = (1..=14)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(directory.path().join("note.txt"), &base).unwrap();
        run_git(directory.path(), &["add", "note.txt"]).await;
        run_git(directory.path(), &["commit", "-m", "multi hunk base"]).await;
        let changed = base
            .replace("line 2\n", "changed 2\n")
            .replace("line 12\n", "changed 12\n");
        std::fs::write(directory.path().join("note.txt"), changed).unwrap();
        directory
    }

    async fn same_hunk_fixture() -> TempDir {
        let directory = modified_text_fixture().await;
        let base = (1..=8)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(directory.path().join("note.txt"), &base).unwrap();
        run_git(directory.path(), &["add", "note.txt"]).await;
        run_git(directory.path(), &["commit", "-m", "line base"]).await;
        let changed = base
            .replace("line 2\n", "changed 2\n")
            .replace("line 4\n", "changed 4\n");
        std::fs::write(directory.path().join("note.txt"), changed).unwrap();
        directory
    }

    #[tokio::test]
    async fn snapshot_separates_staged_unstaged_and_renamed_paths() {
        let fixture = changes_fixture().await;

        let snapshot = ChangesService::default()
            .snapshot(fixture.path())
            .await
            .unwrap();

        assert_eq!(snapshot.staged_count, 2);
        assert_eq!(snapshot.unstaged_count, 2);
        assert!(snapshot.files.iter().any(|file| {
            file.path == "renamed.txt" && file.old_path.as_deref() == Some("old.txt")
        }));
    }

    #[tokio::test]
    async fn text_diff_returns_typed_hunks_and_line_numbers() {
        let fixture = changes_fixture().await;

        let diff = ChangesService::default()
            .file_diff(fixture.path(), "both.txt", ChangeScope::Unstaged)
            .await
            .unwrap();

        assert!(!diff.binary);
        assert_eq!(diff.hunks[0].index, 0);
        assert!(diff.hunks[0].lines.iter().any(|line| {
            line.kind == DiffLineKind::Deletion
                && line.old_line == Some(1)
                && line.new_line.is_none()
        }));
        assert!(diff.hunks[0].lines.iter().any(|line| {
            line.kind == DiffLineKind::Addition
                && line.old_line.is_none()
                && line.new_line == Some(1)
                && line.content == "unstaged"
        }));
    }

    #[tokio::test]
    async fn untracked_text_is_rendered_as_additions_without_staging_it() {
        let fixture = changes_fixture().await;
        let service = ChangesService::default();

        let diff = service
            .file_diff(fixture.path(), "new.txt", ChangeScope::Unstaged)
            .await
            .unwrap();
        let snapshot = service.snapshot(fixture.path()).await.unwrap();

        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].lines[0].kind, DiffLineKind::Addition);
        assert_eq!(diff.hunks[0].lines[0].content, "new");
        let file = snapshot
            .files
            .iter()
            .find(|file| file.path == "new.txt")
            .unwrap();
        assert!(!file.staged);
        assert!(file.unstaged);
    }

    #[test]
    fn status_parser_preserves_worktree_rename_source_path() {
        let snapshot = parse_status(" R renamed.txt\0old.txt\0");

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].path, "renamed.txt");
        assert_eq!(snapshot.files[0].old_path.as_deref(), Some("old.txt"));
        assert!(!snapshot.files[0].staged);
        assert!(snapshot.files[0].unstaged);
    }

    #[tokio::test]
    async fn untracked_binary_and_invalid_utf8_have_explicit_results() {
        let fixture = changes_fixture().await;
        std::fs::write(fixture.path().join("image.bin"), [1, 0, 2, 3]).unwrap();
        std::fs::write(fixture.path().join("legacy.txt"), [0xff, 0xfe]).unwrap();
        let service = ChangesService::default();

        let binary = service
            .file_diff(fixture.path(), "image.bin", ChangeScope::Unstaged)
            .await
            .unwrap();
        let encoding_error = service
            .file_diff(fixture.path(), "legacy.txt", ChangeScope::Unstaged)
            .await
            .unwrap_err();

        assert!(binary.binary);
        assert!(binary.hunks.is_empty());
        assert_eq!(encoding_error.code, ErrorCode::UnsupportedEncoding);
    }

    #[tokio::test]
    async fn diff_rejects_path_traversal_and_supports_spaces() {
        let fixture = changes_fixture().await;
        std::fs::write(fixture.path().join("with space.txt"), "base\n").unwrap();
        run_git(fixture.path(), &["add", "with space.txt"]).await;
        run_git(fixture.path(), &["commit", "-m", "space fixture"]).await;
        std::fs::write(fixture.path().join("with space.txt"), "changed\n").unwrap();
        let service = ChangesService::default();

        let outside = service
            .file_diff(fixture.path(), "../outside.txt", ChangeScope::Unstaged)
            .await
            .unwrap_err();
        let spaced = service
            .file_diff(fixture.path(), "with space.txt", ChangeScope::Unstaged)
            .await
            .unwrap();

        assert_eq!(outside.code, ErrorCode::InvalidPath);
        assert_eq!(spaced.path, "with space.txt");
        assert_eq!(spaced.hunks.len(), 1);
    }

    #[test]
    fn parser_keeps_deletion_only_line_numbers() {
        let hunks =
            parse_unified_diff("diff --git a/note.txt b/note.txt\n@@ -1 +0,0 @@\n-removed\n");

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].lines[0].kind, DiffLineKind::Deletion);
        assert_eq!(hunks[0].lines[0].old_line, Some(1));
        assert_eq!(hunks[0].lines[0].new_line, None);
    }

    #[tokio::test]
    async fn file_mutations_return_authoritative_snapshots() {
        let fixture = modified_text_fixture().await;
        let service = ChangesService::default();

        let staged = service
            .stage_file(fixture.path(), "note.txt")
            .await
            .unwrap();
        assert_eq!(staged.changes.staged_count, 1);
        assert_eq!(staged.changes.unstaged_count, 0);
        assert_eq!(staged.repository.changed_file_count, 1);
        assert_eq!(
            staged.operation_state.kind,
            crate::domain::operation::RepositoryOperationKind::None
        );

        let unstaged = service
            .unstage_file(fixture.path(), "note.txt")
            .await
            .unwrap();
        assert_eq!(unstaged.changes.staged_count, 0);
        assert_eq!(unstaged.changes.unstaged_count, 1);
        assert_eq!(unstaged.repository.changed_file_count, 1);
    }

    #[tokio::test]
    async fn sequencer_state_blocks_ordinary_changes_mutations() {
        let fixture = modified_text_fixture().await;
        std::fs::write(
            fixture.path().join(".git").join("CHERRY_PICK_HEAD"),
            "a".repeat(40),
        )
        .unwrap();

        let error = ChangesService::default()
            .stage_file(fixture.path(), "note.txt")
            .await
            .unwrap_err();
        let snapshot = ChangesService::default()
            .snapshot(fixture.path())
            .await
            .unwrap();

        assert_eq!(error.code, ErrorCode::GitOperationInProgress);
        assert_eq!(snapshot.staged_count, 0);
        assert_eq!(snapshot.unstaged_count, 1);
    }

    #[tokio::test]
    async fn unstage_file_supports_repository_without_head() {
        let fixture = tempfile::tempdir().unwrap();
        run_git(fixture.path(), &["init", "-b", "main"]).await;
        run_git(fixture.path(), &["config", "core.autocrlf", "false"]).await;
        std::fs::write(fixture.path().join("first.txt"), "first\n").unwrap();
        let service = ChangesService::default();

        service
            .stage_file(fixture.path(), "first.txt")
            .await
            .unwrap();
        let result = service
            .unstage_file(fixture.path(), "first.txt")
            .await
            .unwrap();

        assert_eq!(result.changes.staged_count, 0);
        assert_eq!(result.changes.unstaged_count, 1);
        assert_eq!(result.changes.files[0].index_status, "?");
    }

    #[tokio::test]
    async fn hunk_mutations_change_only_the_selected_hunk() {
        let fixture = multi_hunk_fixture().await;
        let service = ChangesService::default();
        let initial = service
            .file_diff(fixture.path(), "note.txt", ChangeScope::Unstaged)
            .await
            .unwrap();
        assert_eq!(initial.hunks.len(), 2);

        let staged = service
            .stage_hunk(fixture.path(), "note.txt", 0)
            .await
            .unwrap();
        let cached = service
            .file_diff(fixture.path(), "note.txt", ChangeScope::Staged)
            .await
            .unwrap();
        let remaining = service
            .file_diff(fixture.path(), "note.txt", ChangeScope::Unstaged)
            .await
            .unwrap();

        assert_eq!(staged.changes.staged_count, 1);
        assert_eq!(staged.changes.unstaged_count, 1);
        assert_eq!(cached.hunks.len(), 1);
        assert!(
            cached.hunks[0]
                .lines
                .iter()
                .any(|line| line.content == "changed 2")
        );
        assert_eq!(remaining.hunks.len(), 1);
        assert!(
            remaining.hunks[0]
                .lines
                .iter()
                .any(|line| line.content == "changed 12")
        );

        let unstaged = service
            .unstage_hunk(fixture.path(), "note.txt", 0)
            .await
            .unwrap();
        assert_eq!(unstaged.changes.staged_count, 0);
        assert_eq!(unstaged.changes.unstaged_count, 1);
    }

    #[tokio::test]
    async fn line_mutations_preserve_unselected_changes_in_the_same_hunk() {
        let fixture = same_hunk_fixture().await;
        let service = ChangesService::default();

        service
            .stage_lines(fixture.path(), "note.txt", 2, 2)
            .await
            .unwrap();
        let cached = service
            .file_diff(fixture.path(), "note.txt", ChangeScope::Staged)
            .await
            .unwrap();
        let remaining = service
            .file_diff(fixture.path(), "note.txt", ChangeScope::Unstaged)
            .await
            .unwrap();

        assert!(
            cached.hunks[0]
                .lines
                .iter()
                .any(|line| { line.kind == DiffLineKind::Addition && line.content == "changed 2" })
        );
        assert!(
            !cached.hunks[0]
                .lines
                .iter()
                .any(|line| { line.kind == DiffLineKind::Addition && line.content == "changed 4" })
        );
        assert!(
            remaining.hunks[0]
                .lines
                .iter()
                .any(|line| { line.kind == DiffLineKind::Addition && line.content == "changed 4" })
        );
        assert!(
            !remaining.hunks[0]
                .lines
                .iter()
                .any(|line| { line.kind == DiffLineKind::Addition && line.content == "changed 2" })
        );

        let unstaged = service
            .unstage_lines(fixture.path(), "note.txt", 2, 2)
            .await
            .unwrap();
        assert_eq!(unstaged.changes.staged_count, 0);
        assert_eq!(unstaged.changes.unstaged_count, 1);
    }

    #[tokio::test]
    async fn discard_restores_tracked_and_removes_untracked_files() {
        let fixture = modified_text_fixture().await;
        std::fs::write(fixture.path().join("scratch.txt"), "temporary\n").unwrap();
        let service = ChangesService::default();

        let after_tracked = service
            .discard_file(fixture.path(), "note.txt")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.path().join("note.txt")).unwrap(),
            "base\n"
        );
        assert_eq!(after_tracked.changes.unstaged_count, 1);

        let after_untracked = service
            .discard_file(fixture.path(), "scratch.txt")
            .await
            .unwrap();
        assert!(!fixture.path().join("scratch.txt").exists());
        assert_eq!(after_untracked.changes.unstaged_count, 0);

        let outside = service
            .discard_file(fixture.path(), "../outside.txt")
            .await
            .unwrap_err();
        assert_eq!(outside.code, ErrorCode::InvalidPath);
    }

    #[tokio::test]
    async fn commit_rejects_empty_message() {
        let fixture = modified_text_fixture().await;
        run_git(fixture.path(), &["add", "note.txt"]).await;

        let error = ChangesService::default()
            .commit(fixture.path(), "   ")
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidPath);
    }

    #[tokio::test]
    async fn commit_rejects_repository_without_staged_changes() {
        let fixture = modified_text_fixture().await;

        let error = ChangesService::default()
            .commit(fixture.path(), "feat: nothing staged")
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::DirtyWorktree);
    }

    #[tokio::test]
    async fn commit_returns_hash_and_preserves_unstaged_changes() {
        let fixture = modified_text_fixture().await;
        run_git(fixture.path(), &["add", "note.txt"]).await;
        std::fs::write(fixture.path().join("scratch.txt"), "not staged\n").unwrap();

        let result = ChangesService::default()
            .commit(fixture.path(), "  feat: commit staged change  ")
            .await
            .unwrap();
        let head = GitCommandRunner::default()
            .run(Some(fixture.path()), ["rev-parse", "--short", "HEAD"])
            .await
            .unwrap();

        assert_eq!(result.short_hash, head.stdout.trim());
        assert_eq!(result.subject, "feat: commit staged change");
        assert_eq!(
            result.workspace.repository.head_short_hash,
            Some(result.short_hash)
        );
        assert_eq!(result.workspace.changes.staged_count, 0);
        assert_eq!(result.workspace.changes.unstaged_count, 1);
        assert_eq!(result.workspace.changes.files[0].path, "scratch.txt");
    }

    #[tokio::test]
    async fn native_smoke_supports_unicode_binary_and_recovers_from_stale_hunk() {
        let fixture = modified_text_fixture().await;
        let unicode_directory = fixture.path().join("src").join("中文路径");
        std::fs::create_dir_all(&unicode_directory).unwrap();
        let unicode_path = unicode_directory.join("界面验证.ts");
        std::fs::write(&unicode_path, "export const value = 1;\n").unwrap();
        run_git(fixture.path(), &["add", "src/中文路径/界面验证.ts"]).await;
        run_git(fixture.path(), &["commit", "-m", "unicode base"]).await;
        std::fs::write(&unicode_path, "export const value = 2;\n").unwrap();
        std::fs::write(fixture.path().join("preview.bin"), [1, 0, 2, 3]).unwrap();
        let service = ChangesService::default();

        let unicode_diff = service
            .file_diff(
                fixture.path(),
                "src/中文路径/界面验证.ts",
                ChangeScope::Unstaged,
            )
            .await
            .unwrap();
        let binary_diff = service
            .file_diff(fixture.path(), "preview.bin", ChangeScope::Unstaged)
            .await
            .unwrap();
        let stale = service
            .stage_hunk(fixture.path(), "src/中文路径/界面验证.ts", 99)
            .await
            .unwrap_err();
        let recovered = service
            .stage_file(fixture.path(), "src/中文路径/界面验证.ts")
            .await
            .unwrap();

        assert_eq!(unicode_diff.hunks.len(), 1);
        assert!(unicode_diff.hunks[0].lines.iter().any(|line| {
            line.kind == DiffLineKind::Addition && line.content == "export const value = 2;"
        }));
        assert!(binary_diff.binary);
        assert_eq!(stale.code, ErrorCode::InvalidPath);
        assert!(recovered.changes.files.iter().any(|file| {
            file.path == "src/中文路径/界面验证.ts" && file.staged && !file.unstaged
        }));
    }
}
