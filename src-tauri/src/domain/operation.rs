use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::domain::changes::WorkingTreeSnapshot;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RepositoryOperationKind {
    None,
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AbortAction {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFileSummary {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryOperationState {
    pub kind: RepositoryOperationKind,
    pub conflicts: Vec<ConflictFileSummary>,
    pub abort_action: Option<AbortAction>,
}

impl Default for RepositoryOperationState {
    fn default() -> Self {
        Self {
            kind: RepositoryOperationKind::None,
            conflicts: Vec::new(),
            abort_action: None,
        }
    }
}

impl RepositoryOperationState {
    pub fn revert(conflicts: Vec<ConflictFileSummary>) -> Self {
        Self { kind: RepositoryOperationKind::Revert, conflicts, abort_action: Some(AbortAction::Revert) }
    }
    pub fn merge(conflicts: Vec<ConflictFileSummary>) -> Self {
        Self {
            kind: RepositoryOperationKind::Merge,
            conflicts,
            abort_action: Some(AbortAction::Merge),
        }
    }

    pub fn rebase(conflicts: Vec<ConflictFileSummary>) -> Self {
        Self {
            kind: RepositoryOperationKind::Rebase,
            conflicts,
            abort_action: Some(AbortAction::Rebase),
        }
    }

    pub fn cherry_pick(conflicts: Vec<ConflictFileSummary>) -> Self {
        Self {
            kind: RepositoryOperationKind::CherryPick,
            conflicts,
            abort_action: Some(AbortAction::CherryPick),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MutationWorkspace {
    pub workspace: WorkingTreeSnapshot,
    pub operation_state: RepositoryOperationState,
}

impl Deref for MutationWorkspace {
    type Target = WorkingTreeSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.workspace
    }
}
