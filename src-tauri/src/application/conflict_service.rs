use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::conflict_content::{
    self as content, MAX_CONFLICT_PREVIEW_BYTES, add_fingerprint, hash_file,
};
use super::conflict_paths::{is_link, validate_relative, validate_target};
use super::mutation_coordinator::RepositoryMutationCoordinator;
use super::operation_state::{
    MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
};
use super::refs_service::RefsService;
use super::repository_service::RepositoryService;
use crate::domain::conflicts::*;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::operation::{ConflictFileSummary, RepositoryOperationKind};
use crate::infrastructure::git_runner::GitCommandRunner;

const MAX_INDEX_RECORD_BYTES: usize = 64 * 1024 * 1024;
const MAX_GIT_METADATA_BYTES: usize = 64 * 1024;
const RECOVERY_DIRECTORY: &str = "hq-git-recovery";
const OPERATION_FILES: &[&str] = &[
    "HEAD",
    "MERGE_HEAD",
    "MERGE_MSG",
    "MERGE_MODE",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "ORIG_HEAD",
    "AUTO_MERGE",
    "sequencer",
    "rebase-merge",
    "rebase-apply",
];

#[derive(Debug, Clone)]
struct Stage {
    mode: String,
    oid: String,
    number: usize,
}
type Unmerged = BTreeMap<String, Vec<Stage>>;

#[derive(Debug, Clone, Default)]
pub struct ConflictService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
}

impl ConflictService {
    pub(super) async fn ai_detail(
        &self,
        requested_root: &Path,
        relative_path: &str,
        token: &str,
    ) -> Result<ConflictDetail, BackendError> {
        use super::conflict_content::MAX_CONFLICT_PREVIEW_BYTES as MAX_VERSION_BYTES;
        validate_relative(relative_path)?;
        let root = self.root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        // Bound the working file before token hashing or reading its contents.
        let target = validate_target(&root, relative_path)?;
        if let Ok(metadata) = fs::symlink_metadata(&target)
            && metadata.len() > MAX_VERSION_BYTES as u64
        {
            return Err(ai_too_large());
        }
        if self.detail_token(&root, relative_path).await? != token {
            return Err(stale());
        }
        let detail = self
            .detail_at_root_with_limit(&root, relative_path, MAX_VERSION_BYTES)
            .await?;
        if detail.token != token {
            return Err(stale());
        }
        if [&detail.base, &detail.ours, &detail.theirs, &detail.working]
            .iter()
            .any(|version| version.kind == ConflictVersionKind::TooLarge)
        {
            return Err(ai_too_large());
        }
        if !detail.editable {
            return Err(unsupported(
                detail
                    .unsupported_reason
                    .clone()
                    .unwrap_or_else(|| "AI 建议仅支持普通 UTF-8 文本。".into()),
            ));
        }
        Ok(detail)
    }

    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
        }
    }

    async fn root(&self, path: &Path) -> Result<PathBuf, BackendError> {
        RepositoryService::with_coordinator(self.runner.clone(), self.coordinator.clone())
            .resolve_root(path)
            .await
    }

    pub async fn snapshot(&self, requested_root: &Path) -> Result<ConflictSnapshot, BackendError> {
        let root = self.root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        self.snapshot_at_root(&root).await
    }

    pub async fn detail(
        &self,
        requested_root: &Path,
        relative_path: &str,
    ) -> Result<ConflictDetail, BackendError> {
        validate_relative(relative_path)?;
        let root = self.root(requested_root).await?;
        let _guard = self.coordinator.read(&root).await;
        self.detail_at_root(&root, relative_path).await
    }

    pub async fn resolve(
        &self,
        requested_root: &Path,
        request: ResolveConflictRequest,
    ) -> Result<ConflictMutationResult, BackendError> {
        validate_relative(&request.relative_path)?;
        let root = self.root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        if self.detail_token(&root, &request.relative_path).await? != request.token {
            return Err(stale());
        }
        let detail = self.detail_at_root(&root, &request.relative_path).await?;
        if detail.token != request.token {
            return Err(stale());
        }
        let operation = read_operation_state(&root, &self.runner).await?;
        ensure_mutation_allowed(&operation, MutationIntent::ResolveConflict)?;
        let target = validate_target(&root, &request.relative_path)?;
        if let Some(reason) = &detail.unsupported_reason {
            // Content limitations restrict editing, but permit exact whole-side adoption.
            if !detail.can_delete {
                return Err(unsupported(reason));
            }
        }
        let template = if detail.working.exists {
            &detail.working
        } else if detail.ours.exists {
            &detail.ours
        } else {
            &detail.theirs
        };
        let text_bytes = match &request.resolution {
            ConflictResolution::Text {
                text,
                acknowledge_markers,
            } => {
                if !detail.editable {
                    return Err(unsupported("This conflict cannot be edited as text."));
                }
                Some(content::encode_text(text, template, *acknowledge_markers)?)
            }
            ConflictResolution::Ours if !detail.can_choose_ours => {
                return Err(unsupported(
                    "Stage 2 is missing or unsupported; choose deletion explicitly when appropriate.",
                ));
            }
            ConflictResolution::Theirs if !detail.can_choose_theirs => {
                return Err(unsupported(
                    "Stage 3 is missing or unsupported; choose deletion explicitly when appropriate.",
                ));
            }
            ConflictResolution::Delete if !detail.can_delete => {
                return Err(unsupported("Deletion is not supported for this path."));
            }
            _ => None,
        };
        let recovery_path = self
            .backup(&root, &target, &request.relative_path, &request.token)
            .await?;
        // The app lock does not exclude external editors. Recheck after backup and immediately before writing.
        validate_target(&root, &request.relative_path)?;
        if self.detail_token(&root, &request.relative_path).await? != request.token {
            return Err(stale());
        }
        let written = match &request.resolution {
            ConflictResolution::Text { .. } => content::replace_text(
                &target,
                text_bytes.as_deref().expect("validated text"),
                template.mode.as_deref(),
            ),
            ConflictResolution::Delete => match fs::remove_file(&target) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                result => result.map_err(BackendError::from),
            },
            ConflictResolution::Ours | ConflictResolution::Theirs => {
                let side = if matches!(request.resolution, ConflictResolution::Ours) {
                    "--ours"
                } else {
                    "--theirs"
                };
                self.runner
                    .run(
                        Some(&root),
                        [
                            "--literal-pathspecs",
                            "checkout",
                            side,
                            "--",
                            &request.relative_path,
                        ],
                    )
                    .await
                    .map(|_| ())
            }
        };
        let result = match written {
            Ok(()) => self.runner.run(Some(&root), ["--literal-pathspecs", "add", "--all", "--", &request.relative_path]).await.map(|_| ()).map_err(|mut error| {
                error.message = format!("The working file was saved, but staging failed; the conflict remains unresolved. {}", error.message);
                error
            }),
            Err(error) => Err(error),
        };
        self.mutation_result(
            &root,
            Some(&request.relative_path),
            result.err(),
            recovery_path,
            false,
        )
        .await
    }

    pub async fn continue_operation(
        &self,
        requested_root: &Path,
        operation_token: &str,
    ) -> Result<ConflictMutationResult, BackendError> {
        let root = self.root(requested_root).await?;
        let _guard = self.coordinator.write(&root).await;
        let snapshot = self.snapshot_at_root(&root).await?;
        if snapshot.operation_token != operation_token {
            return Err(stale());
        }
        ensure_mutation_allowed(&snapshot.operation_state, MutationIntent::ContinueConflict)?;
        let command = match snapshot.operation_state.kind {
            RepositoryOperationKind::Merge => ["-c", "core.editor=true", "commit", "--no-edit"],
            RepositoryOperationKind::Rebase => ["-c", "core.editor=true", "rebase", "--continue"],
            RepositoryOperationKind::CherryPick => {
                ["-c", "core.editor=true", "cherry-pick", "--continue"]
            }
            RepositoryOperationKind::Revert => ["-c", "core.editor=true", "revert", "--continue"],
            RepositoryOperationKind::None => {
                return Err(unsupported("No supported operation can be continued."));
            }
        };
        if self.operation_token(&root).await? != operation_token
            || !self.unmerged(&root).await?.is_empty()
        {
            return Err(stale());
        }
        let result = match self.runner.run_without_editor(Some(&root), command).await {
            Ok(output) => output.into_result().map(|_| ()),
            Err(error) => Err(error),
        };
        self.mutation_result(&root, None, result.err(), None, true)
            .await
    }

    async fn snapshot_at_root(&self, root: &Path) -> Result<ConflictSnapshot, BackendError> {
        let mut operation_state = read_operation_state(root, &self.runner).await?;
        let unmerged = self.unmerged(root).await?;
        let ambiguous = self
            .ambiguous_rename_paths(root, operation_state.kind, &unmerged)
            .await?;
        let files: Vec<_> = unmerged
            .iter()
            .map(|(path, stages)| {
                let reason = unsupported_path(root, path, stages, &ambiguous);
                ConflictFile {
                    path: path.clone(),
                    status: conflict_status(stages).into(),
                    supported: reason.is_none(),
                    reason,
                }
            })
            .collect();
        operation_state.conflicts = files
            .iter()
            .map(|file| ConflictFileSummary {
                path: file.path.clone(),
                status: file.status.clone(),
            })
            .collect();
        let staged = self
            .runner
            .run_bytes(
                Some(root),
                [
                    "diff",
                    "--cached",
                    "--name-only",
                    "--diff-filter=ACDMRT",
                    "-z",
                ],
                MAX_INDEX_RECORD_BYTES,
            )
            .await?;
        let staged_files = strict_utf8(&staged)?
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        let continue_action = if files.is_empty() {
            operation_state.abort_action
        } else {
            None
        };
        let operation_token = self.operation_token(root).await?;
        Ok(ConflictSnapshot {
            operation_state,
            operation_token,
            files,
            continue_action,
            staged_files,
        })
    }

    async fn detail_at_root(
        &self,
        root: &Path,
        path: &str,
    ) -> Result<ConflictDetail, BackendError> {
        self.detail_at_root_with_limit(root, path, MAX_CONFLICT_PREVIEW_BYTES)
            .await
    }

    async fn detail_at_root_with_limit(
        &self,
        root: &Path,
        path: &str,
        max_bytes: usize,
    ) -> Result<ConflictDetail, BackendError> {
        let before = self.detail_token(root, path).await?;
        let unmerged = self.unmerged(root).await?;
        let stages = unmerged.get(path).ok_or_else(stale)?;
        let operation = read_operation_state(root, &self.runner).await?;
        let ambiguous = self
            .ambiguous_rename_paths(root, operation.kind, &unmerged)
            .await?;
        let path_reason = unsupported_path(root, path, stages, &ambiguous);
        let base = self
            .stage_version(
                root,
                stages.iter().find(|stage| stage.number == 1),
                max_bytes,
            )
            .await?;
        let ours = self
            .stage_version(
                root,
                stages.iter().find(|stage| stage.number == 2),
                max_bytes,
            )
            .await?;
        let theirs = self
            .stage_version(
                root,
                stages.iter().find(|stage| stage.number == 3),
                max_bytes,
            )
            .await?;
        let working = match validate_target(root, path) {
            Ok(target) => content::working_version_with_limit(&target, max_bytes)?,
            Err(_) => content::metadata_version(None, None, 0, ConflictVersionKind::Unsupported),
        };
        let supported = path_reason.is_none();
        let editable = supported
            && [&base, &ours, &theirs, &working].iter().all(|version| {
                matches!(
                    version.kind,
                    ConflictVersionKind::Text | ConflictVersionKind::Missing
                )
            });
        let unsupported_reason = path_reason.or_else(|| if editable { None } else { Some("Manual editing requires UTF-8 text up to 2 MiB with consistent line endings. An existing whole side can still be adopted.".into()) });
        let token = self.detail_token(root, path).await?;
        if before != token {
            return Err(stale());
        }
        Ok(ConflictDetail {
            path: path.into(),
            token,
            operation_kind: operation.kind,
            can_choose_ours: supported && ours.exists,
            can_choose_theirs: supported && theirs.exists,
            can_delete: supported,
            base,
            ours,
            theirs,
            working,
            editable,
            unsupported_reason,
        })
    }

    async fn unmerged(&self, root: &Path) -> Result<Unmerged, BackendError> {
        let bytes = self
            .runner
            .run_bytes(
                Some(root),
                ["ls-files", "--unmerged", "-z"],
                MAX_INDEX_RECORD_BYTES,
            )
            .await?;
        parse_unmerged(&bytes)
    }

    async fn ambiguous_rename_paths(
        &self,
        root: &Path,
        kind: RepositoryOperationKind,
        unmerged: &Unmerged,
    ) -> Result<BTreeSet<String>, BackendError> {
        let mut ambiguous = BTreeSet::new();
        if unmerged.is_empty() {
            return Ok(ambiguous);
        }
        let mut comparisons = Vec::new();
        match kind {
            RepositoryOperationKind::Merge => {
                let bases = self
                    .runner
                    .run_allowing_failure(Some(root), ["merge-base", "--all", "HEAD", "MERGE_HEAD"])
                    .await?;
                if bases.is_success() {
                    for base in bases.stdout.lines() {
                        comparisons.push((base.to_owned(), "MERGE_HEAD".to_owned()));
                    }
                } else if bases.status_code != Some(1) {
                    return Err(bases.into_result().expect_err("failed merge-base probe"));
                }
            }
            RepositoryOperationKind::Revert => {
                let parents = self.runner.run(Some(root), ["rev-list", "--parents", "-n", "1", "REVERT_HEAD"]).await?;
                // Revert applies the reverse diff; inspect each parent conservatively for rename ambiguity.
                for parent in parents.stdout.split_whitespace().skip(1) {
                    comparisons.push(("REVERT_HEAD".to_owned(), parent.to_owned()));
                }
            }
            RepositoryOperationKind::Rebase | RepositoryOperationKind::CherryPick => {
                let side = if kind == RepositoryOperationKind::Rebase {
                    "REBASE_HEAD"
                } else {
                    "CHERRY_PICK_HEAD"
                };
                let parent = self
                    .runner
                    .run_allowing_failure(
                        Some(root),
                        ["rev-parse", "--verify", "--quiet", &format!("{side}^1")],
                    )
                    .await?;
                if parent.is_success() {
                    comparisons.push((parent.stdout.trim().to_owned(), side.to_owned()));
                }
            }
            RepositoryOperationKind::None => {
                // Stash conflicts have no sequencer. Retained entries provide candidate original trees.
                let stash_ref = self
                    .runner
                    .run_allowing_failure(
                        Some(root),
                        ["rev-parse", "--verify", "--quiet", "refs/stash"],
                    )
                    .await?;
                if stash_ref.is_success() {
                    let stashes = self
                        .runner
                        .run_bytes(
                            Some(root),
                            ["reflog", "show", "--format=%H", "refs/stash"],
                            MAX_INDEX_RECORD_BYTES,
                        )
                        .await?;
                    for stash in strict_utf8(&stashes)?.lines().collect::<BTreeSet<_>>() {
                        if self.matches_stage_three(root, stash, unmerged).await? {
                            comparisons.push((format!("{stash}^1"), stash.to_owned()));
                        }
                    }
                }
            }
        }
        for (base, side) in comparisons {
            let ours = self.rename_pairs(root, &base, "HEAD").await?;
            let theirs = self.rename_pairs(root, &base, &side).await?;
            let by_destination: BTreeMap<_, _> = ours
                .iter()
                .map(|(source, target)| (target, source))
                .collect();
            for (source, target) in &theirs {
                if let Some(our_target) = ours.get(source)
                    && our_target != target
                {
                    ambiguous.extend([source.clone(), target.clone(), our_target.clone()]);
                }
                if let Some(our_source) = by_destination.get(target)
                    && *our_source != source
                {
                    ambiguous.extend([source.clone(), target.clone(), (*our_source).clone()]);
                }
            }
        }
        Ok(ambiguous)
    }

    async fn rename_pairs(
        &self,
        root: &Path,
        base: &str,
        tip: &str,
    ) -> Result<BTreeMap<String, String>, BackendError> {
        let bytes = self
            .runner
            .run_bytes(
                Some(root),
                [
                    "-c",
                    "diff.renameLimit=0",
                    "diff",
                    "--name-status",
                    "-z",
                    "--find-renames",
                    "--diff-filter=R",
                    base,
                    tip,
                    "--",
                ],
                MAX_INDEX_RECORD_BYTES,
            )
            .await?;
        let mut fields = strict_utf8(&bytes)?
            .split('\0')
            .filter(|field| !field.is_empty());
        let mut renames = BTreeMap::new();
        while let Some(status) = fields.next() {
            let source = fields
                .next()
                .ok_or_else(|| unsupported("Invalid Git rename record."))?;
            let target = fields
                .next()
                .ok_or_else(|| unsupported("Invalid Git rename record."))?;
            if !status.starts_with('R') {
                return Err(unsupported("Unexpected Git rename record."));
            }
            renames.insert(source.to_owned(), target.to_owned());
        }
        Ok(renames)
    }

    async fn matches_stage_three(
        &self,
        root: &Path,
        tree: &str,
        unmerged: &Unmerged,
    ) -> Result<bool, BackendError> {
        let bytes = self
            .runner
            .run_bytes(
                Some(root),
                ["ls-tree", "-r", "-z", "--full-tree", tree],
                MAX_INDEX_RECORD_BYTES,
            )
            .await?;
        let mut entries = BTreeMap::new();
        for record in strict_utf8(&bytes)?
            .split('\0')
            .filter(|record| !record.is_empty())
        {
            let (metadata, path) = record
                .split_once('\t')
                .ok_or_else(|| unsupported("Invalid Git tree record."))?;
            let oid = metadata
                .split_whitespace()
                .nth(2)
                .ok_or_else(|| unsupported("Invalid Git tree object."))?;
            entries.insert(path, oid);
        }
        let mut matched = false;
        for (path, stages) in unmerged {
            if let Some(stage) = stages.iter().find(|stage| stage.number == 3) {
                if entries.get(path.as_str()).copied() != Some(stage.oid.as_str()) {
                    return Ok(false);
                }
                matched = true;
            }
        }
        Ok(matched)
    }

    async fn stage_version(
        &self,
        root: &Path,
        stage: Option<&Stage>,
        max_bytes: usize,
    ) -> Result<ConflictVersion, BackendError> {
        let Some(stage) = stage else {
            return Ok(content::missing());
        };
        let mode = Some(stage.mode.clone());
        let oid = Some(stage.oid.clone());
        if !regular_mode(&stage.mode) {
            return Ok(content::metadata_version(
                mode,
                oid,
                0,
                ConflictVersionKind::Unsupported,
            ));
        }
        let size = self
            .runner
            .run_bytes(
                Some(root),
                ["cat-file", "-s", &stage.oid],
                MAX_GIT_METADATA_BYTES,
            )
            .await?;
        let size: u64 = strict_utf8(&size)?
            .trim()
            .parse()
            .map_err(|_| unsupported("Invalid Git blob size."))?;
        if size > max_bytes as u64 {
            return Ok(content::metadata_version(
                mode,
                oid,
                size,
                ConflictVersionKind::TooLarge,
            ));
        }
        let bytes = self
            .runner
            .run_bytes(Some(root), ["cat-file", "blob", &stage.oid], max_bytes)
            .await?;
        Ok(content::version(&bytes, mode, oid))
    }

    async fn git_path(&self, root: &Path, name: &str) -> Result<PathBuf, BackendError> {
        let bytes = self
            .runner
            .run_bytes(
                Some(root),
                ["rev-parse", "--git-path", name],
                MAX_GIT_METADATA_BYTES,
            )
            .await?;
        let path = PathBuf::from(strict_utf8(&bytes)?.trim_end_matches(['\r', '\n']));
        Ok(if path.is_absolute() {
            path
        } else {
            root.join(path)
        })
    }

    async fn operation_token(&self, root: &Path) -> Result<String, BackendError> {
        let mut hash = Sha256::new();
        add_fingerprint(&mut hash, b"hq-git-conflict-v1");
        add_fingerprint(
            &mut hash,
            root.to_str()
                .ok_or_else(|| unsupported("The repository path is not UTF-8."))?
                .as_bytes(),
        );
        let head = self
            .runner
            .run_allowing_failure(Some(root), ["rev-parse", "--verify", "--quiet", "HEAD"])
            .await?;
        if head.is_success() {
            let oid = head.stdout.trim();
            if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(unsupported("Invalid HEAD object ID."));
            }
            add_fingerprint(&mut hash, oid.as_bytes());
        } else if head.status_code == Some(1) {
            add_fingerprint(&mut hash, b"unborn");
        } else {
            return Err(head.into_result().expect_err("failed HEAD probe"));
        }
        // Raw index catches flags and extensions; full stage records also cover split-index entries.
        let index_path = self.git_path(root, "index").await?;
        let private_directory = index_path
            .parent()
            .ok_or_else(|| unsupported("Missing Git private directory."))?;
        fingerprint_tree(&mut hash, &index_path)?;
        let index = self
            .runner
            .run_bytes(
                Some(root),
                ["ls-files", "--stage", "-z"],
                MAX_INDEX_RECORD_BYTES,
            )
            .await?;
        strict_utf8(&index)?;
        add_fingerprint(&mut hash, &index);
        for name in OPERATION_FILES {
            add_fingerprint(&mut hash, name.as_bytes());
            let path = private_directory.join(name);
            fingerprint_tree(&mut hash, &path)?;
            // Identical commits can be merged again after Abort. Metadata times distinguish the new operation.
            if let Ok(metadata) = fs::symlink_metadata(&path) {
                for timestamp in [metadata.created(), metadata.modified()] {
                    if let Ok(timestamp) = timestamp
                        && let Ok(duration) = timestamp.duration_since(std::time::UNIX_EPOCH)
                    {
                        add_fingerprint(&mut hash, &duration.as_nanos().to_le_bytes());
                    }
                }
            }
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    async fn detail_token(&self, root: &Path, path: &str) -> Result<String, BackendError> {
        validate_relative(path)?;
        let mut hash = Sha256::new();
        add_fingerprint(&mut hash, self.operation_token(root).await?.as_bytes());
        add_fingerprint(&mut hash, path.as_bytes());
        match validate_target(root, path) {
            Ok(target) => fingerprint_tree(&mut hash, &target)?,
            Err(error) => add_fingerprint(&mut hash, error.message.as_bytes()),
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    async fn backup(
        &self,
        root: &Path,
        target: &Path,
        relative: &str,
        token: &str,
    ) -> Result<Option<String>, BackendError> {
        if !target.try_exists()? {
            return Ok(None);
        }
        let directory = self.git_path(root, RECOVERY_DIRECTORY).await?;
        if !directory.exists() {
            fs::create_dir(&directory)?;
        }
        let metadata = fs::symlink_metadata(&directory)?;
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(unsupported(
                "Recovery storage is not a regular private directory.",
            ));
        }
        let backup = directory.join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&backup)?;
        let original = backup.join("original");
        validate_target(root, relative)?;
        fs::copy(target, &original)?;
        OpenOptions::new().write(true).open(&original)?.sync_all()?;
        let metadata = fs::symlink_metadata(target)?;
        let recovery = serde_json::json!({ "path": relative, "sha256": hash_file(&original)?, "mode": content::file_mode(&metadata), "token": token });
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(backup.join("metadata.json"))?;
        file.write_all(
            serde_json::to_string_pretty(&recovery)
                .map_err(|_| unsupported("Unable to serialize recovery metadata."))?
                .as_bytes(),
        )?;
        file.sync_all()?;
        Ok(Some(backup.to_string_lossy().into_owned()))
    }

    async fn mutation_result(
        &self,
        root: &Path,
        path: Option<&str>,
        error: Option<BackendError>,
        recovery_path: Option<String>,
        include_refs: bool,
    ) -> Result<ConflictMutationResult, BackendError> {
        let refresh_error =
            |refresh| preserve_mutation_context(refresh, error.as_ref(), recovery_path.as_deref());
        let workspace = refresh_mutation_workspace(root, &self.runner)
            .await
            .map_err(refresh_error)?;
        let refs = if include_refs {
            Some(
                RefsService::new(self.runner.clone(), self.coordinator.clone())
                    .snapshot_at_root(root)
                    .await
                    .map_err(refresh_error)?,
            )
        } else {
            None
        };
        let conflicts = self.snapshot_at_root(root).await.map_err(refresh_error)?;
        let resolved = error.is_none()
            && path.is_some_and(|path| !conflicts.files.iter().any(|file| file.path == path));
        Ok(ConflictMutationResult {
            workspace: workspace.workspace,
            operation_state: conflicts.operation_state.clone(),
            conflicts,
            refs,
            error,
            recovery_path,
            resolved,
        })
    }
}

fn strict_utf8(bytes: &[u8]) -> Result<&str, BackendError> {
    std::str::from_utf8(bytes).map_err(|_| {
        BackendError::new(
            ErrorCode::UnsupportedEncoding,
            "Git returned a non-UTF-8 path; resolve it externally.",
        )
    })
}

fn preserve_mutation_context(
    mut refresh: BackendError,
    mutation: Option<&BackendError>,
    recovery: Option<&str>,
) -> BackendError {
    let mut diagnostics = Vec::new();
    if let Some(path) = recovery {
        diagnostics.push(format!("Recovery copy: {path}"));
    }
    if let Some(error) = mutation {
        diagnostics.push(format!(
            "Original mutation error ({:?}): {}\n{}",
            error.code,
            error.message,
            error.diagnostics.as_deref().unwrap_or("")
        ));
    }
    diagnostics.push(format!(
        "State refresh error ({:?}): {}\n{}",
        refresh.code,
        refresh.message,
        refresh.diagnostics.as_deref().unwrap_or("")
    ));
    refresh.message = if mutation.is_some() {
        "The Git mutation failed and state refresh failed. Saved working content may remain; inspect the file and index before retrying.".into()
    } else {
        "The change completed, but state refresh failed. Inspect the file and index before retrying.".into()
    };
    if let Some(path) = recovery {
        refresh.message.push_str(&format!(" Recovery copy: {path}"));
    }
    refresh.with_diagnostics(diagnostics.join("\n"))
}

fn parse_unmerged(bytes: &[u8]) -> Result<Unmerged, BackendError> {
    let mut files: Unmerged = BTreeMap::new();
    for record in strict_utf8(bytes)?
        .split('\0')
        .filter(|record| !record.is_empty())
    {
        let (header, path) = record
            .split_once('\t')
            .ok_or_else(|| unsupported("Invalid unmerged index record."))?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || !fields[0].bytes().all(|byte| byte.is_ascii_digit())
            || !matches!(fields[1].len(), 40 | 64)
            || !fields[1].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(unsupported("Invalid unmerged index metadata."));
        }
        let number: usize = fields[2]
            .parse()
            .map_err(|_| unsupported("Invalid index stage."))?;
        if !(1..=3).contains(&number) {
            return Err(unsupported("Invalid index stage."));
        }
        let stages = files.entry(path.to_owned()).or_default();
        if stages.iter().any(|stage| stage.number == number) {
            return Err(unsupported("Duplicate index stage."));
        }
        stages.push(Stage {
            mode: fields[0].into(),
            oid: fields[1].into(),
            number,
        });
    }
    Ok(files)
}

fn regular_mode(mode: &str) -> bool {
    matches!(mode, "100644" | "100755")
}

fn conflict_status(stages: &[Stage]) -> &'static str {
    let mask = stages
        .iter()
        .fold(0, |mask, stage| mask | 1 << (stage.number - 1));
    match mask {
        1 => "DD",
        2 => "AU",
        3 => "UD",
        4 => "UA",
        5 => "DU",
        6 => "AA",
        _ => "UU",
    }
}

fn unsupported_path(
    root: &Path,
    path: &str,
    stages: &[Stage],
    ambiguous: &BTreeSet<String>,
) -> Option<String> {
    if let Err(error) = validate_target(root, path) {
        return Some(error.message);
    }
    if stages.iter().any(|stage| !regular_mode(&stage.mode)) {
        return Some("Symlinks and submodules require external resolution.".into());
    }
    if ambiguous.contains(path) || matches!(conflict_status(stages), "AU" | "UA" | "DD") {
        return Some(
            "Ambiguous rename or directory/file topology requires external resolution.".into(),
        );
    }
    None
}

fn fingerprint_tree(hash: &mut Sha256, path: &Path) -> Result<(), BackendError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            add_fingerprint(hash, b"missing");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if is_link(&metadata) {
        return Err(unsupported(
            "Symbolic links or reparse points cannot be fingerprinted safely.",
        ));
    }
    if metadata.is_file() {
        add_fingerprint(hash, b"file");
        add_fingerprint(hash, content::file_mode(&metadata).as_bytes());
        add_fingerprint(hash, &[u8::from(metadata.permissions().readonly())]);
        add_fingerprint(hash, hash_file(path)?.as_bytes());
    } else if metadata.is_dir() {
        add_fingerprint(hash, b"directory");
        let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            add_fingerprint(
                hash,
                name.to_str()
                    .ok_or_else(|| unsupported("Non-UTF-8 operation metadata path."))?
                    .as_bytes(),
            );
            fingerprint_tree(hash, &entry.path())?;
        }
    } else {
        return Err(unsupported("Special file types are unsupported."));
    }
    Ok(())
}

fn stale() -> BackendError {
    BackendError::new(
        ErrorCode::StaleConflict,
        "The conflict, working file, index, HEAD, or operation changed. Refresh before saving or continuing.",
    )
}
fn ai_too_large() -> BackendError {
    BackendError::new(
        ErrorCode::AiContextTooLarge,
        "AI 冲突建议要求每个源版本不超过 2 MiB；大文件将按差异片段处理。",
    )
}
fn unsupported(message: impl Into<String>) -> BackendError {
    BackendError::new(ErrorCode::UnsupportedConflict, message)
}
