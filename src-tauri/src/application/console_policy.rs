//! Pure input policy. Only reconstructed arguments leave this module.
use crate::domain::console::{ConsoleQuery, QueryKind};
use crate::domain::error::{BackendError, ErrorCode};
use std::collections::HashSet;

fn invalid(reason: &str) -> BackendError {
    BackendError::new(
        ErrorCode::InvalidConsoleCommand,
        format!("{reason} 支持示例：git status；git log --oneline -n 50；git diff --stat。"),
    )
}

pub fn parse_command(input: &str) -> Result<ConsoleQuery, BackendError> {
    let words = lex(input)?;
    if words.first().map(String::as_str) != Some("git") || words.len() < 2 {
        return Err(invalid("命令必须以小写 git 和支持的查询名称开头。"));
    }
    let kind = match words[1].as_str() {
        "status" => QueryKind::Status,
        "log" => QueryKind::Log,
        "diff" => QueryKind::Diff,
        "show" => QueryKind::Show,
        "branch" => QueryKind::Branch,
        "tag" => QueryKind::Tag,
        "remote" => QueryKind::Remote,
        "stash" => QueryKind::Stash,
        _ => return Err(invalid("不支持此命令；首版仅支持八类只读查询。")),
    };
    let mut query = ConsoleQuery {
        kind,
        options: Vec::new(),
        revision: None,
        paths: Vec::new(),
    };
    let mut seen = HashSet::new();
    let mut i = 2;
    while i < words.len() {
        let word = words[i].as_str();
        if kind == QueryKind::Diff && word == "--" {
            query.paths = words[i + 1..].to_vec();
            if query.paths.is_empty()
                || query.paths.len() > 32
                || query.paths.iter().any(|p| !valid_path(p))
            {
                return Err(invalid(
                    "-- 后需要 1–32 个安全的仓库相对路径，使用 / 分隔且不能包含 .git、. 或 .. 路径段。",
                ));
            }
            break;
        }
        let (group, fixed) = match (kind, word) {
            (QueryKind::Status, "--short" | "-s") => ("short", "--short"),
            (QueryKind::Status, "--branch" | "-b") => ("branch", "--branch"),
            (QueryKind::Log, "--oneline") => ("oneline", "--oneline"),
            (QueryKind::Log, "--graph") => ("graph", "--graph"),
            (QueryKind::Log, "--decorate") => ("decorate", "--decorate"),
            (QueryKind::Log, "--all") => ("all", "--all"),
            (QueryKind::Diff, "--cached" | "--staged") => ("cached", "--cached"),
            (QueryKind::Diff | QueryKind::Show, "--stat") => ("display", "--stat"),
            (QueryKind::Diff | QueryKind::Show, "--name-only") => ("display", "--name-only"),
            (QueryKind::Branch, "--list") => ("list", "--list"),
            (QueryKind::Branch, "--all" | "-a") => ("scope", "--all"),
            (QueryKind::Branch, "--remotes" | "-r") => ("scope", "--remotes"),
            (QueryKind::Branch, "-v") => ("verbose", "-v"),
            (QueryKind::Branch, "-vv") => ("verbose", "-vv"),
            (QueryKind::Tag, "--list" | "-l") => ("list", "--list"),
            (QueryKind::Remote, "-v") => ("verbose", "-v"),
            (QueryKind::Stash, "list") => ("list", "list"),
            (QueryKind::Log, _) if word == "-n" || word.starts_with("--max-count=") => {
                let number = if word == "-n" {
                    i += 1;
                    words.get(i).map(String::as_str).unwrap_or("")
                } else {
                    &word[12..]
                };
                let count = decimal(number, 200)
                    .filter(|n| *n > 0)
                    .ok_or_else(|| invalid("日志条数必须为 1–200 的十进制整数。"))?;
                if !seen.insert("count") {
                    return Err(invalid("不接受重复或互斥选项。"));
                }
                query.options.push(format!("--max-count={count}"));
                i += 1;
                continue;
            }
            (QueryKind::Show, _)
                if !word.starts_with('-') && query.revision.is_none() && valid_revision(word) =>
            {
                query.revision = Some(word.to_owned());
                i += 1;
                continue;
            }
            _ => return Err(invalid("存在未知参数、额外位置参数或不支持的提交引用。")),
        };
        if !seen.insert(group) {
            return Err(invalid("不接受重复或互斥选项。"));
        }
        query.options.push(fixed.to_owned());
        i += 1;
    }
    if kind == QueryKind::Stash && !seen.contains("list") {
        return Err(invalid("仅支持 git stash list。"));
    }
    if kind == QueryKind::Log && !seen.contains("count") {
        query.options.push("--max-count=50".into());
    }
    if kind == QueryKind::Show && query.revision.is_none() {
        query.revision = Some("HEAD".into());
    }
    Ok(query)
}

fn lex(input: &str) -> Result<Vec<String>, BackendError> {
    if input.len() > 4096 || input.chars().any(char::is_control) {
        return Err(invalid(
            "输入最多 4096 UTF-8 字节，不能包含控制字符或换行。",
        ));
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for c in input.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            started = true;
        } else if c.is_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else if matches!(c, ';' | '&' | '|' | '<' | '>' | '`' | '$') {
            return Err(invalid("引号外不允许 shell 操作符或替换符号。"));
        } else {
            word.push(c);
            started = true;
        }
        if words.len() > 64 {
            return Err(invalid("最多允许 64 个词元。"));
        }
    }
    if quote.is_some() {
        return Err(invalid("引号未闭合。"));
    }
    if started {
        words.push(word);
    }
    if words.len() > 64 {
        return Err(invalid("最多允许 64 个词元。"));
    }
    Ok(words)
}

fn decimal(value: &str, max: u16) -> Option<u16> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<u16>().ok().filter(|n| *n <= max)
}
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.eq_ignore_ascii_case(".git"))
}
fn valid_revision(value: &str) -> bool {
    let base = if let Some(index) = value.find(['~', '^']) {
        if decimal(&value[index + 1..], 100).is_none() {
            return false;
        }
        &value[..index]
    } else {
        value
    };
    if base == "HEAD"
        || ((4..=64).contains(&base.len()) && base.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return true;
    }
    !base.is_empty()
        && base != "@"
        && !base.starts_with('-')
        && !base.ends_with('.')
        && !base.contains("..")
        && !base.contains("@{")
        && !base.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && base
            .split('/')
            .all(|p| !p.is_empty() && !p.starts_with('.') && !p.ends_with(".lock"))
}

impl ConsoleQuery {
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_deref()
    }
    pub fn argv(&self, commit_oid: Option<&str>) -> Vec<String> {
        let name = match self.kind {
            QueryKind::Status => "status",
            QueryKind::Log => "log",
            QueryKind::Diff => "diff",
            QueryKind::Show => "show",
            QueryKind::Branch => "branch",
            QueryKind::Tag => "tag",
            QueryKind::Remote => "remote",
            QueryKind::Stash => "stash",
        };
        let mut args = vec![name.into()];
        if matches!(self.kind, QueryKind::Diff | QueryKind::Show) {
            args.extend(["--no-ext-diff", "--no-textconv", "--submodule=short"].map(str::to_owned));
        }
        if self.kind == QueryKind::Diff {
            args.push("--ignore-submodules=all".into());
        }
        if self.kind == QueryKind::Branch && !self.options.iter().any(|o| o == "--list") {
            args.push("--list".into());
        }
        args.extend(self.options.clone());
        if self.kind == QueryKind::Show {
            args.push(
                commit_oid
                    .expect("show requires a verified commit OID")
                    .to_owned(),
            );
            args.push("--".into());
        }
        if !self.paths.is_empty() {
            args.push("--".into());
            args.extend(self.paths.clone());
        }
        args
    }
}
