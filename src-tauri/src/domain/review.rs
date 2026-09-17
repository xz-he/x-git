use crate::domain::error::BackendError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReviewSource {
    #[default]
    Staged,
    StagedFiles {
        paths: Vec<String>,
    },
    Commit {
        revision: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSkillInfo {
    pub directory: String,
    pub name: String,
    pub version: Option<String>,
    pub fingerprint: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSkillStatus {
    pub state: String,
    pub info: Option<ReviewSkillInfo>,
    pub error: Option<BackendError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEvidenceSource {
    pub path: String,
    pub revision: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewContext {
    pub source: ReviewSource,
    pub resolved_commit: Option<String>,
    pub base_commit: Option<String>,
    pub changed_file_count: usize,
    pub skill: ReviewSkillInfo,
    pub excluded_files: Vec<String>,
    pub evidence_sources: Vec<ReviewEvidenceSource>,
}
