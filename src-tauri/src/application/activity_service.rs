use super::{
    changes_service::repository_root,
    mutation_coordinator::RepositoryMutationCoordinator,
    operation_state::{
        MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
    },
};
use crate::{
    domain::{
        error::{BackendError, ErrorCode},
        operation::MutationWorkspace,
    },
    infrastructure::git_runner::GitCommandRunner,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

const JOURNAL_DIR: &str = "hq-git-operations";
const MAX_PATCH_BYTES: usize = 8 * 1024 * 1024;
const LIST_LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySummary {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub status: String,
    pub message: String,
    pub rollback_kind: Option<String>,
    pub rollback_reason: String,
    pub rollback_id: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    head: Option<String>,
    branch: Option<String>,
    index: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    summary: ActivitySummary,
    before: Snapshot,
    after: Snapshot,
    #[serde(skip)]
    patch: Option<Vec<u8>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityReply {
    pub value: Option<Value>,
    pub error: Option<BackendError>,
    pub warning: Option<String>,
}

#[derive(Clone)]
pub struct ActivityService {
    runner: GitCommandRunner,
    coordinator: RepositoryMutationCoordinator,
    gate: Arc<Mutex<()>>,
}
impl ActivityService {
    pub async fn record_external(
        &self,
        requested: &Path,
        title: &str,
        success: bool,
        message: &str,
    ) -> Result<(), BackendError> {
        let _gate = self.gate.lock().await;
        let root = repository_root(&self.runner, requested).await?;
        let mut entry = Entry {
            summary: summary(title),
            before: Snapshot::default(),
            after: Snapshot::default(),
            patch: None,
        };
        entry.summary.status = if success { "success" } else { "failed" }.into();
        entry.summary.message = crate::domain::error::sanitize_git_output(message);
        entry.summary.rollback_reason =
            "终端及远程操作不提供自动回滚，请根据实际 Git 状态使用对应功能处理。".into();
        save(&self.directory(&root).await?, &entry)
    }
    pub fn new(runner: GitCommandRunner, coordinator: RepositoryMutationCoordinator) -> Self {
        Self {
            runner,
            coordinator,
            gate: Arc::new(Mutex::new(())),
        }
    }
    async fn directory(&self, root: &Path) -> Result<PathBuf, BackendError> {
        let output = self
            .runner
            .run(Some(root), ["rev-parse", "--absolute-git-dir"])
            .await?;
        Ok(PathBuf::from(output.stdout.trim()).join(JOURNAL_DIR))
    }
    async fn snapshot(&self, root: &Path, index: bool) -> Result<Snapshot, BackendError> {
        let head = self
            .runner
            .run_allowing_failure(Some(root), ["rev-parse", "--verify", "HEAD"])
            .await?;
        let branch = self
            .runner
            .run_allowing_failure(Some(root), ["symbolic-ref", "-q", "HEAD"])
            .await?;
        let tree = if index {
            self.runner
                .run_allowing_failure(Some(root), ["write-tree"])
                .await
                .ok()
                .filter(|o| o.is_success())
                .map(|o| o.stdout.trim().to_owned())
        } else {
            None
        };
        Ok(Snapshot {
            head: head.is_success().then(|| head.stdout.trim().to_owned()),
            branch: branch.is_success().then(|| branch.stdout.trim().to_owned()),
            index: tree,
        })
    }
    pub async fn list(&self, requested: &Path) -> Result<Vec<ActivitySummary>, BackendError> {
        let root = repository_root(&self.runner, requested).await?;
        let dir = self.directory(&root).await?;
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for file in std::fs::read_dir(dir)? {
            let path = file?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let mut item: Entry =
                    serde_json::from_slice(&std::fs::read(path)?).map_err(json_error)?;
                if item.summary.rollback_id.is_some() {
                    item.summary.rollback_kind = None;
                    item.summary.rollback_reason =
                        "已执行或已发起回滚，请查看对应回滚记录。".into();
                }
                records.push(item.summary);
            }
        }
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        records.truncate(LIST_LIMIT);
        Ok(records)
    }
    // All app mutations routed through this boundary are serialized; the existing
    // services retain their repository write guards and operation validation.
    pub async fn execute<F>(
        &self,
        requested: &Path,
        action: &str,
        title: &str,
        work: F,
    ) -> Result<ActivityReply, BackendError>
    where
        F: Future<Output = Result<Value, BackendError>>,
    {
        let _gate = self.gate.lock().await;
        let root = repository_root(&self.runner, requested).await?;
        let dir = self.directory(&root).await?;
        let index = is_index_action(action);
        let before = {
            let _lock = self.coordinator.write(&root).await;
            self.snapshot(&root, index).await?
        };
        let mut entry = Entry {
            summary: summary(title),
            before,
            after: Snapshot::default(),
            patch: None,
        };
        save(&dir, &entry)?;
        let result = work.await;
        let embedded_error = result
            .as_ref()
            .ok()
            .and_then(|value| value.get("error"))
            .filter(|error| !error.is_null());
        let failure = result
            .as_ref()
            .err()
            .map(|error| error.message.clone())
            .or_else(|| {
                embedded_error.and_then(|error| {
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
            });
        entry.summary.status = if failure.is_some() {
            "failed"
        } else {
            "success"
        }
        .into();
        let has_pending_operation = result
            .as_ref()
            .ok()
            .and_then(|value| value.get("operationState"))
            .is_some_and(|state| {
                state
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "none")
            });
        entry.summary.message = failure.unwrap_or_else(|| {
            if has_pending_operation {
                "本次操作已执行，Git 流程尚未结束，请在冲突工作台继续或中止。".into()
            } else {
                "操作完成".into()
            }
        });
        let _lock = self.coordinator.write(&root).await;
        if let Ok(after) = self.snapshot(&root, index).await {
            entry.after = after;
            if entry.summary.status == "success" && !has_pending_operation {
                self.prepare_rollback(&root, action, &mut entry).await;
            }
        }
        let warning = save(&dir, &entry).err().map(|_| {
            "Git 操作已结束，但操作历史写入失败，请刷新仓库确认结果，不要重复执行。".into()
        });
        Ok(match result {
            Ok(value) => ActivityReply {
                value: Some(value),
                error: None,
                warning,
            },
            Err(error) => ActivityReply {
                value: None,
                error: Some(error),
                warning,
            },
        })
    }
    async fn prepare_rollback(&self, root: &Path, action: &str, entry: &mut Entry) {
        entry.summary.rollback_reason = "此操作暂不支持自动回滚，请使用对应 Git 功能处理。".into();
        if is_index_action(action)
            && entry.before.head == entry.after.head
            && entry.before.branch == entry.after.branch
        {
            if let (Some(before), Some(after)) = (&entry.before.index, &entry.after.index) {
                if before == after {
                    entry.summary.rollback_reason = "暂存区没有发生变化，无需回滚。".into();
                    return;
                }
                if let Ok(patch) = self
                    .runner
                    .run_bytes(
                        Some(root),
                        [
                            "diff",
                            "--binary",
                            "--full-index",
                            "--no-color",
                            "--src-prefix=a/",
                            "--dst-prefix=b/",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--no-renames",
                            before,
                            after,
                            "--",
                        ],
                        MAX_PATCH_BYTES,
                    )
                    .await
                {
                    if !patch.is_empty() {
                        entry.patch = Some(patch);
                        entry.summary.rollback_kind = Some("index".into());
                        entry.summary.rollback_reason = "恢复此次操作前的暂存区；保留工作区文件内容。若暂存区或 HEAD 已发生后续变化，将拒绝回滚。".into();
                    }
                }
            }
        } else if matches!(
            action,
            "changes_commit" | "history_revert" | "history_cherry_pick" | "refs_merge"
        ) && entry.before.branch == entry.after.branch
            && entry.before.head != entry.after.head
        {
            if let Some(after) = &entry.after.head {
                if let Ok(parents) = self
                    .runner
                    .run(Some(root), ["rev-list", "--parents", "-n", "1", after])
                    .await
                {
                    let items: Vec<_> = parents.stdout.split_whitespace().collect();
                    if items.len() == 2 && Some(items[1]) == entry.before.head.as_deref() {
                        entry.summary.rollback_kind = Some("revert".into());
                        entry.summary.rollback_reason = "为此次生成的提交创建 Revert 提交，保留原提交历史。需工作区干净，且仍位于操作后的分支和提交。".into();
                    }
                }
            }
        } else if matches!(action, "refs_switch_branch" | "history_checkout")
            && entry.before.head.is_some()
            && entry.before != entry.after
        {
            entry.summary.rollback_kind = Some("switch".into());
            entry.summary.rollback_reason =
                "切回操作前的分支或提交。需工作区干净，且前后分支没有新增提交。".into();
        }
    }
    pub async fn rollback(
        &self,
        requested: &Path,
        id: &str,
    ) -> Result<MutationWorkspace, BackendError> {
        if Uuid::parse_str(id).is_err() {
            return Err(invalid("无效的操作记录。"));
        }
        let _gate = self.gate.lock().await;
        let root = repository_root(&self.runner, requested).await?;
        let _lock = self.coordinator.write(&root).await;
        let dir = self.directory(&root).await?;
        let mut entry: Entry =
            serde_json::from_slice(&std::fs::read(dir.join(format!("{id}.json")))?)
                .map_err(json_error)?;
        if entry.summary.status != "success" || entry.summary.rollback_id.is_some() {
            return Err(invalid("该操作不能重复回滚。"));
        }
        let kind = entry
            .summary
            .rollback_kind
            .clone()
            .ok_or_else(|| invalid(&entry.summary.rollback_reason))?;
        ensure_mutation_allowed(
            &read_operation_state(&root, &self.runner).await?,
            MutationIntent::Changes,
        )?;
        let current = self.snapshot(&root, kind == "index").await?;
        if current != entry.after {
            return Err(invalid(
                "仓库已有后续变化，无法直接回滚这条记录。请先处理较新的操作并刷新。",
            ));
        }
        if kind != "index" {
            let status = self
                .runner
                .run(
                    Some(&root),
                    ["status", "--porcelain=v1", "--untracked-files=all"],
                )
                .await?;
            if !status.stdout.is_empty() {
                return Err(BackendError::new(
                    ErrorCode::DirtyWorktree,
                    "请先提交或贮藏当前修改，再回滚该操作。",
                ));
            }
        }
        if kind == "switch" {
            if let Some(branch) = &entry.before.branch {
                let target = self
                    .runner
                    .run(Some(&root), ["rev-parse", "--verify", branch])
                    .await?;
                if Some(target.stdout.trim()) != entry.before.head.as_deref() {
                    return Err(invalid("原分支已有变化，不能自动切回。"));
                }
            }
        }
        let patch = if kind == "index" {
            std::fs::read(dir.join(format!("{id}.patch")))?
        } else {
            Vec::new()
        };
        let mut rollback = Entry {
            summary: summary(&format!("回滚：{}", entry.summary.title)),
            before: current,
            after: Snapshot::default(),
            patch: None,
        };
        save(&dir, &rollback)?;
        entry.summary.rollback_id = Some(rollback.summary.id.clone());
        save(&dir, &entry)?;
        let result = match kind.as_str() {
            "index" => {
                self.runner
                    .run_with_input(
                        Some(&root),
                        [
                            "apply",
                            "--cached",
                            "--reverse",
                            "--binary",
                            "--whitespace=nowarn",
                            "-",
                        ],
                        patch,
                    )
                    .await
            }
            "revert" => self
                .runner
                .run_without_editor(
                    Some(&root),
                    [
                        "revert",
                        "--no-edit",
                        entry
                            .after
                            .head
                            .as_deref()
                            .ok_or_else(|| invalid("提交记录缺失。"))?,
                    ],
                )
                .await
                .and_then(|out| out.into_result()),
            "switch" => {
                if let Some(branch) = &entry.before.branch {
                    self.runner
                        .run(
                            Some(&root),
                            [
                                "switch",
                                "--",
                                branch
                                    .strip_prefix("refs/heads/")
                                    .ok_or_else(|| invalid("原分支无效。"))?,
                            ],
                        )
                        .await
                } else {
                    self.runner
                        .run(
                            Some(&root),
                            [
                                "switch",
                                "--detach",
                                entry
                                    .before
                                    .head
                                    .as_deref()
                                    .ok_or_else(|| invalid("原提交缺失。"))?,
                                "--",
                            ],
                        )
                        .await
                }
            }
            _ => return Err(invalid("此操作不支持自动回滚。")),
        };
        rollback.summary.status = if result.is_ok() { "success" } else { "failed" }.into();
        rollback.summary.message = result
            .as_ref()
            .err()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "回滚完成".into());
        rollback.summary.rollback_reason = "回滚操作本身不提供再次自动回滚。".into();
        // Persist completion before refreshing UI data; a refresh failure must not
        // make a completed Git mutation available for a second execution.
        save(&dir, &rollback)?;
        result?;
        refresh_mutation_workspace(&root, &self.runner).await
    }
}
fn is_index_action(action: &str) -> bool {
    matches!(
        action,
        "changes_stage_file"
            | "changes_stage_files"
            | "changes_unstage_file"
            | "changes_unstage_files"
            | "changes_stage_hunk"
            | "changes_unstage_hunk"
            | "changes_stage_lines"
            | "changes_unstage_lines"
    )
}
fn summary(title: &str) -> ActivitySummary {
    ActivitySummary {
        id: Uuid::new_v4().to_string(),
        title: title.into(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        status: "pending".into(),
        message: "操作已开始；如应用中断，请刷新仓库确认实际结果。".into(),
        rollback_kind: None,
        rollback_reason: "未确认成功的操作不能自动回滚。".into(),
        rollback_id: None,
    }
}
fn save(dir: &Path, entry: &Entry) -> Result<(), BackendError> {
    std::fs::create_dir_all(dir)?;
    if let Some(patch) = &entry.patch {
        let mut temp = tempfile::NamedTempFile::new_in(dir)?;
        temp.write_all(patch)?;
        temp.as_file().sync_all()?;
        temp.persist(dir.join(format!("{}.patch", entry.summary.id)))
            .map_err(|error| BackendError::from(error.error))?;
    }
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(&serde_json::to_vec(entry).map_err(json_error)?)?;
    temp.as_file().sync_all()?;
    temp.persist(dir.join(format!("{}.json", entry.summary.id)))
        .map_err(|error| BackendError::from(error.error))?;
    Ok(())
}
fn json_error(_: serde_json::Error) -> BackendError {
    BackendError::new(ErrorCode::Io, "操作历史文件读写失败。")
}
fn invalid(message: &str) -> BackendError {
    BackendError::new(ErrorCode::StaleFileOperation, message)
}
