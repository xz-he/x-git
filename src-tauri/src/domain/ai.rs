use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::domain::error::BackendError;
pub use crate::domain::review::{
    ReviewContext, ReviewEvidenceSource, ReviewSkillInfo, ReviewSkillStatus, ReviewSource,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AiProvider {
    #[default]
    OpenAi,
    Qwen,
    Gemini,
    Custom,
}

impl AiProvider {
    pub fn from_stored(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai" | "open_ai" | "open-ai" => Self::OpenAi,
            "qwen" => Self::Qwen,
            "gemini" => Self::Gemini,
            "custom" => Self::Custom,
            _ => Self::Custom,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openAi",
            Self::Qwen => "qwen",
            Self::Gemini => "gemini",
            Self::Custom => "custom",
        }
    }
}

impl Serialize for AiProvider {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AiProvider {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_stored(&value))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AiTaskKind {
    ReviewChanges,
    GenerateCommitMessage,
    ResolveConflict,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AiApiFormat {
    #[default]
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AiIssueSeverity {
    Critical,
    Warning,
    #[default]
    Suggestion,
    #[serde(rename = "P0")]
    P0,
    #[serde(rename = "P1")]
    P1,
    #[serde(rename = "P2")]
    P2,
    #[serde(rename = "P3")]
    P3,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiConnectionConfig {
    pub provider: AiProvider,
    #[serde(default)]
    pub api_format: AiApiFormat,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl fmt::Debug for AiConnectionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AiConnectionConfig")
            .field("provider", &self.provider)
            .field("api_format", &self.api_format)
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiContextSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<crate::domain::ai_conflict::AiConflictContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewContext>,
    pub staged_file_count: usize,
    pub text_file_count: usize,
    pub skipped_binary_files: Vec<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiRunAccepted {
    pub run_id: String,
    pub task: AiTaskKind,
    pub context: AiContextSummary,
    pub total_batch_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiReviewIssue {
    pub severity: AiIssueSeverity,
    pub path: String,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub reason: String,
    pub suggested_fix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
    #[serde(default)]
    pub context_missing: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_relation: Option<String>,
    #[serde(default)]
    pub evidence_sources: Vec<ReviewEvidenceSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiReviewResult {
    #[serde(default)]
    pub markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ReviewContext>,
    #[serde(default)]
    pub uncovered: Vec<String>,
    pub summary: String,
    pub issues: Vec<AiReviewIssue>,
    pub reviewed_files: Vec<String>,
    pub skipped_binary_files: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiCommitMessageResult {
    pub message: String,
    pub context_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiConnectionTestResult {
    pub provider: AiProvider,
    pub model: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiRunEvent {
    pub run_id: String,
    pub sequence: u64,
    pub event: AiRunEventData,
}

impl AiRunEvent {
    pub fn new(run_id: impl Into<String>, sequence: u64, event: AiRunEventData) -> Self {
        Self {
            run_id: run_id.into(),
            sequence,
            event,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AiRunEventData {
    ReviewProgress {
        phase: String,
        message: String,
    },
    Started {
        context: AiContextSummary,
        total_batch_count: usize,
    },
    BatchStarted {
        batch_index: usize,
        file_paths: Vec<String>,
    },
    Delta {
        text: String,
    },
    ReviewBatchCompleted {
        batch_index: usize,
        issues: Vec<AiReviewIssue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<AiReviewResult>,
    },
    ReviewCompleted {
        result: AiReviewResult,
    },
    CommitMessageCompleted {
        result: AiCommitMessageResult,
    },
    ConflictSuggestionCompleted {
        result: crate::domain::ai_conflict::AiConflictSuggestionResult,
    },
    Failed {
        error: BackendError,
    },
    Cancelled {
        completed_batch_count: usize,
        total_batch_count: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_event_serializes_with_stable_nested_kind() {
        let event = AiRunEvent::new(
            "run-1",
            3,
            AiRunEventData::Delta {
                text: "feat".to_owned(),
            },
        );

        let value = serde_json::to_value(event).unwrap();

        assert_eq!(value["runId"], "run-1");
        assert_eq!(value["sequence"], 3);
        assert_eq!(value["event"]["kind"], "delta");
        assert_eq!(value["event"]["text"], "feat");
    }

    #[test]
    fn ai_provider_accepts_legacy_and_unknown_values() {
        assert_eq!(
            serde_json::from_str::<AiProvider>(r#""openai""#).unwrap(),
            AiProvider::OpenAi
        );
        assert_eq!(
            serde_json::from_str::<AiProvider>(r#""openAi""#).unwrap(),
            AiProvider::OpenAi
        );
        assert_eq!(
            serde_json::from_str::<AiProvider>(r#""legacy-compatible""#).unwrap(),
            AiProvider::Custom
        );
        assert_eq!(
            serde_json::to_string(&AiProvider::OpenAi).unwrap(),
            r#""openAi""#
        );
    }

    #[test]
    fn context_summary_uses_skipped_binary_files_key() {
        let summary = AiContextSummary {
            conflict: None,
            review: None,
            staged_file_count: 1,
            text_file_count: 0,
            skipped_binary_files: vec!["preview.bin".to_owned()],
            fingerprint: "abc".to_owned(),
        };

        let value = serde_json::to_value(summary).unwrap();

        assert_eq!(
            value["skippedBinaryFiles"],
            serde_json::json!(["preview.bin"])
        );
    }

    #[test]
    fn connection_config_debug_redacts_api_key() {
        let config = AiConnectionConfig {
            provider: AiProvider::Gemini,
            api_format: AiApiFormat::default(),
            api_key: "secret-key".to_owned(),
            base_url: "https://example.test".to_owned(),
            model: "gemini-test".to_owned(),
        };

        let debug = format!("{config:?}");

        assert!(!debug.contains("secret-key"));
        assert!(debug.contains("[REDACTED]"));
    }
}
