use crate::{
    domain::{
        error::{BackendError, ErrorCode},
        files::FileEntryKind,
    },
    infrastructure::{
        git_runner::GitCommandRunner,
        repository_paths::{
            checked_path, is_git_name, is_link, open_no_follow, unsupported, utf8_path,
            validate_relative,
        },
    },
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata},
    io::Read,
    path::{Path, PathBuf},
};

pub(super) const MAX_NODES: usize = 10_000;
pub(super) const MAX_CONTENT_BYTES: u64 = 512 * 1024 * 1024;
pub(super) const MAX_DEPTH: usize = 64;
const HASH_BUFFER_BYTES: usize = 64 * 1024;
const MAX_GIT_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn stale() -> BackendError {
    BackendError::new(
        ErrorCode::StaleFileOperation,
        "The files or repository changed. Refresh and prepare the operation again.",
    )
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

#[derive(Debug, Clone)]
pub(super) struct FileContext {
    pub root: PathBuf,
    pub private: Vec<PathBuf>,
    pub submodules: Vec<PathBuf>,
    pub git_dir: PathBuf,
    pub identity: String,
}
impl FileContext {
    pub fn is_private_entry(&self, path: &Path) -> Result<bool, BackendError> {
        if self.private.iter().any(|private| path.starts_with(private)) {
            return Ok(true);
        }
        let metadata = fs::symlink_metadata(path)?;
        if is_link(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
            return Ok(false);
        }
        let canonical = path.canonicalize()?;
        Ok(self
            .private
            .iter()
            .any(|private| canonical.starts_with(private)))
    }
    pub async fn read(root: &Path, runner: &GitCommandRunner) -> Result<Self, BackendError> {
        let git_dir = git_path(root, runner, "--absolute-git-dir")
            .await?
            .canonicalize()?;
        let common = git_path(root, runner, "--git-common-dir")
            .await?
            .canonicalize()?;
        let index = resolved_git_path(root, runner, "index").await?;
        let mut hash = Sha256::new();
        field(&mut hash, utf8_path(root)?.as_bytes());
        field(&mut hash, utf8_path(&git_dir)?.as_bytes());
        field(&mut hash, utf8_path(&common)?.as_bytes());
        let head = runner
            .run_allowing_failure(Some(root), ["rev-parse", "--verify", "HEAD"])
            .await?;
        if !head.is_success() && head.status_code != Some(128) {
            return Err(head.into_result().unwrap_err());
        }
        field(&mut hash, head.stdout.as_bytes());
        let symbolic = runner
            .run_allowing_failure(Some(root), ["symbolic-ref", "-q", "HEAD"])
            .await?;
        if !symbolic.is_success() && symbolic.status_code != Some(1) {
            return Err(symbolic.into_result().unwrap_err());
        }
        field(&mut hash, symbolic.stdout.as_bytes());
        hash_optional_file(&index, &mut hash)?;
        let stage = runner
            .run_bytes(
                Some(root),
                ["--no-optional-locks", "ls-files", "--stage", "-z"],
                MAX_GIT_BYTES,
            )
            .await?;
        field(&mut hash, &stage);
        let stage = std::str::from_utf8(&stage).map_err(|_| {
            BackendError::new(
                ErrorCode::UnsupportedEncoding,
                "The index contains a path that is not UTF-8.",
            )
        })?;
        let indexed_submodules = stage
            .split('\0')
            .filter_map(|line| {
                line.strip_prefix("160000 ")
                    .and_then(|line| line.split_once('\t'))
                    .map(|(_, path)| path.to_owned())
            })
            .collect::<Vec<_>>();
        let mut submodules = Vec::with_capacity(indexed_submodules.len());
        for relative in indexed_submodules {
            validate_relative(&relative, false)?;
            let path = root.join(&relative);
            // An indexed descendant can have an externally replaced link parent.
            // Resolve identities only after checking every parent without following it.
            match checked_path(
                root,
                &relative,
                &[git_dir.clone(), common.clone()],
                &[],
                true,
            ) {
                Ok(checked) => {
                    let metadata = fs::symlink_metadata(&checked)?;
                    if !is_link(&metadata) && (metadata.is_file() || metadata.is_dir()) {
                        submodules.push(checked.canonicalize()?);
                    } else {
                        submodules.push(path);
                    }
                }
                // Missing or restricted parents will be rejected by each actual access.
                Err(_) => submodules.push(path),
            }
        }
        // Include all operation metadata, even files not represented by the UI operation enum.
        for name in [
            "HEAD",
            "MERGE_HEAD",
            "MERGE_MSG",
            "MERGE_MODE",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "REBASE_HEAD",
            "BISECT_LOG",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
        ] {
            field(&mut hash, name.as_bytes());
            hash_metadata_tree(&git_dir.join(name), &mut hash, 0, &mut 0usize, &mut 0u64)?;
        }
        Ok(Self {
            root: root.into(),
            private: vec![git_dir.clone(), common],
            submodules,
            git_dir,
            identity: format!("{:x}", hash.finalize()),
        })
    }
    pub fn path(&self, relative: &str, restricted_leaf: bool) -> Result<PathBuf, BackendError> {
        checked_path(
            &self.root,
            relative,
            &self.private,
            &self.submodules,
            restricted_leaf,
        )
    }
}
async fn git_path(
    root: &Path,
    runner: &GitCommandRunner,
    argument: &str,
) -> Result<PathBuf, BackendError> {
    let bytes = runner
        .run_bytes(Some(root), ["rev-parse", argument], MAX_GIT_BYTES)
        .await?;
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| {
            BackendError::new(
                ErrorCode::UnsupportedEncoding,
                "Git metadata path is not UTF-8.",
            )
        })?
        .trim_end_matches(['\n', '\r']);
    let path = PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}
pub(super) async fn resolved_git_path(
    root: &Path,
    runner: &GitCommandRunner,
    name: &str,
) -> Result<PathBuf, BackendError> {
    let bytes = runner
        .run_bytes(Some(root), ["rev-parse", "--git-path", name], MAX_GIT_BYTES)
        .await?;
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| {
            BackendError::new(
                ErrorCode::UnsupportedEncoding,
                "Git metadata path is not UTF-8.",
            )
        })?
        .trim_end_matches(['\n', '\r']);
    let path = PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}
fn hash_optional_file(path: &Path, hash: &mut Sha256) -> Result<(), BackendError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !is_link(&meta) => {
            field(hash, b"present");
            let mut total = 0;
            hash_file(path, hash, &mut total)?;
        }
        Ok(_) => return Err(unsupported("Git identity metadata is not a regular file.")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => field(hash, b"absent"),
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn hash_metadata_tree(
    path: &Path,
    hash: &mut Sha256,
    depth: usize,
    nodes: &mut usize,
    bytes: &mut u64,
) -> Result<(), BackendError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            field(hash, b"absent");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    *nodes += 1;
    if depth > MAX_DEPTH || *nodes > MAX_NODES {
        return Err(unsupported("Operation metadata exceeds the safety limits."));
    }
    if is_link(&meta) {
        return Err(unsupported("Git operation metadata contains a link."));
    }
    if meta.is_file() {
        hash_file(path, hash, bytes)?;
    } else if meta.is_dir() {
        field(hash, b"directory");
        for (name, path) in children(path)? {
            field(hash, name.as_bytes());
            hash_metadata_tree(&path, hash, depth + 1, nodes, bytes)?;
        }
    } else {
        return Err(unsupported(
            "Git operation metadata contains a special file.",
        ));
    }
    Ok(())
}
pub(super) fn children(path: &Path) -> Result<Vec<(String, PathBuf)>, BackendError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        if entries.len() == MAX_NODES {
            return Err(unsupported(
                "This directory has more than 10,000 children. Use external file tools.",
            ));
        }
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|_| {
            BackendError::new(
                ErrorCode::UnsupportedEncoding,
                "A directory entry is not valid UTF-8.",
            )
        })?;
        entries.push((name, entry.path()));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}
pub(super) fn metadata_identity(path: &Path) -> Result<String, BackendError> {
    let metadata = fs::symlink_metadata(path)?;
    let mut hash = Sha256::new();
    field(
        &mut hash,
        format!(
            "{:?}:{:?}:{}:{}:{}",
            metadata.modified()?,
            metadata.created().ok(),
            metadata.len(),
            metadata.is_dir(),
            metadata.permissions().readonly()
        )
        .as_bytes(),
    );
    if !is_link(&metadata) && (metadata.is_dir() || metadata.is_file()) {
        let file = open_no_follow(path)?;
        field(&mut hash, &file_identity(&file)?);
    }
    Ok(format!("{:x}", hash.finalize()))
}
#[cfg(windows)]
fn file_identity(file: &File) -> Result<Vec<u8>, BackendError> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    #[derive(Default)]
    struct FileInfo {
        attributes: u32,
        creation: [u32; 2],
        access: [u32; 2],
        write: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut FileInfo) -> i32;
    }
    let mut info = FileInfo::default();
    // SAFETY: handle remains owned by File; info has the documented Win32 layout.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok([
        info.volume.to_le_bytes(),
        info.index_high.to_le_bytes(),
        info.index_low.to_le_bytes(),
    ]
    .concat())
}
#[cfg(unix)]
fn file_identity(file: &File) -> Result<Vec<u8>, BackendError> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    Ok([
        m.dev().to_le_bytes(),
        m.ino().to_le_bytes(),
        m.ctime().to_le_bytes(),
        m.ctime_nsec().to_le_bytes(),
    ]
    .concat())
}
#[cfg(not(any(windows, unix)))]
fn file_identity(_: &File) -> Result<Vec<u8>, BackendError> {
    Err(unsupported(
        "File identities are unavailable on this platform.",
    ))
}
pub(super) fn hash_file(
    path: &Path,
    hash: &mut Sha256,
    total: &mut u64,
) -> Result<(), BackendError> {
    let before = metadata_identity(path)?;
    let mut file = open_no_follow(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(unsupported("Only ordinary files can be read."));
    }
    if metadata.len() > MAX_CONTENT_BYTES.saturating_sub(*total) {
        return Err(unsupported(
            "Source contents exceed the 512 MiB safety limit.",
        ));
    }
    let mut buffer = [0u8; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        *total += read as u64;
        if *total > MAX_CONTENT_BYTES {
            return Err(unsupported(
                "Source contents exceed the 512 MiB safety limit.",
            ));
        }
        hash.update(&buffer[..read]);
    }
    if metadata_identity(path)? != before {
        return Err(stale());
    }
    Ok(())
}
pub(super) fn same_volume(a: &Path, b: &Path) -> Result<bool, BackendError> {
    let a = file_identity(&open_no_follow(a)?)?;
    let b = file_identity(&open_no_follow(b)?)?;
    #[cfg(windows)]
    let bytes = 4;
    #[cfg(not(windows))]
    let bytes = 8;
    Ok(a.get(..bytes) == b.get(..bytes))
}
pub(super) fn parent_identity(
    context: &FileContext,
    relative: &str,
) -> Result<String, BackendError> {
    let path = context.path(relative, false)?;
    if !fs::symlink_metadata(&path)?.is_dir() {
        return Err(unsupported("The parent must be a directory."));
    }
    let mut hash = Sha256::new();
    field(&mut hash, metadata_identity(&path)?.as_bytes());
    for (name, child) in children(&path)? {
        if is_git_name(&name) || context.is_private_entry(&child)? {
            continue;
        }
        field(&mut hash, name.as_bytes());
        field(&mut hash, metadata_identity(&child)?.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceSummary {
    pub fingerprint: String,
    pub kind: FileEntryKind,
    pub nodes: usize,
    pub files: usize,
    pub directories: usize,
    pub bytes: u64,
}
pub(super) fn source_summary(
    context: &FileContext,
    relative: &str,
) -> Result<SourceSummary, BackendError> {
    let path = context.path(relative, false)?;
    let kind = if fs::symlink_metadata(&path)?.is_dir() {
        FileEntryKind::Directory
    } else {
        FileEntryKind::File
    };
    let mut summary = SourceSummary {
        fingerprint: String::new(),
        kind,
        nodes: 0,
        files: 0,
        directories: 0,
        bytes: 0,
    };
    let mut hash = Sha256::new();
    walk(context, relative, 0, &mut summary, &mut hash)?;
    summary.fingerprint = format!("{:x}", hash.finalize());
    Ok(summary)
}
fn walk(
    context: &FileContext,
    relative: &str,
    depth: usize,
    summary: &mut SourceSummary,
    hash: &mut Sha256,
) -> Result<(), BackendError> {
    summary.nodes += 1;
    if depth > MAX_DEPTH || summary.nodes > MAX_NODES {
        return Err(unsupported(
            "Directory operations allow at most 10,000 nodes and 64 levels.",
        ));
    }
    let path = context.path(relative, false)?;
    let meta: Metadata = fs::symlink_metadata(&path)?;
    field(hash, relative.as_bytes());
    let before = metadata_identity(&path)?;
    field(hash, before.as_bytes());
    if meta.is_file() {
        summary.files += 1;
        hash_file(&path, hash, &mut summary.bytes)?;
    } else if meta.is_dir() {
        summary.directories += 1;
        for (name, _) in children(&path)? {
            walk(
                context,
                &format!("{relative}/{name}"),
                depth + 1,
                summary,
                hash,
            )?;
        }
    } else {
        return Err(unsupported("Special files cannot be mutated."));
    }
    if metadata_identity(&path)? != before {
        return Err(stale());
    }
    Ok(())
}
