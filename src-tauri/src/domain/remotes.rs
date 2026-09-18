use serde::{Deserialize, Serialize};

use crate::domain::changes::WorkingTreeSnapshot;
use crate::domain::error::BackendError;
use crate::domain::operation::RepositoryOperationState;
use crate::domain::refs::RefsSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranchSummary {
    pub name: String,
    pub full_name: String,
    pub object_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking_local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDetail {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
    pub branches: Vec<RemoteBranchSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSnapshot {
    pub remotes: Vec<RemoteDetail>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FetchRequest {
    pub remote: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub remote: String,
    pub remote_branch: String,
    pub local_branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForceWithLease {
    pub expected_remote_oid: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub remote: String,
    pub local_branch: String,
    pub remote_branch: String,
    pub establish_upstream: bool,
    pub force_with_lease: Option<ForceWithLease>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GitRunOperation {
    Fetch,
    Pull,
    Push,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GitRunProgressPhase {
    Enumerating,
    Counting,
    Compressing,
    Receiving,
    Resolving,
    Writing,
    Updating,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteOperationResult {
    pub workspace: WorkingTreeSnapshot,
    pub refs: RefsSnapshot,
    pub remotes: RemoteSnapshot,
    pub operation_state: RepositoryOperationState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitRunAccepted {
    pub run_id: String,
    pub operation: GitRunOperation,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitRunEvent {
    pub run_id: String,
    pub sequence: u64,
    pub event: GitRunEventKind,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GitRunEventKind {
    Started {
        operation: GitRunOperation,
    },
    Progress {
        phase: GitRunProgressPhase,
        text: String,
    },
    Completed {
        result: RemoteOperationResult,
    },
    Conflicted {
        result: RemoteOperationResult,
    },
    Cancelled {
        result: RemoteOperationResult,
    },
    Failed {
        error: BackendError,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<RemoteOperationResult>,
    },
}

impl GitRunEventKind {
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. }
                | Self::Conflicted { .. }
                | Self::Cancelled { .. }
                | Self::Failed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_event_serializes_with_stable_nested_kind() {
        let event = GitRunEvent {
            run_id: "run-1".to_owned(),
            sequence: 2,
            event: GitRunEventKind::Progress {
                phase: GitRunProgressPhase::Receiving,
                text: "Receiving objects: 50%".to_owned(),
            },
        };

        let value = serde_json::to_value(event).unwrap();

        assert_eq!(value["runId"], "run-1");
        assert_eq!(value["sequence"], 2);
        assert_eq!(value["event"]["kind"], "progress");
        assert_eq!(value["event"]["phase"], "receiving");
        assert_eq!(value["event"]["text"], "Receiving objects: 50%");
    }
}
