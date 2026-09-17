use super::terminal_command::{invalid, lex, quote};
use crate::domain::{error::BackendError, terminal::*};
use crate::infrastructure::terminal_pty::{LOCATION_ENV, git_executable};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;
const LIMIT: usize = 80;
const COMMANDS: &[(&str, &str)] = &[
    ("status", "查看工作区与暂存区"),
    ("add", "暂存文件或交互选择修改"),
    ("commit", "提交暂存内容"),
    ("diff", "比较文件与提交差异"),
    ("log", "浏览提交历史"),
    ("show", "查看提交或对象"),
    ("branch", "创建、列出或删除分支"),
    ("switch", "切换分支"),
    ("checkout", "切换分支或恢复文件"),
    ("restore", "恢复工作区或暂存区"),
    ("reset", "重置提交或暂存内容"),
    ("revert", "创建反向提交"),
    ("merge", "合并分支"),
    ("rebase", "变基提交"),
    ("cherry-pick", "应用指定提交"),
    ("fetch", "获取远端更新"),
    ("pull", "拉取并整合远端更新"),
    ("push", "推送提交"),
    ("remote", "管理远程仓库"),
    ("stash", "临时保存或恢复修改"),
    ("tag", "管理标签"),
    ("config", "读取或修改 Git 配置"),
    ("reflog", "查看引用变更历史"),
    ("worktree", "管理工作树"),
    ("clean", "清理未跟踪文件"),
    ("rm", "删除并暂存文件"),
    ("mv", "移动或重命名文件"),
    ("bisect", "二分定位引入问题的提交"),
    ("blame", "查看每行的提交来源"),
    ("help", "查看 Git 帮助"),
    ("init", "初始化 Git 仓库"),
    ("clone", "克隆 Git 仓库"),
    ("submodule", "管理子模块"),
];
fn flags(command: &str) -> &'static [(&'static str, &'static str)] {
    match command {
        "add" => &[
            ("--all", "暂存全部修改"),
            ("--patch", "交互选择修改块"),
            ("--update", "暂存已跟踪文件"),
            ("--intent-to-add", "记录添加意图"),
            ("--dry-run", "预览操作"),
            ("-p", "交互选择修改块"),
        ],
        "commit" => &[
            ("--message", "指定提交说明"),
            ("--amend", "修改最近提交"),
            ("--all", "暂存已跟踪文件并提交"),
            ("--no-edit", "保留原提交说明"),
            ("--patch", "交互选择提交内容"),
            ("-m", "指定提交说明"),
        ],
        "log" => &[
            ("--oneline", "单行显示提交"),
            ("--graph", "绘制提交分支图"),
            ("--all", "显示全部引用"),
            ("--stat", "显示修改统计"),
            ("--max-count=", "限制提交数量"),
            ("--decorate", "显示分支和标签"),
        ],
        "show" => &[
            ("--oneline", "单行显示提交说明"),
            ("--stat", "显示修改统计"),
            ("--name-only", "仅显示路径"),
            ("--format=", "设置显示格式"),
            ("--no-patch", "不显示差异"),
        ],
        "diff" => &[
            ("--cached", "查看暂存区差异"),
            ("--stat", "显示修改统计"),
            ("--name-only", "仅显示路径"),
            ("--word-diff", "按单词比较"),
            ("--color", "启用颜色"),
        ],
        "switch" => &[
            ("--track", "跟踪远程分支"),
            ("--detach", "检出游离提交"),
            ("--create", "创建并切换分支"),
            ("--discard-changes", "丢弃本地修改"),
            ("-c", "创建并切换分支"),
        ],
        "checkout" => &[
            ("--track", "跟踪远程分支"),
            ("--detach", "检出游离提交"),
            ("--force", "强制操作"),
            ("-b", "创建并切换分支"),
            ("--patch", "交互恢复文件"),
        ],
        "branch" => &[
            ("--all", "列出全部分支"),
            ("--remotes", "列出远程分支"),
            ("--delete", "删除分支"),
            ("--move", "重命名分支"),
            ("--show-current", "显示当前分支"),
            ("--track", "设置跟踪分支"),
        ],
        "push" => &[
            ("--set-upstream", "设置上游分支"),
            ("--force-with-lease", "检查远端后强制推送"),
            ("--tags", "推送全部标签"),
            ("--dry-run", "预览推送"),
            ("--delete", "删除远程引用"),
        ],
        "fetch" => &[
            ("--all", "获取全部远端"),
            ("--prune", "清理已删除的远程引用"),
            ("--tags", "获取标签"),
            ("--dry-run", "预览获取操作"),
            ("--depth=", "限制获取深度"),
        ],
        "pull" => &[
            ("--prune", "清理已删除的远程引用"),
            ("--tags", "获取标签"),
            ("--rebase", "变基整合"),
            ("--ff-only", "仅允许快进"),
            ("--no-rebase", "合并整合"),
        ],
        "reset" => &[
            ("--soft", "保留暂存区和工作区"),
            ("--mixed", "重置暂存区"),
            ("--hard", "重置暂存区和工作区"),
            ("--patch", "交互重置"),
        ],
        "restore" => &[
            ("--staged", "恢复暂存区"),
            ("--worktree", "恢复工作区"),
            ("--source=", "指定恢复来源"),
            ("--patch", "交互恢复"),
        ],
        "merge" => &[
            ("--continue", "继续合并"),
            ("--abort", "中止合并"),
            ("--no-commit", "合并但不提交"),
            ("--ff-only", "仅允许快进"),
            ("--squash", "压缩合并"),
        ],
        "rebase" => &[
            ("--continue", "继续变基"),
            ("--abort", "中止变基"),
            ("--skip", "跳过当前提交"),
            ("--interactive", "交互变基"),
            ("--onto", "指定变基目标"),
        ],
        "cherry-pick" | "revert" => &[
            ("--continue", "继续操作"),
            ("--abort", "中止操作"),
            ("--skip", "跳过当前提交"),
            ("--no-commit", "应用修改但不提交"),
        ],
        "tag" => &[
            ("--list", "列出标签"),
            ("--annotate", "创建附注标签"),
            ("--delete", "删除标签"),
            ("--message", "标签说明"),
            ("--force", "替换已有标签"),
        ],
        "status" => &[
            ("--short", "紧凑显示状态"),
            ("--branch", "显示分支信息"),
            ("--porcelain", "稳定机器格式"),
            ("--ignored", "显示忽略文件"),
        ],
        "clean" => &[
            ("--dry-run", "预览将删除的文件"),
            ("-d", "包含未跟踪目录"),
            ("-f", "确认清理"),
            ("-x", "包括忽略的文件"),
        ],
        _ => &[("--help", "查看命令帮助")],
    }
}
/// Fixed read-only queries only. Never executes a user command fragment, alias,
/// pager, editor or fsmonitor. Output and total time are bounded.
pub(crate) async fn query(root: &Path, args: &[&str]) -> Option<String> {
    let mut command = tokio::process::Command::new(git_executable().ok()?);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
        .current_dir(root)
        .args([
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "log.showSignature=false",
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for name in LOCATION_ENV {
        command.env_remove(name);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let operation = async {
        let mut bytes = Vec::new();
        stdout.take(65537).read_to_end(&mut bytes).await.ok()?;
        if bytes.len() > 65536 {
            return None;
        }
        if !child.wait().await.ok()?.success() {
            return None;
        }
        String::from_utf8(bytes).ok()
    };
    let result = tokio::time::timeout(Duration::from_millis(1200), operation)
        .await
        .ok()
        .flatten();
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}
pub async fn complete(
    root: &Path,
    command: &str,
    cursor: usize,
) -> Result<TerminalCompletion, BackendError> {
    match tokio::time::timeout(
        Duration::from_millis(2500),
        complete_inner(root, command, cursor, true),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => complete_inner(root, command, cursor, false).await,
    }
}
async fn complete_inner(
    root: &Path,
    command: &str,
    cursor: usize,
    dynamic: bool,
) -> Result<TerminalCompletion, BackendError> {
    let total = command.encode_utf16().count();
    if cursor > total {
        return Err(invalid("补全光标超出命令范围。"));
    }
    let mut utf16 = 0;
    let mut cursor_byte = command.len();
    for (byte, c) in command.char_indices() {
        if utf16 == cursor {
            cursor_byte = byte;
            break;
        }
        utf16 += c.len_utf16();
        if utf16 > cursor {
            return Err(invalid("补全光标不能位于代理字符中间。"));
        }
    }
    let tokens = lex(command, true)?;
    let current = tokens
        .iter()
        .position(|token| token.start <= cursor_byte && cursor_byte <= token.end);
    let (start, end, prefix) = if let Some(index) = current {
        let token = &tokens[index];
        let prefix = lex(&command[token.start..cursor_byte], true)?
            .first()
            .map(|token| token.value.clone())
            .unwrap_or_default();
        (token.start, token.end, prefix)
    } else {
        (cursor_byte, cursor_byte, String::new())
    };
    let before: Vec<_> = tokens
        .iter()
        .filter(|token| token.end < start || (token.end == start && token.start != start))
        .collect();
    let mut result = TerminalCompletion {
        start: command[..start].encode_utf16().count(),
        end: command[..end].encode_utf16().count(),
        items: Vec::new(),
        has_more: false,
    };
    if before.is_empty() {
        if "git".starts_with(&prefix) {
            add(&mut result, "git", "Git 命令", "command", &prefix);
        }
        return Ok(result);
    }
    if !matches!(
        before[0].value.to_ascii_lowercase().as_str(),
        "git" | "git.exe"
    ) {
        return Ok(result);
    }
    let mut context = dynamic.then(|| root.to_path_buf());
    let mut index = 1;
    let mut subcommand = "";
    while index < before.len() {
        let value = before[index].value.as_str();
        if value == "-C" {
            index += 1;
            if let Some(token) = before.get(index) {
                context = context.and_then(|root| root.join(&token.value).canonicalize().ok());
            } else {
                context = None;
            }
        } else if value == "--git-dir"
            || value == "--work-tree"
            || value.starts_with("--git-dir=")
            || value.starts_with("--work-tree=")
        {
            // These may describe bare/custom layouts. Prefer static suggestions
            // over attaching another repository's branches to an unresolved layout.
            context = None;
            if !value.contains('=') {
                index += 1;
            }
        } else if value == "-c" || value == "--config-env" {
            index += 1;
        } else if value.starts_with('-') {
        } else {
            subcommand = value;
            break;
        }
        index += 1;
    }
    if subcommand.is_empty() {
        for (name, description) in COMMANDS {
            add(&mut result, name, description, "command", &prefix);
        }
        if prefix.starts_with('-') {
            for (name, description) in [
                ("-C", "指定命令工作目录"),
                ("-c", "临时配置 key=value"),
                ("--git-dir=", "指定 Git 元数据目录"),
                ("--work-tree=", "指定工作树"),
                ("--no-pager", "禁用分页器"),
                ("--paginate", "启用分页器"),
            ] {
                add(&mut result, name, description, "flag", &prefix);
            }
        }
        if let Some(root) = context.as_ref() {
            if let Some(aliases) = query(
                root,
                &["config", "--name-only", "--get-regexp", "^alias\\."],
            )
            .await
            {
                for alias in aliases
                    .lines()
                    .filter_map(|line| line.strip_prefix("alias."))
                {
                    add(&mut result, alias, "Git 配置别名", "alias", &prefix);
                }
            }
        }
    } else {
        if prefix.starts_with('-') {
            for (name, description) in flags(subcommand) {
                add(&mut result, name, description, "flag", &prefix);
            }
        }
        let after_dash = before.iter().any(|token| token.value == "--");
        let positional = before
            .iter()
            .skip(index + 1)
            .filter(|token| !token.value.starts_with('-'))
            .count();
        let remote_position = matches!(subcommand, "push" | "pull" | "fetch") && positional == 0;
        if let Some(root) = context.as_ref() {
            if !prefix.starts_with('-') && !after_dash {
                if !remote_position
                    && matches!(
                        subcommand,
                        "switch"
                            | "checkout"
                            | "branch"
                            | "merge"
                            | "rebase"
                            | "reset"
                            | "revert"
                            | "cherry-pick"
                            | "log"
                            | "diff"
                            | "show"
                            | "push"
                            | "pull"
                            | "fetch"
                            | "restore"
                            | "tag"
                    )
                {
                    // Filter literal prefixes in Git before limiting results.
                    // Include nested paths because '*' does not cross '/'.
                    let escaped = escape_ref_pattern(&prefix);
                    let namespaces: &[&str] = if subcommand == "tag" {
                        &["refs/tags/"]
                    } else {
                        &["refs/heads/", "refs/remotes/", "refs/tags/"]
                    };
                    let patterns: Vec<String> = namespaces
                        .iter()
                        .flat_map(|base| {
                            [format!("{base}{escaped}*"), format!("{base}{escaped}*/**")]
                        })
                        .collect();
                    let count = format!("--count={}", LIMIT + 1);
                    let mut arguments =
                        vec!["for-each-ref", count.as_str(), "--format=%(refname)", "--"];
                    arguments.extend(patterns.iter().map(String::as_str));
                    if let Some(refs) = query(root, &arguments).await {
                        if refs.lines().count() > LIMIT {
                            result.has_more = true;
                        }
                        for reference in refs.lines() {
                            for (base, kind, description) in [
                                ("refs/heads/", "branch", "本地分支"),
                                ("refs/remotes/", "branch", "远程分支"),
                                ("refs/tags/", "tag", "标签"),
                            ] {
                                if let Some(value) = reference.strip_prefix(base) {
                                    add(&mut result, value, description, kind, &prefix);
                                }
                            }
                        }
                    }
                }
                if remote_position || (subcommand == "remote" && positional == 1) {
                    if let Some(remotes) = query(root, &["remote"]).await {
                        for remote in remotes.lines() {
                            add(&mut result, remote, "远程仓库", "remote", &prefix);
                        }
                    }
                }
                if matches!(
                    subcommand,
                    "show" | "reset" | "revert" | "cherry-pick" | "diff" | "log"
                ) {
                    if let Some(commits) = query(
                        root,
                        &["log", "-20", "--format=%h%x09%s", "--no-show-signature"],
                    )
                    .await
                    {
                        for line in commits.lines() {
                            if let Some((oid, subject)) = line.split_once('\t') {
                                add(&mut result, oid, subject, "commit", &prefix);
                            }
                        }
                    }
                }
            }
            if !prefix.starts_with('-')
                && (after_dash
                    || matches!(
                        subcommand,
                        "add"
                            | "rm"
                            | "mv"
                            | "restore"
                            | "checkout"
                            | "diff"
                            | "show"
                            | "log"
                            | "blame"
                            | "clean"
                    ))
            {
                complete_paths(root, &prefix, &mut result).await;
            }
        }
        if !after_dash {
            let words: &[(&str, &str)] = match subcommand {
                "stash" => &[
                    ("push", "保存修改"),
                    ("pop", "恢复并删除储藏"),
                    ("apply", "恢复储藏"),
                    ("list", "列出储藏"),
                    ("drop", "删除储藏"),
                ],
                "remote" => &[
                    ("add", "添加远端"),
                    ("remove", "删除远端"),
                    ("set-url", "修改远端地址"),
                    ("show", "查看远端"),
                ],
                "worktree" => &[
                    ("add", "添加工作树"),
                    ("list", "列出工作树"),
                    ("remove", "删除工作树"),
                ],
                _ => &[],
            };
            for (value, description) in words {
                add(&mut result, value, description, "command", &prefix);
            }
        }
    }
    result.items.sort_by(|a, b| a.label.cmp(&b.label));
    result.items.dedup_by(|a, b| a.value == b.value);
    if result.items.len() > LIMIT {
        result.items.truncate(LIMIT);
        result.has_more = true;
    }
    Ok(result)
}
fn escape_ref_pattern(prefix: &str) -> String {
    let mut result = String::new();
    for character in prefix.chars() {
        if matches!(character, '\\' | '*' | '?' | '[' | ']') {
            result.push('\\');
        }
        result.push(character);
    }
    result
}
fn add(result: &mut TerminalCompletion, value: &str, description: &str, kind: &str, prefix: &str) {
    if value.starts_with(prefix) && !value.chars().any(char::is_control) {
        result.items.push(TerminalCompletionItem {
            value: quote(value),
            label: value.to_owned(),
            description: description.to_owned(),
            kind: kind.to_owned(),
        });
    }
}
async fn complete_paths(root: &Path, prefix: &str, result: &mut TerminalCompletion) {
    let normalized = prefix.replace('\\', "/");
    let (parent, _) = normalized
        .rsplit_once('/')
        .map_or(("", normalized.as_str()), |(parent, file)| (parent, file));
    let directory = if normalized.contains('/') {
        root.join(format!("{parent}/"))
    } else {
        root.to_owned()
    };
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
        return;
    };
    // Read directory entries lazily without statting every entry. Narrow
    // prefixes can find matches beyond the initial page; total scan is bounded.
    let scan_started = std::time::Instant::now();
    for count in 0..4096 {
        let Ok(Some(entry)) = entries.next_entry().await else {
            break;
        };
        if count == 4095 || scan_started.elapsed() > Duration::from_millis(150) {
            result.has_more = true;
            break;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name == ".git" {
            continue;
        }
        let mut value = if parent.is_empty() {
            name
        } else {
            format!("{parent}/{name}")
        };
        if !value.starts_with(&normalized) {
            continue;
        }
        let is_dir = entry.file_type().await.is_ok_and(|kind| kind.is_dir());
        if is_dir {
            value.push('/');
        }
        add(
            result,
            &value,
            if is_dir { "目录" } else { "文件" },
            "path",
            &normalized,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn terminal_completion_static_commands_and_utf16_path_ranges() {
        let directory = tempfile::tempdir().unwrap();
        let result = complete(directory.path(), "git che", 7).await.unwrap();
        assert_eq!((result.start, result.end), (4, 7));
        assert!(
            result
                .items
                .iter()
                .any(|item| item.value == "checkout" && !item.description.is_empty())
        );
        std::fs::write(directory.path().join("中文 空间.txt"), "").unwrap();
        let command = "git add -- 中";
        let result = complete(directory.path(), command, command.encode_utf16().count())
            .await
            .unwrap();
        assert!(
            result
                .items
                .iter()
                .any(|item| item.label == "中文 空间.txt")
        );
        let item = result
            .items
            .iter()
            .find(|item| item.label == "中文 空间.txt")
            .unwrap();
        assert_eq!(
            crate::application::terminal_command::parse(&format!("git add -- {}", item.value))
                .unwrap()
                .last()
                .unwrap(),
            "中文 空间.txt"
        );
    }
    #[tokio::test]
    async fn terminal_completion_reads_refs_and_aliases_but_not_alias_scripts() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for args in [
            vec!["init", "-q"],
            vec!["config", "alias.danger", "!touch must-not-exist"],
            vec!["symbolic-ref", "HEAD", "refs/heads/test-main"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let result = complete(root, "git dan", 7).await.unwrap();
        assert!(
            result
                .items
                .iter()
                .any(|item| item.label == "danger" && item.kind == "alias")
        );
        assert!(!root.join("must-not-exist").exists());
        let result = complete(root, "git -C missing checkout te", 26)
            .await
            .unwrap();
        assert!(!result.items.iter().any(|item| item.kind == "branch"));
    }
    #[test]
    fn terminal_completion_flags_are_command_specific() {
        for (command, invalid) in [
            ("branch", "--create"),
            ("branch", "-b"),
            ("checkout", "--create"),
            ("fetch", "--rebase"),
            ("fetch", "--ff-only"),
            ("rebase", "--no-commit"),
            ("show", "--graph"),
        ] {
            assert!(
                !flags(command).iter().any(|(flag, _)| *flag == invalid),
                "{command} incorrectly suggests {invalid}"
            );
        }
    }
    #[tokio::test]
    async fn terminal_completion_push_positions_and_tags() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for args in [
            vec!["init", "-q"],
            vec![
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "test",
            ],
            vec!["branch", "feature-test"],
            vec!["tag", "v1-test"],
            vec!["remote", "add", "origin", "."],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let result = complete(root, "git push ", 9).await.unwrap();
        assert!(result.items.iter().any(|item| item.label == "origin"));
        assert!(!result.items.iter().any(|item| item.kind == "branch"));
        let result = complete(root, "git push origin fe", 18).await.unwrap();
        assert!(result.items.iter().any(|item| item.label == "feature-test"));
        assert!(!result.items.iter().any(|item| item.kind == "remote"));
        assert!(
            complete(root, "git tag v1", 10)
                .await
                .unwrap()
                .items
                .iter()
                .any(|item| item.label == "v1-test")
        );
    }

    #[tokio::test]
    async fn terminal_completion_filters_before_ref_limit_and_escapes_patterns() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for args in [
            vec!["init", "-q"],
            vec![
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "test",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .current_dir(root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let oid = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let oid = String::from_utf8(oid.stdout).unwrap();
        let mut updates = (0..220)
            .map(|n| format!("create refs/heads/aaa-{n:03} {}\n", oid.trim()))
            .collect::<String>();
        for reference in ["refs/heads/zzneedle/topic/nested", "refs/tags/release-test"] {
            updates.push_str(&format!("create {reference} {}\n", oid.trim()));
        }
        let mut child = std::process::Command::new("git")
            .current_dir(root)
            .args(["update-ref", "--stdin"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(updates.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        for (command, expected) in [
            ("git switch zzneedle", "zzneedle/topic/nested"),
            ("git tag release", "release-test"),
        ] {
            let result = complete(root, command, command.encode_utf16().count())
                .await
                .unwrap();
            assert!(
                result.items.iter().any(|item| item.label == expected),
                "missing {expected}: {result:?}"
            );
        }
        let command = "git switch z*";
        assert!(
            complete(root, command, command.len())
                .await
                .unwrap()
                .items
                .is_empty()
        );
        assert!(complete(root, "git switch ", 11).await.unwrap().has_more);
    }
}
