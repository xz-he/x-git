use super::console_runner::*;
use crate::domain::console::{ConsoleOutcome, ConsoleStream};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn stream_caps_drain_a_large_single_line_without_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("large"), vec![b'x'; STREAM_LIMIT * 4]).unwrap();
    let mut output = Vec::new();
    let result = run(
        dir.path(),
        &[
            "diff".into(),
            "--no-index".into(),
            "--no-ext-diff".into(),
            "--no-textconv".into(),
            "--".into(),
            "NUL".into(),
            "large".into(),
        ],
        &CancellationToken::new(),
        Instant::now() + Duration::from_secs(10),
        false,
        |stream, text| output.push((stream, text)),
    )
    .await;
    assert_eq!(result.outcome, ConsoleOutcome::Failed); // diff --no-index exits 1 for differences.
    assert_eq!(result.exit_code, Some(1));
    assert!(result.stdout_truncated);
    assert!(!result.stderr_truncated);
    assert!(output.iter().all(|(_, text)| text.len() <= READ_CHUNK));
    assert!(output.iter().map(|(_, text)| text.len()).sum::<usize>() <= STREAM_LIMIT);
}

#[tokio::test]
async fn cancellation_reaps_the_live_process_and_timeout_reclaims_pipes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("large"), b"a\n".repeat(STREAM_LIMIT * 8)).unwrap();
    let token = CancellationToken::new();
    let on_output = token.clone();
    let result = run(
        dir.path(),
        &[
            "diff".into(),
            "--no-index".into(),
            "--".into(),
            "NUL".into(),
            "large".into(),
        ],
        &token,
        Instant::now() + Duration::from_secs(10),
        false,
        move |_, _| on_output.cancel(),
    )
    .await;
    assert_eq!(result.outcome, ConsoleOutcome::Cancelled);
    let result = run(
        dir.path(),
        &["status".into()],
        &CancellationToken::new(),
        Instant::now(),
        false,
        |_, _| panic!("expired run emitted output"),
    )
    .await;
    assert_eq!(result.outcome, ConsoleOutcome::TimedOut);
    assert_eq!(result.exit_code, None);
}

#[tokio::test]
async fn deadline_kills_a_live_child_after_output_has_started() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("large"), b"a\n".repeat(STREAM_LIMIT * 8)).unwrap();
    let mut saw_output = false;
    let result = run(
        dir.path(),
        &[
            "diff".into(),
            "--no-index".into(),
            "--".into(),
            "NUL".into(),
            "large".into(),
        ],
        &CancellationToken::new(),
        Instant::now() + Duration::from_secs(5),
        false,
        |_, _| {
            if !saw_output {
                saw_output = true;
                std::thread::sleep(Duration::from_millis(5100));
            }
        },
    )
    .await;
    assert!(
        saw_output,
        "the deadline must interrupt a child that has started producing output"
    );
    assert_eq!(result.outcome, ConsoleOutcome::TimedOut);
    std::fs::rename(dir.path().join("large"), dir.path().join("reclaimed")).unwrap();
}

#[test]
fn fixed_program_environment_and_optional_locks() {
    let command = command(std::path::Path::new("."), &["status".into()]);
    let standard = command.as_std();
    assert_eq!(standard.get_program(), "git");
    assert!(
        !standard
            .get_args()
            .any(|argument| argument == "--no-optional-locks")
    );
    let env: std::collections::HashMap<_, _> = standard.get_envs().collect();
    assert_eq!(
        env.get(std::ffi::OsStr::new("GIT_OPTIONAL_LOCKS"))
            .unwrap()
            .unwrap(),
        "0"
    );
    assert_eq!(
        env.get(std::ffi::OsStr::new("GIT_NO_LAZY_FETCH"))
            .unwrap()
            .unwrap(),
        "1"
    );
}

#[tokio::test]
async fn reader_keeps_streams_separate_and_bounds_queue_backpressure() {
    use tokio::io::AsyncWriteExt;
    let (mut writer, reader) = tokio::io::duplex(READ_CHUNK);
    let writer_task = tokio::spawn(async move {
        writer
            .write_all(&vec![b'z'; STREAM_LIMIT + 100])
            .await
            .unwrap();
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let truncated = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader_task = tokio::spawn(read_pipe(
        reader,
        ConsoleStream::Stdout,
        tx.clone(),
        truncated.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        while tx.capacity() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !writer_task.is_finished(),
        "bounded queue applies backpressure"
    );
    drop(tx);
    let mut retained = 0;
    while let Some((stream, bytes)) = rx.recv().await {
        assert_eq!(stream, ConsoleStream::Stdout);
        assert!(bytes.len() <= READ_CHUNK);
        retained += bytes.len();
    }
    reader_task.await.unwrap().unwrap();
    writer_task.await.unwrap();
    assert_eq!(retained, STREAM_LIMIT);
    assert!(truncated.load(std::sync::atomic::Ordering::Relaxed));
    let sink = CancellationToken::new();
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let copy = captured.clone();
    let dir = tempfile::tempdir().unwrap();
    let result = run(
        dir.path(),
        &["rev-parse".into(), "--verify".into(), "missing".into()],
        &sink,
        Instant::now() + Duration::from_secs(10),
        false,
        |stream, text| {
            assert_eq!(stream, ConsoleStream::Stderr);
            copy.lock().unwrap().extend(text.bytes());
        },
    )
    .await;
    assert_eq!(result.outcome, ConsoleOutcome::Failed);
    assert!(!captured.lock().unwrap().is_empty());
}
