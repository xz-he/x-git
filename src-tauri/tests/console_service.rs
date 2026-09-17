use hq_git_lib::application::console_policy::parse_command;
use hq_git_lib::application::{
    console_service::{ConsoleEventSink, ConsoleService},
    mutation_coordinator::RepositoryMutationCoordinator,
};
use hq_git_lib::domain::{
    console::{ConsoleEvent, ConsoleEventKind, ConsoleOutcome},
    error::ErrorCode,
};
use hq_git_lib::infrastructure::git_runner::GitCommandRunner;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<ConsoleEvent>>>);
impl ConsoleEventSink for Events {
    fn emit(&self, event: ConsoleEvent) {
        self.0.lock().unwrap().push(event);
    }
}
impl Events {
    async fn terminal(&self, id: &str) -> ConsoleEvent {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Some(event) = self
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|e| {
                        e.run_id == id && matches!(e.event, ConsoleEventKind::Terminal { .. })
                    })
                    .cloned()
                {
                    return event;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("terminal event")
    }
}
async fn git(root: &Path, args: &[&str]) -> String {
    GitCommandRunner::default()
        .run(Some(root), args)
        .await
        .unwrap()
        .stdout
}
async fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-b", "main"]).await;
    git(p, &["config", "user.name", "Console Test"]).await;
    git(p, &["config", "user.email", "console@example.test"]).await;
    std::fs::write(p.join("中文 空格.txt"), "base\n\nlast\n").unwrap();
    std::fs::write(p.join("deleted.txt"), "deleted\n").unwrap();
    git(p, &["add", "."]).await;
    git(p, &["commit", "-m", "base"]).await;
    dir
}

#[tokio::test]
async fn eight_queries_leave_head_refs_index_and_files_unchanged() {
    let dir = repository().await;
    let p = dir.path();
    git(p, &["tag", "v1"]).await;
    git(
        p,
        &[
            "remote",
            "add",
            "origin",
            "https://user:secret@example.test/repo?token=secret",
        ],
    )
    .await;
    std::fs::write(p.join("中文 空格.txt"), "changed\n\nlast\n").unwrap();
    std::fs::remove_file(p.join("deleted.txt")).unwrap();
    let head = git(p, &["rev-parse", "HEAD"]).await;
    let refs = git(p, &["show-ref"]).await;
    let index = std::fs::read(p.join(".git/index")).unwrap();
    let content = std::fs::read(p.join("中文 空格.txt")).unwrap();
    let service = ConsoleService::default();
    let events = Events::default();
    for (i, command) in [
        "git status --short --branch",
        "git log --oneline -n 1",
        "git diff -- '中文 空格.txt' deleted.txt",
        "git show HEAD --stat",
        "git branch -vv",
        "git tag --list",
        "git remote -v",
        "git stash list",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("run-{i}");
        service
            .start(p, &id, command, events.clone())
            .await
            .unwrap();
        let terminal = events.terminal(&id).await;
        assert!(
            matches!(
                terminal.event,
                ConsoleEventKind::Terminal {
                    outcome: ConsoleOutcome::Completed,
                    exit_code: Some(0),
                    ..
                }
            ),
            "{command}: {terminal:?}"
        );
    }
    assert_eq!(git(p, &["rev-parse", "HEAD"]).await, head);
    assert_eq!(git(p, &["show-ref"]).await, refs);
    assert_eq!(std::fs::read(p.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(p.join("中文 空格.txt")).unwrap(), content);
    assert!(!p.join("deleted.txt").exists());
    assert!(!format!("{:?}", events.0.lock().unwrap()).contains("secret"));
}

#[tokio::test]
async fn cancel_waiting_read_lock_reclaims_slot_and_duplicate_ids_are_rejected() {
    let dir = repository().await;
    let p = dir.path();
    let coordinator = RepositoryMutationCoordinator::default();
    let service = ConsoleService::new(coordinator.clone());
    let events = Events::default();
    let guard = coordinator.write(p).await;
    service
        .start(p, "blocked", "git status", events.clone())
        .await
        .unwrap();
    assert_eq!(
        service
            .start(p, "second", "git status", events.clone())
            .await
            .unwrap_err()
            .code,
        ErrorCode::GitOperationInProgress
    );
    service.cancel(p, "blocked").await.unwrap();
    assert!(matches!(
        events.terminal("blocked").await.event,
        ConsoleEventKind::Terminal {
            outcome: ConsoleOutcome::Cancelled,
            ..
        }
    ));
    assert!(
        service
            .start(p, "blocked", "git status", events.clone())
            .await
            .is_err()
    );
    drop(guard);
    service
        .start(p, "next", "git status", events.clone())
        .await
        .unwrap();
    assert!(matches!(
        events.terminal("next").await.event,
        ConsoleEventKind::Terminal {
            outcome: ConsoleOutcome::Completed,
            ..
        }
    ));
    assert_eq!(
        events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(
                |e| e.run_id == "blocked" && matches!(e.event, ConsoleEventKind::Terminal { .. })
            )
            .count(),
        1
    );
}

#[tokio::test]
async fn cancel_before_start_and_wrong_root_ownership() {
    let dir = repository().await;
    let other = repository().await;
    let service = ConsoleService::default();
    let events = Events::default();
    service.cancel(dir.path(), "early").await.unwrap();
    assert_eq!(
        service
            .start(dir.path(), "early", "git status", events.clone())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(events.0.lock().unwrap().is_empty());
    let coordinator = RepositoryMutationCoordinator::default();
    let service = ConsoleService::new(coordinator.clone());
    let guard = coordinator.write(dir.path()).await;
    service
        .start(dir.path(), "owner", "git status", events.clone())
        .await
        .unwrap();
    assert!(service.cancel(other.path(), "owner").await.is_err());
    service.cancel(dir.path(), "owner").await.unwrap();
    drop(guard);
}

#[tokio::test]
async fn canonical_accepted_root_can_cancel_an_aliased_request() {
    let dir = repository().await;
    let coordinator = RepositoryMutationCoordinator::default();
    let guard = coordinator.write(dir.path()).await;
    let service = ConsoleService::new(coordinator);
    let accepted = service
        .start(
            &dir.path().join("."),
            "alias",
            "git status",
            Events::default(),
        )
        .await
        .unwrap();
    service.cancel(&accepted.root_path, "alias").await.unwrap();
    drop(guard);
}

#[tokio::test]
async fn invalid_policy_runs_no_git_and_empty_show_returns_failure() {
    let dir = tempfile::tempdir().unwrap();
    let service = ConsoleService::default();
    let events = Events::default();
    assert_eq!(
        service
            .start(dir.path(), "invalid", "git reset --hard", events.clone())
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidConsoleCommand
    );
    assert!(events.0.lock().unwrap().is_empty());
    git(dir.path(), &["init"]).await;
    service
        .start(dir.path(), "empty", "git show", events.clone())
        .await
        .unwrap();
    assert!(matches!(
        events.terminal("empty").await.event,
        ConsoleEventKind::Terminal {
            outcome: ConsoleOutcome::Failed,
            exit_code: Some(_),
            ..
        }
    ));
}

#[tokio::test]
async fn timeout_and_shutdown_wait_for_reclamation() {
    let dir = repository().await;
    let coordinator = RepositoryMutationCoordinator::default();
    let service = ConsoleService::with_timeout(coordinator.clone(), Duration::from_millis(30));
    let events = Events::default();
    let guard = coordinator.write(dir.path()).await;
    service
        .start(dir.path(), "timeout", "git status", events.clone())
        .await
        .unwrap();
    assert!(matches!(
        events.terminal("timeout").await.event,
        ConsoleEventKind::Terminal {
            outcome: ConsoleOutcome::TimedOut,
            exit_code: None,
            ..
        }
    ));
    service
        .start(dir.path(), "shutdown", "git status", events.clone())
        .await
        .unwrap();
    service.shutdown().await;
    assert!(matches!(
        events.terminal("shutdown").await.event,
        ConsoleEventKind::Terminal {
            outcome: ConsoleOutcome::Cancelled,
            ..
        }
    ));
    assert!(
        service
            .start(dir.path(), "after", "git status", events)
            .await
            .is_err()
    );
    drop(guard);
}

#[tokio::test]
async fn configured_helpers_are_disabled_and_literal_pathspecs_do_not_expand() {
    let dir = repository().await;
    let p = dir.path();
    let marker = p.join("helper-marker");
    let helper = format!(
        "echo invoked > '{}'",
        marker.display().to_string().replace('\\', "/")
    );
    for key in [
        "core.pager",
        "pager.status",
        "diff.external",
        "diff.console.textconv",
        "core.fsmonitor",
    ] {
        git(p, &["config", key, &helper]).await;
    }
    std::fs::write(p.join(".gitattributes"), "*.txt diff=console\n").unwrap();
    std::fs::write(p.join("中文 空格.txt"), "different\n").unwrap();
    let service = ConsoleService::default();
    let events = Events::default();
    for (i, command) in [
        "git status",
        "git diff",
        "git show",
        "git log",
        "git diff -- '*.txt'",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("helper-{i}");
        service
            .start(p, &id, command, events.clone())
            .await
            .unwrap();
        assert!(matches!(
            events.terminal(&id).await.event,
            ConsoleEventKind::Terminal {
                outcome: ConsoleOutcome::Completed,
                ..
            }
        ));
    }
    assert!(!marker.exists());
    assert!(
        !events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.run_id == "helper-4" && matches!(e.event, ConsoleEventKind::Output { .. }))
    );
}

#[tokio::test]
async fn configured_pretty_formats_do_not_invoke_signature_helpers() {
    let dir = repository().await;
    let p = dir.path();
    let marker = p.join("gpg-marker");
    let helper = p.join("gpg-helper.sh");
    std::fs::write(
        &helper,
        format!(
            "#!/bin/sh\nprintf invoked > '{}'\nexit 1\n",
            marker.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let original = git(p, &["cat-file", "commit", "HEAD"]).await;
    let (headers, message) = original.split_once("\n\n").unwrap();
    let signed = format!(
        "{headers}\ngpgsig -----BEGIN PGP SIGNATURE-----\n fake\n -----END PGP SIGNATURE-----\n\n{message}"
    );
    let oid = GitCommandRunner::default()
        .run_with_input(
            Some(p),
            ["hash-object", "-t", "commit", "-w", "--stdin"],
            signed.into_bytes(),
        )
        .await
        .unwrap()
        .stdout;
    git(p, &["update-ref", "refs/heads/main", oid.trim()]).await;
    git(p, &["config", "gpg.program", helper.to_str().unwrap()]).await;
    git(p, &["config", "gpg.format", "openpgp"]).await;
    git(p, &["config", "format.pretty", "%G?"]).await;
    git(p, &["log", "-1"]).await;
    assert!(
        marker.exists(),
        "positive control must invoke configured signature helper"
    );
    std::fs::remove_file(&marker).unwrap();
    let service = ConsoleService::default();
    let events = Events::default();
    for (i, command) in ["git log", "git show", "git stash list"].iter().enumerate() {
        let id = format!("pretty-{i}");
        service
            .start(p, &id, command, events.clone())
            .await
            .unwrap();
        assert!(matches!(
            events.terminal(&id).await.event,
            ConsoleEventKind::Terminal {
                outcome: ConsoleOutcome::Completed,
                ..
            }
        ));
        assert!(
            !marker.exists(),
            "{command} invoked configured signature helper"
        );
    }
}

#[tokio::test]
async fn linked_worktree_and_conflicted_index_are_readable_without_changes() {
    let dir = repository().await;
    let p = dir.path();
    let linked = tempfile::tempdir().unwrap();
    let root = linked.path().join("linked");
    git(
        p,
        &["worktree", "add", "-b", "linked", root.to_str().unwrap()],
    )
    .await;
    std::fs::write(p.join("中文 空格.txt"), "main\n").unwrap();
    git(p, &["commit", "-am", "main"]).await;
    std::fs::write(root.join("中文 空格.txt"), "linked\n").unwrap();
    git(&root, &["commit", "-am", "linked"]).await;
    let merge = GitCommandRunner::default()
        .run_allowing_failure(Some(&root), ["merge", "main"])
        .await
        .unwrap();
    assert!(!merge.is_success());
    let git_dir = git(&root, &["rev-parse", "--absolute-git-dir"]).await;
    let index_path = Path::new(git_dir.trim()).join("index");
    let index = std::fs::read(&index_path).unwrap();
    let content = std::fs::read(root.join("中文 空格.txt")).unwrap();
    let head = git(&root, &["rev-parse", "HEAD"]).await;
    let service = ConsoleService::default();
    let events = Events::default();
    for (i, command) in [
        "git status",
        "git diff",
        "git show HEAD~0",
        "git branch --all",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("conflict-{i}");
        service
            .start(&root, &id, command, events.clone())
            .await
            .unwrap();
        assert!(matches!(
            events.terminal(&id).await.event,
            ConsoleEventKind::Terminal {
                outcome: ConsoleOutcome::Completed,
                ..
            }
        ));
    }
    assert_eq!(std::fs::read(index_path).unwrap(), index);
    assert_eq!(std::fs::read(root.join("中文 空格.txt")).unwrap(), content);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]).await, head);
}

#[test]
fn accepted_queries_and_aliases() {
    for input in [
        "git status",
        "git status -s -b",
        "git log --all --oneline --graph --decorate -n 200",
        "git log --max-count=1",
        "git diff --staged --stat -- '中文 空格.txt' deleted.txt",
        "git show --name-only main~100",
        "git show HEAD^0",
        "git branch --list -a -vv",
        "git branch -r -v",
        "git tag -l",
        "git remote -v",
        "git stash list",
    ] {
        assert!(parse_command(input).is_ok(), "{input}");
    }
}

#[test]
fn unsafe_or_ambiguous_queries_are_rejected() {
    for input in [
        "",
        "git.exe status",
        "Git status",
        "git -C x status",
        "git status -sb",
        "git status -s --short",
        "git log -n 0",
        "git log -n 201",
        "git log -n +1",
        "git log -n 1 --max-count=2",
        "git diff --stat --name-only",
        "git diff --cached --staged",
        "git show HEAD~1^2",
        "git show @{-1}",
        "git show HEAD:file",
        "git show a..b",
        "git show -bad",
        "git show main.lock",
        "git show x/../y",
        "git branch --all --remotes",
        "git branch -v -vv",
        "git tag -l --list",
        "git remote add",
        "git stash",
        "git stash list -n 1",
        "git status; whoami",
        "git status\n",
        "git status\t",
        "git diff -- 'unterminated",
        "git diff -- /tmp/a",
        "git diff -- C:/a",
        "git diff -- a\\b",
        "git diff -- a//b",
        "git diff -- .",
        "git diff -- a/../b",
        "git diff -- .GiT/config",
        "git diff -- ':(top)a'",
        "git diff --",
    ] {
        assert!(parse_command(input).is_err(), "{input}");
    }
}

#[test]
fn input_size_token_and_path_boundaries() {
    assert!(parse_command(&format!("git diff -- {}", "a".repeat(4084))).is_ok());
    assert!(parse_command(&format!("git diff -- {}", "a".repeat(4085))).is_err());
    assert!(parse_command(&format!("git diff -- {}", ["a"; 32].join(" "))).is_ok());
    assert!(parse_command(&format!("git diff -- {}", ["a"; 33].join(" "))).is_err());
    assert!(parse_command(&format!("git status {}", ["a"; 64].join(" "))).is_err());
    for input in [
        "git diff -- '中文 空格'",
        "git diff -- ab' c'\" d\"",
        "git diff -- '$literal'",
        "git show refs/heads/main",
        "git show abcd",
        "git show main^100",
    ] {
        assert!(parse_command(input).is_ok(), "{input}");
    }
    for input in [
        "git show main^101",
        "git show main~",
        "git show main~+1",
        "git show @",
        "git show .hidden",
        "git show main/",
        "git show a//b",
        "git show 'a b'",
        "git show 'a?b'",
        "git show a\\b",
        "git show a.lock/b",
        "git show a.",
        "git diff -- ''",
        "git status $x",
        "git status `x",
        "git status | x",
        "git status & x",
        "git status < x",
        "git status > x",
    ] {
        assert!(parse_command(input).is_err(), "{input}");
    }
}
