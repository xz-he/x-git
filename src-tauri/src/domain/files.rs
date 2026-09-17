use crate::domain::{
    changes::WorkingTreeSnapshot, error::BackendError, operation::RepositoryOperationState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileEntryKind {
    File,
    Directory,
    Restricted,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFileEntry {
    pub name: String,
    pub relative_path: String,
    pub kind: FileEntryKind,
    pub byte_length: Option<u64>,
    pub reason: Option<String>,
    pub git_status: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDirectoryPage {
    pub relative_dir: String,
    pub token: String,
    pub entries: Vec<RepositoryFileEntry>,
    pub next_cursor: Option<String>,
    pub total_entries: usize,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilePreviewKind {
    Text,
    Binary,
    UnsupportedEncoding,
    TooLarge,
    Unsupported,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileLineEnding {
    Lf,
    Crlf,
    None,
    Mixed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFilePreview {
    pub relative_path: String,
    pub token: String,
    pub kind: FilePreviewKind,
    pub text: Option<String>,
    pub byte_length: u64,
    pub bom: bool,
    pub line_ending: FileLineEnding,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FileOperationIntent {
    CreateFile {
        parent_dir: String,
        name: String,
    },
    CreateDirectory {
        parent_dir: String,
        name: String,
    },
    Rename {
        relative_path: String,
        new_name: String,
    },
    Delete {
        relative_path: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedFileOperation {
    pub intent: FileOperationIntent,
    pub token: String,
    pub source_path: Option<String>,
    pub target_path: Option<String>,
    pub entry_kind: FileEntryKind,
    pub node_count: usize,
    pub file_count: usize,
    pub directory_count: usize,
    pub total_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteFileOperationRequest {
    pub intent: FileOperationIntent,
    pub token: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMutationResult {
    pub applied: bool,
    pub workspace: Option<WorkingTreeSnapshot>,
    pub operation_state: Option<RepositoryOperationState>,
    pub affected_directories: Vec<String>,
    pub selected_path: Option<String>,
    pub recovery_path: Option<String>,
    pub error: Option<BackendError>,
}
