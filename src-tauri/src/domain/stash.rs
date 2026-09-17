use crate::domain::changes::WorkingTreeSnapshot;
use crate::domain::error::BackendError;
use crate::domain::operation::RepositoryOperationState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashEntry {
    pub selector: String,
    pub object_id: String,
    pub branch: Option<String>,
    pub description: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashSelection {
    pub selector: String,
    pub expected_object_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashSnapshot {
    pub entries: Vec<StashEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashFileSummary {
    pub status: String,
    pub path: String,
    pub old_path: Option<String>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
    pub untracked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashDetail {
    pub entry: StashEntry,
    pub files: Vec<StashFileSummary>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashCreateRequest {
    pub message: Option<String>,
    #[serde(default)]
    pub include_untracked: bool,
    /// None means all eligible files; Some([]) must never mean all files.
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StashMutationOutcome {
    Created,
    NoChanges,
    Applied,
    Removed,
    Retained,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashMutationResult {
    pub outcome: StashMutationOutcome,
    pub workspace: WorkingTreeSnapshot,
    pub stashes: StashSnapshot,
    pub operation_state: RepositoryOperationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<BackendError>,
}
