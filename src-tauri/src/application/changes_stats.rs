use super::{ChangesService, repository_root};
use crate::domain::{
    changes::{ChangeLineStat, ChangeScope},
    error::{BackendError, ErrorCode},
};
use crate::infrastructure::repository_paths::{checked_path, open_no_follow};
use std::{io::Read, path::Path};

const MAX_STATS_BYTES: usize = 32 * 1024 * 1024;
const MAX_UNTRACKED_BYTES: u64 = 2 * 1024 * 1024;
const MAX_UNTRACKED_FILES: usize = 500;

fn parse_numstat(bytes: &[u8], scope: ChangeScope) -> Result<Vec<ChangeLineStat>, BackendError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| BackendError::new(ErrorCode::UnsupportedEncoding, "无法解码变更统计。"))?;
    let mut parts = text.split('\0');
    let mut result = vec![];
    while let Some(record) = parts.next() {
        if record.is_empty() {
            continue;
        }
        let mut columns = record.splitn(3, '\t');
        let added = columns.next().unwrap_or("");
        let removed = columns.next().unwrap_or("");
        let path = columns
            .next()
            .ok_or_else(|| BackendError::new(ErrorCode::GitCommandFailed, "无效的行数统计。"))?;
        let path = if path.is_empty() {
            parts.next().ok_or_else(|| {
                BackendError::new(ErrorCode::GitCommandFailed, "缺少重命名来源。")
            })?;
            parts
                .next()
                .ok_or_else(|| BackendError::new(ErrorCode::GitCommandFailed, "缺少重命名目标。"))?
        } else {
            path
        };
        result.push(ChangeLineStat {
            path: path.into(),
            scope,
            additions: added.parse().ok(),
            deletions: removed.parse().ok(),
            binary: added == "-" || removed == "-",
        });
    }
    Ok(result)
}

impl ChangesService {
    pub async fn line_stats(&self, requested: &Path) -> Result<Vec<ChangeLineStat>, BackendError> {
        let root = repository_root(&self.runner, requested).await?;
        let _guard = self.coordinator.read(&root).await;
        let mut result = vec![];
        // Two batched queries; never load every file's full patch to count lines.
        for scope in [ChangeScope::Staged, ChangeScope::Unstaged] {
            let mut args = vec![
                "--no-optional-locks",
                "diff",
                "--numstat",
                "-z",
                "--no-ext-diff",
                "--no-textconv",
                "--find-renames",
            ];
            if scope == ChangeScope::Staged {
                args.push("--cached");
            }
            args.push("--");
            let raw = self
                .runner
                .run_bytes(Some(&root), args, MAX_STATS_BYTES)
                .await?;
            result.extend(parse_numstat(&raw, scope)?);
        }
        let snapshot = self.snapshot_at_root(&root).await?;
        for change in snapshot
            .files
            .iter()
            .filter(|f| f.index_status == "?")
            .take(MAX_UNTRACKED_FILES)
        {
            let contents = (|| -> Result<Vec<u8>, BackendError> {
                let path = checked_path(&root, &change.path, &[], &[], false)?;
                let file = open_no_follow(&path)?;
                if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_UNTRACKED_BYTES {
                    return Err(BackendError::new(
                        ErrorCode::UnsupportedFileOperation,
                        "文件过大或不是普通文件，暂不统计。",
                    ));
                }
                let mut bytes = vec![];
                file.take(MAX_UNTRACKED_BYTES + 1).read_to_end(&mut bytes)?;
                Ok(bytes)
            })();
            // Failed/oversize reads stay unknown, never masquerade as zero changes.
            let Ok(bytes) = contents else { continue };
            if bytes.len() > MAX_UNTRACKED_BYTES as usize {
                continue;
            }
            let binary = bytes.contains(&0) || std::str::from_utf8(&bytes).is_err();
            let lines = bytes.iter().filter(|&&b| b == b'\n').count() as u64
                + u64::from(!bytes.is_empty() && bytes.last() != Some(&b'\n'));
            result.push(ChangeLineStat {
                path: change.path.clone(),
                scope: ChangeScope::Unstaged,
                additions: (!binary).then_some(lines),
                deletions: (!binary).then_some(0),
                binary,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::git_runner::GitCommandRunner;
    #[test]
    fn numstat_handles_rename_tabs_unicode_binary_and_zero() {
        let items = parse_numstat(
            "2\t1\t文件\t名.py\0000\t0\t\0old.py\0new.py\0-\t-\timage.bin\0".as_bytes(),
            ChangeScope::Staged,
        )
        .unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].path, "文件\t名.py");
        assert_eq!(items[0].additions, Some(2));
        assert_eq!(items[1].path, "new.py");
        assert_eq!(items[1].deletions, Some(0));
        assert!(items[2].binary);
        assert_eq!(items[2].additions, None);
    }
    #[tokio::test]
    async fn line_stats_separates_index_worktree_and_untracked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let runner = GitCommandRunner::default();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Stats"],
            vec!["config", "user.email", "stats@example.test"],
            vec!["config", "core.autocrlf", "false"],
        ] {
            runner.run(Some(root), args).await.unwrap();
        }
        std::fs::write(root.join("a.py"), "one\ntwo\n").unwrap();
        runner.run(Some(root), ["add", "."]).await.unwrap();
        runner
            .run(Some(root), ["commit", "-m", "base"])
            .await
            .unwrap();
        std::fs::write(root.join("a.py"), "one\nchanged\nthree\n").unwrap();
        runner.run(Some(root), ["add", "."]).await.unwrap();
        std::fs::write(root.join("a.py"), "one\nchanged\nthree\nfour\n").unwrap();
        std::fs::write(root.join("中文.txt"), "a\nb").unwrap();
        std::fs::write(root.join("binary.bin"), [0, 255]).unwrap();
        let index_before = std::fs::read(root.join(".git/index")).unwrap();
        let stats = ChangesService::default().line_stats(root).await.unwrap();
        let staged = stats
            .iter()
            .find(|s| s.path == "a.py" && s.scope == ChangeScope::Staged)
            .unwrap();
        assert_eq!((staged.additions, staged.deletions), (Some(2), Some(1)));
        let work = stats
            .iter()
            .find(|s| s.path == "a.py" && s.scope == ChangeScope::Unstaged)
            .unwrap();
        assert_eq!((work.additions, work.deletions), (Some(1), Some(0)));
        assert_eq!(
            stats
                .iter()
                .find(|s| s.path == "中文.txt")
                .unwrap()
                .additions,
            Some(2)
        );
        assert!(
            stats
                .iter()
                .find(|s| s.path == "binary.bin")
                .unwrap()
                .binary
        );
        assert_eq!(
            std::fs::read(root.join(".git/index")).unwrap(),
            index_before
        );
    }
}
