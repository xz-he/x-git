use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub root_path: PathBuf,
    pub name: String,
    pub current_branch: Option<String>,
    pub head_short_hash: Option<String>,
    pub is_clean: bool,
    pub changed_file_count: usize,
    pub conflict_count: usize,
    pub remotes: Vec<RemoteSummary>,
    pub upstream: Option<UpstreamSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSummary {
    pub name: String,
    pub fetch_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamSummary {
    pub name: String,
    pub ahead: u32,
    pub behind: u32,
}
