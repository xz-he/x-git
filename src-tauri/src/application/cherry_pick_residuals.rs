//! Leave independent nested repositories in place; never clean or stash their contents.
use std::path::{Component, Path};

use crate::domain::error::{BackendError, ErrorCode};
use crate::infrastructure::git_runner::GitCommandRunner;

#[derive(Debug)]
pub(super) struct ResidualStatus {
    pub ordinary: Vec<String>,
    pub preserved: Vec<String>,
}

pub(super) async fn inspect(
    root: &Path,
    runner: &GitCommandRunner,
    target: &str,
    commit: Option<&str>,
) -> Result<ResidualStatus, BackendError> {
    let status = runner
        .run(
            Some(root),
            [
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ],
        )
        .await?
        .stdout;
    let mut result = ResidualStatus {
        ordinary: vec![],
        preserved: vec![],
    };
    let mut records = status.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        let Some(path) = record.get(3..) else {
            return Err(invalid_status());
        };
        let code = &record.as_bytes()[..2];
        if code.iter().any(|c| matches!(c, b'R' | b'C')) {
            records.next();
        }
        let path = path.trim_end_matches('/');
        if path.is_empty()
            || !Path::new(path)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(invalid_status());
        }
        if matches!(code, b"??" | b" M")
            && independent_repository(root, runner, path, code == b"??").await?
        {
            result.preserved.push(path.to_owned());
        } else {
            result.ordinary.push(record.to_owned());
        }
    }
    if result.preserved.is_empty() {
        return Ok(result);
    }

    let mut affected = runner
        .run(
            Some(root),
            [
                "diff-tree",
                "-r",
                "--no-commit-id",
                "--name-only",
                "--no-renames",
                "--ignore-submodules=none",
                "-z",
                "HEAD",
                target,
                "--",
            ],
        )
        .await?
        .stdout;
    if let Some(commit) = commit {
        // -m includes every parent for merge commits, even though cherry-pick
        // itself will require a mainline choice and currently rejects them.
        affected.push_str(
            &runner
                .run(
                    Some(root),
                    [
                        "diff-tree",
                        "--root",
                        "-m",
                        "-r",
                        "--no-commit-id",
                        "--name-only",
                        "--no-renames",
                        "--ignore-submodules=none",
                        "-z",
                        commit,
                        "--",
                    ],
                )
                .await?
                .stdout,
        );
    }
    let blocked = result
        .preserved
        .iter()
        .filter(|path| {
            affected
                .split('\0')
                .filter(|name| !name.is_empty())
                .any(|name| overlaps(path, name))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !blocked.is_empty() {
        return Err(BackendError::new(ErrorCode::DirtyWorktree,
            "残留仓库目录与目标分支或待移植提交涉及的路径重叠，已停止自动处理；原目录内容保持不变。")
            .with_diagnostics(blocked.join("\n")));
    }
    Ok(result)
}

async fn independent_repository(
    root: &Path,
    runner: &GitCommandRunner,
    path: &str,
    untracked: bool,
) -> Result<bool, BackendError> {
    let target = root.join(path);
    let Ok(metadata) = std::fs::symlink_metadata(&target) else {
        return Ok(false);
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    if !target.canonicalize()?.starts_with(root.canonicalize()?) {
        return Ok(false);
    }
    let entries = runner
        .run(
            Some(root),
            [
                "--literal-pathspecs",
                "ls-files",
                "--stage",
                "-z",
                "--",
                path,
            ],
        )
        .await?
        .stdout;
    if untracked {
        return Ok(entries.is_empty() && target.join(".git").exists());
    }
    let mut entries = entries.split('\0').filter(|entry| !entry.is_empty());
    let Some(entry) = entries.next() else {
        return Ok(false);
    };
    let Some((mode, name)) = entry.split_once('\t') else {
        return Ok(false);
    };
    Ok(entries.next().is_none()
        && name == path
        && mode.starts_with("160000 ")
        && mode.ends_with(" 0"))
}

fn overlaps(left: &str, right: &str) -> bool {
    // Be conservative about case aliases on Windows.
    let (left, right) = if cfg!(windows) {
        (left.to_lowercase(), right.to_lowercase())
    } else {
        (left.to_owned(), right.to_owned())
    };
    left == right || left.starts_with(&(right.clone() + "/")) || right.starts_with(&(left + "/"))
}

fn invalid_status() -> BackendError {
    BackendError::new(
        ErrorCode::GitCommandFailed,
        "无法确认剩余 Git 状态，已停止自动处理。",
    )
}

#[cfg(test)]
mod tests {
    use super::overlaps;
    #[test]
    fn overlap_includes_ancestors_but_not_similar_siblings() {
        assert!(overlaps("tools/repo", "tools/repo/file.py"));
        assert!(overlaps("tools/repo", "tools"));
        assert!(!overlaps("tools/repo", "tools/repo-other/file.py"));
    }
}
