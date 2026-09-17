use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::process::Command;
use tokio::sync::mpsc;
#[derive(Clone)]
struct Sink(mpsc::UnboundedSender<TerminalEvent>);
impl TerminalEventSink for Sink {
    fn emit(&self, event: TerminalEvent) {
        let _ = self.0.send(event);
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}
fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "-q"]);
    git(directory.path(), &["config", "user.name", "Terminal Test"]);
    git(
        directory.path(),
        &["config", "user.email", "terminal@example.invalid"],
    );
    directory
}
async fn finish(
    service: &TerminalService,
    root: &Path,
    id: &str,
    rx: &mut mpsc::UnboundedReceiver<TerminalEvent>,
) -> (String, bool, Option<BackendError>) {
    let mut output = Vec::new();
    let mut sequence = 0;
    loop {
        let event = next_event(service, root, id, rx).await;
        assert!(event.sequence > sequence);
        sequence = event.sequence;
        match event.event {
            TerminalEventKind::Output { data } => {
                let bytes = STANDARD.decode(data).unwrap();
                assert!(bytes.len() <= 8192);
                output.extend(bytes);
                service.ack(root, id, event.sequence).unwrap();
            }
            TerminalEventKind::Exited {
                cancelled, error, ..
            } => {
                return (
                    String::from_utf8_lossy(&output).into_owned(),
                    cancelled,
                    error,
                );
            }
            _ => {}
        }
    }
}
async fn next_event(
    service: &TerminalService,
    root: &Path,
    id: &str,
    rx: &mut mpsc::UnboundedReceiver<TerminalEvent>,
) -> TerminalEvent {
    match tokio::time::timeout(Duration::from_secs(20), rx.recv()).await {
        Ok(Some(event)) => event,
        value => {
            let _ = service.terminate(root, id).await;
            panic!("terminal {id} timed out or closed: {value:?}");
        }
    }
}
#[tokio::test]
async fn terminal_real_pty_executes_writes_aliases_and_quoted_unicode() {
    let directory = repository();
    let root = directory.path();
    let service = TerminalService::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    service
        .start(
            root,
            "write",
            "git config terminal.value '中文 value'",
            80,
            24,
            Sink(tx.clone()),
        )
        .await
        .unwrap();
    let (_, cancelled, error) = finish(&service, root, "write", &mut rx).await;
    assert!(!cancelled);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(
        git(root, &["config", "terminal.value"]).trim(),
        "中文 value"
    );
    git(
        root,
        &[
            "config",
            "alias.terminaltest",
            "config --get terminal.value",
        ],
    );
    service
        .start(root, "alias", "git terminaltest", 100, 30, Sink(tx))
        .await
        .unwrap();
    assert!(
        finish(&service, root, "alias", &mut rx)
            .await
            .0
            .contains("中文 value")
    );
}
#[tokio::test]
async fn terminal_pending_lock_can_be_cancelled_and_ids_cannot_be_reused() {
    let directory = repository();
    let root = directory.path();
    let coordinator = RepositoryMutationCoordinator::default();
    let guard = coordinator.write(root).await;
    let service = TerminalService::new(coordinator);
    let (tx, mut rx) = mpsc::unbounded_channel();
    service
        .start(root, "waiting", "git status", 80, 24, Sink(tx.clone()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), service.terminate(root, "waiting"))
        .await
        .unwrap()
        .unwrap();
    assert!(finish(&service, root, "waiting", &mut rx).await.1);
    assert!(
        service
            .start(root, "waiting", "git status", 80, 24, Sink(tx.clone()))
            .await
            .is_err()
    );
    service.terminate(root, "before-start").await.unwrap();
    assert!(
        service
            .start(root, "before-start", "git status", 80, 24, Sink(tx))
            .await
            .is_err()
    );
    drop(guard);
}
#[tokio::test]
async fn terminal_real_pty_add_patch_accepts_input_and_resize() {
    let directory = repository();
    let root = directory.path();
    std::fs::write(root.join("file.txt"), "before\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "initial"]);
    std::fs::write(root.join("file.txt"), "after\n").unwrap();
    let service = TerminalService::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    service
        .start(root, "patch", "git add -p", 80, 24, Sink(tx))
        .await
        .unwrap();
    let mut output = String::new();
    loop {
        let event = next_event(&service, root, "patch", &mut rx).await;
        if let TerminalEventKind::Output { data } = event.event {
            output.push_str(&String::from_utf8_lossy(&STANDARD.decode(data).unwrap()));
            service.ack(root, "patch", event.sequence).unwrap();
            if output.contains("Stage this hunk") {
                break;
            }
        }
    }
    service.resize(root, "patch", 120, 40).await.unwrap();
    service
        .write(root, "patch", &STANDARD.encode(b"y\r"))
        .await
        .unwrap();
    assert!(finish(&service, root, "patch", &mut rx).await.2.is_none());
    assert!(git(root, &["diff", "--cached"]).contains("+after"));
}

#[tokio::test]
async fn terminal_real_pty_backpressure_cancel_cleans_helpers_and_releases_lock() {
    let directory = repository();
    let root = directory.path();
    git(
        root,
        &[
            "config",
            "alias.flood",
            "!while :; do printf 'terminal-output-0123456789\\n'; done",
        ],
    );
    let coordinator = RepositoryMutationCoordinator::default();
    let service = TerminalService::new(coordinator.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    service
        .start(root, "flood", "git flood", 80, 24, Sink(tx.clone()))
        .await
        .unwrap();
    let mut chunks = 0;
    while chunks < 32 {
        let event = next_event(&service, root, "flood", &mut rx).await;
        if matches!(event.event, TerminalEventKind::Output { .. }) {
            chunks += 1;
        }
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(150), rx.recv())
            .await
            .is_err(),
        "backend exceeded 32 output credits"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), coordinator.read(root))
            .await
            .is_err()
    );
    assert!(
        service
            .start(root, "duplicate-active", "git status", 80, 24, Sink(tx))
            .await
            .is_err()
    );
    assert!(service.ack(root, "flood", u64::MAX).is_err());
    assert!(service.write(root, "stale", "eA==").await.is_err());
    tokio::time::timeout(Duration::from_secs(12), service.terminate(root, "flood"))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), coordinator.read(root))
        .await
        .unwrap();
    let event = rx.recv().await.unwrap();
    assert!(matches!(
        event.event,
        TerminalEventKind::Exited {
            cancelled: true,
            ..
        }
    ));
}

#[tokio::test]
async fn terminal_real_pty_pager_and_ctrl_c_are_interactive() {
    let directory = repository();
    let root = directory.path();
    std::fs::write(
        root.join("long.txt"),
        (0..200)
            .map(|line| format!("pager line {line}\n"))
            .collect::<String>(),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "pager"]);
    let service = TerminalService::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    service
        .start(
            root,
            "pager",
            "git -c core.pager=less --paginate show HEAD:long.txt",
            80,
            12,
            Sink(tx.clone()),
        )
        .await
        .unwrap();
    loop {
        let event = next_event(&service, root, "pager", &mut rx).await;
        if let TerminalEventKind::Output { data } = event.event {
            service.ack(root, "pager", event.sequence).unwrap();
            if String::from_utf8_lossy(&STANDARD.decode(data).unwrap()).contains("pager line") {
                break;
            }
        }
    }
    service
        .write(root, "pager", &STANDARD.encode(b"q"))
        .await
        .unwrap();
    assert!(finish(&service, root, "pager", &mut rx).await.2.is_none());
    git(root, &["config", "alias.wait", "!echo WAITING; sleep 30"]);
    service
        .start(root, "interrupt", "git wait", 80, 24, Sink(tx))
        .await
        .unwrap();
    loop {
        let event = next_event(&service, root, "interrupt", &mut rx).await;
        if let TerminalEventKind::Output { data } = event.event {
            service.ack(root, "interrupt", event.sequence).unwrap();
            if String::from_utf8_lossy(&STANDARD.decode(data).unwrap()).contains("WAITING") {
                break;
            }
        }
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    service
        .write(root, "interrupt", &STANDARD.encode(b"\x03"))
        .await
        .unwrap();
    assert!(!finish(&service, root, "interrupt", &mut rx).await.1);
}

#[tokio::test]
async fn terminal_real_pty_pushes_to_local_bare_remote() {
    let directory = repository();
    let root = directory.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-q"]);
    git(root, &["commit", "--allow-empty", "-qm", "terminal push"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let service = TerminalService::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    service
        .start(
            root,
            "push",
            "git push origin HEAD:refs/heads/terminal",
            80,
            24,
            Sink(tx),
        )
        .await
        .unwrap();
    assert!(finish(&service, root, "push", &mut rx).await.2.is_none());
    assert_eq!(
        git(root, &["rev-parse", "HEAD"]).trim(),
        git(remote.path(), &["rev-parse", "refs/heads/terminal"]).trim()
    );
}

#[tokio::test]
async fn terminal_real_pty_vim_editor_creates_commit() {
    let directory = repository();
    let root = directory.path();
    let git_path = terminal_pty::git_executable().unwrap();
    let vim = git_path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("usr/bin/vim.exe");
    assert!(
        vim.is_file(),
        "Git for Windows Vim must be installed for this integration fixture: {vim:?}"
    );
    git(
        root,
        &[
            "config",
            "core.editor",
            &format!(
                "'{}' -u NONE -n -i NONE",
                vim.to_string_lossy().replace('\\', "/")
            ),
        ],
    );
    std::fs::write(root.join("editor.txt"), "editor integration\n").unwrap();
    git(root, &["add", "."]);
    let service = TerminalService::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    service
        .start(root, "editor", "git commit", 100, 30, Sink(tx))
        .await
        .unwrap();
    let mut output = String::new();
    loop {
        let event = next_event(&service, root, "editor", &mut rx).await;
        if let TerminalEventKind::Output { data } = event.event {
            let bytes = STANDARD.decode(data).unwrap();
            output.push_str(&String::from_utf8_lossy(&bytes));
            service.ack(root, "editor", event.sequence).unwrap();
            if output.contains("COMMIT_EDITMSG") {
                break;
            }
        }
    }
    service
        .write(
            root,
            "editor",
            &STANDARD.encode(b"iVim terminal commit\x1b:wq\r"),
        )
        .await
        .unwrap();
    let (_, _, error) = finish(&service, root, "editor", &mut rx).await;
    assert!(error.is_none(), "{error:?}");
    assert_eq!(
        git(root, &["log", "-1", "--format=%s"]).trim(),
        "Vim terminal commit"
    );
}
