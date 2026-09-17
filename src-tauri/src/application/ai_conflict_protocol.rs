use crate::domain::ai_conflict::{
    AiConflictContext, AiConflictSuggestionKind, AiConflictSuggestionResult,
};
use crate::domain::conflicts::ConflictVersionKind;
use crate::domain::error::{BackendError, ErrorCode};
use serde::Deserialize;

pub(super) const MAX_VERSION_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Suggestion {
    kind: AiConflictSuggestionKind,
    summary: String,
    explanation: String,
    resolved_text: Option<String>,
    risks: Vec<String>,
    context_missing: Vec<String>,
}

impl Suggestion {
    pub(super) fn bind(self, context: AiConflictContext) -> AiConflictSuggestionResult {
        AiConflictSuggestionResult {
            kind: self.kind,
            summary: self.summary,
            explanation: self.explanation,
            resolved_text: self.resolved_text,
            risks: self.risks,
            context_missing: self.context_missing,
            context,
        }
    }
}

pub(super) fn parse(response: &str) -> Result<Suggestion, BackendError> {
    let invalid = || {
        BackendError::new(
            ErrorCode::AiInvalidResponse,
            "AI 冲突建议格式无效，不能应用。请重新生成。",
        )
    };
    let mut value: Suggestion = serde_json::from_str(response).map_err(|_| invalid())?;
    match (value.kind, &value.resolved_text) {
        (AiConflictSuggestionKind::Text, Some(text)) => {
            if text.len() > MAX_VERSION_BYTES {
                return Err(invalid());
            }
            let version = super::conflict_content::version(text.as_bytes(), None, None);
            if version.kind != ConflictVersionKind::Text {
                return Err(invalid());
            }
            // The existing editor/save pipeline owns BOM and source line-ending preservation.
            value.resolved_text = version.text;
        }
        (AiConflictSuggestionKind::AdviceOnly, None) => {}
        _ => return Err(invalid()),
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(text: &str) -> serde_json::Value {
        serde_json::json!({"kind":"text","summary":"合并","explanation":"理由","resolvedText":text,"risks":[],"contextMissing":[]})
    }
    #[test]
    fn ai_conflict_protocol_accepts_empty_and_html_as_literal_text() {
        for text in ["", "<script>alert('literal')</script>", "中文\n"] {
            assert!(parse(&response(text).to_string()).is_ok());
        }
    }
    #[test]
    fn ai_conflict_protocol_rejects_missing_wrong_and_extra_fields() {
        for key in [
            "kind",
            "summary",
            "explanation",
            "resolvedText",
            "risks",
            "contextMissing",
        ] {
            let mut value = response("x");
            value.as_object_mut().unwrap().remove(key);
            assert!(parse(&value.to_string()).is_err(), "{key}");
        }
        for (key, value) in [
            ("kind", serde_json::json!("delete")),
            ("risks", serde_json::json!([1])),
            ("resolvedText", serde_json::Value::Null),
            ("path", serde_json::json!("elsewhere")),
        ] {
            let mut input = response("x");
            input[key] = value;
            assert!(parse(&input.to_string()).is_err());
        }
        for input in ["{broken}", "```json\n{}\n```", "{} trailing"] {
            assert!(parse(input).is_err());
        }
    }
    #[test]
    fn ai_conflict_protocol_advice_only_has_no_candidate() {
        let mut value = response("x");
        value["kind"] = serde_json::json!("adviceOnly");
        assert!(parse(&value.to_string()).is_err());
        value["resolvedText"] = serde_json::Value::Null;
        assert!(parse(&value.to_string()).is_ok());
        value.as_object_mut().unwrap().remove("resolvedText");
        assert!(parse(&value.to_string()).is_ok());
    }
    #[test]
    fn ai_conflict_protocol_candidate_must_fit_and_be_supported_text() {
        assert!(parse(&response(&"a".repeat(64 * 1024)).to_string()).is_ok());
        for text in ["a".repeat(64 * 1024 + 1), "a\0b".into(), "a\r\nb\n".into()] {
            assert!(parse(&response(&text).to_string()).is_err());
        }
    }
    #[test]
    fn ai_conflict_protocol_normalizes_editor_text_without_duplicate_bom() {
        let parsed = parse(&response("\u{feff}中文\r\n").to_string()).unwrap();
        assert_eq!(parsed.resolved_text.as_deref(), Some("中文\n"));
    }
}
