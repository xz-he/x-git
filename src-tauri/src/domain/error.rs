use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    GitNotFound,
    InvalidRepository,
    GitAuthentication,
    GitNetwork,
    GitRefreshFailed,
    GitConflict,
    GitLocked,
    GitCommandFailed,
    DirtyWorktree,
    InvalidReference,
    InvalidConsoleCommand,
    BranchUnavailable,
    CurrentBranchDeletion,
    UnmergedBranchDeletion,
    MissingUpstream,
    NonFastForward,
    LeaseRejected,
    GitOperationInProgress,
    StaleStash,
    StaleConflict,
    UnsupportedConflict,
    InvalidHistoryCursor,
    InvalidPath,
    StaleFileOperation,
    FileAlreadyExists,
    UnsupportedFileOperation,
    UnsupportedEncoding,
    Io,
    SettingsMigration,
    AiConfiguration,
    AiAuthentication,
    AiRateLimited,
    AiTransport,
    AiTimeout,
    AiInvalidResponse,
    AiContextTooLarge,
    AiNoStagedChanges,
    Cancelled,
    Unexpected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackendError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<String>,
}

impl BackendError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            diagnostics: None,
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: impl Into<String>) -> Self {
        self.diagnostics = Some(sanitize_git_output(&diagnostics.into()));
        self
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

impl From<std::io::Error> for BackendError {
    fn from(error: std::io::Error) -> Self {
        Self::new(ErrorCode::Io, "文件系统操作失败。").with_diagnostics(error.to_string())
    }
}

pub fn redact_diagnostics(diagnostics: &str) -> String {
    diagnostics
        .lines()
        .map(redact_diagnostic_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn sanitize_git_output(output: &str) -> String {
    truncate_utf8(&redact_diagnostics(output), MAX_DIAGNOSTIC_BYTES)
}

fn redact_diagnostic_line(line: &str) -> String {
    let line = redact_url_userinfo(line);
    if let Ok(mut value) = serde_json::from_str::<Value>(&line) {
        redact_json_value(&mut value);
        return serde_json::to_string(&value).unwrap_or(line);
    }

    if line.contains('?') {
        return redact_url_query(&line);
    }

    let Some((name, value)) = line.split_once(':') else {
        return line;
    };
    let normalized_name = name.trim().to_ascii_lowercase();

    if matches!(
        normalized_name.as_str(),
        "api-key" | "x-api-key" | "api_key" | "apikey"
    ) {
        return format!("{name}: [REDACTED]");
    }

    let trimmed_value = value.trim_start();
    if normalized_name == "authorization" && starts_with_ascii_case(trimmed_value, "bearer ") {
        return format!("{name}: Bearer [REDACTED]");
    }

    line
}

fn redact_url_userinfo(line: &str) -> String {
    let mut redacted = line.to_owned();
    let mut search_start = 0;
    while let Some(scheme_offset) = redacted[search_start..].find("://") {
        let authority_start = search_start + scheme_offset + 3;
        let authority_end = redacted[authority_start..]
            .find(|character: char| {
                character == '/'
                    || character == '?'
                    || character == '#'
                    || character.is_whitespace()
            })
            .map_or(redacted.len(), |offset| authority_start + offset);
        let Some(userinfo_end) = redacted[authority_start..authority_end].rfind('@') else {
            search_start = authority_end;
            continue;
        };
        redacted.replace_range(
            authority_start..authority_start + userinfo_end,
            "[REDACTED]",
        );
        search_start = authority_start + "[REDACTED]@".len();
    }
    redacted
}

fn redact_url_query(line: &str) -> String {
    let Some((prefix, query)) = line.split_once('?') else {
        return line.to_owned();
    };
    let redacted = query
        .split('&')
        .map(|part| {
            let Some((name, value)) = part.split_once('=') else {
                return part.to_owned();
            };
            if is_secret_name(name) {
                format!("{name}=[REDACTED]")
            } else {
                format!("{name}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{prefix}?{redacted}")
}

fn redact_json_value(value: &mut Value) {
    match value {
        Value::Object(entries) => {
            for (key, value) in entries {
                if is_secret_name(key) {
                    *value = Value::String("[REDACTED]".to_owned());
                } else {
                    redact_json_value(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact_json_value),
        _ => {}
    }
}

fn is_secret_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().replace(['-', '_'], "").as_str(),
        "key" | "apikey" | "xapikey" | "token" | "accesstoken"
    )
}

fn starts_with_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

fn truncate_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_error_serializes_stable_camel_case_contract() {
        let value = serde_json::to_value(
            BackendError::new(ErrorCode::GitNotFound, "Git was not found.")
                .with_diagnostics("git executable was not found"),
        )
        .unwrap();

        assert_eq!(value["code"], "gitNotFound");
        assert_eq!(value["message"], "Git was not found.");
        assert_eq!(value["diagnostics"], "git executable was not found");
    }

    #[test]
    fn diagnostics_redact_sensitive_header_and_json_values() {
        let raw = concat!(
            "Authorization: Bearer secret-token\n",
            "api-key: secret-key\n",
            "x-api-key: secret-x-key\n",
            "https://example.test/models/demo?key=secret-query&name=safe\n",
            r#"{"apiKey":"secret-json","token":"secret-token-json","accessToken":"secret-access"}"#,
        );

        let redacted = redact_diagnostics(raw);

        assert!(!redacted.contains("secret-token"));
        assert!(!redacted.contains("secret-key"));
        assert!(!redacted.contains("secret-x-key"));
        assert!(!redacted.contains("secret-json"));
        assert!(!redacted.contains("secret-query"));
        assert!(!redacted.contains("secret-token-json"));
        assert!(!redacted.contains("secret-access"));
        assert!(redacted.contains("Authorization: Bearer [REDACTED]"));
        assert!(redacted.contains("api-key: [REDACTED]"));
        assert!(redacted.contains("x-api-key: [REDACTED]"));
        assert!(redacted.contains(r#""apiKey":"[REDACTED]""#));
    }
}
