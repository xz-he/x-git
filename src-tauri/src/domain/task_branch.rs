use crate::domain::{error::BackendError, refs::RefsMutationResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskBranchKind {
    Feature,
    Hotfix,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskBranchMode {
    Current,
    RemoteMaster,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskBranchPhase {
    Ready,
    Committing,
    PendingPick,
    Picking,
    Conflict,
    PendingReturn,
    Completed,
    NeedsAttention,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskBranchAction {
    Commit,
    Pick,
    Return,
    Reconcile,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskBranchRequest {
    pub kind: TaskBranchKind,
    pub ticket: String,
    pub slug: String,
    pub description: String,
    pub mode: TaskBranchMode,
    pub remote: Option<String>,
    pub source_branch: String,
    pub expected_head: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBranchRunRequest {
    pub id: String,
    pub action: TaskBranchAction,
    pub message: Option<String>,
    pub return_after_success: bool,
    pub expected_head: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBranchBinding {
    pub id: String,
    pub root_path: String,
    pub source_branch: String,
    pub target_branch: String,
    pub ticket: String,
    pub description: String,
    pub mode: TaskBranchMode,
    pub phase: TaskBranchPhase,
    pub source_commit: Option<String>,
    pub target_commit: Option<String>,
    pub return_after_success: bool,
    pub message: Option<String>,
    // Durable intent evidence; consumers can ignore these fields.
    #[serde(default)]
    pub create_start: Option<String>,
    #[serde(default)]
    pub creation_pending: bool,
    #[serde(default)]
    pub create_source_head: Option<String>,
    #[serde(default)]
    pub create_remote: Option<String>,
    #[serde(default)]
    pub commit_base: Option<String>,
    #[serde(default)]
    pub commit_tree: Option<String>,
    #[serde(default)]
    pub commit_message: Option<String>,
    #[serde(default)]
    pub pick_base: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBranchResult {
    pub bindings: Vec<TaskBranchBinding>,
    pub workspace: Option<RefsMutationResult>,
    pub error: Option<BackendError>,
}
