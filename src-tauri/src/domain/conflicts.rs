use serde::{Deserialize, Serialize};

use super::changes::WorkingTreeSnapshot;
use super::error::BackendError;
use super::operation::{AbortAction, RepositoryOperationKind, RepositoryOperationState};
use super::refs::RefsSnapshot;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictVersionKind {
    Missing,
    Text,
    Binary,
    UnsupportedEncoding,
    TooLarge,
    MixedLineEndings,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictLineEnding {
    Lf,
    Crlf,
    None,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictVersion {
    pub exists: bool,
    pub oid: Option<String>,
    pub mode: Option<String>,
    pub kind: ConflictVersionKind,
    pub text: Option<String>,
    pub byte_length: u64,
    pub bom: bool,
    pub line_ending: ConflictLineEnding,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    pub status: String,
    pub supported: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSnapshot {
    pub operation_state: RepositoryOperationState,
    pub operation_token: String,
    pub files: Vec<ConflictFile>,
    pub continue_action: Option<AbortAction>,
    pub staged_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDetail {
    pub path: String,
    pub token: String,
    pub operation_kind: RepositoryOperationKind,
    pub base: ConflictVersion,
    pub ours: ConflictVersion,
    pub theirs: ConflictVersion,
    pub working: ConflictVersion,
    pub editable: bool,
    pub can_choose_ours: bool,
    pub can_choose_theirs: bool,
    pub can_delete: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConflictResolution {
    Text {
        text: String,
        #[serde(rename = "acknowledgeMarkers")]
        acknowledge_markers: bool,
    },
    Ours,
    Theirs,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConflictRequest {
    pub relative_path: String,
    pub token: String,
    pub resolution: ConflictResolution,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictMutationResult {
    pub workspace: WorkingTreeSnapshot,
    pub operation_state: RepositoryOperationState,
    pub conflicts: ConflictSnapshot,
    pub refs: Option<RefsSnapshot>,
    pub error: Option<BackendError>,
    pub recovery_path: Option<String>,
    pub resolved: bool,
}
