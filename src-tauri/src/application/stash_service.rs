use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::application::changes_service::ChangesService;
use crate::application::diff_parser::parse_unified_diff;
use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::application::operation_state::{
    MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
};
use crate::application::repository_service::RepositoryService;
use crate::domain::changes::{DiffScope, FileDiff};
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::stash::{
    StashCreateRequest, StashDetail, StashEntry, StashFileSummary, StashMutationOutcome,
    StashMutationResult, StashSelection, StashSnapshot,
};
use crate::infrastructure::git_runner::GitCommandRunner;

const STASH_LIST_FORMAT: &str = "--format=%gd%x00%H%x00%gs%x00%cI%x00";

#[derive(Debug, Clone)]
pub struct StashService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
}

impl Default for StashService {
    fn default() -> Self {
        Self::new(
            GitCommandRunner::default(),
            RepositoryMutationCoordinator::default(),
        )
    }
}

impl StashService {
    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
        }
    }

    pub async fn snapshot(&self, requested_root: &Path) -> Result<StashSnapshot, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        self.snapshot_at_root(&root).await
    }

    pub async fn detail(
        &self,
        requested_root: &Path,
        selection: StashSelection,
    ) -> Result<StashDetail, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        let entry = self.resolve_selection(&root, &selection).await?;
        let files = self.files(&root, &entry.object_id).await?;
        Ok(StashDetail { entry, files })
    }

    pub async fn file_diff(
        &self,
        requested_root: &Path,
        selection: StashSelection,
        path: &str,
        untracked: bool,
    ) -> Result<FileDiff, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        let entry = self.resolve_selection(&root, &selection).await?;
        let files = self.files(&root, &entry.object_id).await?;
        let path = path.replace('\\', "/");
        let file = files
            .iter()
            .find(|file| file.path == path && file.untracked == untracked)
            .ok_or_else(|| {
                BackendError::new(ErrorCode::InvalidPath, "所选文件不属于当前 Stash。")
            })?;
        let parents = self.parents(&root, &entry.object_id).await?;
        let (base, target) = if file.untracked {
            (self.empty_tree(&root).await?, parents[2].clone())
        } else {
            (parents[0].clone(), entry.object_id)
        };
        let mut args = vec![
            "--literal-pathspecs",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=3",
            "-M20%",
            base.as_str(),
            target.as_str(),
            "--",
            file.path.as_str(),
        ];
        if let Some(old_path) = &file.old_path {
            args.push(old_path);
        }
        let output = self.runner.run(Some(&root), args).await?;
        Ok(FileDiff {
            path,
            scope: DiffScope::Commit,
            binary: file.binary,
            hunks: if file.binary {
                Vec::new()
            } else {
                parse_unified_diff(&output.stdout)
            },
        })
    }

    pub async fn create(
        &self,
        requested_root: &Path,
        request: StashCreateRequest,
    ) -> Result<StashMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_write_allowed(&root).await?;
        let selected_paths = if let Some(paths) = &request.paths {
            if paths.is_empty() {
                return Err(BackendError::new(
                    ErrorCode::InvalidPath,
                    "请至少选择一个要贮藏的文件。",
                ));
            }
            let snapshot = ChangesService::default().snapshot_at_root(&root).await?;
            let mut selected = Vec::new();
            for path in paths {
                let file = snapshot
                    .files
                    .iter()
                    .find(|file| {
                        &file.path == path
                            && (request.include_untracked || file.index_status != "?")
                            && !file.conflict
                    })
                    .ok_or_else(|| {
                        BackendError::new(
                            ErrorCode::InvalidPath,
                            "所选文件状态已变化，请重新选择要贮藏的文件。",
                        )
                    })?;
                if !selected.contains(&file.path) {
                    selected.push(file.path.clone());
                }
                if let Some(old_path) = &file.old_path {
                    if !selected.contains(old_path) {
                        selected.push(old_path.clone());
                    }
                }
            }
            if !request.include_untracked
                && snapshot
                    .files
                    .iter()
                    .any(|file| file.index_status == "?" && selected.contains(&file.path))
            {
                return Err(BackendError::new(
                    ErrorCode::InvalidPath,
                    "所选路径同时存在未跟踪内容，请勾选“包含未跟踪文件”后重试。",
                ));
            }
            Some(selected)
        } else {
            None
        };
        let changes = self
            .runner
            .run(
                Some(&root),
                [
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--ignore-submodules=all",
                    if request.include_untracked {
                        "--untracked-files=all"
                    } else {
                        "--untracked-files=no"
                    },
                ],
            )
            .await?;
        if changes.stdout.is_empty() {
            return self
                .refresh(&root, StashMutationOutcome::NoChanges, None)
                .await;
        }
        let mut args = vec!["stash", "push"];
        // -u lets native stash accept staged deletions/rename sources that are
        // absent from the index. Selected paths were checked above, so this
        // cannot include unchosen untracked files or bypass the opt-in gate.
        if request.include_untracked || selected_paths.is_some() {
            args.push("--include-untracked");
        }
        if let Some(message) = request
            .message
            .as_deref()
            .map(str::trim)
            .filter(|message| !message.is_empty())
        {
            args.extend(["--message", message]);
        }
        let error = if let Some(paths) = &selected_paths {
            self.create_selected(&root, paths, args).await.err()
        } else {
            self.runner
                .run_allowing_failure(Some(&root), args)
                .await
                .and_then(|output| output.into_result())
                .err()
        };
        self.refresh(
            &root,
            if error.is_some() {
                StashMutationOutcome::Retained
            } else {
                StashMutationOutcome::Created
            },
            error,
        )
        .await
    }

    async fn selected_index(&self, root: &Path, paths: &[String]) -> Result<String, BackendError> {
        let index = self
            .runner
            .run(Some(root), ["ls-files", "--stage", "-z"])
            .await?;
        let paths: HashSet<&str> = paths.iter().map(String::as_str).collect();
        Ok(index
            .stdout
            .split('\0')
            .filter(|entry| {
                entry
                    .split_once('\t')
                    .is_some_and(|(_, path)| paths.contains(path))
            })
            .map(|entry| format!("{entry}\0"))
            .collect())
    }

    async fn create_selected(
        &self,
        root: &Path,
        paths: &[String],
        mut args: Vec<&str>,
    ) -> Result<(), BackendError> {
        let head = self
            .runner
            .run(Some(root), ["rev-parse", "--verify", "HEAD"])
            .await?
            .stdout;
        let head = head.trim();
        let original_index = self.selected_index(root, paths).await?;
        let temporary = tempfile::tempdir()?;
        let index = temporary.path().join("index");
        self.runner
            .run_with_index(root, &index, ["read-tree", head], None)
            .await?;
        // Start with HEAD, remove selected paths, then add ONLY their original
        // staged entries. Native stash push otherwise captures unrelated staged
        // files even when its pathspec limits worktree cleanup.
        let zero = "0".repeat(head.len());
        let mut entries = paths
            .iter()
            .map(|path| format!("0 {zero}\t{path}\0"))
            .collect::<String>();
        entries.push_str(&original_index);
        self.runner
            .run_with_index(
                root,
                &index,
                ["update-index", "-z", "--index-info"],
                Some(entries.into_bytes()),
            )
            .await?;
        let expected_index_tree = self
            .runner
            .run_with_index(root, &index, ["write-tree"], None)
            .await?
            .stdout;
        let before_stashes = self.snapshot_at_root(root).await?;
        let pathspec = paths
            .iter()
            .flat_map(|path| path.as_bytes().iter().copied().chain([0]))
            .collect::<Vec<u8>>();
        // Explicit literal pathspecs avoid exporting GIT_LITERAL_PATHSPECS into
        // stash's internal clean commands (which use their own magic paths).
        let stash_pathspec = paths
            .iter()
            .map(|path| format!(":(literal){path}\0"))
            .collect::<String>()
            .into_bytes();
        args.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
        let pushed = self
            .runner
            .run_with_index(root, &index, args, Some(stash_pathspec))
            .await;
        let after_stashes = self.snapshot_at_root(root).await?;
        let saved = if after_stashes.entries.len() == before_stashes.entries.len() + 1 {
            let entry = &after_stashes.entries[0];
            let parents = self.parents(root, &entry.object_id).await?;
            let saved_index = self
                .runner
                .run(
                    Some(root),
                    ["rev-parse", &format!("{}^2^{{tree}}", entry.object_id)],
                )
                .await?;
            parents.first().is_some_and(|parent| parent == head)
                && saved_index.stdout.trim() == expected_index_tree.trim()
        } else {
            false
        };
        if !saved {
            pushed?;
            return Err(BackendError::new(
                ErrorCode::StaleFileOperation,
                "没有确认到新的贮藏，文件和暂存区未进一步清理，请刷新后重试。",
            ));
        }
        // The selected worktree content is now saved. Only reset the real index
        // if no external Git process changed these entries or HEAD meanwhile.
        let current_head = self
            .runner
            .run(Some(root), ["rev-parse", "--verify", "HEAD"])
            .await?;
        if current_head.stdout.trim() != head
            || self.selected_index(root, paths).await? != original_index
        {
            return Err(BackendError::new(
                ErrorCode::StaleFileOperation,
                "贮藏已保存，但 Git 状态在操作期间变化，未清理暂存区。请检查贮藏与文件状态。",
            ));
        }
        if let Err(push_error) = pushed {
            // Some Git versions save staged deletions/renames successfully, then
            // fail their pathspec cleanup because the old path left the index.
            // The saved HEAD/index tree above proves the backup exists. Finish
            // cleanup using only selected tracked paths in the real index.
            let base_paths = self
                .runner
                .run(Some(root), ["ls-tree", "-r", "--name-only", "-z", head])
                .await?
                .stdout;
            let mut tracked: HashSet<&str> = base_paths
                .split('\0')
                .filter(|path| !path.is_empty())
                .collect();
            tracked.extend(
                original_index
                    .split('\0')
                    .filter_map(|entry| entry.split_once('\t').map(|(_, path)| path)),
            );
            let cleanup = paths
                .iter()
                .filter(|path| tracked.contains(path.as_str()))
                .flat_map(|path| path.as_bytes().iter().copied().chain([0]))
                .collect::<Vec<u8>>();
            if cleanup.is_empty() {
                return Err(push_error);
            }
            self.runner
                .run_with_input(
                    Some(root),
                    [
                        "--literal-pathspecs",
                        "restore",
                        "--source",
                        head,
                        "--staged",
                        "--worktree",
                        "--pathspec-from-file=-",
                        "--pathspec-file-nul",
                    ],
                    cleanup,
                )
                .await?;
            let remaining = ChangesService::default().snapshot_at_root(root).await?;
            if remaining
                .files
                .iter()
                .any(|file| paths.contains(&file.path))
            {
                return Err(push_error);
            }
            return Ok(());
        }
        self.runner
            .run_with_input(
                Some(root),
                [
                    "--literal-pathspecs",
                    "reset",
                    "--quiet",
                    head,
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ],
                pathspec,
            )
            .await?;
        Ok(())
    }

    pub async fn apply(
        &self,
        requested_root: &Path,
        selection: StashSelection,
    ) -> Result<StashMutationResult, BackendError> {
        self.restore(requested_root, selection, false).await
    }

    pub async fn pop(
        &self,
        requested_root: &Path,
        selection: StashSelection,
    ) -> Result<StashMutationResult, BackendError> {
        self.restore(requested_root, selection, true).await
    }

    async fn restore(
        &self,
        requested_root: &Path,
        selection: StashSelection,
        remove: bool,
    ) -> Result<StashMutationResult, BackendError> {
        let root = self.repository_root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        self.ensure_write_allowed(&root).await?;
        let entry = self.resolve_selection(&root, &selection).await?;
        // Apply the immutable object: external pushes cannot redirect the worktree mutation.
        let error = self
            .runner
            .run_allowing_failure(
                Some(&root),
                ["stash", "apply", "--index", entry.object_id.as_str()],
            )
            .await
            .and_then(|output| output.into_result())
            .err();
        let mut result = self
            .refresh(
                &root,
                if error.is_some() {
                    StashMutationOutcome::Retained
                } else {
                    StashMutationOutcome::Applied
                },
                error,
            )
            .await?;
        if !remove || result.error.is_some() {
            return Ok(result);
        }
        if let Err(error) = ensure_mutation_allowed(&result.operation_state, MutationIntent::Stash)
        {
            result.outcome = StashMutationOutcome::Retained;
            result.error = Some(error);
            return Ok(result);
        }
        // Git has no compare-and-delete stash API. Recheck immediately before drop;
        // an external CLI can still race between this check and Git's reflog lock.
        let mut expected = self.snapshot_at_root(&root).await?;
        let selected_index = expected.entries.iter().position(|entry| {
            entry.selector == selection.selector && entry.object_id == selection.expected_object_id
        });
        let Some(selected_index) = selected_index else {
            return self
                .refresh(&root, StashMutationOutcome::Retained, Some(stale_stash()))
                .await;
        };
        expected.entries.remove(selected_index);
        for (index, entry) in expected.entries.iter_mut().enumerate() {
            entry.selector = format!("stash@{{{index}}}");
        }
        let error = self
            .runner
            .run_allowing_failure(Some(&root), ["stash", "drop", entry.selector.as_str()])
            .await
            .and_then(|output| output.into_result())
            .err();
        result = self
            .refresh(
                &root,
                if error.is_some() {
                    StashMutationOutcome::Retained
                } else {
                    StashMutationOutcome::Removed
                },
                error,
            )
            .await?;
        if result.error.is_none() && result.stashes != expected {
            result.outcome = StashMutationOutcome::Retained;
            result.error = Some(stale_stash());
        }
        Ok(result)
    }

    async fn repository_root(&self, requested_root: &Path) -> Result<PathBuf, BackendError> {
        RepositoryService::with_coordinator(self.runner.clone(), self.coordinator.clone())
            .resolve_root(requested_root)
            .await
    }

    async fn ensure_write_allowed(&self, root: &Path) -> Result<(), BackendError> {
        ensure_mutation_allowed(
            &read_operation_state(root, &self.runner).await?,
            MutationIntent::Stash,
        )
    }

    async fn snapshot_at_root(&self, root: &Path) -> Result<StashSnapshot, BackendError> {
        let output = self
            .runner
            .run(Some(root), ["stash", "list", STASH_LIST_FORMAT])
            .await?;
        let mut tokens = output.stdout.split('\0');
        let mut entries = Vec::new();
        while let Some(selector) = tokens.next() {
            let selector = selector.trim_start_matches(['\r', '\n']);
            if selector.is_empty() {
                break;
            }
            let object_id = tokens.next().ok_or_else(parse_error)?;
            let subject = tokens.next().ok_or_else(parse_error)?;
            let timestamp = tokens.next().ok_or_else(parse_error)?;
            let (branch, description) = parse_subject(subject);
            entries.push(StashEntry {
                selector: selector.to_owned(),
                object_id: object_id.to_owned(),
                branch,
                description,
                timestamp: timestamp.to_owned(),
            });
        }
        Ok(StashSnapshot { entries })
    }

    async fn resolve_selection(
        &self,
        root: &Path,
        selection: &StashSelection,
    ) -> Result<StashEntry, BackendError> {
        validate_selection(selection)?;
        self.snapshot_at_root(root)
            .await?
            .entries
            .into_iter()
            .find(|entry| {
                entry.selector == selection.selector
                    && entry.object_id == selection.expected_object_id
            })
            .ok_or_else(stale_stash)
    }

    async fn refresh(
        &self,
        root: &Path,
        outcome: StashMutationOutcome,
        error: Option<BackendError>,
    ) -> Result<StashMutationResult, BackendError> {
        let (workspace, stashes) = tokio::join!(
            refresh_mutation_workspace(root, &self.runner),
            self.snapshot_at_root(root)
        );
        let workspace = workspace?;
        Ok(StashMutationResult {
            outcome,
            workspace: workspace.workspace,
            stashes: stashes?,
            operation_state: workspace.operation_state,
            error,
        })
    }

    async fn parents(&self, root: &Path, object_id: &str) -> Result<Vec<String>, BackendError> {
        let output = self
            .runner
            .run(Some(root), ["show", "--no-patch", "--format=%P", object_id])
            .await?;
        let parents: Vec<String> = output
            .stdout
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if !(2..=3).contains(&parents.len()) {
            return Err(parse_error());
        }
        Ok(parents)
    }

    async fn empty_tree(&self, root: &Path) -> Result<String, BackendError> {
        Ok(self
            .runner
            .run_with_input(
                Some(root),
                ["hash-object", "-t", "tree", "--stdin"],
                Vec::new(),
            )
            .await?
            .stdout
            .trim()
            .to_owned())
    }

    async fn files(
        &self,
        root: &Path,
        object_id: &str,
    ) -> Result<Vec<StashFileSummary>, BackendError> {
        let parents = self.parents(root, object_id).await?;
        let mut files = self.tree_files(root, &parents[0], object_id, false).await?;
        if let Some(untracked) = parents.get(2) {
            files.extend(
                self.tree_files(root, &self.empty_tree(root).await?, untracked, true)
                    .await?,
            );
        }
        let mut paths = HashSet::new();
        if files
            .iter()
            .any(|file| !paths.insert((file.path.clone(), file.untracked)))
        {
            return Err(parse_error());
        }
        Ok(files)
    }

    async fn tree_files(
        &self,
        root: &Path,
        base: &str,
        target: &str,
        untracked: bool,
    ) -> Result<Vec<StashFileSummary>, BackendError> {
        let names = self
            .runner
            .run(
                Some(root),
                [
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "-M20%",
                    "--name-status",
                    "-z",
                    base,
                    target,
                    "--",
                ],
            )
            .await?;
        let stats = self
            .runner
            .run(
                Some(root),
                [
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "-M20%",
                    "--numstat",
                    "-z",
                    base,
                    target,
                    "--",
                ],
            )
            .await?;
        let stats = parse_stats(&stats.stdout)?;
        let mut tokens = names.stdout.split('\0').filter(|token| !token.is_empty());
        let mut files = Vec::new();
        while let Some(status) = tokens.next() {
            let first_path = tokens.next().ok_or_else(parse_error)?;
            let (old_path, path) = if status.starts_with('R') || status.starts_with('C') {
                (
                    Some(first_path.to_owned()),
                    tokens.next().ok_or_else(parse_error)?,
                )
            } else {
                (None, first_path)
            };
            let (additions, deletions) = *stats.get(path).ok_or_else(parse_error)?;
            files.push(StashFileSummary {
                status: status.to_owned(),
                path: path.to_owned(),
                old_path,
                additions,
                deletions,
                binary: additions.is_none() || deletions.is_none(),
                untracked,
            });
        }
        Ok(files)
    }
}

type FileStats = HashMap<String, (Option<u64>, Option<u64>)>;

fn parse_stats(output: &str) -> Result<FileStats, BackendError> {
    let mut tokens = output.split('\0').filter(|token| !token.is_empty());
    let mut stats = HashMap::new();
    while let Some(record) = tokens.next() {
        let mut fields = record.splitn(3, '\t');
        let additions = fields.next().ok_or_else(parse_error)?.parse().ok();
        let deletions = fields.next().ok_or_else(parse_error)?.parse().ok();
        let path = fields.next().ok_or_else(parse_error)?;
        let path = if path.is_empty() {
            let _old_path = tokens.next().ok_or_else(parse_error)?;
            tokens.next().ok_or_else(parse_error)?
        } else {
            path
        };
        stats.insert(path.to_owned(), (additions, deletions));
    }
    Ok(stats)
}

fn parse_subject(subject: &str) -> (Option<String>, String) {
    if let Some((branch, description)) = subject
        .strip_prefix("On ")
        .or_else(|| subject.strip_prefix("WIP on "))
        .and_then(|subject| subject.split_once(": "))
    {
        (Some(branch.to_owned()), description.to_owned())
    } else {
        (None, subject.to_owned())
    }
}

fn validate_selection(selection: &StashSelection) -> Result<(), BackendError> {
    let index = selection
        .selector
        .strip_prefix("stash@{")
        .and_then(|value| value.strip_suffix('}'));
    let valid_selector = index.is_some_and(|index| {
        !index.is_empty()
            && index.bytes().all(|byte| byte.is_ascii_digit())
            && (index == "0" || !index.starts_with('0'))
            && index.parse::<usize>().is_ok()
    });
    let oid = &selection.expected_object_id;
    if !valid_selector
        || !matches!(oid.len(), 40 | 64)
        || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(BackendError::new(
            ErrorCode::InvalidReference,
            "Stash 选择无效，请刷新列表后重试。",
        ));
    }
    Ok(())
}

fn stale_stash() -> BackendError {
    BackendError::new(
        ErrorCode::StaleStash,
        "Stash 列表已变化，请刷新后重新选择。",
    )
}
fn parse_error() -> BackendError {
    BackendError::new(ErrorCode::GitCommandFailed, "无法读取 Stash 内容。")
}
