//! Keep the frozen working file locally; send only bounded, corresponding diff windows.
use super::ai_conflict_protocol::MAX_VERSION_BYTES;
use super::conflict_content::MAX_CONFLICT_PREVIEW_BYTES;
use crate::domain::{
    ai_conflict::{AiConflictContext, AiConflictSuggestionKind, AiConflictSuggestionResult},
    conflicts::ConflictDetail,
    error::{BackendError, ErrorCode},
};
use crate::infrastructure::git_runner::GitCommandRunner;

const CONTEXT_LINES: usize = 8;
const MAX_CHUNKS: usize = 32;
#[derive(Clone, Debug)]
struct Change {
    old_from: usize,
    old_to: usize,
    new_from: usize,
    new_to: usize,
}
pub(super) struct ChunkPlan {
    original: String,
    ranges: Vec<(usize, usize)>,
}

fn too_large() -> BackendError {
    BackendError::new(
        ErrorCode::AiContextTooLarge,
        "单个冲突差异片段（含上下文）超过 64 KiB 或超过 32 个片段，请先缩小冲突范围。未修改文件或草稿。",
    )
}
fn invalid() -> BackendError {
    BackendError::new(ErrorCode::Unexpected, "无法定位冲突差异片段，未修改文件。")
}
fn offsets(text: &str) -> Vec<usize> {
    let mut result = vec![0];
    for line in text.split_inclusive('\n') {
        result.push(result.last().unwrap() + line.len());
    }
    result
}
fn range(text: &str) -> Result<(usize, usize), BackendError> {
    let (start, count) = text.split_once(',').unwrap_or((text, "1"));
    let start: usize = start.parse().map_err(|_| invalid())?;
    let count: usize = count.parse().map_err(|_| invalid())?;
    let from = if count == 0 {
        start
    } else {
        start.checked_sub(1).ok_or_else(invalid)?
    };
    Ok((from, from + count))
}
fn parse_changes(diff: &str) -> Result<Vec<Change>, BackendError> {
    diff.lines()
        .filter(|line| line.starts_with("@@ "))
        .map(|line| {
            let ranges = line
                .strip_prefix("@@ -")
                .and_then(|line| line.split_once(" @@"))
                .ok_or_else(invalid)?
                .0;
            let (old, new) = ranges.split_once(" +").ok_or_else(invalid)?;
            let (old_from, old_to) = range(old)?;
            let (new_from, new_to) = range(new)?;
            Ok(Change {
                old_from,
                old_to,
                new_from,
                new_to,
            })
        })
        .collect()
}
fn source_boundary(position: usize, changes: &[Change], upper: bool) -> usize {
    let mut old_end = 0;
    let mut new_end = 0;
    for change in changes {
        if position < change.new_from {
            break;
        }
        if position == change.new_from {
            return if upper && change.new_from == change.new_to {
                change.old_to
            } else {
                change.old_from
            };
        }
        if position < change.new_to {
            return if upper {
                change.old_to
            } else {
                change.old_from
            };
        }
        old_end = change.old_to;
        new_end = change.new_to;
    }
    old_end + position - new_end
}

pub(super) async fn prepare(
    detail: &ConflictDetail,
) -> Result<(ChunkPlan, Vec<serde_json::Value>), BackendError> {
    let versions = [&detail.base, &detail.ours, &detail.theirs, &detail.working];
    let original = detail.working.text.clone().unwrap_or_default();
    let boundaries = versions.map(|version| offsets(version.text.as_deref().unwrap_or("")));
    let line_count = boundaries[3].len() - 1;
    let temp = tempfile::tempdir()?;
    let working_path = temp.path().join("working.txt");
    std::fs::write(&working_path, &original)?;
    let mut mappings = Vec::new();
    for (index, version) in versions[..3].iter().enumerate() {
        if !version.exists {
            mappings.push(Vec::new());
            continue;
        }
        let source_path = temp.path().join(format!("source-{index}.txt"));
        std::fs::write(&source_path, version.text.as_deref().unwrap_or(""))?;
        let output = GitCommandRunner::default()
            .run_allowing_failure(
                Some(temp.path()),
                [
                    "diff",
                    "--no-index",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--text",
                    "--diff-algorithm=histogram",
                    "--unified=0",
                    "--",
                    &source_path.to_string_lossy(),
                    &working_path.to_string_lossy(),
                ],
            )
            .await?;
        if !matches!(output.status_code, Some(0 | 1)) {
            return Err(invalid().with_diagnostics(output.stderr));
        }
        mappings.push(parse_changes(&output.stdout)?);
    }
    let mut windows = mappings
        .iter()
        .flatten()
        .map(|change| {
            (
                change.new_from.saturating_sub(CONTEXT_LINES),
                (change.new_to + CONTEXT_LINES).min(line_count),
            )
        })
        .collect::<Vec<_>>();
    windows.sort_unstable();
    let mut merged = Vec::<(usize, usize)>::new();
    for (start, end) in windows {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    if merged.len() > MAX_CHUNKS {
        return Err(too_large());
    }
    let mut inputs = Vec::new();
    let mut ranges = Vec::new();
    for (start, end) in merged {
        let mut fragments = serde_json::Map::new();
        let mut lines = serde_json::Map::new();
        for (index, name) in ["base", "ours", "theirs", "working"].iter().enumerate() {
            let mut version = versions[index].clone();
            let (from, to) = if !version.exists {
                (0, 0)
            } else if index == 3 {
                (start, end)
            } else {
                (
                    source_boundary(start, &mappings[index], false),
                    source_boundary(end, &mappings[index], true),
                )
            };
            if from > to || to >= boundaries[index].len() {
                return Err(invalid());
            }
            if let Some(text) = &version.text {
                let fragment = &text[boundaries[index][from]..boundaries[index][to]];
                if fragment.len() > MAX_VERSION_BYTES {
                    return Err(too_large());
                }
                version.text = Some(fragment.to_owned());
            }
            lines.insert(
                (*name).into(),
                serde_json::json!({"startLine":from + 1, "endLineExclusive":to + 1}),
            );
            fragments.insert(
                (*name).into(),
                serde_json::to_value(version).map_err(|_| invalid())?,
            );
        }
        ranges.push((boundaries[3][start], boundaries[3][end]));
        inputs.push(serde_json::json!({"versions":fragments, "lineRanges":lines}));
    }
    Ok((ChunkPlan { original, ranges }, inputs))
}

impl ChunkPlan {
    pub(super) fn assemble(
        self,
        results: Vec<AiConflictSuggestionResult>,
        context: AiConflictContext,
    ) -> Result<AiConflictSuggestionResult, BackendError> {
        if results.len() != self.ranges.len() {
            return Err(invalid());
        }
        let complete = results.iter().all(|result| {
            result.kind == AiConflictSuggestionKind::Text && result.resolved_text.is_some()
        });
        let mut candidate = String::new();
        let mut cursor = 0;
        let mut explanation = Vec::new();
        let mut risks = Vec::new();
        let mut missing = Vec::new();
        for (index, (result, (start, end))) in results.into_iter().zip(self.ranges).enumerate() {
            if complete {
                let replacement = result.resolved_text.as_deref().unwrap();
                if end < self.original.len()
                    && !replacement.is_empty()
                    && !replacement.ends_with('\n')
                {
                    return Err(BackendError::new(
                        ErrorCode::AiInvalidResponse,
                        "AI 片段候选缺少边界换行，不能安全拼接。请重新生成建议。",
                    ));
                }
                candidate.push_str(&self.original[cursor..start]);
                candidate.push_str(replacement);
            }
            cursor = end;
            explanation.push(format!(
                "片段 {}：{}\n{}",
                index + 1,
                result.summary,
                result.explanation
            ));
            risks.extend(result.risks);
            missing.extend(result.context_missing);
        }
        if complete {
            candidate.push_str(&self.original[cursor..]);
        }
        if candidate.len() > MAX_CONFLICT_PREVIEW_BYTES {
            return Err(BackendError::new(
                ErrorCode::AiContextTooLarge,
                "重组后的候选超过 2 MiB，无法填入解决草稿。",
            ));
        }
        risks.push(
            "仅发送差异片段及周边上下文；未改动部分由本地原样保留。请检查跨片段逻辑后再保存。"
                .into(),
        );
        Ok(AiConflictSuggestionResult {
            kind: if complete {
                AiConflictSuggestionKind::Text
            } else {
                AiConflictSuggestionKind::AdviceOnly
            },
            summary: if complete {
                "差异片段已处理，完整候选已在本地重组。"
            } else {
                "部分片段只能提供说明，未生成可应用的完整候选。"
            }
            .into(),
            explanation: explanation.join("\n\n"),
            resolved_text: complete.then_some(candidate),
            risks,
            context_missing: missing,
            context,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::conflict_content;
    use crate::domain::operation::RepositoryOperationKind;
    fn detail(base: &str, ours: &str, theirs: &str, working: &str) -> ConflictDetail {
        let version = |text: &str| conflict_content::version(text.as_bytes(), None, None);
        ConflictDetail {
            path: "file.txt".into(),
            token: "token".into(),
            operation_kind: RepositoryOperationKind::Merge,
            base: version(base),
            ours: version(ours),
            theirs: version(theirs),
            working: version(working),
            editable: true,
            can_choose_ours: true,
            can_choose_theirs: true,
            can_delete: true,
            unsupported_reason: None,
        }
    }
    fn context() -> AiConflictContext {
        AiConflictContext {
            path: "file.txt".into(),
            token: "token".into(),
            operation_kind: RepositoryOperationKind::Merge,
            base_oid: None,
            ours_oid: None,
            theirs_oid: None,
            fingerprint: "frozen".into(),
        }
    }
    fn answer(text: Option<String>) -> AiConflictSuggestionResult {
        AiConflictSuggestionResult {
            kind: if text.is_some() {
                AiConflictSuggestionKind::Text
            } else {
                AiConflictSuggestionKind::AdviceOnly
            },
            summary: "summary".into(),
            explanation: "reason".into(),
            resolved_text: text,
            risks: vec![],
            context_missing: vec![],
            context: context(),
        }
    }
    #[tokio::test]
    async fn ai_conflict_chunk_mapping_preserves_insertions_deletions_and_endings() {
        let base = (0..160)
            .map(|i| format!("line {i} 中文\n"))
            .collect::<String>();
        let ours = base
            .replace("line 0 中文\n", "insert\nline 0 中文\n")
            .replace("line 90 中文\n", "")
            .replace("line 159 中文\n", "last without newline");
        let theirs = base.replace("line 45 中文\n", "theirs\n");
        for working in [&base, &theirs, ""] {
            let mut detail = detail(&base, &ours, &theirs, working);
            detail.base = conflict_content::missing();
            let (plan, inputs) = prepare(&detail).await.unwrap();
            let results = inputs
                .into_iter()
                .map(|input| {
                    answer(Some(
                        input["versions"]["ours"]["text"]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                    ))
                })
                .collect();
            assert_eq!(
                plan.assemble(results, context())
                    .unwrap()
                    .resolved_text
                    .as_deref(),
                Some(ours.as_str())
            );
        }
    }
    #[tokio::test]
    async fn ai_conflict_chunks_never_apply_partial_advice_and_do_not_truncate_large_regions() {
        let (plan, _) = prepare(&detail("a\n", "b\n", "c\n", "d\n")).await.unwrap();
        assert!(
            plan.assemble(vec![answer(None)], context())
                .unwrap()
                .resolved_text
                .is_none()
        );
        assert!(
            prepare(&detail("a", &"b".repeat(MAX_VERSION_BYTES + 1), "c", "d"))
                .await
                .is_err()
        );
        let text = (0..1400)
            .map(|i| format!("original {i}\n"))
            .collect::<String>();
        let mut changed = text.clone();
        for i in (30..1300).step_by(30) {
            changed = changed.replace(&format!("original {i}\n"), &format!("changed {i}\n"));
        }
        assert!(
            prepare(&detail(&text, &changed, &text, &text))
                .await
                .is_err()
        );
        let (plan, inputs) = prepare(&detail(&text, &text, &text, &text)).await.unwrap();
        assert!(inputs.is_empty());
        assert_eq!(
            plan.assemble(vec![], context())
                .unwrap()
                .resolved_text
                .as_deref(),
            Some(text.as_str())
        );
    }

    #[test]
    fn ai_conflict_chunks_reject_a_candidate_that_would_join_unrelated_lines() {
        let plan = ChunkPlan {
            original: "first\nlast\n".into(),
            ranges: vec![(0, 6)],
        };
        assert_eq!(
            plan.assemble(
                vec![answer(Some("replacement without newline".into()))],
                context()
            )
            .unwrap_err()
            .code,
            ErrorCode::AiInvalidResponse
        );
    }
}
