use crate::domain::error::BackendError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAccepted {
    pub run_id: String,
    pub root_path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TerminalEventKind {
    Started,
    Output {
        data: String,
    },
    Exited {
        exit_code: Option<i32>,
        duration_ms: u64,
        cancelled: bool,
        error: Option<BackendError>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEvent {
    pub run_id: String,
    pub root_path: PathBuf,
    pub sequence: u64,
    pub event: TerminalEventKind,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCompletion {
    pub start: usize,
    pub end: usize,
    pub items: Vec<TerminalCompletionItem>,
    pub has_more: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCompletionItem {
    pub value: String,
    pub label: String,
    pub description: String,
    pub kind: String,
}
