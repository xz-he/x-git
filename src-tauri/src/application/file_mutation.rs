use super::{
    file_fingerprint::{
        FileContext, SourceSummary, parent_identity, resolved_git_path, same_volume,
        source_summary, stale,
    },
    file_service::FileService,
    operation_state::{
        MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
    },
};
use crate::{
    domain::{
        error::{BackendError, ErrorCode},
        files::*,
    },
    infrastructure::repository_paths::{
        is_link, unsupported, utf8_path, validate_name, validate_relative,
    },
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const MAX_PREPARED: usize = 128;
const PREPARED_LIFETIME: Duration = Duration::from_secs(10 * 60);
#[derive(Debug, Clone)]
pub(super) struct PreparedRecord {
    root: PathBuf,
    intent: FileOperationIntent,
    identity: OperationIdentity,
    created: Instant,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct OperationIdentity {
    repository: String,
    parent: String,
    parent_identity: String,
    source: Option<String>,
    target: Option<String>,
    summary: SourceSummary,
}

async fn allowed(context: &FileContext, service: &FileService) -> Result<(), BackendError> {
    ensure_mutation_allowed(
        &read_operation_state(&context.root, &service.runner).await?,
        MutationIntent::Files,
    )?;
    for name in [
        "REVERT_HEAD",
        "BISECT_LOG",
        "sequencer",
        "rebase-merge",
        "rebase-apply",
    ] {
        match fs::symlink_metadata(context.git_dir.join(name)) {
            Ok(_) => {
                return Err(BackendError::new(
                    ErrorCode::GitOperationInProgress,
                    "Finish the active Git operation before modifying files.",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn split_parent(relative: &str) -> (&str, &str) {
    relative.rsplit_once('/').unwrap_or(("", relative))
}
fn joined(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else {
        format!("{parent}/{name}")
    }
}
fn absent(context: &FileContext, target: &str) -> Result<(), BackendError> {
    validate_relative(target, false)?;
    let (parent, _) = split_parent(target);
    context.path(parent, false)?;
    let path = context.root.join(target);
    if context.private.iter().any(|p| path.starts_with(p)) {
        return Err(unsupported("Git metadata is restricted."));
    }
    match fs::symlink_metadata(path) {
        Ok(_) => Err(BackendError::new(
            ErrorCode::FileAlreadyExists,
            "The destination already exists; overwriting is not allowed.",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn identity(
    context: &FileContext,
    intent: &FileOperationIntent,
) -> Result<OperationIdentity, BackendError> {
    let (parent, source, target, kind) = match intent {
        FileOperationIntent::CreateFile { parent_dir, name }
        | FileOperationIntent::CreateDirectory { parent_dir, name } => {
            validate_relative(parent_dir, true)?;
            validate_name(name)?;
            (
                parent_dir.clone(),
                None,
                Some(joined(parent_dir, name)),
                if matches!(intent, FileOperationIntent::CreateFile { .. }) {
                    FileEntryKind::File
                } else {
                    FileEntryKind::Directory
                },
            )
        }
        FileOperationIntent::Rename {
            relative_path,
            new_name,
        } => {
            validate_relative(relative_path, false)?;
            validate_name(new_name)?;
            let (parent, name) = split_parent(relative_path);
            #[cfg(windows)]
            if name.eq_ignore_ascii_case(new_name) {
                return Err(unsupported(
                    "Case-only renames are not supported on Windows.",
                ));
            }
            #[cfg(windows)]
            {
                let source = context.path(relative_path, false)?;
                let destination = context.root.join(joined(parent, new_name));
                if let Ok(metadata) = fs::symlink_metadata(&destination)
                    && !is_link(&metadata)
                    && source.canonicalize()? == destination.canonicalize()?
                {
                    return Err(unsupported(
                        "Case-only renames are not supported on Windows.",
                    ));
                }
            }
            #[cfg(not(windows))]
            let _ = name;
            (
                parent.into(),
                Some(relative_path.clone()),
                Some(joined(parent, new_name)),
                FileEntryKind::File,
            )
        }
        FileOperationIntent::Delete { relative_path } => {
            validate_relative(relative_path, false)?;
            (
                split_parent(relative_path).0.into(),
                Some(relative_path.clone()),
                None,
                FileEntryKind::File,
            )
        }
    };
    let parent_identity = parent_identity(context, &parent)?;
    if let Some(target) = &target {
        absent(context, target)?;
    }
    let summary = if let Some(source) = &source {
        source_summary(context, source)?
    } else {
        SourceSummary {
            fingerprint: String::new(),
            kind,
            nodes: 1,
            files: usize::from(kind == FileEntryKind::File),
            directories: usize::from(kind == FileEntryKind::Directory),
            bytes: 0,
        }
    };
    Ok(OperationIdentity {
        repository: context.identity.clone(),
        parent,
        parent_identity,
        source,
        target,
        summary,
    })
}
pub(super) async fn prepare(
    service: &FileService,
    path: &Path,
    intent: FileOperationIntent,
) -> Result<PreparedFileOperation, BackendError> {
    match &intent {
        FileOperationIntent::CreateFile { parent_dir, name }
        | FileOperationIntent::CreateDirectory { parent_dir, name } => {
            validate_relative(parent_dir, true)?;
            validate_name(name)?;
        }
        FileOperationIntent::Rename {
            relative_path,
            new_name,
        } => {
            validate_relative(relative_path, false)?;
            validate_name(new_name)?;
        }
        FileOperationIntent::Delete { relative_path } => validate_relative(relative_path, false)?,
    }
    let root = service.root(path).await?;
    let _lease = service.coordinator.read(&root).await;
    let context = FileContext::read(&root, &service.runner).await?;
    allowed(&context, service).await?;
    let identity = identity(&context, &intent)?;
    let record = PreparedRecord {
        root,
        intent: intent.clone(),
        identity,
        created: Instant::now(),
    };
    validate(service, &record).await?;
    let token = Uuid::new_v4().to_string();
    let result = PreparedFileOperation {
        intent,
        token: token.clone(),
        source_path: record.identity.source.clone(),
        target_path: record.identity.target.clone(),
        entry_kind: record.identity.summary.kind,
        node_count: record.identity.summary.nodes,
        file_count: record.identity.summary.files,
        directory_count: record.identity.summary.directories,
        total_bytes: record.identity.summary.bytes,
    };
    let mut prepared = service.prepared.lock().await;
    prepared.retain(|_, record| record.created.elapsed() < PREPARED_LIFETIME);
    if prepared.len() >= MAX_PREPARED
        && let Some(key) = prepared
            .iter()
            .min_by_key(|(_, r)| r.created)
            .map(|(k, _)| k.clone())
    {
        prepared.remove(&key);
    }
    prepared.insert(token, record);
    Ok(result)
}
async fn validate(
    service: &FileService,
    record: &PreparedRecord,
) -> Result<FileContext, BackendError> {
    let context = FileContext::read(&record.root, &service.runner)
        .await
        .map_err(|_| stale())?;
    allowed(&context, service).await.map_err(|_| stale())?;
    if identity(&context, &record.intent).map_err(|_| stale())? != record.identity {
        return Err(stale());
    }
    Ok(context)
}
pub(super) async fn execute(
    service: &FileService,
    path: &Path,
    request: ExecuteFileOperationRequest,
) -> Result<FileMutationResult, BackendError> {
    let root = service.root(path).await?;
    let _lease = service.coordinator.write(&root).await;
    // Consume once, including failed attempts. A caller must prepare again after any failure.
    let record = service
        .prepared
        .lock()
        .await
        .remove(&request.token)
        .ok_or_else(stale)?;
    if record.root != root
        || record.intent != request.intent
        || record.created.elapsed() >= PREPARED_LIFETIME
    {
        return Err(stale());
    }
    let context = validate(service, &record).await?;
    let recovery = if matches!(record.intent, FileOperationIntent::Delete { .. }) {
        Some(prepare_recovery(service, &context, &record).await?)
    } else {
        None
    };
    // External tools cannot take this lease: residual filesystem TOCTOU remains between
    // this final no-follow/content/index check and the exclusive OS operation below.
    validate(service, &record).await?;
    if let Some(payload) = &recovery {
        validate_recovery(&context, payload)?;
    }
    let target = record.identity.target.as_ref().map(|p| root.join(p));
    let source = record.identity.source.as_ref().map(|p| root.join(p));
    match &record.intent {
        FileOperationIntent::CreateFile { .. } => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target.as_ref().unwrap())
                .map_err(map_mutation_error)?;
        }
        FileOperationIntent::CreateDirectory { .. } => {
            fs::create_dir(target.as_ref().unwrap()).map_err(map_mutation_error)?
        }
        FileOperationIntent::Rename { .. } => {
            rename_no_replace(source.as_ref().unwrap(), target.as_ref().unwrap())?
        }
        FileOperationIntent::Delete { .. } => {
            rename_no_replace(source.as_ref().unwrap(), recovery.as_ref().unwrap())?
        }
    }
    let result = FileMutationResult {
        applied: true,
        workspace: None,
        operation_state: None,
        affected_directories: vec![record.identity.parent],
        selected_path: record.identity.target,
        recovery_path: recovery.as_deref().map(utf8_path).transpose()?,
        error: None,
    };
    Ok(refresh_applied(&root, &service.runner, result).await)
}

async fn refresh_applied(
    root: &Path,
    runner: &crate::infrastructure::git_runner::GitCommandRunner,
    mut result: FileMutationResult,
) -> FileMutationResult {
    match refresh_mutation_workspace(root, runner).await {
        Ok(workspace) => {
            result.workspace = Some(workspace.workspace);
            result.operation_state = Some(workspace.operation_state);
        }
        Err(error) => result.error = Some(error),
    }
    result
}
async fn prepare_recovery(
    service: &FileService,
    context: &FileContext,
    record: &PreparedRecord,
) -> Result<PathBuf, BackendError> {
    let recovery =
        resolved_git_path(&context.root, &service.runner, "hq-git-file-recovery").await?;
    // Resolve from Git but never accept a path outside the actual worktree-private directory.
    if recovery
        .parent()
        .map(Path::canonicalize)
        .transpose()?
        .as_deref()
        != Some(context.git_dir.as_path())
    {
        return Err(unsupported(
            "Git recovery path must be inside the repository-private directory.",
        ));
    }
    // Git may return a DOS absolute path while Rust canonical roots use the Windows
    // extended-length prefix. Compare canonical parents and retain that representation.
    let recovery = context.git_dir.join("hq-git-file-recovery");
    if !same_volume(&context.root, &context.git_dir)? {
        return Err(unsupported(
            "Recovery requires the same filesystem volume; the source was not moved.",
        ));
    }
    ensure_recovery_directory(&recovery)?;
    let record_dir = recovery.join(Uuid::new_v4().to_string());
    fs::create_dir(&record_dir)?;
    let payload = record_dir.join("payload");
    utf8_path(&payload)?;
    let metadata = serde_json::json!({"version":1,"originalPath":record.identity.source,"entryKind":record.identity.summary.kind,"fingerprint":record.identity.summary.fingerprint,"createdAtUnixMillis":SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|unsupported("System clock precedes the Unix epoch."))?.as_millis(),"nodeCount":record.identity.summary.nodes,"fileCount":record.identity.summary.files,"directoryCount":record.identity.summary.directories,"totalBytes":record.identity.summary.bytes,"recoveryInstructions":"Move payload back to originalPath only when the original destination does not exist. No automatic cleanup is performed."});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(record_dir.join("metadata.json"))?;
    file.write_all(&serde_json::to_vec_pretty(&metadata).map_err(|_| {
        BackendError::new(
            ErrorCode::Unexpected,
            "Could not serialize recovery metadata.",
        )
    })?)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        fs::File::open(&record_dir)?.sync_all()?;
        fs::File::open(&recovery)?.sync_all()?;
    }
    validate_recovery(context, &payload)?;
    Ok(payload)
}
fn ensure_recovery_directory(path: &Path) -> Result<(), BackendError> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(path)?;
    if is_link(&meta) || !meta.is_dir() {
        return Err(unsupported(
            "The recovery directory is not an ordinary directory.",
        ));
    }
    Ok(())
}
fn validate_recovery(context: &FileContext, payload: &Path) -> Result<(), BackendError> {
    let relative = payload
        .strip_prefix(&context.git_dir)
        .map_err(|_| unsupported("Recovery path escaped Git metadata."))?;
    let mut current = context.git_dir.clone();
    for component in relative
        .parent()
        .ok_or_else(|| unsupported("Invalid recovery path."))?
        .components()
    {
        current.push(component);
        let meta = fs::symlink_metadata(&current)?;
        if is_link(&meta)
            || !meta.is_dir()
            || !current.canonicalize()?.starts_with(&context.git_dir)
        {
            return Err(unsupported("Recovery parent is unsafe."));
        }
    }
    match fs::symlink_metadata(payload) {
        Ok(_) => Err(BackendError::new(
            ErrorCode::FileAlreadyExists,
            "The recovery payload already exists.",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn map_mutation_error(error: std::io::Error) -> BackendError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        BackendError::new(
            ErrorCode::FileAlreadyExists,
            "The destination already exists; no file was overwritten.",
        )
    } else {
        error.into()
    }
}
#[cfg(windows)]
fn rename_no_replace(source: &Path, target: &Path) -> Result<(), BackendError> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // MOVEFILE_WRITE_THROUGH only: neither REPLACE_EXISTING nor COPY_ALLOWED.
    // SAFETY: both pointers refer to live NUL-terminated UTF-16 buffers.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x8) } == 0 {
        return Err(map_mutation_error(std::io::Error::last_os_error()));
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, target: &Path) -> Result<(), BackendError> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const std::ffi::c_char,
            newdirfd: i32,
            newpath: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| unsupported("Invalid source path."))?;
    let target = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| unsupported("Invalid target path."))?;
    // SAFETY: valid C strings; AT_FDCWD and RENAME_NOREPLACE are fixed Linux ABI values.
    if unsafe { renameat2(-100, source.as_ptr(), -100, target.as_ptr(), 1) } != 0 {
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(22 | 38 | 95)) {
            return Err(unsupported(
                "This filesystem does not support atomic no-replace renames.",
            ));
        }
        return Err(map_mutation_error(error));
    }
    Ok(())
}
#[cfg(not(any(windows, target_os = "linux")))]
fn rename_no_replace(_source: &Path, _target: &Path) -> Result<(), BackendError> {
    Err(unsupported(
        "Atomic no-replace renames are unavailable on this platform.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn applied_result_keeps_recovery_information_when_real_git_refresh_fails() {
        let directory = tempfile::tempdir().unwrap();
        crate::infrastructure::git_runner::GitCommandRunner::default()
            .run(Some(directory.path()), ["init", "-b", "main"])
            .await
            .unwrap();
        let source = directory.path().join("source");
        let payload = directory.path().join(".git/payload");
        fs::write(&source, "recover me").unwrap();
        rename_no_replace(&source, &payload).unwrap();
        // A real repository becomes unreadable only after the source move succeeds.
        fs::write(directory.path().join(".git/config"), "[invalid").unwrap();
        let result = FileMutationResult {
            applied: true,
            workspace: None,
            operation_state: None,
            affected_directories: vec![String::new()],
            selected_path: None,
            recovery_path: Some(utf8_path(&payload).unwrap()),
            error: None,
        };
        let result = refresh_applied(
            directory.path(),
            &crate::infrastructure::git_runner::GitCommandRunner::default(),
            result,
        )
        .await;
        assert!(result.applied);
        assert!(result.workspace.is_none());
        assert!(result.operation_state.is_none());
        assert!(result.error.is_some());
        assert_eq!(
            fs::read_to_string(result.recovery_path.unwrap()).unwrap(),
            "recover me"
        );
        assert!(!source.exists());
    }
    #[test]
    fn native_no_replace_preserves_both_files_when_destination_exists() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, "source").unwrap();
        fs::write(&target, "target").unwrap();
        assert_eq!(
            rename_no_replace(&source, &target).unwrap_err().code,
            ErrorCode::FileAlreadyExists
        );
        assert_eq!(fs::read_to_string(&source).unwrap(), "source");
        assert_eq!(fs::read_to_string(&target).unwrap(), "target");
    }
}
