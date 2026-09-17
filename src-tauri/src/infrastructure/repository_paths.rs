//! Shared lexical and no-follow checks. Callers retain their own target policies.
use crate::domain::error::{BackendError, ErrorCode};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    path::{Path, PathBuf},
};

pub fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && !path.starts_with('/')
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !is_git_name(part)
                && !part.ends_with(['.', ' '])
                && !reserved_device(part)
        })
}
pub fn is_git_name(part: &str) -> bool {
    part.eq_ignore_ascii_case(".git") || part.eq_ignore_ascii_case("git~1")
}
fn reserved_device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        })
}
pub fn validate_relative(path: &str, allow_root: bool) -> Result<(), BackendError> {
    if allow_root && path.is_empty() {
        return Ok(());
    }
    if !is_safe_relative(path)
        || path.split('/').any(extended_reserved_device)
        || path
            .chars()
            .any(|c| c < ' ' || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "Use a safe repository-relative path without Git metadata or reserved names.",
        ));
    }
    Ok(())
}
fn extended_reserved_device(part: &str) -> bool {
    // Keep conflict-path compatibility in is_safe_relative; file management also
    // rejects Win32 console aliases, superscript device digits and padded stems.
    let stem = part
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    reserved_device(&stem)
        || matches!(stem.as_str(), "CONIN$" | "CONOUT$")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|suffix| matches!(suffix, "¹" | "²" | "³"))
        })
}
pub fn validate_name(name: &str) -> Result<(), BackendError> {
    validate_relative(name, false)?;
    if name.contains('/') {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "The name must be a single path component.",
        ));
    }
    Ok(())
}
pub fn is_link(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
pub fn unsupported(message: &str) -> BackendError {
    BackendError::new(ErrorCode::UnsupportedFileOperation, message)
}
pub fn utf8_path(path: &Path) -> Result<String, BackendError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        BackendError::new(
            ErrorCode::UnsupportedEncoding,
            "A filesystem path is not valid UTF-8.",
        )
    })
}
pub fn open_no_follow(path: &Path) -> Result<File, BackendError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000 | 0x0200_0000);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000 | 0x800);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        return Err(unsupported(
            "Safe no-follow access is unavailable on this platform.",
        ));
    }
    #[allow(unreachable_code)]
    let file = options.open(path)?;
    if is_link(&file.metadata()?) {
        return Err(unsupported("Links and reparse points are restricted."));
    }
    Ok(file)
}
pub fn checked_path(
    root: &Path,
    relative: &str,
    private: &[PathBuf],
    submodules: &[PathBuf],
    allow_restricted_leaf: bool,
) -> Result<PathBuf, BackendError> {
    validate_relative(relative, true)?;
    let mut path = root.to_path_buf();
    if is_link(&fs::symlink_metadata(root)?) {
        return Err(unsupported("Repository root changed into a link."));
    }
    if relative.is_empty() {
        return Ok(path);
    }
    let count = relative.split('/').count();
    for (index, component) in relative.split('/').enumerate() {
        path.push(component);
        let metadata = fs::symlink_metadata(&path)?;
        let leaf = index + 1 == count;
        if private.iter().any(|p| path.starts_with(p)) {
            return Err(unsupported("Git metadata is not accessible."));
        }
        if is_link(&metadata) || (!metadata.is_dir() && !metadata.is_file()) {
            if leaf && allow_restricted_leaf {
                return Ok(path);
            }
            return Err(unsupported(
                "Links, nested repositories, submodules and special files are restricted.",
            ));
        }
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(root) {
            return Err(BackendError::new(
                ErrorCode::InvalidPath,
                "Path escapes the repository.",
            ));
        }
        // Canonical paths resolve Windows casing and short-name aliases. Lexical
        // prefix comparisons alone would expose custom Git metadata directories.
        if private.iter().any(|private| canonical.starts_with(private)) {
            return Err(unsupported("Git metadata is not accessible."));
        }
        if submodules.contains(&canonical) || (metadata.is_dir() && has_git_entry(&path)?) {
            if leaf && allow_restricted_leaf {
                return Ok(path);
            }
            return Err(unsupported(
                "Nested repositories and submodules are restricted.",
            ));
        }
        if !leaf && !metadata.is_dir() {
            return Err(unsupported("Every parent must be an ordinary directory."));
        }
    }
    Ok(path)
}
pub fn has_git_entry(path: &Path) -> Result<bool, BackendError> {
    if optional_metadata(&path.join(".git"))?.is_some() {
        return Ok(true);
    }
    // Bare repositories have no .git entry. Treat their identifying structure
    // conservatively as a repository boundary without running Git inside it.
    let head = optional_metadata(&path.join("HEAD"))?;
    let objects = optional_metadata(&path.join("objects"))?;
    let refs = optional_metadata(&path.join("refs"))?;
    let reftable = optional_metadata(&path.join("reftable"))?;
    let directory_marker = |meta: &Metadata| meta.is_dir() || is_link(meta);
    Ok(head.as_ref().is_some_and(|m| m.is_file() || is_link(m))
        && objects.as_ref().is_some_and(directory_marker)
        && (refs.as_ref().is_some_and(directory_marker)
            || reftable.as_ref().is_some_and(directory_marker)))
}
fn optional_metadata(path: &Path) -> Result<Option<Metadata>, BackendError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
