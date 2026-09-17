use crate::domain::changes::{DiffHunk, DiffLine, DiffLineKind};

pub fn parse_unified_diff(output: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0;
    let mut new_line = 0;

    for line in output.lines() {
        if line.starts_with("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            let Some((old_start, new_start)) = parse_hunk_starts(line) else {
                continue;
            };
            old_line = old_start;
            new_line = new_start;
            current = Some(DiffHunk {
                index: hunks.len(),
                header: line.to_owned(),
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };
        let (kind, old_number, new_number, content) = if let Some(content) = line.strip_prefix('+')
        {
            let number = new_line;
            new_line += 1;
            (DiffLineKind::Addition, None, Some(number), content)
        } else if let Some(content) = line.strip_prefix('-') {
            let number = old_line;
            old_line += 1;
            (DiffLineKind::Deletion, Some(number), None, content)
        } else if let Some(content) = line.strip_prefix(' ') {
            let old_number = old_line;
            let new_number = new_line;
            old_line += 1;
            new_line += 1;
            (
                DiffLineKind::Context,
                Some(old_number),
                Some(new_number),
                content,
            )
        } else {
            (DiffLineKind::Meta, None, None, line)
        };
        hunk.lines.push(DiffLine {
            kind,
            old_line: old_number,
            new_line: new_number,
            content: content.to_owned(),
        });
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    hunks
}

pub(crate) fn parse_hunk_starts(header: &str) -> Option<(u32, u32)> {
    let ranges = header.strip_prefix("@@ -")?.split_once(" @@")?.0;
    let (old_range, new_range) = ranges.split_once(" +")?;
    Some((parse_range_start(old_range)?, parse_range_start(new_range)?))
}

fn parse_range_start(range: &str) -> Option<u32> {
    range.split(',').next()?.parse().ok()
}
