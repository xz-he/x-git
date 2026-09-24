use std::path::{Path, PathBuf};

use tokio::io::AsyncReadExt;

use crate::application::changes_service::ChangesService;
use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::application::repository_service::RepositoryService;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::operation::{
    AbortAction, ConflictFileSummary, MutationWorkspace, RepositoryOperationKind,
    RepositoryOperationState,
};
use crate::infrastructure::git_runner::GitCommandRunner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationIntent {
    Changes,
    Files,
    Commit,
    CreateBranch,
    SwitchBranch,
    DeleteBranch,
    Merge,
    Rebase,
    Checkout,
    CherryPick,
    Revert,
    Reset,
    RemoteSync,
    Stash,
    AbortMerge,
    AbortRebase,
    AbortCherryPick,
    AbortRevert,
    ResolveConflict,
    ContinueConflict,
}

pub async fn read_operation_state(
    root: &Path,
    runner: &GitCommandRunner,
) -> Result<RepositoryOperationState, BackendError> {
    let (status, paths) = tokio::join!(
        runner.run(
            Some(root),
            [
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=no",
            ],
        ),
        operation_paths(root, runner),
    );
    let conflicts = parse_conflicts(&status?.stdout);
    let [rebase_merge, rebase_apply, merge, revert, cherry_pick, todo] = paths?;

    if tokio::fs::try_exists(rebase_merge).await? || tokio::fs::try_exists(rebase_apply).await? {
        return Ok(RepositoryOperationState::rebase(conflicts));
    }
    if tokio::fs::try_exists(merge).await? {
        return Ok(RepositoryOperationState::merge(conflicts));
    }
    if tokio::fs::try_exists(revert).await? {
        return Ok(RepositoryOperationState::revert(conflicts));
    }
    if tokio::fs::try_exists(cherry_pick).await? || pending_cherry_pick_sequence(&todo).await? {
        return Ok(RepositoryOperationState::cherry_pick(conflicts));
    }

    Ok(RepositoryOperationState {
        kind: RepositoryOperationKind::None,
        conflicts,
        abort_action: None,
    })
}

pub fn ensure_mutation_allowed(
    state: &RepositoryOperationState,
    intent: MutationIntent,
) -> Result<(), BackendError> {
    let allowed = match intent {
        MutationIntent::ResolveConflict => !state.conflicts.is_empty(),
        MutationIntent::ContinueConflict => {
            state.kind != RepositoryOperationKind::None && state.conflicts.is_empty()
        }
        MutationIntent::AbortMerge => state.abort_action == Some(AbortAction::Merge),
        MutationIntent::AbortRebase => state.abort_action == Some(AbortAction::Rebase),
        MutationIntent::AbortCherryPick => state.abort_action == Some(AbortAction::CherryPick),
        MutationIntent::AbortRevert => state.abort_action == Some(AbortAction::Revert),
        _ => state.kind == RepositoryOperationKind::None && state.conflicts.is_empty(),
    };
    if allowed {
        Ok(())
    } else {
        Err(BackendError::new(
            ErrorCode::GitOperationInProgress,
            operation_blocked_message(state),
        ))
    }
}

pub async fn refresh_mutation_workspace(
    root: &Path,
    runner: &GitCommandRunner,
) -> Result<MutationWorkspace, BackendError> {
    let coordinator = RepositoryMutationCoordinator::default();
    let repositories = RepositoryService::with_coordinator(runner.clone(), coordinator.clone());
    let changes = ChangesService::new(runner.clone(), coordinator);
    let (repository, changes, operation_state) = tokio::join!(
        repositories.snapshot_at_root(root),
        changes.snapshot_at_root(root),
        read_operation_state(root, runner),
    );
    Ok(MutationWorkspace {
        workspace: crate::domain::changes::WorkingTreeSnapshot {
            repository: repository?,
            changes: changes?,
        },
        operation_state: operation_state?,
    })
}

fn parse_conflicts(output: &str) -> Vec<ConflictFileSummary> {
    let mut records = output.split('\0').filter(|record| !record.is_empty());
    let mut conflicts = Vec::new();
    while let Some(record) = records.next() {
        if record.len() < 3 {
            continue;
        }
        let status = &record[..2];
        let path = &record[3..];
        if matches!(status, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU") {
            conflicts.push(ConflictFileSummary {
                path: path.to_owned(),
                status: status.to_owned(),
            });
        }
        if status.starts_with('R') || status.ends_with('R') {
            let _ = records.next();
        }
    }
    conflicts
}

async fn operation_paths(
    root: &Path,
    runner: &GitCommandRunner,
) -> Result<[PathBuf; 6], BackendError> {
    // Ask Git for worktree-aware paths in one process, including linked worktrees.
    let output = runner
        .run(
            Some(root),
            [
                "rev-parse",
                "--git-path",
                "rebase-merge",
                "--git-path",
                "rebase-apply",
                "--git-path",
                "MERGE_HEAD",
                "--git-path",
                "REVERT_HEAD",
                "--git-path",
                "CHERRY_PICK_HEAD",
                "--git-path",
                "sequencer/todo",
            ],
        )
        .await?;
    output
        .stdout
        .lines()
        .map(|line| {
            let path = PathBuf::from(line.trim_end_matches('\r'));
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| BackendError::new(ErrorCode::Unexpected, "Git 操作状态路径数量不正确。"))
}

async fn pending_cherry_pick_sequence(path: &Path) -> Result<bool, BackendError> {
    const MAX_SEQUENCER_TODO_BYTES: u64 = 2 * 1024 * 1024;
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SEQUENCER_TODO_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    let blocked = || {
        BackendError::new(
            ErrorCode::GitOperationInProgress,
            "An unsupported or invalid Git sequencer is still active; finish it externally.",
        )
    };
    if bytes.len() as u64 > MAX_SEQUENCER_TODO_BYTES {
        return Err(blocked());
    }
    let todo = std::str::from_utf8(&bytes).map_err(|_| blocked())?;
    let mut has_pick = false;
    for line in todo
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let mut fields = line.split_whitespace();
        if !matches!(fields.next(), Some("pick" | "p")) || fields.next().is_none() {
            return Err(blocked());
        }
        has_pick = true;
    }
    Ok(has_pick)
}

fn operation_blocked_message(state: &RepositoryOperationState) -> &'static str {
    match state.kind {
        RepositoryOperationKind::Merge => "合并操作尚未完成，请先解决冲突或中止合并。",
        RepositoryOperationKind::Rebase => "变基操作尚未完成，请先解决冲突或中止变基。",
        RepositoryOperationKind::CherryPick => "Cherry-pick 操作尚未完成，请先解决冲突或中止操作。",
        RepositoryOperationKind::Revert => "Revert 回滚尚未完成，请先解决冲突、继续或中止回滚。",
        RepositoryOperationKind::None => "仓库仍有未解决冲突，请先完成冲突处理。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_parser_keeps_unmerged_status_and_unicode_path() {
        let conflicts = parse_conflicts("UU src/冲突.txt\0 M clean.txt\0");
        assert_eq!(
            conflicts,
            [ConflictFileSummary {
                path: "src/冲突.txt".into(),
                status: "UU".into(),
            }]
        );
    }
}
