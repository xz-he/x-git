use crate::domain::error::BackendError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryKind {
    Status,
    Log,
    Diff,
    Show,
    Branch,
    Tag,
    Remote,
    Stash,
}
#[derive(Debug, Clone)]
pub struct ConsoleQuery {
    pub(crate) kind: QueryKind,
    pub(crate) options: Vec<String>,
    pub(crate) revision: Option<String>,
    pub(crate) paths: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleAccepted {
    pub run_id: String,
    pub root_path: PathBuf,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConsoleStream {
    Stdout,
    Stderr,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConsoleOutcome {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleEventKind {
    Started,
    Output {
        stream: ConsoleStream,
        text: String,
    },
    Terminal {
        outcome: ConsoleOutcome,
        exit_code: Option<i32>,
        duration_ms: u64,
        stdout_truncated: bool,
        stderr_truncated: bool,
        error: Option<BackendError>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleEvent {
    pub run_id: String,
    pub root_path: PathBuf,
    pub sequence: u64,
    pub event: ConsoleEventKind,
}
