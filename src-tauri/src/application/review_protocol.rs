//! Reports are free-form text. Only requests that read more context use a schema.
use crate::application::review_evidence::{ContextRequest, MAX_REQUESTS};
use crate::domain::error::{BackendError, ErrorCode};

#[derive(Debug)]
pub enum ReviewEnvelope {
    Requests(Vec<ContextRequest>),
    Result(String),
}

pub fn parse(output: &str) -> Result<ReviewEnvelope, BackendError> {
    let output = output.trim();
    if output.is_empty() {
        return Err(BackendError::new(
            ErrorCode::AiInvalidResponse,
            "AI 未返回审查内容，请重试。",
        ));
    }
    let body = unwrap_fence(output);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        // Never interpret prose containing JSON as a command. Mixed reports stay reports.
        if let Some(object) = value.as_object() {
            if object.len() == 1 {
                if let Some(requests) = object.get("contextRequests") {
                    let requests: Vec<ContextRequest> =
                        serde_json::from_value(requests.clone()).map_err(|_| invalid_request())?;
                    if requests.is_empty() || requests.len() > MAX_REQUESTS {
                        return Err(invalid_request());
                    }
                    return Ok(ReviewEnvelope::Requests(requests));
                }
            }
        }
        // Older/custom models may still return JSON. Preserve every field without
        // requiring findings, severity, confidence or evidence to match a schema.
        let markdown = json_markdown(&value, 2);
        return Ok(ReviewEnvelope::Result(if markdown.trim().is_empty() {
            output.to_owned()
        } else {
            markdown
        }));
    }
    Ok(ReviewEnvelope::Result(body.to_owned()))
}

fn invalid_request() -> BackendError {
    BackendError::new(
        ErrorCode::AiInvalidResponse,
        "源码补充请求无效：仅支持 1 到 8 个文件、符号或技能引用读取请求。",
    )
}

fn unwrap_fence(output: &str) -> &str {
    let Some((opening, body)) = output.split_once('\n') else {
        return output;
    };
    if matches!(opening.trim(), "```markdown" | "```md" | "```json" | "```") {
        if let Some(body) = body.strip_suffix("```") {
            // Strip only a single wrapper, never the first/last of multiple blocks.
            if matches!(opening.trim(), "```markdown" | "```md")
                || !body
                    .lines()
                    .any(|line| line.trim_start().starts_with("```"))
            {
                return body.trim();
            }
        }
    }
    output
}

fn json_markdown(value: &serde_json::Value, level: usize) -> String {
    use serde_json::Value;
    match value {
        Value::String(text) => text.clone(),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| {
                format!(
                    "{} {}\n\n{}",
                    "#".repeat(level.min(6)),
                    key,
                    json_markdown(value, level + 1)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Array(items) if !items.is_empty() => items
            .iter()
            .map(|item| format!("- {}", json_markdown(item, level).replace('\n', "\n  ")))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(output: &str) -> String {
        let ReviewEnvelope::Result(report) = parse(output).unwrap() else {
            panic!("expected report")
        };
        report
    }
    #[test]
    fn accepts_markdown_prose_and_json_with_trailing_text_verbatim() {
        for output in [
            "## 审查摘要\n\n**P1**：需要修改",
            "没有发现问题",
            "{\"reviewResult\":{}}\n补充说明",
            "{incomplete",
        ] {
            assert_eq!(report(output), output);
        }
    }
    #[test]
    fn json_reports_preserve_arbitrary_fields_without_validation_or_demotion() {
        let output = report(
            r#"{"reviewResult":{"summary":"审查结束","issues":[{"severity":"P1","file":"any.rs","custom":"证据"}]},"extra":"备注"}"#,
        );
        for text in ["审查结束", "P1", "any.rs", "证据", "备注"] {
            assert!(output.contains(text));
        }
        assert_eq!(report("```markdown\n# 审查\n```"), "# 审查");
        let blocks = "```rust\nlet a = 1;\n```\n说明\n```rust\nlet b = 2;\n```";
        assert_eq!(report(blocks), blocks);
    }
    #[test]
    fn rejects_empty_reports_but_validates_only_explicit_context_requests() {
        assert!(parse(" \n ").is_err());
        assert!(matches!(parse(r#"{"contextRequests":[{"kind":"file","path":"source.py","startLine":1,"endLine":20}]}"#).unwrap(), ReviewEnvelope::Requests(_)));
        assert!(parse(r#"{"contextRequests":[{"kind":"shell","command":"pwd"}]}"#).is_err());
        assert!(parse(r#"{"contextRequests":[]}"#).is_err());
        let requests = serde_json::json!({"contextRequests":vec![serde_json::json!({"kind":"skill","path":"references/output-format.md"});9]});
        assert!(parse(&requests.to_string()).is_err());
    }
}
