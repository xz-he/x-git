use serde::{Deserialize, Serialize};

use crate::domain::changes::WorkingTreeSnapshot;
use crate::domain::operation::RepositoryOperationState;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchTip {
    pub full_hash: String,
    pub short_hash: String,
    pub subject: String,
    pub author: String,
    pub authored_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummary {
    pub name: String,
    pub full_name: String,
    pub kind: BranchKind,
    pub current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
    pub tip: BranchTip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TagSummary {
    pub name: String,
    pub object_hash: String,
    pub peeled_commit_hash: String,
    pub annotated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tagger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tagged_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
    pub commit_subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RefsSnapshot {
    pub local_branches: Vec<BranchSummary>,
    pub remote_branches: Vec<BranchSummary>,
    pub tags: Vec<TagSummary>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateBranchRequest {
    pub name: String,
    pub start_point: Option<String>,
    pub switch: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBranchRequest {
    pub name: String,
    pub force: bool,
    pub confirmation: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefsMutationResult {
    pub workspace: WorkingTreeSnapshot,
    pub refs: RefsSnapshot,
    pub operation_state: RepositoryOperationState,
}
