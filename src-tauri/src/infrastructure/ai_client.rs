use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use reqwest::{Client, StatusCode, Url};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::domain::ai::{AI_RESPONSE_TIMEOUT, AiApiFormat, AiConnectionConfig, AiProvider};
use crate::domain::error::{BackendError, ErrorCode};

const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_SSE_FRAME_BYTES: usize = 1024 * 1024;

#[path = "ai_responses.rs"]
mod responses;

#[derive(Clone, PartialEq, Eq)]
pub struct AiPromptRequest {
    pub system: String,
    pub user: String,
    pub temperature_milli: u16,
}

impl fmt::Debug for AiPromptRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AiPromptRequest")
            .field("system_bytes", &self.system.len())
            .field("user_bytes", &self.user.len())
            .field("temperature_milli", &self.temperature_milli)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct AiHttpClient {
    client: Client,
}

impl Default for AiHttpClient {
    fn default() -> Self {
        Self::with_timeout(AI_RESPONSE_TIMEOUT)
    }
}

impl AiHttpClient {
    pub fn with_timeout(timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client configuration must be valid");
        Self { client }
    }

    pub async fn stream<F>(
        &self,
        config: &AiConnectionConfig,
        request: AiPromptRequest,
        cancellation: CancellationToken,
        mut on_delta: F,
    ) -> Result<String, BackendError>
    where
        F: FnMut(&str) + Send,
    {
        validate_config(config)?;
        let (endpoint, payload) = build_request(config, &request)?;
        let mut builder = self.client.post(endpoint).json(&payload);
        builder = match config.provider {
            AiProvider::Gemini => builder.header("x-goog-api-key", &config.api_key),
            AiProvider::OpenAi | AiProvider::Qwen | AiProvider::Custom => {
                let authorization = HeaderValue::from_str(&format!("Bearer {}", config.api_key))
                    .map_err(|_| configuration_error())?;
                builder.header(AUTHORIZATION, authorization)
            }
        };

        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(cancelled_error()),
            result = builder.send() => result.map_err(map_reqwest_error)?,
        };

        if !response.status().is_success() {
            return Err(map_http_status(response.status()));
        }

        let uses_responses =
            config.provider != AiProvider::Gemini && config.api_format == AiApiFormat::Responses;
        let is_json = uses_responses
            && response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|header| header.to_str().ok())
                .is_some_and(|mime| {
                    let mime = mime
                        .split(';')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_ascii_lowercase();
                    mime == "application/json" || mime.ends_with("+json")
                });
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut output = String::new();
        let mut done = false;

        while !done {
            let next = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(cancelled_error()),
                item = stream.next() => item,
            };
            let Some(chunk) = next else {
                break;
            };
            buffer.extend_from_slice(&chunk.map_err(map_reqwest_error)?);
            if is_json {
                if buffer.len() > MAX_SSE_FRAME_BYTES {
                    return Err(output_limit_error());
                }
            } else {
                done = drain_sse_frames(
                    &mut buffer,
                    config.provider,
                    uses_responses,
                    &mut output,
                    &mut on_delta,
                )?;
            }
        }

        if is_json {
            let value = serde_json::from_slice(&buffer).map_err(|_| invalid_response_error())?;
            responses::finish_response(&value, &mut output, &mut on_delta)?;
            return Ok(output);
        }
        if !done && buffer.iter().any(|byte| !byte.is_ascii_whitespace()) {
            let frame = std::str::from_utf8(&buffer).map_err(|_| invalid_response_error())?;
            done = process_sse_frame(
                frame,
                config.provider,
                uses_responses,
                &mut output,
                &mut on_delta,
            )?;
        }
        if uses_responses && (!done || output.trim().is_empty()) {
            return Err(responses::incomplete_error());
        }
        Ok(output)
    }
}

fn validate_config(config: &AiConnectionConfig) -> Result<(), BackendError> {
    if config.api_key.trim().is_empty()
        || config.base_url.trim().is_empty()
        || config.model.trim().is_empty()
    {
        return Err(configuration_error());
    }
    Ok(())
}

fn build_request(
    config: &AiConnectionConfig,
    request: &AiPromptRequest,
) -> Result<(Url, Value), BackendError> {
    let mut url = parse_and_validate_url(&config.base_url)?;
    let temperature = f64::from(request.temperature_milli) / 1000.0;

    match config.provider {
        AiProvider::Gemini => {
            let model_action = format!("{}:streamGenerateContent", config.model.trim());
            {
                let mut segments = url.path_segments_mut().map_err(|_| configuration_error())?;
                segments.pop_if_empty();
                segments.push("models");
                segments.push(&model_action);
            }
            url.set_query(Some("alt=sse"));
            Ok((
                url,
                json!({
                    "systemInstruction": {
                        "parts": [{ "text": request.system }]
                    },
                    "contents": [{
                        "role": "user",
                        "parts": [{ "text": request.user }]
                    }],
                    "generationConfig": {
                        "temperature": temperature
                    }
                }),
            ))
        }
        AiProvider::OpenAi | AiProvider::Qwen | AiProvider::Custom => {
            if config.api_format == AiApiFormat::Responses {
                let path = url.path().trim_end_matches('/');
                let path = if let Some(base) = path.strip_suffix("/chat/completions") {
                    format!("{base}/responses")
                } else if path.ends_with("/responses") {
                    path.to_owned()
                } else if path.is_empty() {
                    "/v1/responses".to_owned()
                } else {
                    format!("{path}/responses")
                };
                url.set_path(&path);
                return Ok((
                    url,
                    json!({
                        "model": config.model,
                        "instructions": request.system,
                        "input": [{ "role": "user", "content": request.user }],
                        "stream": true,
                        "store": false
                    }),
                ));
            }
            if url.path().trim_end_matches('/').ends_with("/v1") {
                let path = format!("{}/chat/completions", url.path().trim_end_matches('/'));
                url.set_path(&path);
            }
            Ok((
                url,
                json!({
                    "model": config.model,
                    "messages": [
                        { "role": "system", "content": request.system },
                        { "role": "user", "content": request.user }
                    ],
                    "temperature": temperature,
                    "stream": true
                }),
            ))
        }
    }
}

fn parse_and_validate_url(raw: &str) -> Result<Url, BackendError> {
    let url = Url::parse(raw.trim()).map_err(|_| configuration_error())?;
    if url.username() != "" || url.password().is_some() || url.host_str().is_none() {
        return Err(configuration_error());
    }

    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback(&url) => Ok(url),
        _ => Err(configuration_error()),
    }
}

fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn drain_sse_frames<F>(
    buffer: &mut Vec<u8>,
    provider: AiProvider,
    uses_responses: bool,
    output: &mut String,
    on_delta: &mut F,
) -> Result<bool, BackendError>
where
    F: FnMut(&str),
{
    while let Some((frame_end, delimiter_len)) = find_frame_delimiter(buffer) {
        if frame_end > MAX_SSE_FRAME_BYTES {
            return Err(output_limit_error());
        }
        let frame_bytes = buffer[..frame_end].to_vec();
        buffer.drain(..frame_end + delimiter_len);
        let frame = std::str::from_utf8(&frame_bytes).map_err(|_| invalid_response_error())?;
        if process_sse_frame(frame, provider, uses_responses, output, on_delta)? {
            return Ok(true);
        }
    }
    if buffer.len() > MAX_SSE_FRAME_BYTES {
        return Err(output_limit_error());
    }
    Ok(false)
}

fn find_frame_delimiter(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, Some(right)) => Some((right, 4)),
        (None, None) => None,
    }
}

fn process_sse_frame<F>(
    frame: &str,
    provider: AiProvider,
    uses_responses: bool,
    output: &mut String,
    on_delta: &mut F,
) -> Result<bool, BackendError>
where
    F: FnMut(&str),
{
    let payload = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");

    if payload.is_empty() {
        return Ok(false);
    }
    if payload.trim() == "[DONE]" {
        if uses_responses {
            return Err(responses::incomplete_error());
        }
        return Ok(true);
    }

    let value: Value = serde_json::from_str(&payload).map_err(|_| invalid_response_error())?;
    if uses_responses {
        return responses::process_event(&value, output, on_delta);
    }
    let deltas = match provider {
        AiProvider::Gemini => value
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .ok_or_else(invalid_response_error)?
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        AiProvider::OpenAi | AiProvider::Qwen | AiProvider::Custom => value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .into_iter()
            .collect::<Vec<_>>(),
    };

    for delta in deltas {
        if output.len() + delta.len() > MAX_OUTPUT_BYTES {
            return Err(output_limit_error());
        }
        output.push_str(delta);
        on_delta(delta);
    }
    Ok(false)
}

fn output_limit_error() -> BackendError {
    BackendError::new(
        ErrorCode::AiContextTooLarge,
        "AI 响应超过 256 KiB 或 SSE 帧超过 1 MiB 限制。",
    )
}
fn map_http_status(status: StatusCode) -> BackendError {
    match status.as_u16() {
        401 | 403 => BackendError::new(ErrorCode::AiAuthentication, "AI 服务认证失败。"),
        429 => BackendError::new(ErrorCode::AiRateLimited, "AI 服务请求过于频繁。"),
        _ => BackendError::new(ErrorCode::AiTransport, "AI 服务请求失败。"),
    }
}

fn map_reqwest_error(error: reqwest::Error) -> BackendError {
    if error.is_timeout() {
        BackendError::new(ErrorCode::AiTimeout, "AI 服务响应超时。")
    } else {
        BackendError::new(ErrorCode::AiTransport, "无法连接 AI 服务。")
    }
}

fn configuration_error() -> BackendError {
    BackendError::new(ErrorCode::AiConfiguration, "AI 服务配置无效。")
}

fn invalid_response_error() -> BackendError {
    BackendError::new(
        ErrorCode::AiInvalidResponse,
        "AI 服务返回了无法解析的响应。",
    )
}

fn cancelled_error() -> BackendError {
    BackendError::new(ErrorCode::Cancelled, "AI 任务已取消。")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::domain::ai::{AiConnectionConfig, AiProvider};
    use crate::domain::error::ErrorCode;

    #[derive(Clone)]
    struct FakeAiServer {
        base_url: String,
        request: Arc<Mutex<String>>,
    }

    impl FakeAiServer {
        async fn spawn(status: u16, chunks: Vec<Vec<u8>>, delay: Duration) -> Self {
            Self::spawn_with_headers(status, "", chunks, delay).await
        }

        async fn spawn_with_headers(
            status: u16,
            headers: &str,
            chunks: Vec<Vec<u8>>,
            delay: Duration,
        ) -> Self {
            Self::spawn_response(status, headers, "text/event-stream", chunks, delay).await
        }

        async fn spawn_response(
            status: u16,
            headers: &str,
            content_type: &str,
            chunks: Vec<Vec<u8>>,
            delay: Duration,
        ) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let request = Arc::new(Mutex::new(String::new()));
            let captured = request.clone();
            let headers = headers.to_owned();
            let content_type = content_type.to_owned();
            tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let count = stream.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if request_is_complete(&bytes) {
                        break;
                    }
                }
                *captured.lock().unwrap() = String::from_utf8_lossy(&bytes).into_owned();
                let reason = if status == 200 { "OK" } else { "Error" };
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nConnection: close\r\n{headers}\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                for chunk in chunks {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    if stream.write_all(&chunk).await.is_err() {
                        break;
                    }
                    let _ = stream.flush().await;
                }
            });
            Self {
                base_url: format!("http://{address}"),
                request,
            }
        }

        fn request(&self) -> String {
            self.request.lock().unwrap().clone()
        }
    }

    fn request_is_complete(bytes: &[u8]) -> bool {
        let text = String::from_utf8_lossy(bytes);
        let Some((headers, body)) = text.split_once("\r\n\r\n") else {
            return false;
        };
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        body.len() >= content_length
    }

    fn prompt() -> AiPromptRequest {
        AiPromptRequest {
            system: "system".to_owned(),
            user: "user".to_owned(),
            temperature_milli: 200,
        }
    }

    fn config(provider: AiProvider, base_url: String) -> AiConnectionConfig {
        AiConnectionConfig {
            provider,
            api_format: AiApiFormat::default(),
            api_key: "test-key".to_owned(),
            base_url,
            model: "test-model".to_owned(),
        }
    }

    fn responses_config(base_url: String) -> AiConnectionConfig {
        serde_json::from_value(json!({ "provider": "openAi", "apiFormat": "responses",
            "apiKey": "test-key", "baseUrl": base_url, "model": "gpt-6-astra" }))
        .unwrap()
    }

    #[tokio::test]
    async fn responses_stream_reads_typed_events_and_uses_responses_request() {
        let server = FakeAiServer::spawn(200, vec![
            b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n".to_vec(),
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"检查\"}\r\n\r\n".as_bytes().to_vec(),
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\" OK\"}\n\n".to_vec(),
            b"data: {\"type\":\"response.output_text.done\",\"text\":\"ignored duplicate\"}\n\n".to_vec(),
            b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n".to_vec(),
        ], Duration::ZERO).await;
        let mut emitted = String::new();
        let result = AiHttpClient::default()
            .stream(
                &responses_config(format!("{}/v1", server.base_url)),
                prompt(),
                CancellationToken::new(),
                |delta| emitted.push_str(delta),
            )
            .await
            .unwrap();
        assert_eq!(result, "检查 OK");
        assert_eq!(emitted, result);
        let request = server.request();
        assert!(request.starts_with("POST /v1/responses "));
        let payload: Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["instructions"], "system");
        assert_eq!(payload["input"][0]["role"], "user");
        assert_eq!(payload["input"][0]["content"], "user");
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["store"], false);
        assert!(payload.get("messages").is_none());
        assert!(payload.get("temperature").is_none());
    }

    #[test]
    fn responses_resolves_base_and_full_endpoint_urls() {
        for (base, expected) in [
            (
                "https://example.test/v1",
                "https://example.test/v1/responses",
            ),
            (
                "https://example.test/v1/",
                "https://example.test/v1/responses",
            ),
            (
                "https://example.test/v1/responses",
                "https://example.test/v1/responses",
            ),
            (
                "https://example.test/gateway/v1/chat/completions?route=test",
                "https://example.test/gateway/v1/responses?route=test",
            ),
            ("https://example.test", "https://example.test/v1/responses"),
        ] {
            assert_eq!(
                build_request(&responses_config(base.to_owned()), &prompt())
                    .unwrap()
                    .0
                    .as_str(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn responses_rejects_failed_incomplete_and_truncated_streams() {
        for (body, expected) in [
            (
                "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"invalid_api_key\",\"message\":\"secret-body\"}}}\n\n",
                ErrorCode::AiAuthentication,
            ),
            (
                "data: {\"type\":\"error\",\"code\":\"rate_limit_exceeded\"}\n\n",
                ErrorCode::AiRateLimited,
            ),
            (
                "data: {\"type\":\"response.incomplete\"}\n\n",
                ErrorCode::AiInvalidResponse,
            ),
            (
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
                ErrorCode::AiInvalidResponse,
            ),
            ("data: [DONE]\n\n", ErrorCode::AiInvalidResponse),
        ] {
            let server =
                FakeAiServer::spawn(200, vec![body.as_bytes().to_vec()], Duration::ZERO).await;
            let error = AiHttpClient::default()
                .stream(
                    &responses_config(server.base_url),
                    prompt(),
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, expected);
            assert!(!format!("{error:?}").contains("secret-body"));
        }
    }

    fn completed_response(text: &str) -> Value {
        json!({ "status": "completed", "output": [
            { "type": "reasoning", "summary": [] },
            { "type": "message", "role": "assistant", "content": [
                { "type": "output_text", "text": text }
            ] }
        ] })
    }

    #[tokio::test]
    async fn responses_accepts_json_and_completed_snapshot_without_duplicate_text() {
        for (mime, body) in [
            (
                "application/json; charset=utf-8",
                completed_response("检查 OK").to_string(),
            ),
            (
                "text/event-stream",
                format!(
                    "data: {}\n\n",
                    json!({ "type": "response.completed", "response": completed_response("检查 OK") })
                ),
            ),
            (
                "text/event-stream",
                format!(
                    "data: {}\n\ndata: {}\n\n",
                    json!({ "type": "response.output_text.delta", "delta": "检查" }),
                    json!({ "type": "response.completed", "response": completed_response("检查 OK") })
                ),
            ),
        ] {
            // Split in the middle of a Chinese UTF-8 character, not at an SSE boundary.
            let split = body.find('检').unwrap() + 1;
            let server = FakeAiServer::spawn_response(
                200,
                "",
                mime,
                vec![
                    body.as_bytes()[..split].to_vec(),
                    body.as_bytes()[split..].to_vec(),
                ],
                Duration::ZERO,
            )
            .await;
            let mut emitted = String::new();
            let result = AiHttpClient::default()
                .stream(
                    &responses_config(server.base_url),
                    prompt(),
                    CancellationToken::new(),
                    |delta| emitted.push_str(delta),
                )
                .await
                .unwrap();
            assert_eq!(result, "检查 OK");
            assert_eq!(emitted, result);
        }
    }

    #[tokio::test]
    async fn responses_rejects_json_failure_refusal_and_empty_output() {
        for (body, code) in [
            (
                json!({ "status": "failed", "error": { "code": "invalid_api_key", "message": "secret" } }),
                ErrorCode::AiAuthentication,
            ),
            (
                json!({ "status": "incomplete", "output": [] }),
                ErrorCode::AiInvalidResponse,
            ),
            (
                json!({ "status": "completed", "output": [] }),
                ErrorCode::AiInvalidResponse,
            ),
            (
                json!({ "status": "completed", "output": [{ "type": "message", "role": "assistant", "content": [{ "type": "refusal", "refusal": "secret" }] }] }),
                ErrorCode::AiInvalidResponse,
            ),
            (
                json!({ "choices": [{ "message": { "content": "wrong protocol" } }] }),
                ErrorCode::AiInvalidResponse,
            ),
        ] {
            let server = FakeAiServer::spawn_response(
                200,
                "",
                "application/json",
                vec![body.to_string().into_bytes()],
                Duration::ZERO,
            )
            .await;
            let error = AiHttpClient::default()
                .stream(
                    &responses_config(server.base_url),
                    prompt(),
                    CancellationToken::new(),
                    |_| panic!("Invalid response emitted text"),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            assert!(!format!("{error:?}").contains("secret"));
        }
    }

    #[tokio::test]
    async fn responses_enforces_output_limit_for_json_and_sse() {
        let large = "x".repeat(MAX_OUTPUT_BYTES + 1);
        for (mime, body) in [
            ("application/json", completed_response(&large).to_string()),
            (
                "text/event-stream",
                format!(
                    "data: {}\n\n",
                    json!({ "type": "response.output_text.delta", "delta": large })
                ),
            ),
        ] {
            let server = FakeAiServer::spawn_response(
                200,
                "",
                mime,
                vec![body.into_bytes()],
                Duration::ZERO,
            )
            .await;
            let error = AiHttpClient::default()
                .stream(
                    &responses_config(server.base_url),
                    prompt(),
                    CancellationToken::new(),
                    |_| panic!("Oversized output emitted"),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::AiContextTooLarge);
        }
    }

    #[tokio::test]
    async fn responses_cancellation_and_timeout_abort_json_and_sse() {
        for mime in ["application/json", "text/event-stream"] {
            for cancel in [true, false] {
                let server = FakeAiServer::spawn_response(
                    200,
                    "",
                    mime,
                    vec![b"waiting".to_vec()],
                    Duration::from_secs(5),
                )
                .await;
                let cancellation = CancellationToken::new();
                let trigger = cancellation.clone();
                if cancel {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        trigger.cancel();
                    });
                }
                let timeout = if cancel {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(40)
                };
                let error = AiHttpClient::with_timeout(timeout)
                    .stream(
                        &responses_config(server.base_url),
                        prompt(),
                        cancellation,
                        |_| {},
                    )
                    .await
                    .unwrap_err();
                assert_eq!(
                    error.code,
                    if cancel {
                        ErrorCode::Cancelled
                    } else {
                        ErrorCode::AiTimeout
                    }
                );
            }
        }
    }

    #[tokio::test]
    async fn open_ai_compatible_stream_normalizes_split_deltas() {
        let server = FakeAiServer::spawn(
            200,
            vec![
                b"data: {\"choices\":[{\"delta\":{\"content\":\"fea".to_vec(),
                b"t\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\": test\"}}]}\n\n"
                    .to_vec(),
                b"data: [DONE]\n\n".to_vec(),
            ],
            Duration::ZERO,
        )
        .await;
        let mut deltas = Vec::new();

        let output = AiHttpClient::default()
            .stream(
                &config(AiProvider::OpenAi, format!("{}/v1", server.base_url)),
                prompt(),
                CancellationToken::new(),
                |delta| deltas.push(delta.to_owned()),
            )
            .await
            .unwrap();

        assert_eq!(output, "feat: test");
        assert_eq!(deltas, ["feat", ": test"]);
        let request = server.request();
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-key")
        );
        assert!(request.contains("\"stream\":true"));
    }

    #[tokio::test]
    async fn qwen_and_custom_use_open_ai_compatible_payloads() {
        for provider in [AiProvider::Qwen, AiProvider::Custom] {
            let server =
                FakeAiServer::spawn(200, vec![b"data: [DONE]\n\n".to_vec()], Duration::ZERO).await;

            AiHttpClient::default()
                .stream(
                    &config(provider, server.base_url.clone()),
                    prompt(),
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap();

            let request = server.request();
            assert!(request.contains("\"model\":\"test-model\""));
            assert!(request.contains("\"role\":\"system\""));
            assert!(request.contains("\"role\":\"user\""));
            assert!(request.contains("\"temperature\":0.2"));
        }
    }

    #[tokio::test]
    async fn stream_preserves_utf8_split_across_tcp_chunks() {
        let frame = "data: {\"choices\":[{\"delta\":{\"content\":\"检查\"}}]}\n\n";
        let bytes = frame.as_bytes();
        let split = frame.find('检').unwrap() + 1;
        let server = FakeAiServer::spawn(
            200,
            vec![bytes[..split].to_vec(), bytes[split..].to_vec()],
            Duration::ZERO,
        )
        .await;

        let output = AiHttpClient::default()
            .stream(
                &config(AiProvider::Custom, server.base_url),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();

        assert_eq!(output, "检查");
    }

    #[tokio::test]
    async fn gemini_stream_uses_native_payload_and_header() {
        let server = FakeAiServer::spawn(
            200,
            vec![
                b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"review\"}]}}]}\n\n"
                    .to_vec(),
            ],
            Duration::ZERO,
        )
        .await;

        let output = AiHttpClient::default()
            .stream(
                &config(AiProvider::Gemini, format!("{}/v1beta", server.base_url)),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();

        assert_eq!(output, "review");
        let request = server.request();
        assert!(
            request.starts_with("POST /v1beta/models/test-model:streamGenerateContent?alt=sse ")
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-goog-api-key: test-key")
        );
        assert!(request.contains("\"systemInstruction\""));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
    }

    #[tokio::test]
    async fn gemini_model_is_encoded_as_one_path_segment() {
        let server = FakeAiServer::spawn(
            200,
            vec![b"data: {\"candidates\":[{\"content\":{\"parts\":[]}}]}\n\n".to_vec()],
            Duration::ZERO,
        )
        .await;
        let mut provider_config = config(AiProvider::Gemini, format!("{}/v1beta", server.base_url));
        provider_config.model = "models/gemini test".to_owned();

        AiHttpClient::default()
            .stream(&provider_config, prompt(), CancellationToken::new(), |_| {})
            .await
            .unwrap();

        assert!(server.request().starts_with(
            "POST /v1beta/models/models%2Fgemini%20test:streamGenerateContent?alt=sse "
        ));
    }

    #[tokio::test]
    async fn transport_maps_http_status_without_response_body() {
        for (status, code) in [
            (401, ErrorCode::AiAuthentication),
            (403, ErrorCode::AiAuthentication),
            (429, ErrorCode::AiRateLimited),
            (500, ErrorCode::AiTransport),
        ] {
            let server = FakeAiServer::spawn(
                status,
                vec![b"secret provider body".to_vec()],
                Duration::ZERO,
            )
            .await;
            let error = AiHttpClient::default()
                .stream(
                    &config(AiProvider::Custom, server.base_url),
                    prompt(),
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            assert!(
                !error
                    .diagnostics
                    .as_deref()
                    .unwrap_or_default()
                    .contains("secret provider body")
            );
        }
    }

    #[tokio::test]
    async fn transport_rejects_plain_http_outside_loopback() {
        let error = AiHttpClient::default()
            .stream(
                &config(
                    AiProvider::Custom,
                    "http://example.com/v1/chat/completions".to_owned(),
                ),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiConfiguration);
    }

    #[tokio::test]
    async fn transport_rejects_malformed_urls() {
        let error = AiHttpClient::default()
            .stream(
                &config(AiProvider::Custom, "not a url".to_owned()),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiConfiguration);
    }

    #[tokio::test]
    async fn transport_does_not_follow_redirects() {
        let target =
            FakeAiServer::spawn(200, vec![b"data: [DONE]\n\n".to_vec()], Duration::ZERO).await;
        let source = FakeAiServer::spawn_with_headers(
            302,
            &format!("Location: {}\r\n", target.base_url),
            Vec::new(),
            Duration::ZERO,
        )
        .await;

        let error = AiHttpClient::default()
            .stream(
                &config(AiProvider::Custom, source.base_url),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiTransport);
        assert!(target.request().is_empty());
    }

    #[tokio::test]
    async fn malformed_sse_json_is_an_invalid_response() {
        let server =
            FakeAiServer::spawn(200, vec![b"data: {not-json}\n\n".to_vec()], Duration::ZERO).await;

        let error = AiHttpClient::default()
            .stream(
                &config(AiProvider::Custom, server.base_url),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiInvalidResponse);
        assert!(error.diagnostics.is_none());
    }

    #[test]
    fn prompt_debug_does_not_expose_prompt_text() {
        let request = AiPromptRequest {
            system: "private-system".to_owned(),
            user: "private-diff".to_owned(),
            temperature_milli: 200,
        };

        let debug = format!("{request:?}");

        assert!(!debug.contains("private-system"));
        assert!(!debug.contains("private-diff"));
    }
    #[test]
    fn provider_output_limit_is_enforced_before_emitting_delta() {
        let content = "x".repeat(256 * 1024 + 1);
        let payload = serde_json::json!({"choices":[{"delta":{"content":content}}]});
        let frame = format!("data: {payload}");
        let mut output = String::new();
        let mut emitted = 0;
        assert!(
            process_sse_frame(
                &frame,
                AiProvider::Custom,
                false,
                &mut output,
                &mut |text| { emitted += text.len() }
            )
            .is_err()
        );
        assert_eq!(emitted, 0);
        assert!(output.is_empty());
    }
    #[tokio::test]
    async fn unfinished_sse_frame_limit_aborts_transport() {
        let server =
            FakeAiServer::spawn(200, vec![vec![b'x'; 1024 * 1024 + 1]], Duration::ZERO).await;
        let error = AiHttpClient::default()
            .stream(
                &config(AiProvider::Custom, server.base_url),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AiContextTooLarge);
    }

    #[tokio::test]
    async fn cancellation_aborts_a_waiting_stream() {
        let server = FakeAiServer::spawn(
            200,
            vec![b"data: [DONE]\n\n".to_vec()],
            Duration::from_secs(5),
        )
        .await;
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            trigger.cancel();
        });

        let error = AiHttpClient::default()
            .stream(
                &config(AiProvider::Custom, server.base_url),
                prompt(),
                cancellation,
                |_| {},
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn request_timeout_has_a_distinct_error() {
        let server = FakeAiServer::spawn(
            200,
            vec![b"data: [DONE]\n\n".to_vec()],
            Duration::from_secs(5),
        )
        .await;

        let error = AiHttpClient::with_timeout(Duration::from_millis(40))
            .stream(
                &config(AiProvider::Custom, server.base_url),
                prompt(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiTimeout);
    }
}
