use super::operation::RepositoryOperationKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiConflictContext {
    pub path: String,
    pub token: String,
    pub operation_kind: RepositoryOperationKind,
    pub base_oid: Option<String>,
    pub ours_oid: Option<String>,
    pub theirs_oid: Option<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AiConflictSuggestionKind {
    Text,
    AdviceOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiConflictSuggestionResult {
    pub kind: AiConflictSuggestionKind,
    pub summary: String,
    pub explanation: String,
    pub resolved_text: Option<String>,
    pub risks: Vec<String>,
    pub context_missing: Vec<String>,
    pub context: AiConflictContext,
}
