use super::{
    file_fingerprint::{FileContext, resolved_git_path},
    file_service::FileService,
    operation_state::{
        MutationIntent, ensure_mutation_allowed, read_operation_state, refresh_mutation_workspace,
    },
};
use crate::{
    domain::{
        error::{BackendError, ErrorCode},
        files::*,
        operation::MutationWorkspace,
    },
    infrastructure::repository_paths::{
        is_link, open_no_follow, unsupported, utf8_path, validate_relative,
    },
};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_IGNORE_BYTES: u64 = 1024 * 1024;
const MAX_RULE_BYTES: usize = 4096;
const MAX_INSPECTION_BYTES: usize = 2 * 1024 * 1024;
#[cfg(windows)]
const SHELL_SHOW_NORMAL: i32 = 1;
#[cfg(windows)]
const SHELL_ERROR_MAX: isize = 32;
static IGNORE_WRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn invalid(message: &str) -> BackendError {
    BackendError::new(ErrorCode::InvalidPath, message)
}
fn literal(value: &str) -> String {
    let mut result = String::new();
    for c in value.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']' | '#' | '!' | ' ') {
            result.push('\\');
        }
        result.push(c);
    }
    result
}
pub(super) fn ignore_pattern(request: &IgnoreRequest) -> Result<String, BackendError> {
    validate_relative(&request.relative_path, false)?;
    let pattern = match &request.rule {
        IgnoreRule::Exact => format!("/{}", literal(&request.relative_path)),
        IgnoreRule::Extension => {
            let name = request.relative_path.rsplit('/').next().unwrap_or("");
            let (_, extension) = name
                .rsplit_once('.')
                .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
                .ok_or_else(|| invalid("该文件没有可忽略的扩展名。"))?;
            format!("*.{}", literal(extension))
        }
        IgnoreRule::Directory { directory } => {
            validate_relative(directory, false)?;
            if !request.relative_path.starts_with(&format!("{directory}/")) {
                return Err(invalid("请选择该文件所在的父目录。"));
            }
            format!("/{}/", literal(directory))
        }
        IgnoreRule::Custom { pattern } => pattern.clone(),
    };
    if pattern.trim().is_empty()
        || pattern.len() > MAX_RULE_BYTES
        || pattern.contains(['\r', '\n', '\0'])
        || pattern.starts_with('#')
    {
        return Err(invalid(
            "请输入单行有效的 Git 忽略规则，不支持空规则、注释或换行。",
        ));
    }
    Ok(pattern)
}

// Append only; never rewrite or normalize an existing user's rules.
fn append_rule(path: &Path, pattern: &str) -> Result<(), BackendError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link(&metadata) => {
                return Err(unsupported("忽略文件及父目录不能是符号链接或重解析点。"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut existing = String::new();
    if path.exists() {
        let mut file = open_no_follow(path)?;
        if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_IGNORE_BYTES {
            return Err(unsupported("忽略目标必须是小于 1 MiB 的普通文本文件。"));
        }
        file.read_to_string(&mut existing)
            .map_err(|_| unsupported("忽略文件不是 UTF-8 文本，未修改原文件。"))?;
    }
    if existing
        .trim_start_matches('\u{feff}')
        .lines()
        .any(|line| line == pattern)
    {
        return Ok(());
    }
    fs::create_dir_all(path.parent().ok_or_else(|| invalid("忽略文件路径无效。"))?)?;
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let prefix = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        newline
    };
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000);
    }
    let mut file = options.open(path)?;
    if is_link(&file.metadata()?) {
        return Err(unsupported("忽略文件不能是链接。"));
    }
    file.write_all(format!("{prefix}{pattern}{newline}").as_bytes())?;
    file.sync_all()?;
    Ok(())
}

impl FileService {
    async fn ignore_target(
        &self,
        root: &Path,
        scope: &IgnoreScope,
    ) -> Result<PathBuf, BackendError> {
        match scope {
            IgnoreScope::Repository => Ok(root.join(".gitignore")),
            IgnoreScope::Local => resolved_git_path(root, &self.runner, "info/exclude").await,
            IgnoreScope::Global => {
                let config = self
                    .runner
                    .run_allowing_failure(
                        Some(root),
                        ["config", "--global", "--path", "--get", "core.excludesFile"],
                    )
                    .await?;
                if config.is_success() && !config.stdout.trim().is_empty() {
                    let path = PathBuf::from(config.stdout.trim());
                    if !path.is_absolute() {
                        return Err(invalid("全局 core.excludesFile 必须配置为绝对路径。"));
                    }
                    return Ok(path);
                }
                if config.status_code != Some(1) {
                    config.into_result()?;
                }
                let base = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .or_else(|| std::env::var_os("USERPROFILE"))
                            .map(|p| PathBuf::from(p).join(".config"))
                    })
                    .ok_or_else(|| invalid("无法确定全局忽略文件位置。"))?;
                Ok(base.join("git/ignore"))
            }
        }
    }
    pub async fn ignore_preview(
        &self,
        path: &Path,
        request: &IgnoreRequest,
    ) -> Result<IgnorePreview, BackendError> {
        let root = self.root(path).await?;
        let _guard = self.coordinator.read(&root).await;
        let context = FileContext::read(&root, &self.runner).await?;
        context.path(&request.relative_path, false)?;
        let tracked = self
            .runner
            .run(
                Some(&root),
                [
                    "--literal-pathspecs",
                    "ls-files",
                    "--",
                    &request.relative_path,
                ],
            )
            .await?;
        Ok(IgnorePreview {
            pattern: ignore_pattern(request)?,
            target_path: utf8_path(&self.ignore_target(&root, &request.scope).await?)?,
            tracked: !tracked.stdout.is_empty(),
        })
    }
    pub async fn ignore_file(
        &self,
        path: &Path,
        request: &IgnoreRequest,
    ) -> Result<MutationWorkspace, BackendError> {
        let root = self.root(path).await?;
        let _guard = self.coordinator.write(&root).await;
        ensure_mutation_allowed(
            &read_operation_state(&root, &self.runner).await?,
            MutationIntent::Files,
        )?;
        let context = FileContext::read(&root, &self.runner).await?;
        context.path(&request.relative_path, false)?;
        let pattern = ignore_pattern(request)?;
        let target = self.ignore_target(&root, &request.scope).await?;
        let _global_guard = IGNORE_WRITE_LOCK.lock().await;
        append_rule(&target, &pattern)?;
        refresh_mutation_workspace(&root, &self.runner).await
    }
    pub async fn untrack_file(
        &self,
        path: &Path,
        relative: &str,
    ) -> Result<MutationWorkspace, BackendError> {
        let root = self.root(path).await?;
        let _guard = self.coordinator.write(&root).await;
        ensure_mutation_allowed(
            &read_operation_state(&root, &self.runner).await?,
            MutationIntent::Changes,
        )?;
        let context = FileContext::read(&root, &self.runner).await?;
        context.path(relative, false)?;
        self.runner
            .run(
                Some(&root),
                ["--literal-pathspecs", "rm", "--cached", "--", relative],
            )
            .await?;
        refresh_mutation_workspace(&root, &self.runner).await
    }
    pub async fn track_lfs(
        &self,
        path: &Path,
        relative: &str,
    ) -> Result<MutationWorkspace, BackendError> {
        let root = self.root(path).await?;
        let _guard = self.coordinator.write(&root).await;
        ensure_mutation_allowed(
            &read_operation_state(&root, &self.runner).await?,
            MutationIntent::Files,
        )?;
        let context = FileContext::read(&root, &self.runner).await?;
        context.path(relative, false)?;
        if fs::symlink_metadata(root.join(".gitattributes")).is_ok() {
            context.path(".gitattributes", false)?;
        }
        let pattern = ignore_pattern(&IgnoreRequest {
            relative_path: relative.into(),
            rule: IgnoreRule::Extension,
            scope: IgnoreScope::Repository,
        })?;
        self.runner
            .run(Some(&root), ["lfs", "track", "--", &pattern])
            .await?;
        refresh_mutation_workspace(&root, &self.runner).await
    }
    pub async fn inspect_file(
        &self,
        path: &Path,
        relative: &str,
        kind: FileInspectionKind,
    ) -> Result<String, BackendError> {
        let root = self.root(path).await?;
        let _guard = self.coordinator.read(&root).await;
        validate_relative(relative, false)?;
        if matches!(kind, FileInspectionKind::Blame) {
            FileContext::read(&root, &self.runner)
                .await?
                .path(relative, false)?;
        }
        let args = match kind {
            FileInspectionKind::History => vec![
                "--literal-pathspecs",
                "log",
                "-100",
                "--follow",
                "--date=short",
                "--format=%h  %ad  %an%n%s%n",
                "--",
                relative,
            ],
            FileInspectionKind::Blame => vec![
                "--literal-pathspecs",
                "blame",
                "--date=short",
                "--",
                relative,
            ],
        };
        let output = self
            .runner
            .run_bytes(Some(&root), args, MAX_INSPECTION_BYTES)
            .await?;
        String::from_utf8(output).map_err(|_| unsupported("文件内容不是 UTF-8，无法显示按行追溯。"))
    }
    pub async fn open_file(
        &self,
        path: &Path,
        relative: &str,
        reveal: bool,
    ) -> Result<(), BackendError> {
        let root = self.root(path).await?;
        let _guard = self.coordinator.read(&root).await;
        let context = FileContext::read(&root, &self.runner).await?;
        let full = context.path(relative, false)?;
        open_native(&full, reveal)
    }
}

#[cfg(windows)]
fn open_native(path: &Path, reveal: bool) -> Result<(), BackendError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let display = path.to_string_lossy();
    let display = display
        .strip_prefix("\\\\?\\UNC\\")
        .map(|tail| format!("\\\\{tail}"))
        .unwrap_or_else(|| {
            display
                .strip_prefix("\\\\?\\")
                .unwrap_or(&display)
                .to_owned()
        });
    let file: Vec<u16> = std::ffi::OsStr::new(if reveal { "explorer.exe" } else { &display })
        .encode_wide()
        .chain(Some(0))
        .collect();
    let params: Vec<u16> = format!("/select,\"{display}\"")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            file.as_ptr(),
            if reveal {
                params.as_ptr()
            } else {
                std::ptr::null()
            },
            std::ptr::null(),
            SHELL_SHOW_NORMAL,
        )
    };
    if (result as isize) <= SHELL_ERROR_MAX {
        return Err(unsupported(
            "无法打开文件，请检查文件关联或资源管理器是否可用。",
        ));
    }
    Ok(())
}
#[cfg(not(windows))]
fn open_native(_path: &Path, _reveal: bool) -> Result<(), BackendError> {
    Err(unsupported("当前系统暂不支持外部打开文件。"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_escape_literal_paths_and_validate_directory_boundaries() {
        let mut request = IgnoreRequest {
            relative_path: "tests/a [1].ts".into(),
            rule: IgnoreRule::Exact,
            scope: IgnoreScope::Local,
        };
        assert_eq!(ignore_pattern(&request).unwrap(), "/tests/a\\ \\[1\\].ts");
        request.rule = IgnoreRule::Extension;
        assert_eq!(ignore_pattern(&request).unwrap(), "*.ts");
        request.rule = IgnoreRule::Directory {
            directory: "tests".into(),
        };
        assert_eq!(ignore_pattern(&request).unwrap(), "/tests/");
        request.rule = IgnoreRule::Directory {
            directory: "other".into(),
        };
        assert!(ignore_pattern(&request).is_err());
        request.rule = IgnoreRule::Custom {
            pattern: "foo\nbar".into(),
        };
        assert!(ignore_pattern(&request).is_err());
    }
    #[test]
    fn appending_preserves_existing_crlf_rules_and_deduplicates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ignore");
        fs::write(&path, "# mine\r\nkeep").unwrap();
        append_rule(&path, "/test/").unwrap();
        append_rule(&path, "/test/").unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "# mine\r\nkeep\r\n/test/\r\n"
        );
    }
}
