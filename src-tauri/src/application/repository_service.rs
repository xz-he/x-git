use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::application::mutation_coordinator::RepositoryMutationCoordinator;
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::repository::{RemoteSummary, RepositorySnapshot, UpstreamSummary, VcsKind};
use crate::infrastructure::git_runner::{GitCommandRunner, GitOutput};
use crate::infrastructure::svn_runner::SvnCommandRunner;

#[derive(Debug, Clone)]
pub struct RepositoryService {
    runner: GitCommandRunner,
    svn_runner: SvnCommandRunner,
    coordinator: RepositoryMutationCoordinator,
}

impl Default for RepositoryService {
    fn default() -> Self {
        Self::new(GitCommandRunner::default())
    }
}

impl RepositoryService {
    pub fn new(runner: GitCommandRunner) -> Self {
        Self::with_runners(runner, SvnCommandRunner::default(), RepositoryMutationCoordinator::default())
    }

    pub fn with_runners(
        runner: GitCommandRunner,
        svn_runner: SvnCommandRunner,
        coordinator: RepositoryMutationCoordinator,
    ) -> Self {
        Self {
            runner,
            svn_runner,
            coordinator,
        }
    }

    pub fn with_coordinator(
        runner: GitCommandRunner,
        coordinator: RepositoryMutationCoordinator,
    ) -> Self {
        Self::with_runners(runner, SvnCommandRunner::default(), coordinator)
    }

    pub async fn open_repository(
        &self,
        requested_path: &Path,
    ) -> Result<RepositorySnapshot, BackendError> {
        let requested_path = canonicalize_path(requested_path)?;
        if let Some(root_path) = self.resolve_svn_root(&requested_path).await? {
            let _guard = self.coordinator.read(&root_path).await;
            return self.snapshot_svn_at_root(&root_path).await;
        }
        let root_path = self.resolve_root(&requested_path).await?;
        let _guard = self.coordinator.read(&root_path).await;
        self.snapshot_at_root(&root_path).await
    }

    pub async fn refresh_repository(
        &self,
        requested_path: &Path,
    ) -> Result<RepositorySnapshot, BackendError> {
        let requested_path = canonicalize_path(requested_path)?;
        if let Some(root_path) = self.resolve_svn_root(&requested_path).await? {
            let _guard = self.coordinator.write(&root_path).await;
            self.svn_runner.run(Some(&root_path), ["update"]).await?;
            return self.snapshot_svn_at_root(&root_path).await;
        }
        self.open_repository(&requested_path).await
    }

    async fn resolve_svn_root(&self, requested_path: &Path) -> Result<Option<PathBuf>, BackendError> {
        let result = self
            .svn_runner
            .run_allowing_failure(Some(requested_path), ["info", "--show-item", "wc-root"])
            .await;
        let output = match result {
            Ok(output) if output.is_success() => output,
            Ok(_) | Err(_) => return Ok(None),
        };
        let root = output.stdout.trim();
        if root.is_empty() {
            return Ok(None);
        }
        Ok(Some(canonicalize_path(Path::new(root))?))
    }

    pub(crate) async fn resolve_root(
        &self,
        requested_path: &Path,
    ) -> Result<PathBuf, BackendError> {
        let requested_path = canonicalize_path(requested_path)?;
        let root_output = self
            .runner
            .run(Some(&requested_path), ["rev-parse", "--show-toplevel"])
            .await
            .map_err(map_repository_probe_error)?;
        canonicalize_path(Path::new(root_output.stdout.trim()))
    }

    pub(crate) async fn snapshot_at_root(
        &self,
        root_path: &Path,
    ) -> Result<RepositorySnapshot, BackendError> {
        let status_future = self.runner.run(
            Some(root_path),
            [
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "-z",
                "--branch",
            ],
        );
        let head_future = self.runner.run(
            Some(root_path),
            ["rev-parse", "--verify", "--short", "HEAD"],
        );
        let remotes_future = self.runner.run(Some(root_path), ["remote", "-v"]);
        let (status_result, head_result, remotes_result) =
            tokio::join!(status_future, head_future, remotes_future);

        let status = parse_status(&status_result?.stdout);
        let head_short_hash = parse_optional_head(head_result)?;
        let remotes = parse_remotes(&remotes_result?.stdout);
        let name = root_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root_path.display().to_string());

        Ok(RepositorySnapshot {
            root_path: root_path.to_path_buf(),
            name,
            current_branch: status.current_branch,
            head_short_hash,
            is_clean: status.changed_file_count == 0,
            changed_file_count: status.changed_file_count,
            conflict_count: status.conflict_count,
            remotes,
            upstream: status.upstream,
            vcs: VcsKind::Git,
        })
    }

    async fn snapshot_svn_at_root(&self, root_path: &Path) -> Result<RepositorySnapshot, BackendError> {
        let (status, revision, url) = tokio::try_join!(
            self.svn_runner.run(Some(root_path), ["status", "--xml", "--verbose"]),
            self.svn_runner.run(Some(root_path), ["info", "--show-item", "revision"]),
            self.svn_runner.run(Some(root_path), ["info", "--show-item", "url"]),
        )?;
        let parsed = parse_svn_status(&status.stdout);
        let name = root_path.file_name().map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root_path.display().to_string());
        let revision = revision.stdout.trim().to_owned();
        let url = url.stdout.trim().to_owned();
        Ok(RepositorySnapshot {
            root_path: root_path.to_path_buf(),
            name,
            current_branch: None,
            head_short_hash: (!revision.is_empty()).then_some(revision),
            is_clean: parsed.changed_file_count == 0,
            changed_file_count: parsed.changed_file_count,
            conflict_count: parsed.conflict_count,
            remotes: if url.is_empty() { Vec::new() } else { vec![RemoteSummary { name: "svn".into(), fetch_url: url }] },
            upstream: None,
            vcs: VcsKind::Svn,
        })
    }

    pub async fn init_repository(
        &self,
        target_path: &Path,
    ) -> Result<RepositorySnapshot, BackendError> {
        let _guard = self.coordinator.write(target_path).await;
        let args = vec![
            OsString::from("init"),
            OsString::from("-b"),
            OsString::from("main"),
            OsString::from("--"),
            target_path.as_os_str().to_os_string(),
        ];
        self.runner.run(None, args).await?;
        let root = self.resolve_root(target_path).await?;
        self.snapshot_at_root(&root).await
    }

    pub async fn clone_repository(
        &self,
        url: &str,
        target_path: &Path,
    ) -> Result<RepositorySnapshot, BackendError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(BackendError::new(
                ErrorCode::InvalidPath,
                "请输入要克隆的仓库地址。",
            ));
        }
        ensure_clone_target_is_empty(target_path)?;
        let _guard = self.coordinator.write(target_path).await;

        let args = vec![
            OsString::from("clone"),
            OsString::from("--progress"),
            OsString::from("--"),
            OsString::from(url),
            target_path.as_os_str().to_os_string(),
        ];
        self.runner.run(None, args).await?;
        let root = self.resolve_root(target_path).await?;
        self.snapshot_at_root(&root).await
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedStatus {
    current_branch: Option<String>,
    changed_file_count: usize,
    conflict_count: usize,
    upstream: Option<UpstreamSummary>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedSvnStatus {
    changed_file_count: usize,
    conflict_count: usize,
}

fn parse_svn_status(xml: &str) -> ParsedSvnStatus {
    let mut result = ParsedSvnStatus::default();
    let mut cursor = 0;
    while let Some(start) = xml[cursor..].find("<wc-status").map(|index| cursor + index) {
        let Some(end) = xml[start..].find('>').map(|index| start + index) else { break; };
        let item = xml_attribute(&xml[start..=end], "item").unwrap_or_default();
        if !matches!(item.as_str(), "normal" | "ignored" | "none") {
            result.changed_file_count += 1;
            if item == "conflicted" {
                result.conflict_count += 1;
            }
        }
        cursor = end + 1;
    }
    result
}

fn xml_attribute(tag: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=\"");
    let start = tag.find(&marker)? + marker.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].replace("&quot;", "\"").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">"))
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, BackendError> {
    path.canonicalize().map_err(|error| {
        BackendError::new(ErrorCode::InvalidPath, "无法访问指定路径。")
            .with_diagnostics(error.to_string())
    })
}

fn map_repository_probe_error(error: BackendError) -> BackendError {
    if error.code != ErrorCode::GitCommandFailed {
        return error;
    }

    let mut mapped = BackendError::new(ErrorCode::InvalidRepository, "所选目录不是 Git 仓库。");
    if let Some(diagnostics) = error.diagnostics {
        mapped = mapped.with_diagnostics(diagnostics);
    }
    mapped
}

fn parse_optional_head(
    result: Result<GitOutput, BackendError>,
) -> Result<Option<String>, BackendError> {
    match result {
        Ok(output) => {
            let hash = output.stdout.trim();
            Ok((!hash.is_empty()).then(|| hash.to_owned()))
        }
        Err(error)
            if error.code == ErrorCode::GitCommandFailed
                && error.diagnostics.as_deref().is_some_and(is_missing_head) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn is_missing_head(diagnostics: &str) -> bool {
    let diagnostics = diagnostics.to_ascii_lowercase();
    diagnostics.contains("needed a single revision")
        || diagnostics.contains("unknown revision")
        || diagnostics.contains("ambiguous argument 'head'")
}

fn ensure_clone_target_is_empty(path: &Path) -> Result<(), BackendError> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "克隆目标必须是目录。",
        ));
    }

    let mut entries = path.read_dir().map_err(|error| {
        BackendError::new(ErrorCode::InvalidPath, "无法读取克隆目标目录。")
            .with_diagnostics(error.to_string())
    })?;
    if entries
        .next()
        .transpose()
        .map_err(BackendError::from)?
        .is_some()
    {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "克隆目标目录必须为空。",
        ));
    }
    Ok(())
}

fn parse_status(output: &str) -> ParsedStatus {
    let mut records = output.split('\0');
    let header = records.next().unwrap_or_default();
    let (current_branch, upstream) = parse_branch_header(header);
    let mut changed_file_count = 0;
    let mut conflict_count = 0;
    let mut skip_rename_source = false;

    for record in records {
        if record.is_empty() {
            continue;
        }
        if skip_rename_source {
            skip_rename_source = false;
            continue;
        }

        let status = record.as_bytes();
        if status.len() < 2 {
            continue;
        }
        changed_file_count += 1;
        let pair = &record[..2];
        if matches!(pair, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU") {
            conflict_count += 1;
        }
        skip_rename_source = matches!(status[0], b'R' | b'C') || matches!(status[1], b'R' | b'C');
    }

    ParsedStatus {
        current_branch,
        changed_file_count,
        conflict_count,
        upstream,
    }
}

fn parse_branch_header(header: &str) -> (Option<String>, Option<UpstreamSummary>) {
    let header = header.strip_prefix("## ").unwrap_or(header);
    if let Some(branch) = header
        .strip_prefix("No commits yet on ")
        .or_else(|| header.strip_prefix("Initial commit on "))
    {
        return (Some(branch.trim().to_owned()), None);
    }
    if header.starts_with("HEAD (no branch)") {
        return (None, None);
    }

    let Some((branch, tracking)) = header.split_once("...") else {
        let branch = header.split_whitespace().next().unwrap_or_default();
        return ((!branch.is_empty()).then(|| branch.to_owned()), None);
    };
    let upstream_name = tracking
        .split_once(" [")
        .map_or(tracking, |(name, _)| name)
        .trim();
    let (ahead, behind) = parse_divergence(tracking);
    let upstream = (!upstream_name.is_empty()).then(|| UpstreamSummary {
        name: upstream_name.to_owned(),
        ahead,
        behind,
    });

    (Some(branch.to_owned()), upstream)
}

fn parse_divergence(tracking: &str) -> (u32, u32) {
    let mut ahead = 0;
    let mut behind = 0;
    let Some((_, divergence)) = tracking.split_once(" [") else {
        return (ahead, behind);
    };

    for item in divergence.trim_end_matches(']').split(',') {
        let item = item.trim();
        if let Some(value) = item.strip_prefix("ahead ") {
            ahead = value.parse().unwrap_or(0);
        } else if let Some(value) = item.strip_prefix("behind ") {
            behind = value.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

fn parse_remotes(output: &str) -> Vec<RemoteSummary> {
    output
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('\t')?;
            let fetch_url = value.strip_suffix(" (fetch)")?;
            Some(RemoteSummary {
                name: name.to_owned(),
                fetch_url: fetch_url.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::domain::error::ErrorCode;
    use crate::infrastructure::git_runner::GitCommandRunner;

    async fn init_fixture() -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        let runner = GitCommandRunner::default();
        runner
            .run(Some(directory.path()), ["init", "-b", "main"])
            .await
            .unwrap();
        runner
            .run(Some(directory.path()), ["config", "user.name", "HQ Test"])
            .await
            .unwrap();
        runner
            .run(
                Some(directory.path()),
                ["config", "user.email", "hq@example.test"],
            )
            .await
            .unwrap();
        std::fs::write(directory.path().join("README.md"), "# fixture\n").unwrap();
        runner
            .run(Some(directory.path()), ["add", "README.md"])
            .await
            .unwrap();
        runner
            .run(Some(directory.path()), ["commit", "-m", "initial"])
            .await
            .unwrap();
        directory
    }

    #[tokio::test]
    async fn open_repository_returns_one_clean_snapshot() {
        let directory = init_fixture().await;
        let service = RepositoryService::default();

        let snapshot = service.open_repository(directory.path()).await.unwrap();

        assert_eq!(snapshot.root_path, directory.path().canonicalize().unwrap());
        assert_eq!(
            snapshot.name,
            directory.path().file_name().unwrap().to_string_lossy()
        );
        assert_eq!(snapshot.current_branch.as_deref(), Some("main"));
        assert!(snapshot.head_short_hash.is_some());
        assert!(snapshot.is_clean);
        assert_eq!(snapshot.changed_file_count, 0);
        assert_eq!(snapshot.conflict_count, 0);
        assert!(snapshot.remotes.is_empty());
        assert!(snapshot.upstream.is_none());
    }

    #[tokio::test]
    async fn nested_directory_resolves_to_repository_root() {
        let directory = init_fixture().await;
        let nested = directory.path().join("src").join("feature");
        std::fs::create_dir_all(&nested).unwrap();
        let service = RepositoryService::default();

        let snapshot = service.open_repository(&nested).await.unwrap();

        assert_eq!(snapshot.root_path, directory.path().canonicalize().unwrap());
    }

    #[tokio::test]
    async fn non_repository_returns_invalid_repository() {
        let directory = tempfile::tempdir().unwrap();
        let service = RepositoryService::default();

        let error = service.open_repository(directory.path()).await.unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidRepository);
    }

    #[tokio::test]
    async fn init_repository_creates_main_branch() {
        let parent = tempfile::tempdir().unwrap();
        let target = parent.path().join("new-repository");
        let service = RepositoryService::default();

        let snapshot = service.init_repository(&target).await.unwrap();

        assert_eq!(snapshot.root_path, target.canonicalize().unwrap());
        assert_eq!(snapshot.current_branch.as_deref(), Some("main"));
        assert!(snapshot.head_short_hash.is_none());
        assert!(snapshot.is_clean);
    }

    #[tokio::test]
    async fn clone_repository_supports_a_local_source_path() {
        let source = init_fixture().await;
        let parent = tempfile::tempdir().unwrap();
        let target = parent.path().join("cloned-repository");
        let service = RepositoryService::default();

        let snapshot = service
            .clone_repository(&source.path().to_string_lossy(), &target)
            .await
            .unwrap();

        assert_eq!(snapshot.root_path, target.canonicalize().unwrap());
        assert_eq!(snapshot.name, "cloned-repository");
        assert_eq!(snapshot.current_branch.as_deref(), Some("main"));
        assert_eq!(
            snapshot.remotes,
            vec![crate::domain::repository::RemoteSummary {
                name: "origin".to_owned(),
                fetch_url: source.path().to_string_lossy().into_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn clone_repository_rejects_empty_url_and_non_empty_target() {
        let service = RepositoryService::default();
        let directory = tempfile::tempdir().unwrap();

        let empty_url_error = service
            .clone_repository("", &directory.path().join("clone"))
            .await
            .unwrap_err();
        assert_eq!(empty_url_error.code, ErrorCode::InvalidPath);

        std::fs::write(directory.path().join("occupied.txt"), "occupied").unwrap();
        let source = PathBuf::from("unused-source");
        let non_empty_error = service
            .clone_repository(&source.to_string_lossy(), directory.path())
            .await
            .unwrap_err();
        assert_eq!(non_empty_error.code, ErrorCode::InvalidPath);
    }

    #[test]
    fn status_parser_counts_changes_conflicts_and_upstream_divergence() {
        let status = concat!(
            "## main...origin/main [ahead 2, behind 1]\0",
            " M README.md\0",
            "UU conflict.txt\0",
            "R  renamed.txt\0",
            "old-name.txt\0",
        );

        let parsed = parse_status(status);

        assert_eq!(parsed.current_branch.as_deref(), Some("main"));
        assert_eq!(parsed.changed_file_count, 3);
        assert_eq!(parsed.conflict_count, 1);
        assert_eq!(
            parsed.upstream,
            Some(crate::domain::repository::UpstreamSummary {
                name: "origin/main".to_owned(),
                ahead: 2,
                behind: 1,
            })
        );
    }

    #[test]
    fn remote_parser_keeps_only_fetch_urls() {
        let remotes = parse_remotes(concat!(
            "origin\thttps://example.test/repo.git (fetch)\n",
            "origin\tssh://example.test/repo.git (push)\n",
            "backup\tD:/repos/backup (fetch)\n",
        ));

        assert_eq!(
            remotes,
            vec![
                crate::domain::repository::RemoteSummary {
                    name: "origin".to_owned(),
                    fetch_url: "https://example.test/repo.git".to_owned(),
                },
                crate::domain::repository::RemoteSummary {
                    name: "backup".to_owned(),
                    fetch_url: "D:/repos/backup".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn svn_status_parser_counts_modified_and_conflicted_entries() {
        let xml = concat!(
            "<status><target path=\".\">",
            "<entry path=\"README.md\"><wc-status item=\"modified\" /></entry>",
            "<entry path=\"conflict.txt\"><wc-status item=\"conflicted\" /></entry>",
            "<entry path=\"clean.txt\"><wc-status item=\"normal\" /></entry>",
            "</target></status>"
        );
        assert_eq!(
            parse_svn_status(xml),
            ParsedSvnStatus { changed_file_count: 2, conflict_count: 1 }
        );
    }
}
