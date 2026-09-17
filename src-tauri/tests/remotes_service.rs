use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hq_git_lib::application::remote_service::{GitRunEventSink, RemoteService};
use hq_git_lib::domain::error::ErrorCode;
use hq_git_lib::domain::remotes::{
    FetchRequest, ForceWithLease, GitRunEvent, GitRunEventKind, PullRequest, PushRequest,
    RemoteOperationResult,
};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use tempfile::TempDir;
use tokio::sync::Notify;
use tokio::time::timeout;

async fn run_git(root: &Path, args: &[&str]) {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap();
}

async fn git_stdout(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
        .trim()
        .to_owned()
}

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<GitRunEvent>>>,
    changed: Arc<Notify>,
}

impl GitRunEventSink for RecordingSink {
    fn emit(&self, event: GitRunEvent) {
        self.events.lock().unwrap().push(event);
        self.changed.notify_waiters();
    }
}

impl RecordingSink {
    async fn terminal(&self, run_id: &str) -> GitRunEventKind {
        timeout(Duration::from_secs(10), async {
            loop {
                let notified = self.changed.notified();
                if let Some(event) = self
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|event| event.run_id == run_id && event.event.is_terminal())
                    .cloned()
                {
                    return event.event;
                }
                notified.await;
            }
        })
        .await
        .unwrap()
    }
}

fn completed(event: GitRunEventKind) -> RemoteOperationResult {
    let GitRunEventKind::Completed { result } = event else {
        panic!("expected completed event, got {event:?}");
    };
    result
}

fn conflicted(event: GitRunEventKind) -> RemoteOperationResult {
    let GitRunEventKind::Conflicted { result } = event else {
        panic!("expected conflicted event, got {event:?}");
    };
    result
}

async fn committed_repository(root: &Path) {
    run_git(root, &["init", "-b", "main"]).await;
    run_git(root, &["config", "user.name", "HQ Test"]).await;
    run_git(root, &["config", "user.email", "hq@example.test"]).await;
    run_git(root, &["config", "core.autocrlf", "false"]).await;
    std::fs::write(root.join("README.md"), "base\n").unwrap();
    run_git(root, &["add", "README.md"]).await;
    run_git(root, &["commit", "-m", "initial"]).await;
}

struct RemoteFixture {
    _directory: TempDir,
    worktree: PathBuf,
    fetch_remote: PathBuf,
    push_remote: PathBuf,
}

struct SyncFixture {
    _directory: TempDir,
    worktree: PathBuf,
    remote_head: String,
}

async fn local_with_bare_remotes() -> RemoteFixture {
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("工作区");
    let fetch_remote = directory.path().join("远程 仓库.git");
    let push_remote = directory.path().join("推送 仓库.git");
    std::fs::create_dir(&worktree).unwrap();
    run_git(
        directory.path(),
        &["init", "--bare", fetch_remote.to_str().unwrap()],
    )
    .await;
    run_git(
        directory.path(),
        &["init", "--bare", push_remote.to_str().unwrap()],
    )
    .await;
    committed_repository(&worktree).await;
    run_git(
        &worktree,
        &["remote", "add", "origin", fetch_remote.to_str().unwrap()],
    )
    .await;
    run_git(&worktree, &["push", "-u", "origin", "main"]).await;
    run_git(
        &worktree,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            push_remote.to_str().unwrap(),
        ],
    )
    .await;
    run_git(
        &worktree,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )
    .await;
    std::fs::write(worktree.join("README.md"), "base\nlocal\n").unwrap();
    run_git(&worktree, &["commit", "-am", "local ahead"]).await;

    RemoteFixture {
        _directory: directory,
        worktree,
        fetch_remote,
        push_remote,
    }
}

async fn remote_advanced_fixture() -> SyncFixture {
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("同步 工作区");
    let peer = directory.path().join("远程作者");
    let remote = directory.path().join("origin.git");
    std::fs::create_dir(&worktree).unwrap();
    run_git(
        directory.path(),
        &["init", "--bare", remote.to_str().unwrap()],
    )
    .await;
    committed_repository(&worktree).await;
    run_git(
        &worktree,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    )
    .await;
    run_git(&worktree, &["push", "-u", "origin", "main"]).await;
    run_git(
        directory.path(),
        &[
            "clone",
            "-b",
            "main",
            remote.to_str().unwrap(),
            peer.to_str().unwrap(),
        ],
    )
    .await;
    run_git(&peer, &["config", "user.name", "Remote Test"]).await;
    run_git(&peer, &["config", "user.email", "remote@example.test"]).await;
    run_git(&peer, &["config", "core.autocrlf", "false"]).await;
    std::fs::write(peer.join("README.md"), "base\nremote\n").unwrap();
    run_git(&peer, &["commit", "-am", "remote change"]).await;
    run_git(&peer, &["push", "origin", "main"]).await;
    let remote_head = git_stdout(&peer, &["rev-parse", "HEAD"]).await;

    SyncFixture {
        _directory: directory,
        worktree,
        remote_head,
    }
}

#[tokio::test]
async fn remotes_snapshot_lists_urls_branches_tracking_oids_and_divergence() {
    let fixture = local_with_bare_remotes().await;

    let snapshot = RemoteService::default()
        .snapshot(&fixture.worktree)
        .await
        .unwrap();

    assert_eq!(snapshot.remotes.len(), 1);
    let origin = &snapshot.remotes[0];
    assert_eq!(origin.name, "origin");
    assert_eq!(PathBuf::from(&origin.fetch_url), fixture.fetch_remote);
    assert_eq!(PathBuf::from(&origin.push_url), fixture.push_remote);
    assert_eq!(origin.branches.len(), 1);
    let main = &origin.branches[0];
    assert_eq!(main.name, "main");
    assert_eq!(main.full_name, "refs/remotes/origin/main");
    assert_eq!(main.object_id.len(), 40);
    assert_eq!(main.tracking_local.as_deref(), Some("main"));
    assert_eq!((main.ahead, main.behind), (Some(1), Some(0)));
}

#[tokio::test]
async fn snapshot_supports_no_remotes_detached_head_and_missing_tracking() {
    let empty = tempfile::tempdir().unwrap();
    committed_repository(empty.path()).await;
    let no_remotes = RemoteService::default()
        .snapshot(empty.path())
        .await
        .unwrap();
    assert!(no_remotes.remotes.is_empty());

    let fixture = local_with_bare_remotes().await;
    run_git(
        &fixture.worktree,
        &["config", "--unset", "branch.main.remote"],
    )
    .await;
    run_git(
        &fixture.worktree,
        &["config", "--unset", "branch.main.merge"],
    )
    .await;
    run_git(&fixture.worktree, &["switch", "--detach", "HEAD"]).await;

    let detached = RemoteService::default()
        .snapshot(&fixture.worktree)
        .await
        .unwrap();
    let main = &detached.remotes[0].branches[0];
    assert_eq!(main.tracking_local, None);
    assert_eq!((main.ahead, main.behind), (None, None));
}

#[tokio::test]
async fn snapshot_never_exposes_remote_url_credentials() {
    let fixture = tempfile::tempdir().unwrap();
    committed_repository(fixture.path()).await;
    run_git(
        fixture.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://user:password@example.test/repo?token=secret",
        ],
    )
    .await;

    let snapshot = RemoteService::default()
        .snapshot(fixture.path())
        .await
        .unwrap();
    let serialized = serde_json::to_string(&snapshot).unwrap();

    assert!(!serialized.contains("password"));
    assert!(!serialized.contains("secret"));
    assert!(serialized.contains("[REDACTED]"));
}

#[tokio::test]
async fn fetch_updates_remote_refs_and_returns_refreshed_snapshots() {
    let fixture = remote_advanced_fixture().await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let accepted = service
        .start_fetch(
            &fixture.worktree,
            "fetch-success",
            FetchRequest {
                remote: "origin".to_owned(),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let result = completed(sink.terminal(&accepted.run_id).await);

    assert_eq!(
        result.remotes.remotes[0].branches[0].object_id,
        fixture.remote_head
    );
    assert_eq!(
        result.refs.remote_branches[0].tip.full_hash,
        fixture.remote_head
    );
    assert_eq!(
        result.operation_state.kind,
        hq_git_lib::domain::operation::RepositoryOperationKind::None
    );
}

#[tokio::test]
async fn conflicting_pull_preserves_conflict_state_without_inventing_abort() {
    let fixture = remote_advanced_fixture().await;
    std::fs::write(fixture.worktree.join("README.md"), "base\nlocal\n").unwrap();
    run_git(
        &fixture.worktree,
        &["commit", "-am", "conflicting local change"],
    )
    .await;
    run_git(&fixture.worktree, &["config", "pull.rebase", "false"]).await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let accepted = service
        .start_pull(
            &fixture.worktree,
            "pull-conflict",
            PullRequest {
                remote: "origin".to_owned(),
                remote_branch: "main".to_owned(),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let result = conflicted(sink.terminal(&accepted.run_id).await);

    assert!(result.workspace.repository.conflict_count > 0);
    assert!(!result.operation_state.conflicts.is_empty());
    assert_eq!(result.operation_state.abort_action, None);

    let blocked = service
        .start_fetch(
            &fixture.worktree,
            "fetch-blocked",
            FetchRequest {
                remote: "origin".to_owned(),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let GitRunEventKind::Failed { error, result } = sink.terminal(&blocked.run_id).await else {
        panic!("expected failed event");
    };
    assert_eq!(error.code, ErrorCode::GitOperationInProgress);
    assert!(result.is_some());
}

#[tokio::test]
async fn unknown_remote_and_branch_fail_with_refreshed_state() {
    let fixture = remote_advanced_fixture().await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let unknown_remote = service
        .start_fetch(
            &fixture.worktree,
            "unknown-remote",
            FetchRequest {
                remote: "--all".to_owned(),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let GitRunEventKind::Failed { error, result } = sink.terminal(&unknown_remote.run_id).await
    else {
        panic!("expected failed event");
    };
    assert_eq!(error.code, ErrorCode::BranchUnavailable);
    assert!(result.is_some());

    let unknown_branch = service
        .start_pull(
            &fixture.worktree,
            "unknown-branch",
            PullRequest {
                remote: "origin".to_owned(),
                remote_branch: "--upload-pack=evil".to_owned(),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let GitRunEventKind::Failed { error, result } = sink.terminal(&unknown_branch.run_id).await
    else {
        panic!("expected failed event");
    };
    assert_eq!(error.code, ErrorCode::BranchUnavailable);
    assert!(result.is_some());
}

#[tokio::test]
async fn cancellation_is_terminal_and_returns_refreshed_state() {
    let fixture = remote_advanced_fixture().await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let accepted = service
        .start_fetch(
            &fixture.worktree,
            "fetch-cancelled",
            FetchRequest {
                remote: "origin".to_owned(),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    service.cancel(&accepted.run_id).await.unwrap();

    let GitRunEventKind::Cancelled { result } = sink.terminal(&accepted.run_id).await else {
        panic!("expected cancelled event");
    };
    assert_eq!(
        result.workspace.repository.root_path,
        fixture.worktree.canonicalize().unwrap()
    );
}

#[tokio::test]
async fn pull_respects_configured_reconciliation_and_refreshes_failure() {
    let fixture = remote_advanced_fixture().await;
    std::fs::write(fixture.worktree.join("local.txt"), "local\n").unwrap();
    run_git(&fixture.worktree, &["add", "local.txt"]).await;
    run_git(&fixture.worktree, &["commit", "-m", "local change"]).await;
    run_git(&fixture.worktree, &["config", "pull.ff", "only"]).await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let accepted = service
        .start_pull(
            &fixture.worktree,
            "pull-ff-only",
            PullRequest {
                remote: "origin".to_owned(),
                remote_branch: "main".to_owned(),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let GitRunEventKind::Failed {
        error,
        result: Some(result),
    } = sink.terminal(&accepted.run_id).await
    else {
        panic!("expected failed event with refreshed state");
    };

    assert_eq!(error.code, ErrorCode::GitCommandFailed);
    assert_eq!(
        result.remotes.remotes[0].branches[0].object_id,
        fixture.remote_head
    );
    assert!(fixture.worktree.join("local.txt").exists());
}

#[tokio::test]
async fn push_can_establish_explicit_upstream_and_create_remote_branch() {
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("推送 工作区");
    let remote = directory.path().join("empty origin.git");
    std::fs::create_dir(&worktree).unwrap();
    run_git(
        directory.path(),
        &["init", "--bare", remote.to_str().unwrap()],
    )
    .await;
    committed_repository(&worktree).await;
    run_git(
        &worktree,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    )
    .await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let accepted = service
        .start_push(
            &worktree,
            "push-upstream",
            PushRequest {
                remote: "origin".to_owned(),
                local_branch: "main".to_owned(),
                remote_branch: "release/main".to_owned(),
                establish_upstream: true,
                force_with_lease: None,
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let result = completed(sink.terminal(&accepted.run_id).await);

    assert_eq!(
        result.workspace.repository.upstream.unwrap().name,
        "origin/release/main"
    );
    assert_eq!(
        git_stdout(&remote, &["rev-parse", "refs/heads/release/main"]).await,
        git_stdout(&worktree, &["rev-parse", "refs/heads/main"]).await
    );
}

#[tokio::test]
async fn push_uses_the_selected_local_branch_without_switching() {
    let fixture = remote_advanced_fixture().await;
    run_git(&fixture.worktree, &["switch", "-c", "topic"]).await;
    std::fs::write(fixture.worktree.join("topic.txt"), "topic\n").unwrap();
    run_git(&fixture.worktree, &["add", "topic.txt"]).await;
    run_git(&fixture.worktree, &["commit", "-m", "topic"]).await;
    run_git(&fixture.worktree, &["switch", "main"]).await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let accepted = service
        .start_push(
            &fixture.worktree,
            "push-topic",
            PushRequest {
                remote: "origin".to_owned(),
                local_branch: "topic".to_owned(),
                remote_branch: "selected/topic".to_owned(),
                establish_upstream: false,
                force_with_lease: None,
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let result = completed(sink.terminal(&accepted.run_id).await);

    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
    assert_eq!(
        git_stdout(&fixture.worktree, &["rev-parse", "refs/heads/topic"]).await,
        git_stdout(
            &fixture.worktree,
            &["rev-parse", "refs/remotes/origin/selected/topic"]
        )
        .await
    );
}

#[tokio::test]
async fn ordinary_push_maps_non_fast_forward_and_refreshes_state() {
    let fixture = remote_advanced_fixture().await;
    std::fs::write(fixture.worktree.join("local.txt"), "local\n").unwrap();
    run_git(&fixture.worktree, &["add", "local.txt"]).await;
    run_git(&fixture.worktree, &["commit", "-m", "local divergence"]).await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let accepted = service
        .start_push(
            &fixture.worktree,
            "push-rejected",
            PushRequest {
                remote: "origin".to_owned(),
                local_branch: "main".to_owned(),
                remote_branch: "main".to_owned(),
                establish_upstream: false,
                force_with_lease: None,
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let GitRunEventKind::Failed {
        error,
        result: Some(result),
    } = sink.terminal(&accepted.run_id).await
    else {
        panic!("expected failed event with refreshed state");
    };

    assert_eq!(error.code, ErrorCode::NonFastForward);
    assert_eq!(
        result.workspace.repository.current_branch.as_deref(),
        Some("main")
    );
}

#[tokio::test]
async fn force_with_lease_succeeds_for_snapshot_oid_and_rejects_a_race() {
    let success = remote_advanced_fixture().await;
    run_git(&success.worktree, &["fetch", "origin"]).await;
    let expected = git_stdout(
        &success.worktree,
        &["rev-parse", "refs/remotes/origin/main"],
    )
    .await;
    std::fs::write(success.worktree.join("forced.txt"), "forced\n").unwrap();
    run_git(&success.worktree, &["add", "forced.txt"]).await;
    run_git(&success.worktree, &["commit", "-m", "force candidate"]).await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();
    let accepted = service
        .start_push(
            &success.worktree,
            "force-success",
            PushRequest {
                remote: "origin".to_owned(),
                local_branch: "main".to_owned(),
                remote_branch: "main".to_owned(),
                establish_upstream: false,
                force_with_lease: Some(ForceWithLease {
                    expected_remote_oid: expected,
                }),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let result = completed(sink.terminal(&accepted.run_id).await);
    assert_eq!(
        result.remotes.remotes[0].branches[0].object_id,
        git_stdout(&success.worktree, &["rev-parse", "refs/heads/main"]).await
    );

    let raced = remote_advanced_fixture().await;
    let stale_oid = git_stdout(&raced.worktree, &["rev-parse", "refs/remotes/origin/main"]).await;
    std::fs::write(raced.worktree.join("forced.txt"), "forced\n").unwrap();
    run_git(&raced.worktree, &["add", "forced.txt"]).await;
    run_git(&raced.worktree, &["commit", "-m", "stale force"]).await;
    let accepted = service
        .start_push(
            &raced.worktree,
            "force-race",
            PushRequest {
                remote: "origin".to_owned(),
                local_branch: "main".to_owned(),
                remote_branch: "main".to_owned(),
                establish_upstream: false,
                force_with_lease: Some(ForceWithLease {
                    expected_remote_oid: stale_oid,
                }),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let GitRunEventKind::Failed { error, .. } = sink.terminal(&accepted.run_id).await else {
        panic!("expected failed event");
    };
    assert_eq!(error.code, ErrorCode::LeaseRejected);
}

#[tokio::test]
async fn force_requires_an_existing_remote_oid_and_push_can_be_cancelled() {
    let fixture = remote_advanced_fixture().await;
    let service = RemoteService::default();
    let sink = RecordingSink::default();

    let missing = service
        .start_push(
            &fixture.worktree,
            "force-missing",
            PushRequest {
                remote: "origin".to_owned(),
                local_branch: "main".to_owned(),
                remote_branch: "new-branch".to_owned(),
                establish_upstream: false,
                force_with_lease: Some(ForceWithLease {
                    expected_remote_oid: "0".repeat(40),
                }),
            },
            sink.clone(),
        )
        .await
        .unwrap();
    let GitRunEventKind::Failed { error, result } = sink.terminal(&missing.run_id).await else {
        panic!("expected failed event");
    };
    assert_eq!(error.code, ErrorCode::LeaseRejected);
    assert!(result.is_some());

    let cancelled = service
        .start_push(
            &fixture.worktree,
            "push-cancelled",
            PushRequest {
                remote: "origin".to_owned(),
                local_branch: "main".to_owned(),
                remote_branch: "cancelled".to_owned(),
                establish_upstream: false,
                force_with_lease: None,
            },
            sink.clone(),
        )
        .await
        .unwrap();
    service.cancel(&cancelled.run_id).await.unwrap();
    let GitRunEventKind::Cancelled { result } = sink.terminal(&cancelled.run_id).await else {
        panic!("expected cancelled event");
    };
    assert_eq!(
        result.workspace.repository.root_path,
        fixture.worktree.canonicalize().unwrap()
    );
}
