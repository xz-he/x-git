use crate::domain::error::{BackendError, ErrorCode};
pub fn parse(command: &str) -> Result<Vec<String>, BackendError> {
    let tokens = lex(command, false)?;
    if !tokens.first().is_some_and(|token| {
        token.value.eq_ignore_ascii_case("git") || token.value.eq_ignore_ascii_case("git.exe")
    }) {
        return Err(invalid("命令必须以 git 或 git.exe 开始。"));
    }
    Ok(tokens
        .into_iter()
        .skip(1)
        .map(|token| token.value)
        .collect())
}
#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub value: String,
    pub start: usize,
    pub end: usize,
}
pub(crate) fn invalid(message: &str) -> BackendError {
    BackendError::new(ErrorCode::InvalidConsoleCommand, message)
}
pub(crate) fn lex(command: &str, partial: bool) -> Result<Vec<Token>, BackendError> {
    if command.len() > 32768 || command.chars().any(|c| matches!(c, '\0' | '\n' | '\r')) {
        return Err(invalid("命令过长或包含换行/NUL。"));
    }
    let mut chars = command.char_indices().peekable();
    let mut tokens = Vec::new();
    while let Some((start, first)) = chars.next() {
        if first.is_whitespace() {
            continue;
        }
        let mut value = String::new();
        let mut quote = None;
        let mut current = Some((start, first));
        let mut end = command.len();
        while let Some((offset, c)) = current.take() {
            match quote {
                Some('\'') => {
                    if c == '\'' {
                        quote = None;
                    } else {
                        value.push(c);
                    }
                }
                Some('"') => {
                    if c == '"' {
                        quote = None;
                    } else if c == '\\' && chars.peek().is_some_and(|(_, next)| *next == '"') {
                        value.push(chars.next().unwrap().1);
                    } else {
                        value.push(c);
                    }
                }
                _ => {
                    if c.is_whitespace() {
                        end = offset;
                        break;
                    }
                    if matches!(c, '\'' | '"') {
                        quote = Some(c);
                    } else if matches!(c, '|' | '&' | ';' | '<' | '>' | '`' | '(' | ')') {
                        return Err(invalid(
                            "不支持 shell 管道、重定向或命令连接；请分别执行 Git 命令。",
                        ));
                    } else {
                        value.push(c);
                    }
                }
            }
            current = chars.next();
        }
        if quote.is_some() && !partial {
            return Err(invalid("引号未闭合。"));
        }
        tokens.push(Token { value, start, end });
    }
    Ok(tokens)
}
pub(crate) fn quote(value: &str) -> String {
    if !value.is_empty()
        && !value.chars().any(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '\'' | '"' | '|' | '&' | ';' | '<' | '>' | '`' | '(' | ')'
                )
        })
    {
        return value.to_owned();
    }
    // Single-quoted pieces preserve every Windows backslash. A literal quote
    // is represented by a double-quoted one between those pieces.
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_parser_preserves_git_arguments_and_windows_paths() {
        assert_eq!(
            parse(r#"git.exe -C "C:\工作 区" commit -m 'hello "world"' --unknown=x"#).unwrap(),
            vec![
                "-C",
                "C:\\工作 区",
                "commit",
                "-m",
                "hello \"world\"",
                "--unknown=x"
            ]
        );
        assert_eq!(
            parse(r#"git config alias.test "!printf 'ok'""#).unwrap(),
            vec!["config", "alias.test", "!printf 'ok'"]
        );
        assert_eq!(
            parse(r#"git commit -m "a\"b" -- path\file"#).unwrap(),
            vec!["commit", "-m", "a\"b", "--", "path\\file"]
        );
    }
    #[test]
    fn terminal_parser_rejects_shell_and_incomplete_commands() {
        for command in [
            "cmd /c git",
            "git status && echo x",
            "git status | more",
            "git status > out",
            "git\nstatus",
            "git x\0",
            "git 'x",
            "git x; y",
            "git x `whoami`",
        ] {
            assert!(parse(command).is_err(), "{command:?}");
        }
    }
    #[test]
    fn terminal_parser_preserves_unc_paths_and_completion_roundtrips() {
        assert_eq!(
            parse(r#"git add "\\server\共享\file.txt""#)
                .unwrap()
                .last()
                .unwrap(),
            r"\\server\共享\file.txt"
        );
        for value in [
            r"\\server\共享 文件\",
            "文件's name",
            "a\"b",
            "emoji😀 file",
        ] {
            assert_eq!(
                parse(&format!("git add {}", quote(value)))
                    .unwrap()
                    .last()
                    .unwrap(),
                value
            );
        }
    }
}
