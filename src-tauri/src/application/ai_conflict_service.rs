use super::{
    ai_conflict_context, ai_conflict_protocol,
    ai_service::{connection_config, validate_run_id},
};
use super::{
    ai_service::{AiEventSink, AiService},
    conflict_service::ConflictService,
};
use crate::domain::ai::{AiRunEvent, AiRunEventData, AiTaskKind};
use crate::domain::{
    ai::AiRunAccepted,
    error::{BackendError, ErrorCode},
    settings::AppSettings,
};
use std::{path::Path, sync::Arc};

impl AiService {
    #[allow(clippy::too_many_arguments)]
    pub async fn start_conflict_suggestion(
        &self,
        run_id: &str,
        root: &Path,
        settings: &AppSettings,
        conflicts: &ConflictService,
        relative_path: &str,
        token: &str,
        sink: Arc<dyn AiEventSink>,
    ) -> Result<AiRunAccepted, BackendError> {
        validate_run_id(run_id)?;
        let started_at = tokio::time::Instant::now();
        let deadline = started_at + self.conflict_timeout;
        let cancel = self.register_run(run_id).await?;
        let prepared = tokio::select! { biased;
            _ = cancel.cancelled() => Err(cancelled()),
            _ = tokio::time::sleep_until(deadline) => Err(timeout()),
            result = ai_conflict_context::capture(conflicts, root, relative_path, token) => result,
        };
        let frozen = match prepared {
            Ok(frozen) => frozen,
            Err(error) => {
                self.active_runs.lock().await.remove(run_id);
                return Err(error);
            }
        };
        let total = frozen.prompts.len();
        let accepted = AiRunAccepted {
            run_id: run_id.into(),
            task: AiTaskKind::ResolveConflict,
            context: frozen.summary.clone(),
            total_batch_count: total,
        };
        if let Err(error) = sink.emit(&AiRunEvent::new(
            run_id,
            1,
            AiRunEventData::Started {
                context: frozen.summary,
                total_batch_count: total,
            },
        )) {
            self.active_runs.lock().await.remove(run_id);
            return Err(error);
        }
        let service = self.clone();
        let run_id = run_id.to_owned();
        let config = connection_config(settings);
        tokio::spawn(async move {
            let mut sequence = 1;
            let mut completed = 0;
            let execution = async {
                let mut results = Vec::new();
                for (index, prompt) in frozen.prompts.into_iter().enumerate() {
                    sequence += 1;
                    sink.emit(&AiRunEvent::new(
                        &run_id,
                        sequence,
                        AiRunEventData::ReviewProgress {
                            phase: "conflictSuggestion".into(),
                            message: format!("正在处理冲突差异片段 {} / {}", index + 1, total),
                        },
                    ))?;
                    let mut delta_error = None;
                    let output = service
                        .http_client
                        .stream(&config, prompt, cancel.clone(), |text| {
                            if delta_error.is_some() {
                                return;
                            }
                            sequence += 1;
                            if let Err(error) = sink.emit(&AiRunEvent::new(
                                &run_id,
                                sequence,
                                AiRunEventData::Delta { text: text.into() },
                            )) {
                                delta_error = Some(error);
                            }
                        })
                        .await?;
                    if let Some(error) = delta_error {
                        return Err(error);
                    }
                    results
                        .push(ai_conflict_protocol::parse(&output)?.bind(frozen.context.clone()));
                    completed += 1;
                }
                if let Some(chunks) = frozen.chunks {
                    chunks.assemble(results, frozen.context)
                } else {
                    results.pop().ok_or_else(|| {
                        BackendError::new(ErrorCode::AiInvalidResponse, "没有生成冲突建议。")
                    })
                }
            };
            let result = tokio::select! { biased;
                _ = cancel.cancelled() => Err(cancelled()),
                _ = tokio::time::sleep_until(deadline) => Err(timeout()),
                result = execution => result,
            };
            service.active_runs.lock().await.remove(&run_id);
            sequence += 1;
            let event = match result {
                Ok(result) => AiRunEventData::ConflictSuggestionCompleted { result },
                Err(error) if error.code == ErrorCode::Cancelled => AiRunEventData::Cancelled {
                    completed_batch_count: completed,
                    total_batch_count: total,
                },
                Err(error) => AiRunEventData::Failed { error },
            };
            let _ = sink.emit(&AiRunEvent::new(run_id, sequence, event));
        });
        Ok(accepted)
    }
}

fn cancelled() -> BackendError {
    BackendError::new(ErrorCode::Cancelled, "AI 冲突建议已取消。")
}
fn timeout() -> BackendError {
    BackendError::new(
        ErrorCode::AiTimeout,
        "AI 冲突建议超过 30 分钟，请缩小冲突范围后重试。",
    )
}

#[cfg(test)]
mod tests {
    use super::super::{
        ai_context::AiContextBuilder,
        ai_service::tests::{RecordingSink, recording_server, settings},
        conflict_service_tests::conflict_versions,
    };
    use super::*;
    use crate::{domain::ai::AiRunEventData, infrastructure::ai_client::AiHttpClient};
    use std::time::Duration;

    fn service() -> AiService {
        AiService::new(AiContextBuilder::default(), AiHttpClient::default())
    }
    fn response() -> String {
        serde_json::json!({"kind":"text", "summary":"done", "explanation":"reason", "resolvedText":"resolved\n", "risks":[], "contextMissing":[]}).to_string()
    }
    #[tokio::test]
    async fn ai_conflict_large_file_sends_windows_and_reassembles_without_writes() {
        let base = (0..25_000)
            .map(|i| {
                format!(
                    "setting_{i:05} = 'unchanged application content with 中文 and extra context'\n"
                )
            })
            .collect::<String>();
        let ours = base
            .replace("setting_00100 =", "ours_00100 =")
            .replace("setting_20000 =", "ours_20000 =");
        let theirs = base
            .replace("setting_00100 =", "theirs_00100 =")
            .replace("setting_20000 =", "theirs_20000 =");
        let working = base
            .replace(
                "setting_00100 =",
                "<<<<<<< ours\nours_00100\n=======\ntheirs_00100\n>>>>>>> theirs\nsetting_00100 =",
            )
            .replace(
                "setting_20000 =",
                "<<<<<<< ours\nours_20000\n=======\ntheirs_20000\n>>>>>>> theirs\nsetting_20000 =",
            );
        let repo = conflict_versions(
            "file.txt",
            Some(base.as_bytes()),
            Some(ours.as_bytes()),
            Some(theirs.as_bytes()),
        );
        std::fs::write(repo.path().join("file.txt"), &working).unwrap();
        let index_before = std::fs::read(repo.path().join(".git/index")).unwrap();
        let conflicts = ConflictService::default();
        let detail = conflicts.detail(repo.path(), "file.txt").await.unwrap();
        assert!(detail.ours.byte_length > 1_700_000);
        let frozen =
            ai_conflict_context::capture(&conflicts, repo.path(), "file.txt", &detail.token)
                .await
                .unwrap();
        assert_eq!(frozen.prompts.len(), 2);
        let responses = frozen
            .prompts
            .iter()
            .map(|prompt| {
                assert!(prompt.user.len() < 16 * 1024);
                assert!(!prompt.user.contains("setting_10000"));
                let input: serde_json::Value = serde_json::from_str(&prompt.user).unwrap();
                assert_eq!(input["mode"], "fragment");
                let mut response: serde_json::Value = serde_json::from_str(&response()).unwrap();
                response["resolvedText"] = input["versions"]["ours"]["text"].clone();
                (Duration::ZERO, response.to_string())
            })
            .collect();
        let (url, requests) = recording_server(responses).await;
        let sink = RecordingSink::default();
        let ai = service();
        let accepted = ai
            .start_conflict_suggestion(
                &uuid::Uuid::new_v4().to_string(),
                repo.path(),
                &settings(url),
                &conflicts,
                "file.txt",
                &detail.token,
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        assert_eq!(accepted.total_batch_count, 2);
        sink.wait_for_terminal().await;
        match &sink.events().last().unwrap().event {
            AiRunEventData::ConflictSuggestionCompleted { result } => {
                assert_eq!(result.resolved_text.as_deref(), Some(ours.as_str()))
            }
            event => panic!("unexpected terminal: {event:?}"),
        }
        assert_eq!(requests.lock().unwrap().len(), 2);
        assert_eq!(
            std::fs::read(repo.path().join(".git/index")).unwrap(),
            index_before
        );
        assert_eq!(
            std::fs::read_to_string(repo.path().join("file.txt")).unwrap(),
            working
        );
    }
    #[tokio::test]
    async fn ai_conflict_http_receives_frozen_stages_and_disk_only_without_writes() {
        let repo = conflict_versions(
            "file.txt",
            Some(b"base\n"),
            Some(b"ours\n"),
            Some(b"theirs\n"),
        );
        std::fs::write(repo.path().join("file.txt"), "disk text\n").unwrap();
        let conflicts = ConflictService::default();
        let detail = conflicts.detail(repo.path(), "file.txt").await.unwrap();
        let before = std::fs::read(repo.path().join(".git/index")).unwrap();
        let (url, requests) =
            recording_server(vec![(Duration::from_millis(100), response())]).await;
        let sink = RecordingSink::default();
        let ai = service();
        let accepted = ai
            .start_conflict_suggestion(
                &uuid::Uuid::new_v4().to_string(),
                repo.path(),
                &settings(url),
                &conflicts,
                "file.txt",
                &detail.token,
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
        sink.wait_for_terminal().await;
        assert_eq!(accepted.context.staged_file_count, 0);
        assert_eq!(accepted.context.text_file_count, 1);
        let events = sink.events();
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert!(!matches!(
            events.last().unwrap().event,
            AiRunEventData::Failed { .. }
        ));
        assert!(
            serde_json::to_value(events.last().unwrap()).unwrap()["event"]["result"]["context"]["token"]
                == detail.token
        );
        let body: serde_json::Value = {
            let requests = requests.lock().unwrap();
            let (_, body) = requests[0].split_once("\r\n\r\n").unwrap();
            serde_json::from_str(body).unwrap()
        };
        let prompt = body["messages"][1]["content"].as_str().unwrap();
        let data: serde_json::Value = serde_json::from_str(prompt).unwrap();
        assert_eq!(data["versions"]["base"]["text"], "base\n");
        assert_eq!(data["versions"]["ours"]["text"], "ours\n");
        assert_eq!(data["versions"]["theirs"]["text"], "theirs\n");
        assert_eq!(data["versions"]["working"]["text"], "disk text\n");
        assert_eq!(
            std::fs::read(repo.path().join(".git/index")).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read_to_string(repo.path().join("file.txt")).unwrap(),
            "disk text\n"
        );
        assert!(ai.active_runs.lock().await.is_empty());
    }
    #[tokio::test]
    async fn ai_conflict_stale_source_never_calls_provider_and_frees_slot() {
        let repo = conflict_versions(
            "file.txt",
            Some(b"base\n"),
            Some(b"ours\n"),
            Some(b"theirs\n"),
        );
        let conflicts = ConflictService::default();
        let detail = conflicts.detail(repo.path(), "file.txt").await.unwrap();
        std::fs::write(repo.path().join("file.txt"), "later").unwrap();
        let (url, requests) = recording_server(vec![(Duration::ZERO, response())]).await;
        let ai = service();
        let error = ai
            .start_conflict_suggestion(
                &uuid::Uuid::new_v4().to_string(),
                repo.path(),
                &settings(url),
                &conflicts,
                "file.txt",
                &detail.token,
                Arc::new(RecordingSink::default()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleConflict);
        assert!(requests.lock().unwrap().is_empty());
        assert!(ai.active_runs.lock().await.is_empty());
    }

    #[tokio::test]
    async fn ai_conflict_cancel_waiting_for_shared_lock_frees_slot_before_http() {
        use super::super::mutation_coordinator::RepositoryMutationCoordinator;
        use crate::infrastructure::git_runner::GitCommandRunner;
        let repo = conflict_versions(
            "file.txt",
            Some(b"base\n"),
            Some(b"ours\n"),
            Some(b"theirs\n"),
        );
        let coordinator = RepositoryMutationCoordinator::default();
        let conflicts = ConflictService::new(GitCommandRunner::default(), coordinator.clone());
        let token = conflicts
            .detail(repo.path(), "file.txt")
            .await
            .unwrap()
            .token;
        let guard = coordinator.write(repo.path()).await;
        let ai = service();
        let run = uuid::Uuid::new_v4().to_string();
        let (url, requests) = recording_server(vec![(Duration::ZERO, response())]).await;
        let provider = settings(url);
        let starting = ai.start_conflict_suggestion(
            &run,
            repo.path(),
            &provider,
            &conflicts,
            "file.txt",
            &token,
            Arc::new(RecordingSink::default()),
        );
        let cancelling = async {
            while ai.active_runs.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
            // All task types share the same registry, even while preparation waits for a lock.
            let other = ai
                .start_commit_message(
                    &uuid::Uuid::new_v4().to_string(),
                    repo.path(),
                    &provider,
                    Arc::new(RecordingSink::default()),
                )
                .await
                .unwrap_err();
            assert_eq!(other.code, ErrorCode::AiConfiguration);
            ai.cancel(&run).await.unwrap();
        };
        let (result, ()) = tokio::join!(starting, cancelling);
        assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
        assert!(ai.active_runs.lock().await.is_empty());
        assert!(requests.lock().unwrap().is_empty());
        drop(guard);
    }

    #[tokio::test]
    async fn ai_conflict_total_deadline_includes_preparation_lock_wait() {
        use super::super::mutation_coordinator::RepositoryMutationCoordinator;
        use crate::infrastructure::git_runner::GitCommandRunner;
        let repo = conflict_versions(
            "file.txt",
            Some(b"base\n"),
            Some(b"ours\n"),
            Some(b"theirs\n"),
        );
        let coordinator = RepositoryMutationCoordinator::default();
        let conflicts = ConflictService::new(GitCommandRunner::default(), coordinator.clone());
        let token = conflicts
            .detail(repo.path(), "file.txt")
            .await
            .unwrap()
            .token;
        let _guard = coordinator.write(repo.path()).await;
        let mut ai = service();
        ai.conflict_timeout = Duration::from_millis(50);
        let error = ai
            .start_conflict_suggestion(
                &uuid::Uuid::new_v4().to_string(),
                repo.path(),
                &settings("http://127.0.0.1:9".into()),
                &conflicts,
                "file.txt",
                &token,
                Arc::new(RecordingSink::default()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AiTimeout);
        assert!(ai.active_runs.lock().await.is_empty());
    }

    #[tokio::test]
    async fn ai_conflict_http_cancellation_releases_read_lock_and_allows_retry() {
        use super::super::mutation_coordinator::RepositoryMutationCoordinator;
        use crate::infrastructure::git_runner::GitCommandRunner;
        let repo = conflict_versions(
            "file.txt",
            Some(b"base\n"),
            Some(b"ours\n"),
            Some(b"theirs\n"),
        );
        let coordinator = RepositoryMutationCoordinator::default();
        let conflicts = ConflictService::new(GitCommandRunner::default(), coordinator.clone());
        let detail = conflicts.detail(repo.path(), "file.txt").await.unwrap();
        let (url, requests) = recording_server(vec![(Duration::from_secs(5), response())]).await;
        let ai = service();
        let sink = RecordingSink::default();
        let run = uuid::Uuid::new_v4().to_string();
        ai.start_conflict_suggestion(
            &run,
            repo.path(),
            &settings(url),
            &conflicts,
            "file.txt",
            &detail.token,
            Arc::new(sink.clone()),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while requests.lock().unwrap().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let guard = tokio::time::timeout(Duration::from_secs(1), coordinator.write(repo.path()))
            .await
            .unwrap();
        ai.cancel(&run).await.unwrap();
        sink.wait_for_terminal().await;
        assert!(matches!(
            sink.events().last().unwrap().event,
            AiRunEventData::Cancelled { .. }
        ));
        assert!(ai.active_runs.lock().await.is_empty());
        drop(guard);
        let (url, _) = recording_server(vec![(Duration::ZERO, response())]).await;
        let retry_sink = RecordingSink::default();
        ai.start_conflict_suggestion(
            &uuid::Uuid::new_v4().to_string(),
            repo.path(),
            &settings(url),
            &conflicts,
            "file.txt",
            &detail.token,
            Arc::new(retry_sink.clone()),
        )
        .await
        .unwrap();
        retry_sink.wait_for_terminal().await;
        assert!(matches!(
            retry_sink.events().last().unwrap().event,
            AiRunEventData::ConflictSuggestionCompleted { .. }
        ));
    }

    #[tokio::test]
    async fn ai_conflict_http_validates_complete_output_and_transport_timeout() {
        let repo = conflict_versions(
            "file.txt",
            Some(b"base\n"),
            Some(b"ours\n"),
            Some(b"theirs\n"),
        );
        let conflicts = ConflictService::default();
        let token = conflicts
            .detail(repo.path(), "file.txt")
            .await
            .unwrap()
            .token;
        let advice = serde_json::json!({"kind":"adviceOnly", "summary":"review", "explanation":"missing", "resolvedText":null, "risks":[], "contextMissing":["callers"]}).to_string();
        let too_big = serde_json::json!({"kind":"text", "summary":"review", "explanation":"missing", "resolvedText":"a".repeat(64*1024+1), "risks":[], "contextMissing":[]}).to_string();
        for (response, delay, expected) in [
            (advice, Duration::ZERO, None),
            (
                "{bad json".into(),
                Duration::ZERO,
                Some(ErrorCode::AiInvalidResponse),
            ),
            (too_big, Duration::ZERO, Some(ErrorCode::AiInvalidResponse)),
            (
                response(),
                Duration::from_secs(2),
                Some(ErrorCode::AiTimeout),
            ),
        ] {
            let (url, _) = recording_server(vec![(delay, response)]).await;
            let ai = AiService::new(
                AiContextBuilder::default(),
                AiHttpClient::with_timeout(Duration::from_millis(500)),
            );
            let sink = RecordingSink::default();
            ai.start_conflict_suggestion(
                &uuid::Uuid::new_v4().to_string(),
                repo.path(),
                &settings(url),
                &conflicts,
                "file.txt",
                &token,
                Arc::new(sink.clone()),
            )
            .await
            .unwrap();
            sink.wait_for_terminal().await;
            let events = sink.events();
            match (&events.last().unwrap().event, expected) {
                (AiRunEventData::Failed { error }, Some(code)) => assert_eq!(error.code, code),
                (AiRunEventData::ConflictSuggestionCompleted { result }, None) => {
                    assert!(result.resolved_text.is_none())
                }
                other => panic!("unexpected terminal: {other:?}"),
            }
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.event,
                        AiRunEventData::Failed { .. }
                            | AiRunEventData::Cancelled { .. }
                            | AiRunEventData::ConflictSuggestionCompleted { .. }
                    ))
                    .count(),
                1
            );
            assert!(ai.active_runs.lock().await.is_empty());
        }
    }
}
