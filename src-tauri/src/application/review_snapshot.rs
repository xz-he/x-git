//! Immutable Git-object review targets; working tree content is never a code source.
use crate::application::ai_context::AiContextBatch;
use crate::application::{
    ai_context::build_review_batches, changes_service::StagedFilePatch,
    diff_parser::parse_unified_diff, review_git,
};
use crate::domain::changes::DiffLineKind;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::review::ReviewSource;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
const MAX_MANIFEST_ENTRIES: usize = 200_000;
const MAX_PATCH_BYTES: usize = 48 * 1024 * 64;
const MAX_SELECTED_PATHS: usize = 10_000;
const MAX_SELECTED_PATH_BYTES: usize = 4096;
const MAX_SELECTION_BYTES: usize = 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
struct Entry {
    oid: String,
    mode: String,
}

pub struct ReviewSnapshot {
    pub root: PathBuf,
    pub source: ReviewSource,
    pub resolved_commit: Option<String>,
    pub base_commit: Option<String>,
    pub manifest: BTreeMap<String, String>,
    pub deleted: BTreeMap<String, String>,
    pub batches: Vec<AiContextBatch>,
    pub excluded: Vec<String>,
    pub skipped_binary: Vec<String>,
    pub changed_file_count: usize,
}

impl ReviewSnapshot {
    pub async fn capture(
        root: &Path,
        mut source: ReviewSource,
        cancel: &CancellationToken,
    ) -> Result<Self, BackendError> {
        if let ReviewSource::StagedFiles { paths } = &mut source {
            validate_selection(paths)?;
        }
        let root = root.canonicalize().map_err(|_| invalid("无法访问仓库。"))?;
        let actual =
            review_git::bytes(&root, &["rev-parse", "--show-toplevel"], 32 * 1024, cancel).await?;
        let actual = PathBuf::from(text(&actual)?.trim())
            .canonicalize()
            .map_err(|_| invalid("无法确定仓库根目录。"))?;
        if actual != root {
            return Err(invalid("审查路径必须为仓库根目录。"));
        }
        let (resolved_commit, base_commit, manifest, original_index) = match &source {
            ReviewSource::Staged | ReviewSource::StagedFiles { .. } => {
                let head = resolve_head(&root, cancel).await?;
                let raw = review_git::bytes(
                    &root,
                    &["ls-files", "--stage", "-z"],
                    MAX_MANIFEST_BYTES,
                    cancel,
                )
                .await?;
                let index = parse_manifest(&raw, true)?;
                (head.clone(), head, index, Some(raw))
            }
            ReviewSource::Commit { revision } => {
                if revision.is_empty()
                    || revision.len() > 256
                    || revision.starts_with('-')
                    || revision.contains('\0')
                {
                    return Err(invalid("无效提交引用。"));
                }
                let oid = resolve(&root, revision, cancel).await?;
                let commit =
                    review_git::bytes(&root, &["cat-file", "commit", &oid], 1024 * 1024, cancel)
                        .await?;
                let header = text(&commit)?.split("\n\n").next().unwrap_or("");
                let parent = header
                    .lines()
                    .find_map(|line| line.strip_prefix("parent "))
                    .map(str::to_owned);
                if let Some(parent) = &parent {
                    validate_oid(parent)?;
                    // Read the declared parent even in a shallow repository. Missing is an error, never an empty base.
                    review_git::bytes(&root, &["cat-file", "commit", parent], 1024 * 1024, cancel)
                        .await?;
                }
                let tree = tree_manifest(&root, &oid, cancel).await?;
                (Some(oid), parent, tree, None)
            }
        };
        let base = match &base_commit {
            Some(oid) => tree_manifest(&root, oid, cancel).await?,
            None => BTreeMap::new(),
        };
        let mut changed = base
            .keys()
            .chain(manifest.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter(|path| base.get(*path) != manifest.get(*path))
            .cloned()
            .collect::<Vec<_>>();
        if let ReviewSource::StagedFiles { paths } = &source {
            let changed_paths = changed.iter().map(String::as_str).collect::<BTreeSet<_>>();
            if paths
                .iter()
                .any(|path| !changed_paths.contains(path.as_str()))
            {
                return Err(BackendError::new(
                    ErrorCode::StaleFileOperation,
                    "所选文件不再包含已暂存变更，请刷新后重新选择。",
                ));
            }
            let mut selected = paths.iter().cloned().collect::<BTreeSet<_>>();
            include_selected_rename_sources(
                &root,
                base_commit.as_deref(),
                &base,
                &manifest,
                &changed_paths,
                &mut selected,
                cancel,
            )
            .await?;
            changed.retain(|path| selected.contains(path));
        }
        let mut snapshot = Self {
            root,
            source,
            resolved_commit,
            base_commit,
            manifest: BTreeMap::new(),
            deleted: BTreeMap::new(),
            batches: Vec::new(),
            excluded: Vec::new(),
            skipped_binary: Vec::new(),
            changed_file_count: changed.len(),
        };
        for (path, entry) in &manifest {
            if ordinary(entry) {
                snapshot.manifest.insert(path.clone(), entry.oid.clone());
            }
        }
        let mut files = Vec::new();
        let mut patch_bytes = 0;
        for path in changed {
            if excluded_path(&path)
                || manifest
                    .get(&path)
                    .or_else(|| base.get(&path))
                    .is_some_and(|entry| !ordinary(entry))
            {
                snapshot.excluded.push(path);
                continue;
            }
            let old = base.get(&path).filter(|entry| ordinary(entry));
            let new = manifest.get(&path).filter(|entry| ordinary(entry));
            if new.is_none()
                && let Some(old) = old
            {
                snapshot.deleted.insert(path.clone(), old.oid.clone());
            }
            let remaining = MAX_PATCH_BYTES.saturating_sub(patch_bytes);
            let patch = match (old, new) {
                (Some(old), Some(new)) if old.oid == new.oid => format!(
                    "diff --git a/{path} b/{path}\nold mode {}\nnew mode {}\n",
                    old.mode, new.mode
                ),
                (Some(old), Some(new)) => {
                    let raw = review_git::bytes(
                        &snapshot.root,
                        &[
                            "diff",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--no-color",
                            "--no-renames",
                            "--unified=3",
                            &old.oid,
                            &new.oid,
                            "--",
                        ],
                        remaining,
                        cancel,
                    )
                    .await?;
                    match String::from_utf8(raw) {
                        Ok(patch) => format!("Reviewed file: {path}\n{patch}"),
                        Err(_) => {
                            snapshot.skipped_binary.push(path);
                            continue;
                        }
                    }
                }
                (old, new) => {
                    let entry = new.or(old).ok_or_else(|| invalid("变更对象缺失。"))?;
                    let raw = review_git::bytes(
                        &snapshot.root,
                        &["cat-file", "blob", &entry.oid],
                        remaining,
                        cancel,
                    )
                    .await?;
                    if raw.contains(&0) {
                        snapshot.skipped_binary.push(path);
                        continue;
                    }
                    let content = match String::from_utf8(raw) {
                        Ok(value) => value,
                        Err(_) => {
                            snapshot.skipped_binary.push(path);
                            continue;
                        }
                    };
                    let count = content.lines().count();
                    let (old_count, new_count, marker) = if new.is_some() {
                        (0, count, '+')
                    } else {
                        (count, 0, '-')
                    };
                    let mut patch = format!(
                        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n",
                        usize::from(old_count > 0),
                        old_count,
                        usize::from(new_count > 0),
                        new_count
                    );
                    for line in content.lines() {
                        patch.push(marker);
                        patch.push_str(line);
                        patch.push('\n');
                    }
                    patch
                }
            };
            patch_bytes += patch.len();
            if patch_bytes > MAX_PATCH_BYTES {
                return Err(BackendError::new(
                    ErrorCode::AiContextTooLarge,
                    "审查差异超过 64 批次容量。",
                ));
            }
            if patch
                .lines()
                .any(|line| line.starts_with("Binary files ") || line == "GIT binary patch")
            {
                snapshot.skipped_binary.push(path);
                continue;
            }
            let changed_line_anchors = parse_unified_diff(&patch)
                .into_iter()
                .flat_map(|hunk| hunk.lines)
                .filter(|line| matches!(line.kind, DiffLineKind::Addition | DiffLineKind::Deletion))
                .filter_map(|line| line.new_line.or(line.old_line))
                .collect();
            files.push(StagedFilePatch {
                path,
                status: String::new(),
                binary: false,
                patch,
                changed_line_anchors,
            });
        }
        snapshot.batches = build_review_batches(&files)?;
        if let Some(original) = original_index {
            let current = review_git::bytes(
                &snapshot.root,
                &["ls-files", "--stage", "-z"],
                MAX_MANIFEST_BYTES,
                cancel,
            )
            .await?;
            if original != current
                || resolve_head(&snapshot.root, cancel).await? != snapshot.resolved_commit
            {
                return Err(BackendError::new(
                    ErrorCode::StaleFileOperation,
                    "捕获期间暂存区或 HEAD 发生变化，请重试。",
                ));
            }
        }
        Ok(snapshot)
    }
    pub fn object(&self, path: &str) -> Result<&str, BackendError> {
        if excluded_path(path) {
            return Err(invalid("构建产物已排除，不能读取正文。"));
        }
        self.manifest
            .get(path)
            .or_else(|| self.deleted.get(path))
            .map(String::as_str)
            .ok_or_else(|| invalid("请求的文件不在冻结源码清单中。"))
    }
    pub async fn blob(
        &self,
        path: &str,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<(String, Vec<u8>), BackendError> {
        let oid = self.object(path)?;
        let bytes =
            review_git::bytes(&self.root, &["cat-file", "blob", oid], limit, cancel).await?;
        Ok((oid.to_owned(), bytes))
    }
}

fn validate_selection(paths: &mut Vec<String>) -> Result<(), BackendError> {
    if paths.is_empty() || paths.len() > MAX_SELECTED_PATHS {
        return Err(invalid("请选择 1 到 10000 个已暂存文件。"));
    }
    let mut total = 0usize;
    let mut unique = BTreeSet::new();
    for path in paths.iter() {
        total = total.saturating_add(path.len());
        if path.len() > MAX_SELECTED_PATH_BYTES
            || total > MAX_SELECTION_BYTES
            || path.contains(['\0', '\\', ':'])
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(invalid("所选文件必须使用有效的仓库相对路径。"));
        }
    }
    paths.retain(|path| unique.insert(path.clone()));
    Ok(())
}

// Git discovers rename pairs, but only exact paths in the frozen manifests may expand selection.
async fn include_selected_rename_sources(
    root: &Path,
    base_commit: Option<&str>,
    base: &BTreeMap<String, Entry>,
    manifest: &BTreeMap<String, Entry>,
    changed: &BTreeSet<&str>,
    selected: &mut BTreeSet<String>,
    cancel: &CancellationToken,
) -> Result<(), BackendError> {
    if base_commit.is_none() {
        return Ok(());
    }
    let mut args = vec![
        "diff",
        "--cached",
        "--name-status",
        "-z",
        "--find-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
    ];
    if let Some(base) = base_commit {
        args.push(base);
    }
    args.push("--");
    let raw = review_git::bytes(root, &args, MAX_MANIFEST_BYTES, cancel).await?;
    let mut records = raw.split(|byte| *byte == 0).peekable();
    while let Some(status) = records.next() {
        if status.is_empty() && records.peek().is_none() {
            break;
        }
        let status = text(status)?;
        let old = text(
            records
                .next()
                .ok_or_else(|| invalid("Git 变更清单格式错误。"))?,
        )?;
        if status.starts_with('R') || status.starts_with('C') {
            let new = text(
                records
                    .next()
                    .ok_or_else(|| invalid("Git 重命名清单格式错误。"))?,
            )?;
            if status.starts_with('R') && selected.contains(new) {
                if !changed.contains(old)
                    || !changed.contains(new)
                    || !base.contains_key(old)
                    || manifest.contains_key(old)
                    || !manifest.contains_key(new)
                {
                    return Err(BackendError::new(
                        ErrorCode::StaleFileOperation,
                        "捕获期间重命名变更发生变化，请刷新后重试。",
                    ));
                }
                selected.insert(old.to_owned());
            }
        }
    }
    Ok(())
}

pub(crate) fn excluded_path(path: &str) -> bool {
    path.starts_with("extra_apps/xadmin/static/vue_common/")
        || path.starts_with("extra_apps/xadmin/static/js/")
        || path.ends_with(".map")
}
fn ordinary(entry: &Entry) -> bool {
    matches!(entry.mode.as_str(), "100644" | "100755")
}
fn invalid(message: &str) -> BackendError {
    BackendError::new(ErrorCode::InvalidPath, message)
}
fn text(bytes: &[u8]) -> Result<&str, BackendError> {
    std::str::from_utf8(bytes).map_err(|_| {
        BackendError::new(
            ErrorCode::UnsupportedEncoding,
            "Git 清单或文本必须使用 UTF-8。",
        )
    })
}
fn validate_oid(oid: &str) -> Result<(), BackendError> {
    if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("无效 Git 对象标识。"));
    }
    Ok(())
}
async fn resolve(
    root: &Path,
    revision: &str,
    cancel: &CancellationToken,
) -> Result<String, BackendError> {
    let raw = review_git::bytes(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{revision}^{{commit}}"),
        ],
        256,
        cancel,
    )
    .await?;
    let oid = text(&raw)?.trim().to_owned();
    validate_oid(&oid)?;
    Ok(oid)
}
async fn resolve_head(
    root: &Path,
    cancel: &CancellationToken,
) -> Result<Option<String>, BackendError> {
    let (success, raw) = review_git::optional_bytes(
        root,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        256,
        cancel,
    )
    .await?;
    if success {
        let oid = text(&raw)?.trim().to_owned();
        validate_oid(&oid)?;
        return Ok(Some(oid));
    }
    let branch =
        review_git::bytes(root, &["symbolic-ref", "--quiet", "HEAD"], 4096, cancel).await?;
    let (exists, _) = review_git::optional_bytes(
        root,
        &["show-ref", "--verify", text(&branch)?.trim()],
        4096,
        cancel,
    )
    .await?;
    if exists {
        return Err(invalid("HEAD 不能解析为提交。"));
    }
    Ok(None)
}
async fn tree_manifest(
    root: &Path,
    oid: &str,
    cancel: &CancellationToken,
) -> Result<BTreeMap<String, Entry>, BackendError> {
    let raw = review_git::bytes(
        root,
        &["ls-tree", "-r", "-z", "--full-tree", oid],
        MAX_MANIFEST_BYTES,
        cancel,
    )
    .await?;
    parse_manifest(&raw, false)
}
fn parse_manifest(raw: &[u8], index: bool) -> Result<BTreeMap<String, Entry>, BackendError> {
    let mut entries = BTreeMap::new();
    for record in raw
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if entries.len() >= MAX_MANIFEST_ENTRIES {
            return Err(BackendError::new(
                ErrorCode::AiContextTooLarge,
                "Git 清单超过 200000 项。",
            ));
        }
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| invalid("Git 清单格式错误。"))?;
        let fields = text(&record[..tab])?.split(' ').collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(invalid("Git 清单格式错误。"));
        }
        if index && fields[2] != "0" {
            return Err(BackendError::new(
                ErrorCode::GitConflict,
                "暂存区存在冲突，无法冻结审查。",
            ));
        }
        let oid = if index { fields[1] } else { fields[2] };
        validate_oid(oid)?;
        let path = text(&record[tab + 1..])?;
        if path.is_empty()
            || path.contains(['\\', ':'])
            || Path::new(path)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(invalid("Git 清单含不安全路径。"));
        }
        if entries
            .insert(
                path.to_owned(),
                Entry {
                    oid: oid.to_owned(),
                    mode: fields[0].to_owned(),
                },
            )
            .is_some()
        {
            return Err(invalid("Git 清单路径重复。"));
        }
    }
    Ok(entries)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::infrastructure::git_runner::GitCommandRunner;
    pub async fn git(root: &Path, args: &[&str]) -> String {
        GitCommandRunner::default()
            .run(Some(root), args)
            .await
            .unwrap()
            .stdout
            .trim()
            .to_owned()
    }
    pub async fn repository() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]).await;
        git(dir.path(), &["config", "user.name", "Review Test"]).await;
        git(dir.path(), &["config", "user.email", "review@example.test"]).await;
        git(dir.path(), &["config", "core.autocrlf", "false"]).await;
        dir
    }
    async fn commit(root: &Path, text: &str) -> String {
        std::fs::write(root.join("code.py"), text).unwrap();
        git(root, &["add", "code.py"]).await;
        git(root, &["commit", "-m", "fixture"]).await;
        git(root, &["rev-parse", "HEAD"]).await
    }
    #[tokio::test]
    async fn review_large_commit_and_staged_patch_are_split_without_worktree_content() {
        let dir = repository().await;
        let original = (0..2100)
            .map(|i| format!("old_value_{i} = '原始内容'\n"))
            .collect::<String>();
        let updated = (0..2100)
            .map(|i| format!("new_value_{i} = '更新内容'\n"))
            .collect::<String>();
        commit(dir.path(), &original).await;
        std::fs::write(dir.path().join("code.py"), &updated).unwrap();
        git(dir.path(), &["add", "code.py"]).await;
        let cancel = CancellationToken::new();
        let staged = ReviewSnapshot::capture(dir.path(), ReviewSource::Staged, &cancel)
            .await
            .unwrap();
        let revision = commit(dir.path(), &updated).await;
        std::fs::write(dir.path().join("code.py"), "WORKTREE_ONLY\n").unwrap();
        let history =
            ReviewSnapshot::capture(dir.path(), ReviewSource::Commit { revision }, &cancel)
                .await
                .unwrap();
        assert_eq!(staged.batches, history.batches);
        assert!(history.batches.len() > 1);
        let mut removed = Vec::new();
        let mut added = Vec::new();
        for batch in &history.batches {
            assert!(batch.text.len() <= super::super::ai_context::MAX_DIFF_BATCH_BYTES);
            assert!(!batch.text.contains("WORKTREE_ONLY"));
            let mut anchors = Vec::new();
            for line in parse_unified_diff(&batch.text)
                .into_iter()
                .flat_map(|h| h.lines)
            {
                match line.kind {
                    DiffLineKind::Addition => {
                        added.push(line.new_line.unwrap());
                        anchors.push(line.new_line.unwrap());
                    }
                    DiffLineKind::Deletion => {
                        removed.push(line.old_line.unwrap());
                        anchors.push(line.old_line.unwrap());
                    }
                    _ => {}
                }
            }
            anchors.sort_unstable();
            anchors.dedup();
            assert_eq!(batch.allowed_line_anchors["code.py"], anchors);
        }
        assert_eq!(added, (1..=2100).collect::<Vec<_>>());
        assert_eq!(removed, (1..=2100).collect::<Vec<_>>());
    }

    fn selected(paths: &[&str]) -> ReviewSource {
        serde_json::from_value(serde_json::json!({"kind": "stagedFiles", "paths": paths}))
            .expect("selected staged review source must deserialize")
    }
    #[test]
    fn selected_staged_request_bounds_reject_oversized_input() {
        assert!(validate_selection(&mut vec!["file".to_owned(); MAX_SELECTED_PATHS + 1]).is_err());
        assert!(validate_selection(&mut vec!["x".repeat(MAX_SELECTED_PATH_BYTES + 1)]).is_err());
        assert!(
            validate_selection(&mut vec![
                "x".repeat(MAX_SELECTED_PATH_BYTES);
                MAX_SELECTION_BYTES / MAX_SELECTED_PATH_BYTES + 1
            ])
            .is_err()
        );
    }
    #[tokio::test]
    async fn selected_staged_snapshot_limits_targets_before_loading_blobs() {
        let dir = repository().await;
        commit(dir.path(), "base\n").await;
        std::fs::write(dir.path().join("code.py"), "selected staged\n").unwrap();
        std::fs::write(dir.path().join("huge.txt"), vec![b'x'; MAX_PATCH_BYTES + 1]).unwrap();
        std::fs::write(dir.path().join("related.py"), "related staged\n").unwrap();
        git(dir.path(), &["add", "."]).await;
        let index = std::fs::read(dir.path().join(".git/index")).unwrap();
        std::fs::write(dir.path().join("code.py"), "unstaged\n").unwrap();
        let cancel = CancellationToken::new();
        let snapshot =
            ReviewSnapshot::capture(dir.path(), selected(&["code.py", "code.py"]), &cancel)
                .await
                .unwrap();
        assert_eq!(snapshot.changed_file_count, 1);
        assert_eq!(snapshot.batches[0].file_paths, ["code.py"]);
        assert!(snapshot.batches[0].text.contains("+selected staged"));
        assert!(!snapshot.batches[0].text.contains("unstaged"));
        assert_eq!(
            serde_json::to_value(&snapshot.source).unwrap(),
            serde_json::json!({"kind": "stagedFiles", "paths": ["code.py"]})
        );
        assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
        std::fs::write(dir.path().join("related.py"), "related later\n").unwrap();
        git(dir.path(), &["add", "."]).await;
        assert_eq!(
            snapshot.blob("related.py", 32768, &cancel).await.unwrap().1,
            b"related staged\n"
        );
        assert_eq!(
            snapshot.blob("code.py", 32768, &cancel).await.unwrap().1,
            b"selected staged\n"
        );
    }
    #[tokio::test]
    async fn selected_staged_rejects_empty_invalid_and_no_longer_staged_paths() {
        let dir = repository().await;
        commit(dir.path(), "base\n").await;
        std::fs::write(dir.path().join("other.py"), "staged\n").unwrap();
        git(dir.path(), &["add", "."]).await;
        for paths in [
            vec![],
            vec![""],
            vec!["code.py"],
            vec!["missing.py"],
            vec!["../other.py"],
            vec!["/other.py"],
            vec!["C:/other.py"],
            vec!["./other.py"],
            vec!["a//b"],
            vec!["a\\b"],
            vec!["other.py", "missing.py"],
            vec!["a\0b"],
        ] {
            assert!(
                ReviewSnapshot::capture(dir.path(), selected(&paths), &CancellationToken::new())
                    .await
                    .is_err(),
                "must reject {paths:?}"
            );
        }
        git(dir.path(), &["reset", "--", "other.py"]).await;
        assert!(
            ReviewSnapshot::capture(
                dir.path(),
                selected(&["other.py"]),
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn selected_staged_root_commit_paths_are_literal_and_unicode_safe() {
        let dir = repository().await;
        for path in ["[draft].txt", "draft.txt", "文件 空格.txt", "--option.txt"] {
            std::fs::write(dir.path().join(path), format!("{path}\n")).unwrap();
        }
        git(dir.path(), &["add", "."]).await;
        let snapshot = ReviewSnapshot::capture(
            dir.path(),
            selected(&["[draft].txt", "文件 空格.txt", "--option.txt"]),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(snapshot.changed_file_count, 3);
        let paths = snapshot
            .batches
            .iter()
            .flat_map(|batch| batch.file_paths.iter())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 3);
        assert!(!paths.iter().any(|path| path.as_str() == "draft.txt"));
        assert!(snapshot.base_commit.is_none());
    }
    #[tokio::test]
    async fn selected_staged_rename_includes_only_its_old_path_and_deletion_has_evidence() {
        let dir = repository().await;
        commit(dir.path(), "rename source\n").await;
        std::fs::write(dir.path().join("unrelated.txt"), "different content\n").unwrap();
        git(dir.path(), &["add", "."]).await;
        git(dir.path(), &["commit", "-m", "other"]).await;
        git(dir.path(), &["mv", "code.py", "renamed.py"]).await;
        git(dir.path(), &["rm", "unrelated.txt"]).await;
        let cancel = CancellationToken::new();
        let snapshot = ReviewSnapshot::capture(dir.path(), selected(&["renamed.py"]), &cancel)
            .await
            .unwrap();
        assert_eq!(snapshot.changed_file_count, 2);
        assert_eq!(snapshot.batches[0].file_paths, ["code.py", "renamed.py"]);
        assert_eq!(
            snapshot.object("code.py").unwrap(),
            snapshot.object("renamed.py").unwrap()
        );
        assert_eq!(
            serde_json::to_value(snapshot.source).unwrap(),
            serde_json::json!({"kind": "stagedFiles", "paths": ["renamed.py"]})
        );
        let deleted = ReviewSnapshot::capture(dir.path(), selected(&["unrelated.txt"]), &cancel)
            .await
            .unwrap();
        assert_eq!(deleted.changed_file_count, 1);
        assert_eq!(
            deleted
                .blob("unrelated.txt", 32768, &cancel)
                .await
                .unwrap()
                .1,
            b"different content\n"
        );
    }
    #[tokio::test]
    async fn selected_staged_exclusions_and_binary_counts_only_include_selection() {
        let dir = repository().await;
        for path in ["chosen.map", "other.map", "chosen.bin", "other.bin"] {
            std::fs::write(dir.path().join(path), [0, 255, 1]).unwrap();
        }
        git(dir.path(), &["add", "."]).await;
        let snapshot = ReviewSnapshot::capture(
            dir.path(),
            selected(&["chosen.map", "chosen.bin"]),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(snapshot.changed_file_count, 2);
        assert_eq!(snapshot.excluded, ["chosen.map"]);
        assert_eq!(snapshot.skipped_binary, ["chosen.bin"]);
        assert!(snapshot.batches.is_empty());
    }
    #[tokio::test]
    async fn staged_snapshot_and_evidence_stay_on_frozen_index() {
        let dir = repository().await;
        commit(dir.path(), "base\n").await;
        std::fs::write(dir.path().join("code.py"), "staged\n").unwrap();
        git(dir.path(), &["add", "code.py"]).await;
        let index = std::fs::read(dir.path().join(".git/index")).unwrap();
        std::fs::write(dir.path().join("code.py"), "unstaged\n").unwrap();
        let cancel = CancellationToken::new();
        let snapshot = ReviewSnapshot::capture(dir.path(), ReviewSource::Staged, &cancel)
            .await
            .unwrap();
        assert!(snapshot.batches[0].text.contains("+staged"));
        assert!(!snapshot.batches[0].text.contains("unstaged"));
        assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
        git(dir.path(), &["add", "code.py"]).await;
        assert_eq!(
            snapshot.blob("code.py", 32768, &cancel).await.unwrap().1,
            b"staged\n"
        );
        assert!(snapshot.blob("untracked.py", 32768, &cancel).await.is_err());
    }
    #[tokio::test]
    async fn historical_root_and_nonroot_are_isolated_from_current_index() {
        let dir = repository().await;
        let root_oid = commit(dir.path(), "first\n").await;
        let next = commit(dir.path(), "second\n").await;
        std::fs::write(dir.path().join("code.py"), "current\n").unwrap();
        git(dir.path(), &["add", "code.py"]).await;
        let cancel = CancellationToken::new();
        let snapshot = ReviewSnapshot::capture(
            dir.path(),
            ReviewSource::Commit {
                revision: root_oid.clone(),
            },
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(snapshot.resolved_commit.as_deref(), Some(root_oid.as_str()));
        assert!(snapshot.base_commit.is_none());
        assert!(snapshot.batches[0].text.contains("+first"));
        assert_eq!(
            snapshot.blob("code.py", 32768, &cancel).await.unwrap().1,
            b"first\n"
        );
        let snapshot =
            ReviewSnapshot::capture(dir.path(), ReviewSource::Commit { revision: next }, &cancel)
                .await
                .unwrap();
        assert_eq!(snapshot.base_commit.as_deref(), Some(root_oid.as_str()));
        assert!(snapshot.batches[0].text.contains("+second"));
        assert!(!snapshot.batches[0].text.contains("current"));
    }
    #[tokio::test]
    async fn deletion_evidence_uses_parent_and_artifacts_are_excluded() {
        let dir = repository().await;
        commit(dir.path(), "deleted source\n").await;
        git(dir.path(), &["rm", "code.py"]).await;
        std::fs::write(dir.path().join("bundle.map"), [0xff; 128]).unwrap();
        git(dir.path(), &["add", "bundle.map"]).await;
        let cancel = CancellationToken::new();
        let snapshot = ReviewSnapshot::capture(dir.path(), ReviewSource::Staged, &cancel)
            .await
            .unwrap();
        assert_eq!(snapshot.excluded, ["bundle.map"]);
        assert!(snapshot.blob("bundle.map", 32768, &cancel).await.is_err());
        assert_eq!(
            snapshot.blob("code.py", 32768, &cancel).await.unwrap().1,
            b"deleted source\n"
        );
        assert_eq!(snapshot.changed_file_count, 2);
    }

    #[tokio::test]
    async fn binary_marker_inside_source_text_does_not_exclude_source() {
        let dir = repository().await;
        std::fs::write(
            dir.path().join("text.txt"),
            "print('Binary files differ')\n",
        )
        .unwrap();
        git(dir.path(), &["add", "text.txt"]).await;
        let snapshot =
            ReviewSnapshot::capture(dir.path(), ReviewSource::Staged, &CancellationToken::new())
                .await
                .unwrap();
        assert_eq!(snapshot.batches.len(), 1);
        assert!(snapshot.skipped_binary.is_empty());
    }

    #[tokio::test]
    async fn merge_review_compares_first_parent_and_rename_keeps_both_objects() {
        let dir = repository().await;
        commit(dir.path(), "base\n").await;
        git(dir.path(), &["checkout", "-b", "feature"]).await;
        std::fs::write(dir.path().join("feature.txt"), "feature value\n").unwrap();
        git(dir.path(), &["add", "feature.txt"]).await;
        git(dir.path(), &["commit", "-m", "feature"]).await;
        git(dir.path(), &["checkout", "main"]).await;
        let parent = commit(dir.path(), "main value\n").await;
        git(dir.path(), &["merge", "--no-ff", "feature", "-m", "merge"]).await;
        let snapshot = ReviewSnapshot::capture(
            dir.path(),
            ReviewSource::Commit {
                revision: "HEAD".to_owned(),
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(snapshot.base_commit, Some(parent));
        assert_eq!(snapshot.changed_file_count, 1);
        assert_eq!(snapshot.batches[0].file_paths, ["feature.txt"]);
        git(dir.path(), &["mv", "feature.txt", "renamed.txt"]).await;
        let snapshot =
            ReviewSnapshot::capture(dir.path(), ReviewSource::Staged, &CancellationToken::new())
                .await
                .unwrap();
        assert_eq!(snapshot.changed_file_count, 2);
        assert_eq!(
            snapshot.object("feature.txt").unwrap(),
            snapshot.object("renamed.txt").unwrap()
        );
    }

    #[tokio::test]
    async fn missing_shallow_parent_is_error_instead_of_root_commit() {
        let dir = repository().await;
        commit(dir.path(), "base\n").await;
        commit(dir.path(), "next\n").await;
        let clone = tempfile::tempdir().unwrap();
        let source_url = format!(
            "file:///{}",
            dir.path().to_str().unwrap().replace('\\', "/")
        );
        git(clone.path(), &["clone", "--depth=1", &source_url, "."]).await;
        let result = ReviewSnapshot::capture(
            clone.path(),
            ReviewSource::Commit {
                revision: "HEAD".to_owned(),
            },
            &CancellationToken::new(),
        )
        .await;
        assert!(result.is_err());
    }
}
