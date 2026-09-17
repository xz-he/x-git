use serde_json::Value;

use super::{MAX_OUTPUT_BYTES, invalid_response_error, output_limit_error};
use crate::domain::error::{BackendError, ErrorCode};

pub(super) fn process_event<F: FnMut(&str)>(
    value: &Value,
    output: &mut String,
    on_delta: &mut F,
) -> Result<bool, BackendError> {
    if value.get("error").is_some_and(Value::is_object) {
        return Err(provider_error(&value["error"]));
    }
    match value.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") => {
            append(
                value
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid_response_error)?,
                output,
                on_delta,
            )?;
        }
        Some("response.completed") => {
            finish_response(&value["response"], output, on_delta)?;
            return Ok(true);
        }
        Some("response.failed") => return Err(provider_error(&value["response"]["error"])),
        Some("response.incomplete") => return Err(incomplete_error()),
        Some("error") => return Err(provider_error(value)),
        Some("response.refusal.delta" | "response.refusal.done") => return Err(refusal_error()),
        // Reasoning, item lifecycle and final text snapshots are not text deltas.
        Some(kind) if kind.starts_with("response.") => {}
        _ => return Err(invalid_response_error()),
    }
    Ok(false)
}

pub(super) fn finish_response<F: FnMut(&str)>(
    value: &Value,
    output: &mut String,
    on_delta: &mut F,
) -> Result<(), BackendError> {
    if value.get("error").is_some_and(Value::is_object) {
        return Err(provider_error(&value["error"]));
    }
    if value.get("status").and_then(Value::as_str) != Some("completed") {
        return Err(incomplete_error());
    }
    let items = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(invalid_response_error)?;
    let mut final_text = String::new();
    for item in items {
        if item["type"] != "message" || item["role"] != "assistant" {
            continue;
        }
        let parts = item
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(invalid_response_error)?;
        for part in parts {
            match part["type"].as_str() {
                Some("output_text") => {
                    let text = part["text"].as_str().ok_or_else(invalid_response_error)?;
                    append(text, &mut final_text, &mut |_| {})?;
                }
                Some("refusal") => return Err(refusal_error()),
                _ => {}
            }
        }
    }
    // A completed snapshot may be the only text returned by a compatible service.
    // Append only a missing suffix so done/completed events never duplicate deltas.
    if !final_text.is_empty() {
        let suffix = final_text
            .strip_prefix(output.as_str())
            .ok_or_else(invalid_response_error)?;
        append(suffix, output, on_delta)?;
    }
    if output.trim().is_empty() {
        return Err(invalid_response_error());
    }
    Ok(())
}

fn append<F: FnMut(&str)>(
    text: &str,
    output: &mut String,
    on_delta: &mut F,
) -> Result<(), BackendError> {
    if output.len() + text.len() > MAX_OUTPUT_BYTES {
        return Err(output_limit_error());
    }
    if !text.is_empty() {
        output.push_str(text);
        on_delta(text);
    }
    Ok(())
}

pub(super) fn incomplete_error() -> BackendError {
    BackendError::new(
        ErrorCode::AiInvalidResponse,
        "AI 响应未完成，请重试或检查服务的 Responses API 配置。",
    )
}

fn refusal_error() -> BackendError {
    BackendError::new(
        ErrorCode::AiInvalidResponse,
        "AI 服务拒绝了本次请求，未生成有效结果。",
    )
}

fn provider_error(error: &Value) -> BackendError {
    match error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "invalid_api_key" | "authentication_error" | "unauthorized" => {
            BackendError::new(ErrorCode::AiAuthentication, "AI 服务认证失败。")
        }
        "rate_limit_exceeded" | "rate_limit_error" | "insufficient_quota" => {
            BackendError::new(ErrorCode::AiRateLimited, "AI 服务请求过于频繁或额度不足。")
        }
        "context_length_exceeded" => {
            BackendError::new(ErrorCode::AiContextTooLarge, "AI 请求超出模型上下文限制。")
        }
        "invalid_request_error" | "unsupported_parameter" | "model_not_found" => BackendError::new(
            ErrorCode::AiConfiguration,
            "AI 服务不支持当前模型或请求参数，请检查配置。",
        ),
        _ => BackendError::new(ErrorCode::AiTransport, "AI 服务生成失败，请重试。"),
    }
}
