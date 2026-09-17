use super::file_fingerprint::{FileContext, children, digest, field, metadata_identity, stale};
use crate::{
    domain::{
        error::{BackendError, ErrorCode},
        files::*,
    },
    infrastructure::{
        git_runner::GitCommandRunner,
        repository_paths::{
            has_git_entry, is_git_name, is_link, open_no_follow, unsupported, validate_relative,
        },
    },
};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, io::Read};

const PAGE_SIZE: usize = 500;
const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;
const MAX_STATUS_BYTES: usize = 64 * 1024 * 1024;

pub(super) async fn list(
    context: &FileContext,
    runner: &GitCommandRunner,
    relative: &str,
    cursor: Option<&str>,
) -> Result<FileDirectoryPage, BackendError> {
    let path = context.path(relative, false)?;
    if !fs::symlink_metadata(&path)?.is_dir() {
        return Err(unsupported("Only ordinary directories can be expanded."));
    }
    let before = metadata_identity(&path)?;
    let statuses = statuses(context, runner).await?;
    let mut hash = Sha256::new();
    field(
        &mut hash,
        context
            .root
            .to_str()
            .ok_or_else(|| {
                BackendError::new(
                    ErrorCode::UnsupportedEncoding,
                    "Repository root is not UTF-8.",
                )
            })?
            .as_bytes(),
    );
    field(&mut hash, relative.as_bytes());
    field(&mut hash, before.as_bytes());
    let mut entries = Vec::new();
    for (name, child) in children(&path)? {
        if is_git_name(&name) || context.is_private_entry(&child)? {
            continue;
        }
        let relative_path = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        let meta = fs::symlink_metadata(&child)?;
        let reason = if validate_relative(&relative_path, false).is_err() {
            Some("This filename is not supported for safe access.")
        } else if is_link(&meta) {
            Some("Links and reparse points are restricted.")
        } else if context.submodules.contains(&child.canonicalize()?) {
            Some("Git submodules are restricted.")
        } else if meta.is_dir() && has_git_entry(&child)? {
            Some("Nested repositories are restricted.")
        } else if !meta.is_file() && !meta.is_dir() {
            Some("Special files are restricted.")
        } else {
            None
        };
        let kind = if reason.is_some() {
            FileEntryKind::Restricted
        } else if meta.is_dir() {
            FileEntryKind::Directory
        } else {
            FileEntryKind::File
        };
        let git_status = statuses
            .get(&relative_path)
            .or_else(|| statuses.get(&format!("{relative_path}/")))
            .cloned();
        let entry = RepositoryFileEntry {
            name,
            relative_path,
            kind,
            byte_length: meta.is_file().then_some(meta.len()),
            reason: reason.map(str::to_owned),
            git_status,
        };
        field(&mut hash, metadata_identity(&child)?.as_bytes());
        field(
            &mut hash,
            &serde_json::to_vec(&entry).map_err(|_| {
                BackendError::new(ErrorCode::Unexpected, "Cannot encode directory entry.")
            })?,
        );
        entries.push(entry);
    }
    if metadata_identity(&path)? != before {
        return Err(stale());
    }
    entries.sort_by(|a, b| {
        (a.kind != FileEntryKind::Directory, &a.name)
            .cmp(&(b.kind != FileEntryKind::Directory, &b.name))
    });
    let token = format!("{:x}", hash.finalize());
    let offset = match cursor {
        None => 0,
        Some(cursor) => {
            let (snapshot, offset) = cursor.split_once(':').ok_or_else(stale)?;
            let offset = offset.parse::<usize>().map_err(|_| stale())?;
            if snapshot != token
                || offset == 0
                || offset % PAGE_SIZE != 0
                || offset >= entries.len()
            {
                return Err(stale());
            }
            offset
        }
    };
    let total_entries = entries.len();
    let end = offset.saturating_add(PAGE_SIZE).min(total_entries);
    let next_cursor = (end < total_entries).then(|| format!("{token}:{end}"));
    Ok(FileDirectoryPage {
        relative_dir: relative.into(),
        token,
        entries: entries[offset..end].to_vec(),
        next_cursor,
        total_entries,
    })
}
async fn statuses(
    context: &FileContext,
    runner: &GitCommandRunner,
) -> Result<HashMap<String, String>, BackendError> {
    let bytes = runner
        .run_bytes(
            Some(&context.root),
            [
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=normal",
                // Older Git supports --ignored only as a boolean flag.
                "--ignored",
                "--ignore-submodules=all",
            ],
            MAX_STATUS_BYTES,
        )
        .await?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        BackendError::new(
            ErrorCode::UnsupportedEncoding,
            "Git status contains a path that is not UTF-8.",
        )
    })?;
    let mut map = HashMap::new();
    let mut records = text.split('\0').filter(|x| !x.is_empty());
    while let Some(record) = records.next() {
        let bytes = record.as_bytes();
        if bytes.len() < 3 || !bytes[0].is_ascii() || !bytes[1].is_ascii() {
            return Err(unsupported("Unexpected Git status output."));
        }
        map.insert(record[3..].into(), record[..2].into());
        if matches!(bytes[0], b'R' | b'C') || matches!(bytes[1], b'R' | b'C') {
            records.next();
        }
    }
    Ok(map)
}
pub(super) fn preview(
    context: &FileContext,
    relative: &str,
) -> Result<RepositoryFilePreview, BackendError> {
    validate_relative(relative, false)?;
    let path = context.path(relative, true)?;
    let meta = fs::symlink_metadata(&path)?;
    let before = metadata_identity(&path)?;
    let mut result = RepositoryFilePreview {
        relative_path: relative.into(),
        token: before.clone(),
        kind: FilePreviewKind::Unsupported,
        text: None,
        byte_length: meta.len(),
        bom: false,
        line_ending: FileLineEnding::None,
        reason: None,
    };
    if is_link(&meta) || !meta.is_file() || context.submodules.contains(&path.canonicalize()?) {
        result.reason = Some("Only ordinary files support preview.".into());
        return Ok(result);
    }
    if meta.len() > MAX_PREVIEW_BYTES {
        result.kind = FilePreviewKind::TooLarge;
        result.reason = Some("Text previews are limited to 2 MiB.".into());
        return Ok(result);
    }
    let file = open_no_follow(&path)?;
    if !file.metadata()?.is_file() {
        return Err(stale());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_PREVIEW_BYTES + 1).read_to_end(&mut bytes)?;
    if metadata_identity(&path)? != before || bytes.len() as u64 != meta.len() {
        return Err(stale());
    }
    if bytes.len() as u64 > MAX_PREVIEW_BYTES {
        result.kind = FilePreviewKind::TooLarge;
        result.reason = Some("Text previews are limited to 2 MiB.".into());
        return Ok(result);
    }
    result.token = digest(&[before.as_bytes(), &bytes].concat());
    result.bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    if bytes.contains(&0) {
        result.kind = FilePreviewKind::Binary;
        result.reason = Some("Binary content has no text preview.".into());
        return Ok(result);
    }
    let content = if result.bom { &bytes[3..] } else { &bytes };
    let text = match std::str::from_utf8(content) {
        Ok(text) => text,
        Err(_) => {
            result.kind = FilePreviewKind::UnsupportedEncoding;
            result.reason = Some("The file is not valid UTF-8.".into());
            return Ok(result);
        }
    };
    result.kind = FilePreviewKind::Text;
    result.line_ending = line_ending(text);
    result.text = Some(text.into());
    Ok(result)
}
fn line_ending(text: &str) -> FileLineEnding {
    let bytes = text.as_bytes();
    let mut lf = 0;
    let mut crlf = 0;
    let mut cr = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            if index > 0 && bytes[index - 1] == b'\r' {
                crlf += 1
            } else {
                lf += 1
            }
        } else if *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n') {
            cr += 1
        }
    }
    match (lf > 0, crlf > 0, cr > 0) {
        (false, false, false) => FileLineEnding::None,
        (true, false, false) => FileLineEnding::Lf,
        (false, true, false) => FileLineEnding::Crlf,
        _ => FileLineEnding::Mixed,
    }
}
