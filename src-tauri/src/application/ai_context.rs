use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::application::changes_service::{ChangesService, StagedFilePatch};
use crate::domain::ai::AiContextSummary;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::settings::AppSettings;

pub const MAX_DIFF_BATCH_BYTES: usize = 48 * 1024;
const MAX_BATCHES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiContextBatch {
    pub index: usize,
    pub file_paths: Vec<String>,
    pub allowed_line_anchors: BTreeMap<String, Vec<u32>>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiFrozenContext {
    pub summary: AiContextSummary,
    pub rules: String,
    pub warnings: Vec<String>,
    pub review_batches: Vec<AiContextBatch>,
    pub commit_message_input: String,
}

#[derive(Debug, Clone)]
pub struct AiContextBuilder {
    changes: ChangesService,
}

impl Default for AiContextBuilder {
    fn default() -> Self {
        Self::new(ChangesService::default())
    }
}

impl AiContextBuilder {
    pub async fn review_snapshot(
        &self,
        root: &Path,
        source: crate::domain::ai::ReviewSource,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<super::review_snapshot::ReviewSnapshot, BackendError> {
        self.changes.review_snapshot(root, source, cancel).await
    }
    pub fn new(changes: ChangesService) -> Self {
        Self { changes }
    }

    pub async fn capture(
        &self,
        root: &Path,
        _settings: &AppSettings,
    ) -> Result<AiFrozenContext, BackendError> {
        let snapshot = self.changes.staged_patch_snapshot(root).await?;
        if snapshot.files.is_empty() {
            return Err(BackendError::new(
                ErrorCode::AiNoStagedChanges,
                "没有可供 AI 处理的已暂存变更。",
            ));
        }
        let (rules, warnings) = (String::new(), Vec::new());
        let review_batches = build_review_batches(&snapshot.files)?;
        let skipped_binary_files = snapshot
            .files
            .iter()
            .filter(|file| file.binary)
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let commit_message_input = build_commit_message_input(&snapshot.files);
        let fingerprint = staged_fingerprint(&snapshot.files);
        let summary = AiContextSummary {
            conflict: None,
            review: None,
            staged_file_count: snapshot.files.len(),
            text_file_count: snapshot.files.iter().filter(|file| !file.binary).count(),
            skipped_binary_files,
            fingerprint,
        };
        Ok(AiFrozenContext {
            summary,
            rules,
            warnings,
            review_batches,
            commit_message_input,
        })
    }
}

pub(crate) fn build_review_batches(
    files: &[StagedFilePatch],
) -> Result<Vec<AiContextBatch>, BackendError> {
    let mut batches = Vec::<AiContextBatch>::new();
    for file in files.iter().filter(|file| !file.binary) {
        for piece in split_patch(&file.patch)? {
            let anchors = super::diff_parser::parse_unified_diff(&piece)
                .into_iter()
                .flat_map(|hunk| hunk.lines)
                .filter(|line| {
                    matches!(
                        line.kind,
                        crate::domain::changes::DiffLineKind::Addition
                            | crate::domain::changes::DiffLineKind::Deletion
                    )
                })
                .filter_map(|line| line.new_line.or(line.old_line))
                .collect::<Vec<_>>();
            let piece_bytes = piece.len();
            let append_to_current = batches
                .last()
                .is_some_and(|batch| batch.text.len() + 1 + piece_bytes <= MAX_DIFF_BATCH_BYTES);
            if append_to_current {
                let batch = batches.last_mut().expect("batch exists");
                batch.text.push('\n');
                batch.text.push_str(&piece);
                if !batch.file_paths.contains(&file.path) {
                    batch.file_paths.push(file.path.clone());
                }
                batch
                    .allowed_line_anchors
                    .entry(file.path.clone())
                    .or_default()
                    .extend(anchors);
            } else {
                if batches.len() == MAX_BATCHES {
                    return Err(BackendError::new(
                        ErrorCode::AiContextTooLarge,
                        "变更超过 64 个 AI 审查批次，请缩小审查范围。",
                    ));
                }
                batches.push(AiContextBatch {
                    index: batches.len() + 1,
                    file_paths: vec![file.path.clone()],
                    allowed_line_anchors: BTreeMap::from([(file.path.clone(), anchors)]),
                    text: piece,
                });
            }
        }
    }
    for batch in &mut batches {
        for anchors in batch.allowed_line_anchors.values_mut() {
            anchors.sort_unstable();
            anchors.dedup();
        }
    }
    Ok(batches)
}

fn split_patch(patch: &str) -> Result<Vec<String>, BackendError> {
    if patch.len() <= MAX_DIFF_BATCH_BYTES {
        return Ok(vec![patch.to_owned()]);
    }
    let mut preamble = String::new();
    let mut hunks = Vec::<String>::new();
    let mut current = String::new();
    for line in patch.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            if !current.is_empty() {
                hunks.push(std::mem::take(&mut current));
            }
            current.push_str(line);
        } else if current.is_empty() {
            preamble.push_str(line);
        } else {
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        hunks.push(current);
    }
    if hunks.is_empty() {
        return Err(split_error("Diff 元数据超过 48 KiB，无法分批审查。"));
    }
    let mut pieces = Vec::with_capacity(hunks.len());
    for hunk in hunks {
        let piece = format!("{preamble}{hunk}");
        if piece.len() > MAX_DIFF_BATCH_BYTES {
            pieces.extend(split_large_hunk(&preamble, &hunk)?);
        } else {
            pieces.push(piece);
        }
    }
    Ok(pieces)
}

fn split_error(message: &str) -> BackendError {
    BackendError::new(ErrorCode::AiContextTooLarge, message)
}

/// Split only between complete diff lines; each fragment retains its file header
/// and independently correct unified-diff ranges. EOF markers stay with their line.
fn split_large_hunk(preamble: &str, hunk: &str) -> Result<Vec<String>, BackendError> {
    let invalid = || split_error("Diff 块格式无效，无法安全分批审查。");
    let (header, body) = hunk.split_once('\n').ok_or_else(invalid)?;
    let (ranges, suffix) = header
        .strip_prefix("@@ -")
        .and_then(|v| v.split_once(" @@"))
        .ok_or_else(invalid)?;
    let (old, new) = ranges.split_once(" +").ok_or_else(invalid)?;
    let parse_range = |range: &str| -> Result<(usize, usize), BackendError> {
        let (start, count) = range.split_once(',').unwrap_or((range, "1"));
        Ok((
            start.parse().map_err(|_| invalid())?,
            count.parse().map_err(|_| invalid())?,
        ))
    };
    let (old_start, old_total) = parse_range(old)?;
    let (new_start, new_total) = parse_range(new)?;
    // Cursors point at the next actual line, even for a zero-length source range.
    let mut old_cursor = old_start + usize::from(old_total == 0);
    let mut new_cursor = new_start + usize::from(new_total == 0);
    let mut old_count = 0;
    let mut new_count = 0;
    let mut content = String::new();
    let mut pieces = Vec::new();
    let render_header =
        |old_cursor: usize, new_cursor: usize, old_count: usize, new_count: usize| {
            let old_start = old_cursor.saturating_sub(usize::from(old_count == 0));
            let new_start = new_cursor.saturating_sub(usize::from(new_count == 0));
            format!("{preamble}@@ -{old_start},{old_count} +{new_start},{new_count} @@{suffix}\n")
        };
    let mut lines = body.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let (old_delta, new_delta) = match line.as_bytes().first() {
            Some(b' ') => (1, 1),
            Some(b'-') => (1, 0),
            Some(b'+') => (0, 1),
            _ => return Err(invalid()),
        };
        let marker = if lines
            .peek()
            .is_some_and(|line| line.starts_with("\\ No newline at end of file"))
        {
            lines.next().unwrap_or("")
        } else {
            ""
        };
        let unit_bytes = line.len() + marker.len();
        let candidate = render_header(
            old_cursor,
            new_cursor,
            old_count + old_delta,
            new_count + new_delta,
        );
        if candidate.len() + content.len() + unit_bytes > MAX_DIFF_BATCH_BYTES
            && !content.is_empty()
        {
            pieces.push(render_header(old_cursor, new_cursor, old_count, new_count) + &content);
            old_cursor += old_count;
            new_cursor += new_count;
            old_count = 0;
            new_count = 0;
            content.clear();
        }
        let candidate = render_header(
            old_cursor,
            new_cursor,
            old_count + old_delta,
            new_count + new_delta,
        );
        if candidate.len() + content.len() + unit_bytes > MAX_DIFF_BATCH_BYTES {
            return Err(split_error(
                "单行 Diff（含文件头）超过 48 KiB，无法按完整行分批，请缩小审查范围。",
            ));
        }
        content.push_str(line);
        content.push_str(marker);
        old_count += old_delta;
        new_count += new_delta;
    }
    if !content.is_empty() {
        pieces.push(render_header(old_cursor, new_cursor, old_count, new_count) + &content);
    }
    if pieces.is_empty() {
        return Err(invalid());
    }
    Ok(pieces)
}

fn build_commit_message_input(files: &[StagedFilePatch]) -> String {
    files
        .iter()
        .map(|file| {
            if file.binary {
                format!("[binary] {} {}", file.status, file.path)
            } else {
                file.patch.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn staged_fingerprint(files: &[StagedFilePatch]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        hasher.update(file.status.as_bytes());
        hasher.update([0]);
        hasher.update(if file.binary {
            b"[binary]".as_slice()
        } else {
            file.patch.as_bytes()
        });
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::*;
    use crate::domain::error::ErrorCode;
    use crate::domain::settings::AppSettings;
    use crate::infrastructure::git_runner::GitCommandRunner;

    async fn run_git(directory: &Path, args: &[&str]) {
        GitCommandRunner::default()
            .run(Some(directory), args)
            .await
            .unwrap();
    }

    #[test]
    fn review_batches_split_large_replacement_without_losing_lines() {
        use crate::application::diff_parser::parse_unified_diff;
        let body = (0..2100)
            .map(|i| format!("-old_value_{i} = '原始内容'\n"))
            .collect::<String>()
            + &(0..2100)
                .map(|i| format!("+new_value_{i} = '更新内容'\n"))
                .collect::<String>();
        let patch = format!(
            "Reviewed file: large.py\ndiff --git a/large.py b/large.py\n--- a/large.py\n+++ b/large.py\n@@ -7,2100 +9,2100 @@ function\n{body}"
        );
        let expected = parse_unified_diff(&patch)
            .into_iter()
            .flat_map(|h| h.lines)
            .collect::<Vec<_>>();
        let batches = build_review_batches(&[StagedFilePatch {
            path: "large.py".into(),
            status: "M".into(),
            binary: false,
            patch,
            changed_line_anchors: (9..2109).collect(),
        }])
        .expect("large ordinary hunks must be split, not rejected");
        assert!(batches.len() > 1);
        let actual = batches
            .iter()
            .flat_map(|batch| {
                assert!(batch.text.len() <= MAX_DIFF_BATCH_BYTES);
                assert!(batch.text.starts_with("Reviewed file: large.py\n"));
                assert_eq!(batch.file_paths, ["large.py"]);
                parse_unified_diff(&batch.text)
                    .into_iter()
                    .flat_map(|h| h.lines)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn review_batches_preserve_added_deleted_and_context_lines_with_eof_markers() {
        use crate::application::diff_parser::parse_unified_diff;
        for (ranges, prefix) in [
            ("-0,0 +1,4000", '+'),
            ("-1,4000 +0,0", '-'),
            ("-3,4000 +5,4000", ' '),
        ] {
            let body = (0..4000)
                .map(|i| format!("{prefix}第{i}行：完整内容\r\n"))
                .collect::<String>()
                + "\\ No newline at end of file\n";
            let preamble = "diff --git a/test b/test\n--- a/test\n+++ b/test\n";
            let patch = format!("{preamble}@@ {ranges} @@\n{body}");
            let pieces = split_patch(&patch).unwrap();
            assert!(pieces.len() > 1);
            assert!(
                pieces
                    .iter()
                    .all(|piece| piece.len() <= MAX_DIFF_BATCH_BYTES)
            );
            let reconstructed = pieces
                .iter()
                .map(|piece| {
                    piece
                        .strip_prefix(preamble)
                        .unwrap()
                        .split_once('\n')
                        .unwrap()
                        .1
                })
                .collect::<String>();
            assert_eq!(reconstructed, body);
            let expected = parse_unified_diff(&patch)
                .into_iter()
                .flat_map(|h| h.lines)
                .collect::<Vec<_>>();
            let actual = pieces
                .iter()
                .flat_map(|piece| parse_unified_diff(piece))
                .flat_map(|h| h.lines)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
            assert_eq!(
                pieces
                    .iter()
                    .filter(|piece| piece.contains("\\ No newline"))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn review_batches_reject_unsplittable_input_and_keep_batch_limit() {
        let long_line = format!("@@ -0,0 +1 @@\n+{}\n", "x".repeat(MAX_DIFF_BATCH_BYTES));
        assert!(
            split_patch(&long_line)
                .unwrap_err()
                .message
                .contains("单行")
        );
        assert!(split_patch(&"x".repeat(MAX_DIFF_BATCH_BYTES + 1)).is_err());
        let file = StagedFilePatch {
            path: "large.txt".into(),
            status: "A".into(),
            binary: false,
            patch: format!(
                "@@ -0,0 +1,2000 @@\n{}",
                "+123456789012345678901234567890\n".repeat(2000)
            ),
            changed_line_anchors: (1..=2000).collect(),
        };
        assert_eq!(
            build_review_batches(&vec![file; MAX_BATCHES + 1])
                .unwrap_err()
                .code,
            ErrorCode::AiContextTooLarge
        );
    }

    async fn initialized_repository() -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        run_git(directory.path(), &["init", "-b", "main"]).await;
        run_git(directory.path(), &["config", "user.name", "HQ Test"]).await;
        run_git(
            directory.path(),
            &["config", "user.email", "hq@example.test"],
        )
        .await;
        run_git(directory.path(), &["config", "core.autocrlf", "false"]).await;
        std::fs::write(directory.path().join("note.txt"), "base\n").unwrap();
        run_git(directory.path(), &["add", "note.txt"]).await;
        run_git(directory.path(), &["commit", "-m", "base"]).await;
        directory
    }

    #[tokio::test]
    async fn review_context_contains_only_the_frozen_index() {
        let fixture = initialized_repository().await;
        std::fs::write(fixture.path().join("note.txt"), "staged value\n").unwrap();
        run_git(fixture.path(), &["add", "note.txt"]).await;
        std::fs::write(fixture.path().join("note.txt"), "unstaged value\n").unwrap();
        std::fs::write(fixture.path().join("scratch.txt"), "untracked value\n").unwrap();

        let context = AiContextBuilder::default()
            .capture(fixture.path(), &AppSettings::default())
            .await
            .unwrap();

        let review = context
            .review_batches
            .iter()
            .map(|batch| batch.text.as_str())
            .collect::<String>();
        assert!(review.contains("staged value"));
        assert!(!review.contains("unstaged value"));
        assert!(!review.contains("untracked value"));
        assert_eq!(context.summary.staged_file_count, 1);
        assert_eq!(context.summary.text_file_count, 1);
        assert_eq!(context.summary.fingerprint.len(), 64);
    }

    #[tokio::test]
    async fn context_is_unchanged_after_the_worktree_moves() {
        let fixture = initialized_repository().await;
        std::fs::write(fixture.path().join("note.txt"), "staged value\n").unwrap();
        run_git(fixture.path(), &["add", "note.txt"]).await;
        let context = AiContextBuilder::default()
            .capture(fixture.path(), &AppSettings::default())
            .await
            .unwrap();

        std::fs::write(fixture.path().join("note.txt"), "later value\n").unwrap();

        assert!(context.commit_message_input.contains("staged value"));
        assert!(!context.commit_message_input.contains("later value"));
    }

    #[tokio::test]
    async fn binary_index_is_skipped_for_review_but_described_for_commit() {
        let fixture = initialized_repository().await;
        std::fs::write(fixture.path().join("preview.bin"), [1, 0, 2, 3]).unwrap();
        run_git(fixture.path(), &["add", "preview.bin"]).await;

        let context = AiContextBuilder::default()
            .capture(fixture.path(), &AppSettings::default())
            .await
            .unwrap();

        assert!(context.review_batches.is_empty());
        assert_eq!(
            context.summary.skipped_binary_files,
            vec!["preview.bin".to_owned()]
        );
        assert!(
            context
                .commit_message_input
                .contains("[binary] A preview.bin")
        );
        assert!(!context.commit_message_input.as_bytes().contains(&0));
    }

    #[tokio::test]
    async fn commit_context_ignores_legacy_review_rules_including_invalid_paths() {
        let fixture = initialized_repository().await;
        std::fs::write(fixture.path().join("note.txt"), "staged value\n").unwrap();
        run_git(fixture.path(), &["add", "note.txt"]).await;
        std::fs::write(
            fixture.path().join("AGENTS.md"),
            "Prefer explicit errors.\n",
        )
        .unwrap();
        let settings = AppSettings {
            review_rule_files: vec!["AGENTS.md".to_owned()],
            use_review_rule_files_in_review: true,
            ..AppSettings::default()
        };

        let context = AiContextBuilder::default()
            .capture(fixture.path(), &settings)
            .await
            .unwrap();

        assert!(context.rules.is_empty());

        let invalid = AppSettings {
            review_rule_files: vec!["../outside.md".to_owned()],
            use_review_rule_files_in_review: true,
            ..AppSettings::default()
        };
        let context = AiContextBuilder::default()
            .capture(fixture.path(), &invalid)
            .await
            .unwrap();
        assert!(context.rules.is_empty());
    }

    #[tokio::test]
    async fn empty_index_is_rejected_before_ai_transport() {
        let fixture = initialized_repository().await;

        let error = AiContextBuilder::default()
            .capture(fixture.path(), &AppSettings::default())
            .await
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AiNoStagedChanges);
    }
}
