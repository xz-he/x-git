use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::{Uuid, Version};

#[cfg(test)]
use crate::application::ai_context::AiContextBatch;
use crate::application::ai_context::{AiContextBuilder, AiFrozenContext, MAX_DIFF_BATCH_BYTES};
use crate::domain::ai::{
    AiCommitMessageResult, AiConnectionConfig, AiConnectionTestResult, AiIssueSeverity,
    AiReviewIssue, AiRunAccepted, AiRunEvent, AiRunEventData, AiTaskKind,
};
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::settings::AppSettings;
use crate::infrastructure::ai_client::{AiHttpClient, AiPromptRequest};

pub trait AiEventSink: Send + Sync {
    fn emit(&self, event: &AiRunEvent) -> Result<(), BackendError>;
}

#[derive(Debug, Clone)]
pub struct AiService {
    pub(super) context_builder: AiContextBuilder,
    pub(super) http_client: AiHttpClient,
    pub(super) active_runs: Arc<Mutex<HashMap<String, CancellationToken>>>,
    pub(super) review_timeout: std::time::Duration,
    pub(super) conflict_timeout: std::time::Duration,
}

impl AiService {
    pub fn new(context_builder: AiContextBuilder, http_client: AiHttpClient) -> Self {
        Self {
            context_builder,
            http_client,
            active_runs: Arc::new(Mutex::new(HashMap::new())),
            review_timeout: super::review_service::TOTAL_TIMEOUT,
            conflict_timeout: std::time::Duration::from_secs(2 * 60),
        }
    }

    pub async fn start_review(
        &self,
        run_id: &str,
        root: &Path,
        settings: &AppSettings,
        sink: Arc<dyn AiEventSink>,
    ) -> Result<AiRunAccepted, BackendError> {
        self.start_review_source(
            run_id,
            root,
            settings,
            crate::domain::ai::ReviewSource::Staged,
            sink,
        )
        .await
    }

    pub async fn start_commit_message(
        &self,
        run_id: &str,
        root: &Path,
        settings: &AppSettings,
        sink: Arc<dyn AiEventSink>,
    ) -> Result<AiRunAccepted, BackendError> {
        validate_run_id(run_id)?;
        let cancellation = self.register_run(run_id).await?;
        let capture = tokio::select! { biased; _ = cancellation.cancelled() => Err(cancelled()), result = self.context_builder.capture(root, settings) => result };
        let context = match capture {
            Ok(context) => context,
            Err(error) => {
                self.active_runs.lock().await.remove(run_id);
                return Err(error);
            }
        };
        if context.commit_message_input.len() > MAX_DIFF_BATCH_BYTES {
            self.active_runs.lock().await.remove(run_id);
            return Err(BackendError::new(
                ErrorCode::AiContextTooLarge,
                "用于生成提交信息的已暂存变更超过 48 KiB，请缩小变更范围。",
            ));
        }
        let accepted = AiRunAccepted {
            run_id: run_id.to_owned(),
            task: AiTaskKind::GenerateCommitMessage,
            context: context.summary.clone(),
            total_batch_count: 1,
        };
        if let Err(error) = sink.emit(&AiRunEvent::new(
            run_id,
            1,
            AiRunEventData::Started {
                context: context.summary.clone(),
                total_batch_count: 1,
            },
        )) {
            self.active_runs.lock().await.remove(run_id);
            return Err(error);
        }

        let service = self.clone();
        let run_id = run_id.to_owned();
        let config = connection_config(settings);
        tokio::spawn(async move {
            service
                .run_commit_message(run_id, context, config, cancellation, sink)
                .await;
        });
        Ok(accepted)
    }

    pub async fn cancel(&self, run_id: &str) -> Result<(), BackendError> {
        if let Some(cancellation) = self.active_runs.lock().await.get(run_id).cloned() {
            cancellation.cancel();
        }
        Ok(())
    }

    pub async fn test_connection(
        &self,
        config: AiConnectionConfig,
    ) -> Result<AiConnectionTestResult, BackendError> {
        let output = self
            .http_client
            .stream(
                &config,
                AiPromptRequest {
                    system: "你是连接测试助手。".to_owned(),
                    user: "请仅回复 OK".to_owned(),
                    temperature_milli: 0,
                },
                CancellationToken::new(),
                |_| {},
            )
            .await?;
        if output.trim().is_empty() {
            return Err(invalid_response());
        }
        Ok(AiConnectionTestResult {
            provider: config.provider,
            model: config.model,
            message: "连接成功。".to_owned(),
        })
    }

    pub(super) async fn register_run(
        &self,
        run_id: &str,
    ) -> Result<CancellationToken, BackendError> {
        let mut runs = self.active_runs.lock().await;
        if !runs.is_empty() {
            return Err(BackendError::new(
                ErrorCode::AiConfiguration,
                "AI 任务标识已在使用。",
            ));
        }
        let cancellation = CancellationToken::new();
        runs.insert(run_id.to_owned(), cancellation.clone());
        Ok(cancellation)
    }

    async fn run_commit_message(
        &self,
        run_id: String,
        context: AiFrozenContext,
        config: AiConnectionConfig,
        cancellation: CancellationToken,
        sink: Arc<dyn AiEventSink>,
    ) {
        let mut sequence = 1;
        let result = self
            .execute_commit_message(
                &run_id,
                &context,
                &config,
                &cancellation,
                sink.as_ref(),
                &mut sequence,
            )
            .await;
        if let Err(error) = result {
            sequence += 1;
            let event = if error.code == ErrorCode::Cancelled {
                AiRunEventData::Cancelled {
                    completed_batch_count: 0,
                    total_batch_count: 1,
                }
            } else {
                AiRunEventData::Failed { error }
            };
            let _ = sink.emit(&AiRunEvent::new(&run_id, sequence, event));
        }
        self.active_runs.lock().await.remove(&run_id);
    }

    async fn execute_commit_message(
        &self,
        run_id: &str,
        context: &AiFrozenContext,
        config: &AiConnectionConfig,
        cancellation: &CancellationToken,
        sink: &dyn AiEventSink,
        sequence: &mut u64,
    ) -> Result<(), BackendError> {
        *sequence += 1;
        sink.emit(&AiRunEvent::new(
            run_id,
            *sequence,
            AiRunEventData::BatchStarted {
                batch_index: 1,
                file_paths: Vec::new(),
            },
        ))?;
        let mut delta_error = None;
        let output = self
            .http_client
            .stream(
                config,
                commit_prompt(&context.commit_message_input),
                cancellation.clone(),
                |text| {
                    if delta_error.is_some() {
                        return;
                    }
                    *sequence += 1;
                    if let Err(error) = sink.emit(&AiRunEvent::new(
                        run_id,
                        *sequence,
                        AiRunEventData::Delta {
                            text: text.to_owned(),
                        },
                    )) {
                        delta_error = Some(error);
                    }
                },
            )
            .await?;
        if let Some(error) = delta_error {
            return Err(error);
        }
        let message = parse_commit_message(&output)?;
        *sequence += 1;
        sink.emit(&AiRunEvent::new(
            run_id,
            *sequence,
            AiRunEventData::CommitMessageCompleted {
                result: AiCommitMessageResult {
                    message,
                    context_fingerprint: context.summary.fingerprint.clone(),
                },
            },
        ))
    }
}

#[cfg(test)]
fn parse_review_response(
    response: &str,
    batch: &AiContextBatch,
) -> Result<super::review_protocol::ValidatedReview, BackendError> {
    match super::review_protocol::parse(response, batch, &[])? {
        super::review_protocol::ReviewEnvelope::Result(result) => Ok(result),
        _ => Err(invalid_response()),
    }
}

pub(super) fn merge_issues(target: &mut Vec<AiReviewIssue>, incoming: Vec<AiReviewIssue>) {
    let mut existing = target
        .iter()
        .enumerate()
        .map(|(index, issue)| (issue_key(issue), index))
        .collect::<HashMap<_, _>>();
    for issue in incoming {
        let key = issue_key(&issue);
        if let Some(index) = existing.get(&key).copied() {
            if severity_rank(issue.severity) < severity_rank(target[index].severity) {
                target[index] = issue;
            }
        } else {
            existing.insert(key, target.len());
            target.push(issue);
        }
    }
}

fn issue_key(issue: &AiReviewIssue) -> String {
    format!(
        "{}\0{:?}\0{:?}\0{}",
        issue.path,
        issue.start_line,
        issue.end_line,
        issue.title.as_deref().unwrap_or(&issue.reason).trim()
    )
}

#[cfg(test)]
fn review_summary(reviewed_files: usize, issues: &[AiReviewIssue]) -> String {
    let critical = issues
        .iter()
        .filter(|issue| issue.severity == AiIssueSeverity::Critical)
        .count();
    let warning = issues
        .iter()
        .filter(|issue| issue.severity == AiIssueSeverity::Warning)
        .count();
    let suggestion = issues
        .iter()
        .filter(|issue| issue.severity == AiIssueSeverity::Suggestion)
        .count();
    format!("已审查 {reviewed_files} 个文件：严重 {critical}，警告 {warning}，建议 {suggestion}。")
}

fn severity_rank(severity: AiIssueSeverity) -> u8 {
    match severity {
        AiIssueSeverity::Critical | AiIssueSeverity::P0 => 0,
        AiIssueSeverity::P1 => 1,
        AiIssueSeverity::Warning | AiIssueSeverity::P2 => 2,
        AiIssueSeverity::Suggestion | AiIssueSeverity::P3 => 3,
    }
}

pub fn parse_commit_message(response: &str) -> Result<String, BackendError> {
    let message = strip_markdown_fence(response)?.trim();
    let subject = message.lines().next().unwrap_or_default();
    if !valid_conventional_subject(subject) {
        return Err(invalid_response());
    }
    Ok(message.to_owned())
}

fn valid_conventional_subject(subject: &str) -> bool {
    if !subject.is_ascii() {
        return false;
    }
    let Some((prefix, description)) = subject.split_once(": ") else {
        return false;
    };
    if description.trim().is_empty() || description != description.trim() {
        return false;
    }
    let commit_type = if let Some((kind, scope)) = prefix.split_once('(') {
        if scope.is_empty() || !scope.ends_with(')') || scope[..scope.len() - 1].is_empty() {
            return false;
        }
        kind
    } else {
        prefix
    };
    matches!(
        commit_type,
        "feat"
            | "fix"
            | "docs"
            | "style"
            | "refactor"
            | "perf"
            | "test"
            | "build"
            | "ci"
            | "chore"
            | "revert"
    )
}

fn strip_markdown_fence(value: &str) -> Result<&str, BackendError> {
    let trimmed = value.trim();
    if !trimmed.starts_with("```") {
        return Ok(trimmed);
    }
    let Some(first_newline) = trimmed.find('\n') else {
        return Err(invalid_response());
    };
    let Some(content) = trimmed[first_newline + 1..].strip_suffix("```") else {
        return Err(invalid_response());
    };
    Ok(content.trim())
}

fn commit_prompt(input: &str) -> AiPromptRequest {
    AiPromptRequest {
        system: concat!(
            "Generate one English Conventional Commit message using an allowed type. ",
            "Return only type(scope): subject or type: subject, with an optional short body."
        )
        .to_owned(),
        user: format!("Staged changes only:\n{input}"),
        temperature_milli: 200,
    }
}

pub(super) fn connection_config(settings: &AppSettings) -> AiConnectionConfig {
    AiConnectionConfig {
        provider: settings.ai_provider,
        api_format: settings.ai_api_format,
        api_key: settings.api_key.clone(),
        base_url: settings.base_url.clone(),
        model: settings.model.clone(),
    }
}

pub(super) fn validate_run_id(run_id: &str) -> Result<(), BackendError> {
    let uuid = Uuid::parse_str(run_id).map_err(|_| invalid_run_id())?;
    if uuid.get_version() != Some(Version::Random) {
        return Err(invalid_run_id());
    }
    Ok(())
}

fn invalid_run_id() -> BackendError {
    BackendError::new(ErrorCode::AiConfiguration, "AI 任务标识必须是 UUID v4。")
}

fn invalid_response() -> BackendError {
    BackendError::new(ErrorCode::AiInvalidResponse, "AI 服务返回了无效内容。")
}

fn cancelled() -> BackendError {
    BackendError::new(ErrorCode::Cancelled, "AI 任务已取消。")
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    use super::*;
    use crate::application::ai_context::{AiContextBatch, AiContextBuilder};
    use crate::domain::ai::{AiIssueSeverity, AiProvider, AiRunEventData};
    use crate::domain::error::ErrorCode;
    use crate::domain::settings::AppSettings;
    use crate::infrastructure::ai_client::AiHttpClient;
    use crate::infrastructure::git_runner::GitCommandRunner;

    #[derive(Clone, Default)]
    pub(in crate::application) struct RecordingSink {
        events: Arc<Mutex<Vec<AiRunEvent>>>,
        changed: Arc<Notify>,
    }

    impl AiEventSink for RecordingSink {
        fn emit(&self, event: &AiRunEvent) -> Result<(), BackendError> {
            self.events.lock().unwrap().push(event.clone());
            self.changed.notify_waiters();
            Ok(())
        }
    }

    impl RecordingSink {
        pub(in crate::application) async fn wait_for_terminal(&self) {
            tokio::time::timeout(Duration::from_secs(6), async {
                loop {
                    if self.events.lock().unwrap().last().is_some_and(|event| {
                        matches!(
                            event.event,
                            AiRunEventData::ReviewCompleted { .. }
                                | AiRunEventData::ConflictSuggestionCompleted { .. }
                                | AiRunEventData::CommitMessageCompleted { .. }
                                | AiRunEventData::Failed { .. }
                                | AiRunEventData::Cancelled { .. }
                        )
                    }) {
                        break;
                    }
                    self.changed.notified().await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for terminal: {:?}", self.events()));
        }

        pub(in crate::application) fn events(&self) -> Vec<AiRunEvent> {
            self.events.lock().unwrap().clone()
        }

        async fn wait_for_completed_batches(&self, expected: usize) {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let count = self
                        .events
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|event| {
                            matches!(event.event, AiRunEventData::ReviewBatchCompleted { .. })
                        })
                        .count();
                    if count >= expected {
                        break;
                    }
                    self.changed.notified().await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for batches: {:?}", self.events()));
        }
    }

    async fn run_git(directory: &Path, args: &[&str]) {
        GitCommandRunner::default()
            .run(Some(directory), args)
            .await
            .unwrap();
    }

    async fn staged_repository(bytes: &[u8]) -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        run_git(directory.path(), &["init", "-b", "main"]).await;
        run_git(directory.path(), &["config", "user.name", "HQ Test"]).await;
        run_git(
            directory.path(),
            &["config", "user.email", "hq@example.test"],
        )
        .await;
        std::fs::write(directory.path().join("change.txt"), bytes).unwrap();
        run_git(directory.path(), &["add", "change.txt"]).await;
        install_skill(directory.path());
        directory
    }

    async fn staged_multi_batch_repository() -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        run_git(directory.path(), &["init", "-b", "main"]).await;
        let content = "changed line\n".repeat(2_500);
        std::fs::write(directory.path().join("a.txt"), &content).unwrap();
        std::fs::write(directory.path().join("b.txt"), &content).unwrap();
        run_git(directory.path(), &["add", "a.txt", "b.txt"]).await;
        install_skill(directory.path());
        directory
    }

    fn install_skill(root: &Path) {
        std::fs::create_dir(root.join("code-review-expert")).unwrap();
        std::fs::write(
            root.join("code-review-expert/SKILL.md"),
            "---\nname: test-policy\n---\nreview only changed code",
        )
        .unwrap();
    }

    fn rich_response(path: &str, severity: &str) -> String {
        serde_json::json!({"reviewResult":{"summary":"checked", "uncovered":[], "issues":[{"severity":severity,"file":path,"line":1,"title":"Changed behavior","impact":format!("{path} impact"),"recommendation":"Check input","evidence":"Changed code permits unsafe input", "context_missing":[],"change_relation":"introduced","confidence":8,"evidenceSources":[]}]}}).to_string()
    }

    async fn one_response_server(response_text: &str) -> String {
        response_server(vec![(Duration::ZERO, response_text.to_owned())]).await
    }

    async fn response_server(responses: Vec<(Duration, String)>) -> String {
        recording_server(responses).await.0
    }

    pub(in crate::application) async fn recording_server(
        responses: Vec<(Duration, String)>,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        recording_server_with_format(responses, false).await
    }

    pub(in crate::application) async fn recording_server_with_format(
        responses: Vec<(Duration, String)>,
        use_responses: bool,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recording = requests.clone();
        tokio::spawn(async move {
            for (delay, response_text) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let count = stream.read(&mut buffer).await.unwrap();
                    request.extend_from_slice(&buffer[..count]);
                    if count == 0 || request_is_complete(&request) {
                        break;
                    }
                }
                let payload = serde_json::json!({
                    "choices": [{ "delta": { "content": response_text } }]
                });
                recording
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                let body = if use_responses {
                    let delta = serde_json::json!({"type":"response.output_text.delta", "delta":response_text});
                    let completed = serde_json::json!({"type":"response.completed", "response":{"status":"completed", "output":[]}});
                    format!("data: {delta}\n\ndata: {completed}\n\n")
                } else {
                    format!("data: {payload}\n\ndata: [DONE]\n\n")
                };
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .unwrap();
                tokio::time::sleep(delay).await;
                if stream.write_all(body.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
        (format!("http://{address}/v1/chat/completions"), requests)
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

    pub(in crate::application) fn settings(base_url: String) -> AppSettings {
        AppSettings {
            ai_provider: AiProvider::Custom,
            api_key: "test-key".to_owned(),
            base_url,
            model: "test-model".to_owned(),
            ..AppSettings::default()
        }
    }

    fn service() -> AiService {
        AiService::new(AiContextBuilder::default(), AiHttpClient::default())
    }

    #[tokio::test]
    async fn review_total_deadline_preserves_completed_batch_provenance() {
        let fixture = staged_multi_batch_repository().await;
        let (provider, _) = recording_server(vec![
            (Duration::ZERO, rich_response("a.txt", "P2")),
            (
                Duration::ZERO,
                "Markdown instead of a review envelope".to_owned(),
            ),
            (Duration::from_secs(5), rich_response("b.txt", "P2")),
        ])
        .await;
        let mut service = service();
        service.review_timeout = Duration::from_secs(3);
        let sink = RecordingSink::default();
        service
            .start_review(
                &Uuid::new_v4().to_string(),
                fixture.path(),
                &settings(provider),
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;
        let events = sink.events();
        assert!(events.iter().any(|event| matches!(
            event.event,
            AiRunEventData::ReviewBatchCompleted {
                result: Some(_),
                ..
            }
        )));
        let AiRunEventData::Failed { error } = &events.last().unwrap().event else {
            panic!("expected total deadline error")
        };
        assert_eq!(error.code, ErrorCode::AiTimeout);
        assert!(service.active_runs.lock().await.is_empty());
    }

    #[tokio::test]
    async fn cancel_on_initial_progress_stops_capture_and_releases_registry() {
        struct CancelOnProgress {
            service: AiService,
        }
        impl AiEventSink for CancelOnProgress {
            fn emit(&self, event: &AiRunEvent) -> Result<(), BackendError> {
                let service = self.service.clone();
                let run_id = event.run_id.clone();
                tokio::spawn(async move {
                    service.cancel(&run_id).await.unwrap();
                });
                Ok(())
            }
        }
        let fixture = staged_repository(b"text\n").await;
        let service = service();
        let error = service
            .start_review(
                &Uuid::new_v4().to_string(),
                fixture.path(),
                &settings("http://127.0.0.1:9".to_owned()),
                Arc::new(CancelOnProgress {
                    service: service.clone(),
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(service.active_runs.lock().await.is_empty());
    }
    #[test]
    fn merged_findings_keep_strongest_severity_for_the_same_location_and_title() {
        let finding = |severity| AiReviewIssue {
            severity,
            path: "x.py".to_owned(),
            start_line: Some(2),
            end_line: Some(2),
            title: Some("Same root cause".to_owned()),
            reason: "impact".to_owned(),
            ..AiReviewIssue::default()
        };
        let mut issues = vec![finding(AiIssueSeverity::P3)];
        merge_issues(&mut issues, vec![finding(AiIssueSeverity::P1)]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, AiIssueSeverity::P1);
    }

    #[tokio::test]
    async fn skill_review_provider_rounds_read_frozen_definitions_and_full_policy() {
        let fixture = staged_repository(b"staged_marker\n").await;
        let root = fixture.path();
        std::fs::write(
            root.join("helper.py"),
            "def helper():\n    return 'frozen_definition'\n",
        )
        .unwrap();
        run_git(root, &["add", "helper.py"]).await;
        std::fs::write(root.join("helper.py"), "private_unstaged_definition").unwrap();
        std::fs::write(root.join("private.txt"), "private_untracked_marker").unwrap();
        let rules = root.join("code-review-expert");
        std::fs::create_dir(rules.join("references")).unwrap();
        std::fs::write(rules.join("SKILL.md"), "---\nname: code-review-expert\n---\n`references/severity-guide.md` `references/false-positive-rules.md` `references/output-format.md` `references/python-rules.md` `references/database-rules.md`").unwrap();
        for name in ["severity-guide", "false-positive-rules", "output-format"] {
            std::fs::write(
                rules.join(format!("references/{name}.md")),
                format!("mandatory_{name}"),
            )
            .unwrap();
        }
        std::fs::write(
            rules.join("references/python-rules.md"),
            format!("{}python_rule_tail", "python_policy ".repeat(900)),
        )
        .unwrap();
        std::fs::write(
            rules.join("references/database-rules.md"),
            "on_demand_database_rule",
        )
        .unwrap();
        let first = serde_json::json!({"contextRequests":[{"kind":"symbol","path":"helper.py","symbol":"helper("},{"kind":"file","path":"helper.py","startLine":1,"endLine":2},{"kind":"skill","path":"references/database-rules.md"},{"kind":"file","path":"private.txt","startLine":1,"endLine":3}]}).to_string();
        let final_result = serde_json::json!({"reviewResult":{"summary":"checked","issues":[],"uncovered":["external dependency unavailable"]}}).to_string();
        let (provider, requests) = recording_server(vec![
            (Duration::ZERO, first),
            (Duration::ZERO, final_result),
        ])
        .await;
        let sink = RecordingSink::default();
        service()
            .start_review(
                &Uuid::new_v4().to_string(),
                root,
                &settings(provider),
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;
        let events = sink.events();
        let AiRunEventData::ReviewCompleted { result } = &events.last().unwrap().event else {
            panic!("expected result: {events:?}")
        };
        assert!(
            result
                .context
                .as_ref()
                .unwrap()
                .skill
                .files
                .contains(&"references/database-rules.md".to_owned())
        );
        assert!(!result.context.as_ref().unwrap().evidence_sources.is_empty());
        assert!(
            result
                .uncovered
                .iter()
                .any(|text| text.contains("private.txt"))
        );
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("python_rule_tail"));
        assert!(requests[0].contains("mandatory_severity-guide"));
        assert!(!requests[0].contains("on_demand_database_rule"));
        assert!(requests[1].contains("on_demand_database_rule"));
        assert!(requests[1].contains("frozen_definition"));
        assert!(
            requests
                .iter()
                .all(|request| !request.contains("private_unstaged_definition")
                    && !request.contains("private_untracked_marker"))
        );
    }

    #[tokio::test]
    async fn selected_staged_review_provider_receives_only_selected_diff_and_repository_skill() {
        let fixture = staged_repository(b"selected_staged_marker\n").await;
        let root = fixture.path();
        std::fs::write(root.join("unselected.txt"), "unselected_staged_marker\n").unwrap();
        run_git(root, &["add", "unselected.txt"]).await;
        std::fs::write(root.join("change.txt"), "private_unstaged_marker\n").unwrap();
        let (provider, requests) = recording_server(vec![(
            Duration::ZERO,
            serde_json::json!({"reviewResult":{"summary":"checked","issues":[],"uncovered":[]}})
                .to_string(),
        )])
        .await;
        let sink = RecordingSink::default();
        let source = crate::domain::ai::ReviewSource::StagedFiles {
            paths: vec!["change.txt".to_owned()],
        };
        let accepted = service()
            .start_review_source(
                &Uuid::new_v4().to_string(),
                root,
                &settings(provider),
                source.clone(),
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        assert_eq!(accepted.context.staged_file_count, 1);
        assert_eq!(accepted.context.text_file_count, 1);
        assert_eq!(accepted.context.review.as_ref().unwrap().source, source);
        sink.wait_for_terminal().await;
        let events = sink.events();
        let AiRunEventData::ReviewCompleted { result } = &events.last().unwrap().event else {
            panic!("expected result: {events:?}")
        };
        assert_eq!(result.reviewed_files, ["change.txt"]);
        assert_eq!(result.context.as_ref().unwrap().source, source);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("selected_staged_marker"));
        assert!(requests[0].contains("Current repository policy"));
        assert!(requests[0].contains("StagedFiles"));
        assert!(!requests[0].contains("unselected_staged_marker"));
        assert!(!requests[0].contains("private_unstaged_marker"));
    }

    #[tokio::test]
    async fn historical_review_provider_uses_selected_commit_and_first_parent() {
        let fixture = staged_repository(b"root_marker\n").await;
        let root = fixture.path();
        run_git(root, &["commit", "-m", "root"]).await;
        let base = GitCommandRunner::default()
            .run(Some(root), &["rev-parse", "HEAD"])
            .await
            .unwrap()
            .stdout
            .trim()
            .to_owned();
        std::fs::write(root.join("change.txt"), "selected_commit_marker\n").unwrap();
        run_git(root, &["add", "change.txt"]).await;
        run_git(root, &["commit", "-m", "selected"]).await;
        let selected = GitCommandRunner::default()
            .run(Some(root), &["rev-parse", "HEAD"])
            .await
            .unwrap()
            .stdout
            .trim()
            .to_owned();
        std::fs::write(root.join("change.txt"), "current_index_marker\n").unwrap();
        run_git(root, &["add", "change.txt"]).await;
        let (provider, requests) = recording_server(vec![(
            Duration::ZERO,
            serde_json::json!({"reviewResult":{"summary":"checked","issues":[],"uncovered":[]}})
                .to_string(),
        )])
        .await;
        let sink = RecordingSink::default();
        let accepted = service()
            .start_review_source(
                &Uuid::new_v4().to_string(),
                root,
                &settings(provider),
                crate::domain::ai::ReviewSource::Commit {
                    revision: selected.clone(),
                },
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        assert_eq!(accepted.context.staged_file_count, 0);
        sink.wait_for_terminal().await;
        let review = accepted.context.review.unwrap();
        assert_eq!(review.resolved_commit, Some(selected));
        assert_eq!(review.base_commit, Some(base));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("selected_commit_marker"));
        assert!(!requests[0].contains("current_index_marker"));
    }

    #[tokio::test]
    async fn repeated_context_requests_stop_after_four_rounds_with_uncovered_scope() {
        let fixture = staged_repository(b"text\n").await;
        let response = serde_json::json!({"contextRequests":[{"kind":"file","path":"absent.py","startLine":1,"endLine":4}]}).to_string();
        let (provider, requests) = recording_server(vec![(Duration::ZERO, response); 5]).await;
        let sink = RecordingSink::default();
        service()
            .start_review(
                &Uuid::new_v4().to_string(),
                fixture.path(),
                &settings(provider),
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;
        let events = sink.events();
        let AiRunEventData::ReviewCompleted { result } = &events.last().unwrap().event else {
            panic!("expected bounded partial completion")
        };
        assert!(result.reviewed_files.is_empty());
        assert!(!result.uncovered.is_empty());
        assert_eq!(requests.lock().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn missing_skill_rejects_review_before_http_and_releases_registry() {
        let fixture = staged_repository(b"text\n").await;
        std::fs::remove_file(fixture.path().join("code-review-expert/SKILL.md")).unwrap();
        let service = service();
        let result = service
            .start_review(
                &Uuid::new_v4().to_string(),
                fixture.path(),
                &settings("http://127.0.0.1:9".to_owned()),
                Arc::new(RecordingSink::default()),
            )
            .await;
        assert_eq!(result.unwrap_err().code, ErrorCode::AiConfiguration);
        assert!(service.active_runs.lock().await.is_empty());
    }
    #[tokio::test]
    async fn shared_registry_rejects_second_task_until_cancelled_owner_reclaimed() {
        let service = service();
        let first = Uuid::new_v4().to_string();
        let second = Uuid::new_v4().to_string();
        service.register_run(&first).await.unwrap();
        assert!(service.register_run(&second).await.is_err());
        service.cancel(&first).await.unwrap();
        assert!(service.register_run(&second).await.is_err());
        service.active_runs.lock().await.remove(&first);
        service.register_run(&second).await.unwrap();
    }

    fn batch() -> AiContextBatch {
        AiContextBatch {
            index: 1,
            file_paths: vec!["src/main.rs".to_owned()],
            allowed_line_anchors: BTreeMap::from([("src/main.rs".to_owned(), vec![10, 20])]),
            text: "diff".to_owned(),
        }
    }

    #[test]
    fn review_parser_extracts_fenced_rich_json_without_clamping_lines() {
        let response = format!("```json\n{}\n```", rich_response("src/main.rs", "P2"));
        let parsed = parse_review_response(&response, &batch()).unwrap();
        assert_eq!(parsed.summary, "checked");
        assert_eq!(parsed.issues.len(), 1);
        assert_eq!(parsed.issues[0].severity, AiIssueSeverity::P2);
        assert_eq!(parsed.issues[0].start_line, None);
        assert!(!parsed.warnings.is_empty());
    }

    #[test]
    fn review_parser_rejects_unknown_paths_and_malformed_json() {
        let unknown_error =
            parse_review_response(&rich_response("other.rs", "P2"), &batch()).unwrap_err();
        let malformed_error = parse_review_response("{nope}", &batch()).unwrap_err();
        assert_eq!(unknown_error.code, ErrorCode::AiInvalidResponse);
        assert_eq!(malformed_error.code, ErrorCode::AiInvalidResponse);
        assert!(unknown_error.diagnostics.is_none());
        assert!(
            malformed_error
                .diagnostics
                .as_deref()
                .unwrap()
                .contains("line 1")
        );
    }

    #[test]
    fn commit_parser_accepts_conventional_commit_and_rejects_commentary() {
        assert_eq!(
            parse_commit_message("```text\nfeat(ai): add review\n\nDescribe behavior.\n```")
                .unwrap(),
            "feat(ai): add review\n\nDescribe behavior."
        );
        assert_eq!(
            parse_commit_message("fix: handle timeout").unwrap(),
            "fix: handle timeout"
        );
        assert_eq!(
            parse_commit_message("Here is the commit:\nfeat: add it")
                .unwrap_err()
                .code,
            ErrorCode::AiInvalidResponse
        );
        assert_eq!(
            parse_commit_message("feature: invalid type")
                .unwrap_err()
                .code,
            ErrorCode::AiInvalidResponse
        );
    }

    #[test]
    fn review_summary_reports_each_severity_count() {
        let issue = |severity| AiReviewIssue {
            severity,
            path: "src/main.rs".to_owned(),
            start_line: None,
            end_line: None,
            reason: "reason".to_owned(),
            suggested_fix: "fix".to_owned(),
            ..AiReviewIssue::default()
        };
        let issues = vec![
            issue(AiIssueSeverity::Critical),
            issue(AiIssueSeverity::Warning),
            issue(AiIssueSeverity::Warning),
            issue(AiIssueSeverity::Suggestion),
        ];

        assert_eq!(
            review_summary(2, &issues),
            "已审查 2 个文件：严重 1，警告 2，建议 1。"
        );
    }

    #[tokio::test]
    async fn binary_only_review_is_rejected_without_http_and_cleans_up_run() {
        let fixture = staged_repository(&[0, 1, 2, 3]).await;
        let sink = RecordingSink::default();
        let run_id = "6ba7b810-9dad-4c64-9d0c-42ca61dd2577";
        let mut provider_settings = settings("http://127.0.0.1:9".to_owned());
        provider_settings.use_review_rule_files_in_review = true;
        provider_settings.review_rule_files = vec!["missing-rules.md".to_owned()];

        let service = service();
        let error = service
            .start_review(
                run_id,
                fixture.path(),
                &provider_settings,
                Arc::new(sink.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AiNoStagedChanges);
        assert!(service.active_runs.lock().await.is_empty());
    }

    #[tokio::test]
    async fn commit_generation_emits_deltas_and_validated_completion() {
        let fixture = staged_repository(b"new text\n").await;
        std::fs::remove_file(fixture.path().join("code-review-expert/SKILL.md")).unwrap();
        let provider = one_response_server("feat(core): add text").await;
        let sink = RecordingSink::default();

        let accepted = service()
            .start_commit_message(
                "d9b8b379-d087-4844-b2ae-63d951aa765b",
                fixture.path(),
                &settings(provider),
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;

        let events = sink.events();
        assert!(
            events
                .iter()
                .any(|event| matches!(event.event, AiRunEventData::Delta { .. }))
        );
        let AiRunEventData::CommitMessageCompleted { result } = &events.last().unwrap().event
        else {
            panic!("expected commit completion");
        };
        assert_eq!(result.message, "feat(core): add text");
        assert_eq!(result.context_fingerprint, accepted.context.fingerprint);
    }

    #[tokio::test]
    async fn cancellation_retains_completed_review_batch_count_and_cleans_up() {
        let fixture = staged_multi_batch_repository().await;
        let response = |path: &str| rich_response(path, "P2");
        let provider = response_server(vec![
            (Duration::ZERO, response("a.txt")),
            (Duration::from_secs(5), response("b.txt")),
        ])
        .await;
        let provider_settings = settings(provider);
        let sink = RecordingSink::default();
        let service = service();
        let run_id = "8c0ee5ab-a03d-419c-ae19-9936aaefa6d4";

        let accepted = service
            .start_review(
                run_id,
                fixture.path(),
                &provider_settings,
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        assert_eq!(accepted.total_batch_count, 2);
        sink.wait_for_completed_batches(1).await;
        let batch_event = sink
            .events()
            .into_iter()
            .find_map(|event| match event.event {
                AiRunEventData::ReviewBatchCompleted { result, .. } => result,
                _ => None,
            })
            .unwrap();
        assert_eq!(batch_event.reviewed_files, ["a.txt"]);
        assert!(!batch_event.context.unwrap().evidence_sources.is_empty());
        service.cancel(run_id).await.unwrap();
        sink.wait_for_terminal().await;

        let events = sink.events();
        let AiRunEventData::Cancelled {
            completed_batch_count,
            total_batch_count,
        } = events.last().unwrap().event
        else {
            panic!("expected cancellation");
        };
        assert_eq!(completed_batch_count, 1);
        assert_eq!(total_batch_count, 2);

        service
            .start_review(
                run_id,
                fixture.path(),
                &provider_settings,
                Arc::new(RecordingSink::default()),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn review_emits_ordered_batches_and_one_merged_result() {
        let fixture = staged_multi_batch_repository().await;
        let response = rich_response;
        let provider = response_server(vec![
            (Duration::ZERO, response("a.txt", "P1")),
            (Duration::ZERO, response("b.txt", "P3")),
        ])
        .await;
        let sink = RecordingSink::default();

        service()
            .start_review(
                "de252725-2ebd-49c8-be45-a129e1c7c22a",
                fixture.path(),
                &settings(provider),
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;

        let events = sink.events();
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        let batch_issue_counts = events
            .iter()
            .filter_map(|event| match &event.event {
                AiRunEventData::ReviewBatchCompleted { issues, .. } => Some(issues.len()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(batch_issue_counts, [1, 1]);
        let AiRunEventData::ReviewCompleted { result } = &events.last().unwrap().event else {
            panic!("expected merged completion");
        };
        assert_eq!(result.issues.len(), 2);
        assert!(
            result
                .summary
                .starts_with("已审查 2 个文件，发现 2 个问题。")
        );
        assert!(result.context.is_some());
    }

    #[tokio::test]
    async fn oversized_commit_context_is_rejected_before_http() {
        let fixture = staged_multi_batch_repository().await;
        let error = service()
            .start_commit_message(
                "cdb5da36-8517-4a18-8842-f44aeb99bd19",
                fixture.path(),
                &settings("http://127.0.0.1:9".to_owned()),
                Arc::new(RecordingSink::default()),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiContextTooLarge);
    }

    #[tokio::test]
    async fn skill_review_repairs_missing_fields_for_staged_and_commit_sources() {
        for commit in [false, true] {
            let fixture = staged_repository(b"new text\n").await;
            let source = if commit {
                run_git(fixture.path(), &["commit", "-m", "test change"]).await;
                crate::domain::ai::ReviewSource::Commit {
                    revision: "HEAD".to_owned(),
                }
            } else {
                crate::domain::ai::ReviewSource::Staged
            };
            let malformed = r#"{"reviewResult":{"summary":"checked","issues":[]}}"#;
            let (provider, requests) = recording_server(vec![
                (Duration::ZERO, malformed.to_owned()),
                (Duration::ZERO, rich_response("change.txt", "P1")),
            ])
            .await;
            let sink = RecordingSink::default();
            let service = service();
            service
                .start_review_source(
                    &Uuid::new_v4().to_string(),
                    fixture.path(),
                    &settings(provider),
                    source,
                    Arc::new(sink.clone()),
                )
                .await
                .unwrap();
            sink.wait_for_terminal().await;
            let events = sink.events();
            let AiRunEventData::ReviewCompleted { result } = &events.last().unwrap().event else {
                panic!("expected repaired result: {events:?}");
            };
            // Repair must still pass the ordinary evidence gates.
            assert_eq!(result.issues[0].severity, AiIssueSeverity::P3);
            assert!(events.iter().any(|event| matches!(&event.event,
                AiRunEventData::ReviewProgress { phase, .. } if phase == "repair")));
            assert!(service.active_runs.lock().await.is_empty());
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[1].contains("uncovered"));
            assert!(requests[1].contains("previousResponse"));
        }
    }

    #[tokio::test]
    async fn skill_review_repair_supports_evidence_rounds_but_is_bounded_per_batch() {
        let fixture = staged_repository(b"new text\n").await;
        let context = r#"{"contextRequests":[{"kind":"file","path":"change.txt","startLine":1,"endLine":2}]}"#;
        let (provider, requests) = recording_server(vec![
            (Duration::ZERO, "## 审查摘要\n未发现问题".to_owned()),
            (Duration::ZERO, context.to_owned()),
            (Duration::ZERO, rich_response("outside-batch.txt", "P1")),
        ])
        .await;
        let sink = RecordingSink::default();
        let service = service();
        service
            .start_review(
                &Uuid::new_v4().to_string(),
                fixture.path(),
                &settings(provider),
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;
        let events = sink.events();
        let AiRunEventData::Failed { error } = &events.last().unwrap().event else {
            panic!("unauthorized path must fail: {events:?}");
        };
        assert!(error.message.contains("当前批次"));
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(&event.event,
            AiRunEventData::ReviewProgress { phase, .. } if phase == "repair"))
                .count(),
            1
        );
        assert!(service.active_runs.lock().await.is_empty());
    }

    #[tokio::test]
    async fn malformed_provider_output_emits_failure_and_cleans_up() {
        let fixture = staged_repository(b"new text\n").await;
        let (provider, requests) = recording_server(vec![
            (Duration::ZERO, "{malformed}".to_owned()),
            (Duration::ZERO, "{malformed}".to_owned()),
        ])
        .await;
        let provider_settings = settings(provider);
        let sink = RecordingSink::default();
        let service = service();
        let run_id = "a0a6d37a-ab63-4fbf-aa4d-7faec023d54a";

        service
            .start_review(
                run_id,
                fixture.path(),
                &provider_settings,
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;

        let events = sink.events();
        let AiRunEventData::Failed { error } = &events.last().unwrap().event else {
            panic!("expected failure");
        };
        assert_eq!(error.code, ErrorCode::AiInvalidResponse);
        assert!(error.message.contains("已尝试一次自动纠正"));
        assert!(error.diagnostics.as_deref().unwrap().contains("line 1"));
        assert_eq!(requests.lock().unwrap().len(), 2);
        service
            .start_review(
                run_id,
                fixture.path(),
                &provider_settings,
                Arc::new(RecordingSink::default()),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn connection_test_returns_metadata_and_rejects_empty_output() {
        let provider = one_response_server(" OK ").await;
        let provider_config = connection_config(&settings(provider));

        let result = service().test_connection(provider_config).await.unwrap();

        assert_eq!(result.provider, AiProvider::Custom);
        assert_eq!(result.model, "test-model");
        assert_eq!(result.message, "连接成功。");

        let empty_provider = one_response_server("").await;
        let error = service()
            .test_connection(connection_config(&settings(empty_provider)))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AiInvalidResponse);
    }

    #[tokio::test]
    async fn responses_saved_format_drives_connection_commit_and_skill_review() {
        let fixture = staged_repository(b"new text\n").await;
        let (provider, requests) = recording_server_with_format(
            vec![
                (Duration::ZERO, "OK".to_owned()),
                (Duration::ZERO, "feat: update text".to_owned()),
                (Duration::ZERO, rich_response("change.txt", "P2")),
            ],
            true,
        )
        .await;
        let mut provider_settings = settings(provider);
        provider_settings.ai_api_format = crate::domain::ai::AiApiFormat::Responses;
        let service = service();
        assert!(
            service
                .test_connection(connection_config(&provider_settings))
                .await
                .is_ok()
        );
        let commit_sink = RecordingSink::default();
        service
            .start_commit_message(
                &Uuid::new_v4().to_string(),
                fixture.path(),
                &provider_settings,
                Arc::new(commit_sink.clone()),
            )
            .await
            .unwrap();
        commit_sink.wait_for_terminal().await;
        assert!(matches!(
            &commit_sink.events().last().unwrap().event,
            AiRunEventData::CommitMessageCompleted { .. }
        ));
        let review_sink = RecordingSink::default();
        service
            .start_review(
                &Uuid::new_v4().to_string(),
                fixture.path(),
                &provider_settings,
                Arc::new(review_sink.clone()),
            )
            .await
            .unwrap();
        review_sink.wait_for_terminal().await;
        assert!(matches!(
            &review_sink.events().last().unwrap().event,
            AiRunEventData::ReviewCompleted { .. }
        ));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(
            requests
                .iter()
                .all(|request| request.starts_with("POST /v1/responses "))
        );
    }

    #[tokio::test]
    async fn cancel_is_idempotent_and_invalid_run_ids_are_rejected() {
        let service = service();
        service.cancel("unknown").await.unwrap();
        let fixture = staged_repository(b"text\n").await;
        let error = service
            .start_review(
                "not-a-uuid",
                fixture.path(),
                &settings("http://127.0.0.1:9".to_owned()),
                Arc::new(RecordingSink::default()),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiConfiguration);
    }
}
