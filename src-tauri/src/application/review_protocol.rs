use crate::application::{ai_context::AiContextBatch, review_evidence::ContextRequest};
use crate::domain::ai::{AiReviewIssue, ReviewEvidenceSource};
use crate::domain::error::{BackendError, ErrorCode};

#[derive(Debug)]
pub enum ReviewEnvelope {
    Requests(Vec<ContextRequest>),
    Result(ValidatedReview),
}
#[derive(Debug)]
pub struct ValidatedReview {
    pub summary: String,
    pub issues: Vec<AiReviewIssue>,
    pub warnings: Vec<String>,
    pub uncovered: Vec<String>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultBody {
    summary: String,
    issues: Vec<Finding>,
    uncovered: Vec<String>,
}
#[derive(serde::Deserialize)]
struct Finding {
    severity: String,
    file: String,
    line: Option<u32>,
    title: String,
    impact: String,
    recommendation: String,
    evidence: String,
    context_missing: Vec<String>,
    change_relation: String,
    confidence: u8,
    #[serde(rename = "evidenceSources")]
    evidence_sources: Vec<ReviewEvidenceSource>,
}
fn invalid(reason: &str) -> BackendError {
    BackendError::new(
        ErrorCode::AiInvalidResponse,
        format!("AI 审查响应格式不正确：{reason}"),
    )
}
pub fn parse(
    output: &str,
    batch: &AiContextBatch,
    sources: &[ReviewEvidenceSource],
) -> Result<ReviewEnvelope, BackendError> {
    use crate::domain::ai::AiIssueSeverity;
    let output = output.trim();
    let output = if output.starts_with("```") {
        output
            .split_once('\n')
            .and_then(|(_, body)| body.strip_suffix("```"))
            .ok_or_else(|| invalid("Markdown 代码块未正确结束。"))?
            .trim()
    } else {
        output
    };
    let value: serde_json::Value = serde_json::from_str(output).map_err(|error| {
        invalid("回复不是有效 JSON。请检查模型是否返回了说明文字或截断的内容。")
            .with_diagnostics(error.to_string())
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("顶层必须是 JSON 对象。"))?;
    if object.len() != 1 {
        return Err(invalid(
            "顶层只能包含 reviewResult 或 contextRequests 其中一个字段。",
        ));
    }
    if let Some(requests) = object.get("contextRequests") {
        let requests: Vec<ContextRequest> =
            serde_json::from_value(requests.clone()).map_err(|error| {
                invalid("contextRequests 的字段或类型不符合要求。")
                    .with_diagnostics(error.to_string())
            })?;
        if requests.is_empty() || requests.len() > super::review_evidence::MAX_REQUESTS {
            return Err(invalid("contextRequests 必须包含 1 到 8 个请求。"));
        }
        return Ok(ReviewEnvelope::Requests(requests));
    }
    let body: ResultBody = serde_json::from_value(
        object
            .get("reviewResult")
            .ok_or_else(|| invalid("缺少 reviewResult 包装字段。"))?
            .clone(),
    )
    .map_err(|error| {
        invalid("reviewResult 缺少必需字段或字段类型不正确。").with_diagnostics(error.to_string())
    })?;
    let mut result = ValidatedReview {
        summary: body.summary,
        issues: Vec::new(),
        warnings: Vec::new(),
        uncovered: body.uncovered,
    };
    for finding in body.issues {
        let mut severity = match finding.severity.as_str() {
            "P0" => AiIssueSeverity::P0,
            "P1" => AiIssueSeverity::P1,
            "P2" => AiIssueSeverity::P2,
            "P3" => AiIssueSeverity::P3,
            _ => return Err(invalid("问题 severity 必须为 P0、P1、P2 或 P3。")),
        };
        if !batch.file_paths.contains(&finding.file) {
            return Err(invalid("问题 file 必须是当前批次的变更文件路径。"));
        }
        if !(1..=10).contains(&finding.confidence) {
            return Err(invalid("问题 confidence 必须是 1 到 10 的整数。"));
        }
        if !matches!(
            finding.change_relation.as_str(),
            "introduced" | "exacerbated" | "unclear"
        ) {
            return Err(invalid(
                "问题 change_relation 必须为 introduced、exacerbated 或 unclear。",
            ));
        }
        if [
            &finding.title,
            &finding.impact,
            &finding.recommendation,
            &finding.evidence,
        ]
        .iter()
        .any(|text| text.trim().is_empty())
        {
            return Err(invalid(
                "问题 title、impact、recommendation、evidence 不得为空。",
            ));
        }
        let mut missing = finding.context_missing;
        let mut verified = Vec::new();
        for source in finding.evidence_sources {
            if source.start_line > 0
                && source.end_line >= source.start_line
                && sources.iter().any(|read| {
                    read.path == source.path
                        && read.revision == source.revision
                        && read.start_line <= source.start_line
                        && read.end_line >= source.end_line
                })
            {
                verified.push(source);
            } else {
                missing.push(format!("未验证的证据引用：{}", source.path));
            }
        }
        if verified.is_empty() {
            missing.push("没有可验证的已读取证据引用".to_owned());
        }
        let mut confidence = finding.confidence;
        if matches!(severity, AiIssueSeverity::P0 | AiIssueSeverity::P1)
            && (!missing.is_empty() || finding.change_relation == "unclear" || confidence < 7)
        {
            severity = AiIssueSeverity::P3;
            result.warnings.push(format!(
                "{}：高等级证据门槛不足，已降为 P3。",
                finding.title
            ));
        }
        if !missing.is_empty() {
            confidence = confidence.min(5);
        }
        let line = finding.line.filter(|line| {
            batch
                .allowed_line_anchors
                .get(&finding.file)
                .is_some_and(|anchors| anchors.contains(line))
        });
        if line != finding.line {
            result.warnings.push(format!(
                "{}：位置不在本批变更行内，已移除定位。",
                finding.file
            ));
        }
        result.issues.push(AiReviewIssue {
            severity,
            path: finding.file,
            start_line: line,
            end_line: line,
            reason: finding.impact.clone(),
            suggested_fix: finding.recommendation,
            title: Some(finding.title),
            impact: Some(finding.impact),
            evidence: Some(finding.evidence),
            confidence: Some(confidence),
            context_missing: missing,
            change_relation: Some(finding.change_relation),
            evidence_sources: verified,
        });
    }
    Ok(ReviewEnvelope::Result(result))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai::AiIssueSeverity;
    fn batch() -> AiContextBatch {
        AiContextBatch {
            index: 1,
            file_paths: vec!["source.py".to_owned()],
            allowed_line_anchors: std::collections::BTreeMap::from([(
                "source.py".to_owned(),
                vec![2, 9],
            )]),
            text: String::new(),
        }
    }
    fn response() -> serde_json::Value {
        serde_json::json!({"reviewResult":{"summary":"checked", "uncovered":[], "issues":[{"severity":"P1","file":"source.py","line":2,"title":"Unsafe call","impact":"Crashes on null","recommendation":"Check input","evidence":"Caller passes null to definition","context_missing":[],"change_relation":"introduced","confidence":8,"evidenceSources":[{"path":"source.py","revision":"abc","startLine":2,"endLine":2}]}]}})
    }
    fn sources() -> Vec<ReviewEvidenceSource> {
        vec![ReviewEvidenceSource {
            path: "source.py".to_owned(),
            revision: "abc".to_owned(),
            start_line: 1,
            end_line: 4,
        }]
    }
    #[test]
    fn rich_findings_keep_exact_severity_and_verified_evidence() {
        let ReviewEnvelope::Result(result) =
            parse(&response().to_string(), &batch(), &sources()).unwrap()
        else {
            panic!("expected result")
        };
        assert_eq!(result.issues[0].severity, AiIssueSeverity::P1);
        assert_eq!(result.issues[0].title.as_deref(), Some("Unsafe call"));
        assert_eq!(result.issues[0].confidence, Some(8));
    }
    #[test]
    fn incomplete_high_severity_is_demoted_and_invalid_locations_not_clamped() {
        let mut response = response();
        response["reviewResult"]["issues"][0]["context_missing"] =
            serde_json::json!(["callee unavailable"]);
        response["reviewResult"]["issues"][0]["line"] = serde_json::json!(5);
        let ReviewEnvelope::Result(result) = parse(&response.to_string(), &batch(), &[]).unwrap()
        else {
            panic!("expected result")
        };
        assert_eq!(result.issues[0].severity, AiIssueSeverity::P3);
        assert_eq!(result.issues[0].confidence, Some(5));
        assert_eq!(result.issues[0].start_line, None);
        assert!(!result.warnings.is_empty());
    }
    #[test]
    fn rejects_missing_rich_fields_and_unauthorized_target_paths() {
        let mut response = response();
        response["reviewResult"]["issues"][0]["file"] = serde_json::json!("unmodified.py");
        assert!(parse(&response.to_string(), &batch(), &sources()).is_err());
        response["reviewResult"]["issues"][0]["file"] = serde_json::json!("source.py");
        response["reviewResult"]["issues"][0]
            .as_object_mut()
            .unwrap()
            .remove("impact");
        assert!(parse(&response.to_string(), &batch(), &sources()).is_err());
    }
    #[test]
    fn reports_missing_field_without_exposing_the_response_body() {
        let error = parse(
            r#"{"reviewResult":{"summary":"private source text","issues":[]}}"#,
            &batch(),
            &[],
        )
        .unwrap_err();
        assert!(
            error
                .diagnostics
                .as_deref()
                .unwrap()
                .contains("missing field `uncovered`")
        );
        assert!(!format!("{error:?}").contains("private source text"));
    }
    #[test]
    fn accepts_only_bounded_explicit_context_envelopes() {
        assert!(matches!(parse(r#"{"contextRequests":[{"kind":"file","path":"source.py","startLine":1,"endLine":20}]}"#, &batch(), &[]).unwrap(), ReviewEnvelope::Requests(_)));
        assert!(
            parse(
                r#"{"contextRequests":[{"kind":"shell","command":"pwd"}]}"#,
                &batch(),
                &[]
            )
            .is_err()
        );
        let requests = serde_json::json!({"contextRequests":vec![serde_json::json!({"kind":"skill","path":"references/output-format.md"});9]});
        assert!(parse(&requests.to_string(), &batch(), &[]).is_err());
    }
}
