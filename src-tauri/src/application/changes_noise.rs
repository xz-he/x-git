//! Conservative whole-file cleanup: only CRLF/LF differences, never whitespace folding.
use super::{ChangesService, repository_root};
use crate::application::file_fingerprint::{FileContext, digest, field, stale};
use crate::application::operation_state::MutationIntent;
use crate::domain::changes::{
    FileChange, NoiseCandidate, NoiseRestoreResult, NoiseScan, NoiseSkipped,
};
use crate::domain::error::{BackendError, ErrorCode};
use crate::infrastructure::repository_paths::open_no_follow;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};

const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_SCAN_FILES: usize = 500;

#[derive(Clone)]
struct Entry {
    mode: String,
    oid: String,
}
struct Prepared {
    candidate: NoiseCandidate,
    work_digest: String,
    index_record: Vec<u8>,
    head: Option<String>,
}

fn invalid(message: &str) -> BackendError {
    BackendError::new(ErrorCode::InvalidPath, message)
}

fn manifest(raw: &[u8], index: bool) -> Result<BTreeMap<String, Entry>, BackendError> {
    let text = std::str::from_utf8(raw).map_err(|_| invalid("文件名编码不受支持。"))?;
    let mut entries = BTreeMap::new();
    for record in text.split('\0').filter(|r| !r.is_empty()) {
        let (meta, path) = record
            .split_once('\t')
            .ok_or_else(|| invalid("无法读取 Git 文件清单。"))?;
        let parts: Vec<_> = meta.split_whitespace().collect();
        if parts.len() != 3 || (index && parts[2] != "0") {
            return Err(stale());
        }
        entries.insert(
            path.to_owned(),
            Entry {
                mode: parts[0].into(),
                oid: parts[if index { 1 } else { 2 }].into(),
            },
        );
    }
    Ok(entries)
}

fn read_work(context: &FileContext, path: &str) -> Result<Vec<u8>, BackendError> {
    let path = context.path(path, false)?;
    let file = open_no_follow(&path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_FILE_BYTES as u64 {
        return Err(invalid("非普通文件或超过 2 MiB，已跳过。"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(stale());
    }
    Ok(bytes)
}

fn normalized(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    std::str::from_utf8(bytes)
        .ok()
        .map(|s| s.replace("\r\n", "\n"))
}

impl ChangesService {
    pub async fn scan_noise(&self, requested: &Path) -> Result<NoiseScan, BackendError> {
        let root = repository_root(&self.runner, requested).await?;
        let _guard = self.coordinator.read(&root).await;
        let (scan, _, _) = self.prepare_noise(&root).await?;
        Ok(scan)
    }

    async fn prepare_noise(
        &self,
        root: &Path,
    ) -> Result<(NoiseScan, FileContext, Vec<Prepared>), BackendError> {
        self.ensure_operation_allows(root, MutationIntent::Changes)
            .await?;
        let context = FileContext::read(root, &self.runner).await?;
        let snapshot = self.snapshot_at_root(root).await?;
        let index = manifest(
            &self
                .runner
                .run_bytes(
                    Some(root),
                    ["ls-files", "--stage", "-z"],
                    MAX_MANIFEST_BYTES,
                )
                .await?,
            true,
        )?;
        let head = self
            .runner
            .run_allowing_failure(Some(root), ["rev-parse", "--verify", "HEAD"])
            .await?;
        let base = if head.is_success() {
            manifest(
                &self
                    .runner
                    .run_bytes(
                        Some(root),
                        ["ls-tree", "-r", "-z", head.stdout.trim()],
                        MAX_MANIFEST_BYTES,
                    )
                    .await?,
                false,
            )?
        } else {
            BTreeMap::new()
        };
        let mut scan = NoiseScan {
            candidates: vec![],
            skipped: vec![],
        };
        let mut prepared = vec![];
        for (number, change) in snapshot.files.iter().enumerate() {
            let result = if number >= MAX_SCAN_FILES {
                Err(invalid("本次最多检测 500 个文件，请分批处理。"))
            } else {
                self.noise_candidate(&context, change, &index, &base).await
            };
            match result {
                Ok(mut item) => {
                    if item.candidate.staged {
                        item.head = Some(head.stdout.trim().to_owned());
                    }
                    scan.candidates.push(item.candidate.clone());
                    prepared.push(item);
                }
                Err(error) => scan.skipped.push(NoiseSkipped {
                    path: change.path.clone(),
                    reason: error.message,
                }),
            }
        }
        if FileContext::read(root, &self.runner).await?.identity != context.identity {
            return Err(stale());
        }
        Ok((scan, context, prepared))
    }

    async fn noise_candidate(
        &self,
        context: &FileContext,
        change: &FileChange,
        index: &BTreeMap<String, Entry>,
        base: &BTreeMap<String, Entry>,
    ) -> Result<Prepared, BackendError> {
        if change.conflict
            || change.old_path.is_some()
            || !matches!(change.index_status.as_str(), " " | "M")
            || !matches!(change.worktree_status.as_str(), " " | "M")
        {
            return Err(invalid("新增、删除、重命名或冲突文件不自动还原。"));
        }
        let entry = index.get(&change.path).ok_or_else(stale)?;
        let baseline = if change.staged {
            base.get(&change.path).ok_or_else(stale)?
        } else {
            entry
        };
        if !matches!(entry.mode.as_str(), "100644" | "100755") || entry.mode != baseline.mode {
            return Err(invalid("文件类型或权限发生变化，不自动还原。"));
        }
        let raw = self
            .runner
            .run(
                Some(&context.root),
                [
                    "--no-optional-locks",
                    "--literal-pathspecs",
                    "diff",
                    "--raw",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-renames",
                    "--",
                    &change.path,
                ],
            )
            .await?;
        if let Some(line) = raw.stdout.lines().next() {
            let modes: Vec<_> = line.split_whitespace().take(2).collect();
            if modes.len() != 2 || modes[0].trim_start_matches(':') != modes[1] {
                return Err(invalid("工作区文件类型或权限发生变化，不自动还原。"));
            }
        }
        let attributes = self
            .runner
            .run_bytes(
                Some(&context.root),
                [
                    "--literal-pathspecs",
                    "check-attr",
                    "-z",
                    "filter",
                    "working-tree-encoding",
                    "--",
                    &change.path,
                ],
                MAX_MANIFEST_BYTES,
            )
            .await?;
        if attributes
            .split(|b| *b == 0)
            .collect::<Vec<_>>()
            .chunks(3)
            .any(|v| v.len() == 3 && v[2] != b"unspecified" && v[2] != b"unset")
        {
            return Err(invalid("存在内容过滤器或编码转换，不自动还原。"));
        }
        let work = read_work(context, &change.path)?;
        let indexed = self
            .runner
            .run_bytes(
                Some(&context.root),
                ["cat-file", "blob", &entry.oid],
                MAX_FILE_BYTES,
            )
            .await?;
        let original = if entry.oid == baseline.oid {
            indexed.clone()
        } else {
            self.runner
                .run_bytes(
                    Some(&context.root),
                    ["cat-file", "blob", &baseline.oid],
                    MAX_FILE_BYTES,
                )
                .await?
        };
        let clean =
            normalized(&original).ok_or_else(|| invalid("二进制或非 UTF-8 文件，不自动还原。"))?;
        if normalized(&indexed).as_ref() != Some(&clean)
            || normalized(&work).as_ref() != Some(&clean)
        {
            return Err(invalid("包含内容、空格、缩进或编码差异，不自动还原。"));
        }
        let mut hash = Sha256::new();
        for value in [
            context.identity.as_bytes(),
            change.path.as_bytes(),
            entry.oid.as_bytes(),
            baseline.oid.as_bytes(),
            &work,
        ] {
            field(&mut hash, value);
        }
        let index_record =
            format!("{} {} 0\t{}\0", entry.mode, entry.oid, change.path).into_bytes();
        Ok(Prepared {
            candidate: NoiseCandidate {
                path: change.path.clone(),
                staged: change.staged,
                fingerprint: format!("{:x}", hash.finalize()),
            },
            work_digest: digest(&work),
            index_record,
            head: None,
        })
    }

    pub async fn restore_noise(
        &self,
        requested: &Path,
        selected: Vec<NoiseCandidate>,
    ) -> Result<NoiseRestoreResult, BackendError> {
        if selected.is_empty() || selected.len() > MAX_SCAN_FILES {
            return Err(invalid("请选择检测出的文件后再还原。"));
        }
        let root = repository_root(&self.runner, requested).await?;
        let _guard = self.coordinator.write(&root).await;
        let (_, context, prepared) = self.prepare_noise(&root).await?;
        let mut unique = BTreeSet::new();
        // Validate every selection before the first write; stale scans never widen the scope.
        for item in &selected {
            if !unique.insert(&item.path) || !prepared.iter().any(|p| p.candidate == *item) {
                return Err(stale());
            }
        }
        let mut restored = vec![];
        let mut skipped = vec![];
        for item in selected {
            let prepared = prepared
                .iter()
                .find(|p| p.candidate == item)
                .ok_or_else(stale)?;
            let result = async {
                self.ensure_operation_allows(&root, MutationIntent::Changes)
                    .await?;
                let current_index = self
                    .runner
                    .run_bytes(
                        Some(&root),
                        [
                            "--literal-pathspecs",
                            "ls-files",
                            "--stage",
                            "-z",
                            "--",
                            &item.path,
                        ],
                        MAX_MANIFEST_BYTES,
                    )
                    .await?;
                if current_index != prepared.index_record {
                    return Err(stale());
                }
                if let Some(head) = &prepared.head {
                    if self
                        .runner
                        .run(Some(&root), ["rev-parse", "HEAD"])
                        .await?
                        .stdout
                        .trim()
                        != head
                    {
                        return Err(stale());
                    }
                }
                if digest(&read_work(&context, &item.path)?) != prepared.work_digest {
                    return Err(stale());
                }
                let mut args = vec!["--literal-pathspecs", "checkout"];
                if let Some(head) = &prepared.head {
                    args.push(head);
                }
                args.extend(["--", &item.path]);
                self.runner.run(Some(&root), args).await?;
                Ok::<(), BackendError>(())
            }
            .await;
            match result {
                Ok(()) => restored.push(item.path),
                Err(error) => skipped.push(NoiseSkipped {
                    path: item.path,
                    reason: error.message,
                }),
            }
        }
        Ok(NoiseRestoreResult {
            workspace: self.refreshed_workspace(&root).await?,
            restored,
            skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::git_runner::GitCommandRunner;
    async fn git(root: &Path, args: &[&str]) -> String {
        GitCommandRunner::default()
            .run(Some(root), args)
            .await
            .unwrap()
            .stdout
    }
    async fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-b", "main"]).await;
        git(root, &["config", "user.name", "Noise Test"]).await;
        git(root, &["config", "user.email", "noise@example.test"]).await;
        git(root, &["config", "core.autocrlf", "false"]).await;
        std::fs::write(root.join(".gitattributes"), "* -text\n").unwrap();
        for path in [
            "staged.py",
            "unstaged.py",
            "real.py",
            "indent.py",
            "[a].py",
            "a.py",
            "中文.py",
        ] {
            std::fs::write(root.join(path), "if True:\n    run()\n").unwrap();
        }
        git(root, &["add", "."]).await;
        git(root, &["commit", "-m", "base"]).await;
        dir
    }
    #[test]
    fn noise_normalization_preserves_meaningful_whitespace_and_encoding() {
        assert_eq!(normalized(b"a\r\nb\n"), normalized(b"a\nb\n"));
        for value in [
            b"a \nb\n".as_slice(),
            b"a\nb",
            b"a\nb\n\n",
            b"\xef\xbb\xbfa\nb\n",
            b" a\nb\n",
            b"a\rb\n",
        ] {
            assert_ne!(normalized(value), normalized(b"a\nb\n"));
        }
        assert!(normalized(b"a\0b").is_none());
        assert!(normalized(&[255]).is_none());
    }
    #[tokio::test]
    async fn noise_restore_limits_scope_and_preserves_real_changes_and_history() {
        let dir = fixture().await;
        let root = dir.path();
        let head = git(root, &["rev-parse", "HEAD"]).await;
        for path in ["staged.py", "unstaged.py", "[a].py", "中文.py"] {
            std::fs::write(root.join(path), "if True:\r\n    run()\r\n").unwrap();
        }
        git(root, &["add", "staged.py", "中文.py"]).await;
        std::fs::write(root.join("real.py"), "real code\r\n").unwrap();
        std::fs::write(root.join("indent.py"), "if True:\n  run()\n").unwrap();
        std::fs::write(root.join("new.py"), "new code\n").unwrap();
        let service = ChangesService::default();
        let scan = service.scan_noise(root).await.unwrap();
        assert_eq!(scan.candidates.len(), 4, "{scan:?}");
        assert_eq!(scan.skipped.len(), 3);
        let selected = scan
            .candidates
            .into_iter()
            .filter(|c| c.path != "unstaged.py")
            .collect();
        let result = service.restore_noise(root, selected).await.unwrap();
        assert_eq!(result.restored.len(), 3, "{result:?}");
        assert!(result.skipped.is_empty());
        assert_eq!(
            std::fs::read(root.join("[a].py")).unwrap(),
            b"if True:\n    run()\n"
        );
        assert_eq!(
            std::fs::read(root.join("unstaged.py")).unwrap(),
            b"if True:\r\n    run()\r\n"
        );
        assert_eq!(
            std::fs::read(root.join("real.py")).unwrap(),
            b"real code\r\n"
        );
        assert!(root.join("new.py").exists());
        assert_eq!(git(root, &["rev-parse", "HEAD"]).await, head);
        assert_eq!(git(root, &["diff", "--cached", "--name-only"]).await, "");
    }
    #[tokio::test]
    async fn noise_rejects_stale_preview_before_any_write() {
        let dir = fixture().await;
        let root = dir.path();
        for path in ["staged.py", "unstaged.py"] {
            std::fs::write(root.join(path), "if True:\r\n    run()\r\n").unwrap();
        }
        let service = ChangesService::default();
        let scan = service.scan_noise(root).await.unwrap();
        std::fs::write(root.join("unstaged.py"), "new work after scan\n").unwrap();
        assert_eq!(
            service
                .restore_noise(root, scan.candidates)
                .await
                .unwrap_err()
                .code,
            ErrorCode::StaleFileOperation
        );
        assert_eq!(
            std::fs::read(root.join("staged.py")).unwrap(),
            b"if True:\r\n    run()\r\n"
        );
        assert_eq!(
            std::fs::read(root.join("unstaged.py")).unwrap(),
            b"new work after scan\n"
        );
    }
    #[tokio::test]
    async fn noise_skips_mixed_staging_modes_filters_and_forged_selection() {
        let dir = fixture().await;
        let root = dir.path();
        std::fs::write(root.join("staged.py"), "if True:\r\n    run()\r\n").unwrap();
        git(root, &["add", "staged.py"]).await;
        std::fs::write(root.join("staged.py"), "unstaged meaningful\n").unwrap();
        std::fs::write(root.join("real.py"), "staged meaningful\n").unwrap();
        git(root, &["add", "real.py"]).await;
        std::fs::write(root.join("real.py"), "if True:\n    run()\n").unwrap();
        git(root, &["update-index", "--chmod=+x", "indent.py"]).await;
        std::fs::write(
            root.join(".gitattributes"),
            "* -text\nunstaged.py filter=custom\n",
        )
        .unwrap();
        std::fs::write(root.join("unstaged.py"), "if True:\r\n    run()\r\n").unwrap();
        let service = ChangesService::default();
        assert!(
            service
                .scan_noise(root)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
        let fake = NoiseCandidate {
            path: "../outside.py".into(),
            staged: true,
            fingerprint: "fake".into(),
        };
        assert!(service.restore_noise(root, vec![fake]).await.is_err());
        assert_eq!(
            std::fs::read(root.join("staged.py")).unwrap(),
            b"unstaged meaningful\n"
        );
        assert_eq!(
            git(root, &["show", ":real.py"]).await,
            "staged meaningful\n"
        );
    }
}
