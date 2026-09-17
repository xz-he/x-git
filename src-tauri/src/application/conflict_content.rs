use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::domain::conflicts::{ConflictLineEnding, ConflictVersion, ConflictVersionKind};
use crate::domain::error::{BackendError, ErrorCode};

pub const MAX_CONFLICT_PREVIEW_BYTES: usize = 2 * 1024 * 1024;
const HASH_BUFFER_BYTES: usize = 64 * 1024;
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

pub fn missing() -> ConflictVersion {
    ConflictVersion {
        exists: false,
        oid: None,
        mode: None,
        kind: ConflictVersionKind::Missing,
        text: None,
        byte_length: 0,
        bom: false,
        line_ending: ConflictLineEnding::None,
    }
}

pub fn metadata_version(
    mode: Option<String>,
    oid: Option<String>,
    size: u64,
    kind: ConflictVersionKind,
) -> ConflictVersion {
    ConflictVersion {
        exists: true,
        oid,
        mode,
        kind,
        text: None,
        byte_length: size,
        bom: false,
        line_ending: ConflictLineEnding::None,
    }
}

pub fn version(bytes: &[u8], mode: Option<String>, oid: Option<String>) -> ConflictVersion {
    let mut value = metadata_version(mode, oid, bytes.len() as u64, ConflictVersionKind::Text);
    if bytes.len() > MAX_CONFLICT_PREVIEW_BYTES {
        value.kind = ConflictVersionKind::TooLarge;
        return value;
    }
    value.bom = bytes.starts_with(UTF8_BOM);
    let content = if value.bom {
        &bytes[UTF8_BOM.len()..]
    } else {
        bytes
    };
    if content.contains(&0) {
        value.kind = ConflictVersionKind::Binary;
        return value;
    }
    let Ok(text) = std::str::from_utf8(content) else {
        value.kind = ConflictVersionKind::UnsupportedEncoding;
        return value;
    };
    let crlf = content.windows(2).filter(|pair| *pair == b"\r\n").count();
    let lf = content.iter().filter(|byte| **byte == b'\n').count();
    let cr = content.iter().filter(|byte| **byte == b'\r').count();
    value.line_ending = if cr > crlf || (crlf > 0 && lf > crlf) {
        ConflictLineEnding::Mixed
    } else if crlf > 0 {
        ConflictLineEnding::Crlf
    } else if lf > 0 {
        ConflictLineEnding::Lf
    } else {
        ConflictLineEnding::None
    };
    if value.line_ending == ConflictLineEnding::Mixed {
        value.kind = ConflictVersionKind::MixedLineEndings;
    }
    value.text = Some(text.replace("\r\n", "\n"));
    value
}

pub fn working_version_with_limit(
    path: &Path,
    max_bytes: usize,
) -> Result<ConflictVersion, BackendError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(missing()),
        Err(error) => return Err(error.into()),
    };
    let mode = Some(file_mode(&metadata));
    if metadata.len() > max_bytes as u64 {
        return Ok(metadata_version(
            mode,
            None,
            metadata.len(),
            ConflictVersionKind::TooLarge,
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Ok(metadata_version(
            mode,
            None,
            bytes.len() as u64,
            ConflictVersionKind::TooLarge,
        ));
    }
    Ok(version(&bytes, mode, None))
}

pub fn file_mode(metadata: &Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 {
            return "100755".into();
        }
    }
    let _ = metadata;
    "100644".into()
}

pub fn add_fingerprint(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

pub fn hash_file(path: &Path) -> Result<String, BackendError> {
    let mut hash = Sha256::new();
    let mut file = File::open(path)?;
    let mut buffer = [0; HASH_BUFFER_BYTES];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn encode_text(
    text: &str,
    template: &ConflictVersion,
    acknowledge_markers: bool,
) -> Result<Vec<u8>, BackendError> {
    if !acknowledge_markers
        && text.lines().any(|line| {
            ["<<<<<<<", "|||||||", "=======", ">>>>>>>"]
                .iter()
                .any(|marker| line.starts_with(marker))
        })
    {
        return Err(BackendError::new(
            ErrorCode::UnsupportedConflict,
            "Remaining conflict markers require explicit acknowledgement.",
        ));
    }
    if text.len() > MAX_CONFLICT_PREVIEW_BYTES
        || text.contains('\0')
        || text.replace("\r\n", "").contains('\r')
    {
        return Err(BackendError::new(
            ErrorCode::UnsupportedConflict,
            "The edited result is not supported UTF-8 text within the 2 MiB limit.",
        ));
    }
    let normalized = text.replace("\r\n", "\n");
    let normalized = if template.line_ending == ConflictLineEnding::Crlf {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    };
    let mut bytes = Vec::with_capacity(normalized.len() + UTF8_BOM.len());
    if template.bom {
        bytes.extend_from_slice(UTF8_BOM);
    }
    bytes.extend_from_slice(normalized.as_bytes());
    if bytes.len() > MAX_CONFLICT_PREVIEW_BYTES {
        return Err(BackendError::new(
            ErrorCode::UnsupportedConflict,
            "The encoded result exceeds the 2 MiB limit.",
        ));
    }
    Ok(bytes)
}

pub fn replace_text(path: &Path, bytes: &[u8], mode: Option<&str>) -> Result<(), BackendError> {
    let parent = path
        .parent()
        .ok_or_else(|| BackendError::new(ErrorCode::InvalidPath, "Missing parent directory."))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    if let Ok(metadata) = fs::metadata(path) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !path.exists() {
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(if mode == Some("100755") {
                    0o755
                } else {
                    0o644
                }))?;
        }
    }
    let _ = mode;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| BackendError::from(error.error))?;
    Ok(())
}
