use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::application::changes_service::ChangesService;
use crate::domain::error::BackendError;
use crate::infrastructure::git_runner::GitCommandRunner;

#[derive(PartialEq, Eq)]
pub(super) struct Fingerprint {
    pub worktree: [u8; 32],
    pub metadata: [u8; 32],
}

fn field(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

// Content, not mtime: saving identical bytes or refreshing index stat data must
// not trigger a UI refresh. Stream files so large binary changes stay bounded.
async fn file(hash: &mut Sha256, path: &Path) -> Result<(), BackendError> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            field(hash, b"missing");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.is_symlink() {
        field(hash, b"symlink");
        field(
            hash,
            tokio::fs::read_link(path)
                .await?
                .to_string_lossy()
                .as_bytes(),
        );
    } else if metadata.is_file() {
        let mut contents = Sha256::new();
        let mut input = tokio::fs::File::open(path).await?;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let count = input.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            contents.update(&buffer[..count]);
        }
        field(hash, b"file");
        field(hash, &contents.finalize());
    } else {
        field(hash, b"directory");
    }
    Ok(())
}

pub(super) async fn capture(root: &Path, git_dir: &Path) -> Result<Fingerprint, BackendError> {
    let runner = GitCommandRunner::default();
    let changes = ChangesService::default().snapshot_at_root(root).await?;
    let mut worktree = Sha256::new();
    for changed in &changes.files {
        field(&mut worktree, changed.path.as_bytes());
        field(
            &mut worktree,
            changed.old_path.as_deref().unwrap_or_default().as_bytes(),
        );
        field(&mut worktree, changed.index_status.as_bytes());
        field(&mut worktree, changed.worktree_status.as_bytes());
        file(&mut worktree, &root.join(&changed.path)).await?;
    }
    // Raw object IDs detect changes to already-staged files even if their XY
    // status and working bytes remain unchanged. Index timestamps are excluded.
    if changes
        .files
        .iter()
        .any(|file| file.staged || file.conflict)
    {
        let index = runner
            .run(
                Some(root),
                [
                    "diff",
                    "--cached",
                    "--raw",
                    "--no-abbrev",
                    "--no-renames",
                    "--no-ext-diff",
                    "-z",
                ],
            )
            .await?;
        field(&mut worktree, index.stdout.as_bytes());
    }

    let mut metadata = Sha256::new();
    let status = runner
        .run(
            Some(root),
            [
                "--no-optional-locks",
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=no",
            ],
        )
        .await?;
    for header in status
        .stdout
        .split('\0')
        .take_while(|entry| entry.starts_with("# "))
    {
        field(&mut metadata, header.as_bytes());
    }
    let refs = runner
        .run(
            Some(root),
            [
                "for-each-ref",
                "--sort=refname",
                "--format=%(refname) %(objectname) %(symref)",
            ],
        )
        .await?;
    field(&mut metadata, refs.stdout.as_bytes());
    let config = runner
        .run(Some(root), ["config", "--null", "--list"])
        .await?;
    field(&mut metadata, config.stdout.as_bytes());
    // Operation markers affect conflict/continue controls without moving HEAD.
    for name in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
        "rebase-merge",
        "rebase-merge/head-name",
        "rebase-merge/git-rebase-todo",
        "rebase-merge/done",
        "rebase-apply",
        "rebase-apply/next",
        "rebase-apply/last",
        "sequencer/todo",
    ] {
        field(&mut metadata, name.as_bytes());
        file(&mut metadata, &git_dir.join(name)).await?;
    }
    Ok(Fingerprint {
        worktree: worktree.finalize().into(),
        metadata: metadata.finalize().into(),
    })
}
