use crate::application::{review_skill::ReviewSkillPackage, review_snapshot::ReviewSnapshot};
use crate::domain::ai::ReviewEvidenceSource;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub const MAX_READ_BYTES: usize = 32 * 1024;
pub const MAX_EVIDENCE_BYTES: usize = 128 * 1024;
pub const MAX_REQUESTS: usize = 8;
pub const MAX_ROUNDS: usize = 4;
const MAX_SCANNED_OBJECT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ContextRequest {
    #[serde(rename_all = "camelCase")]
    File {
        path: String,
        start_line: u32,
        end_line: u32,
    },
    Symbol {
        path: String,
        symbol: String,
    },
    Skill {
        path: String,
    },
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceResponse {
    pub status: String,
    pub message: String,
    pub text: String,
    pub sources: Vec<ReviewEvidenceSource>,
    pub whole_file_read: bool,
    pub searched_whole_file: bool,
    pub total_lines: Option<u32>,
}
#[derive(Default)]
pub struct EvidenceLedger {
    pub sources: Vec<ReviewEvidenceSource>,
    pub uncovered: Vec<String>,
    pub bytes: usize,
}
impl EvidenceLedger {
    pub async fn request(
        &mut self,
        snapshot: &ReviewSnapshot,
        skill: &mut ReviewSkillPackage,
        request: &ContextRequest,
        cancel: &CancellationToken,
    ) -> EvidenceResponse {
        let result = self.read(snapshot, skill, request, cancel).await;
        match result {
            Ok(response) => {
                self.bytes += response.text.len();
                self.sources.extend(response.sources.clone());
                response
            }
            Err(message) => {
                self.uncovered.push(message.clone());
                EvidenceResponse {
                    status: "unavailable".to_owned(),
                    message,
                    text: String::new(),
                    sources: vec![],
                    whole_file_read: false,
                    searched_whole_file: false,
                    total_lines: None,
                }
            }
        }
    }
    async fn read(
        &self,
        snapshot: &ReviewSnapshot,
        skill: &mut ReviewSkillPackage,
        request: &ContextRequest,
        cancel: &CancellationToken,
    ) -> Result<EvidenceResponse, String> {
        if cancel.is_cancelled() {
            return Err("审查已取消。".to_owned());
        }
        let mut response = EvidenceResponse {
            status: "available".to_owned(),
            message: String::new(),
            text: String::new(),
            sources: vec![],
            whole_file_read: false,
            searched_whole_file: false,
            total_lines: None,
        };
        if let ContextRequest::Skill { path } = request {
            skill.load_reference(path).map_err(|error| error.message)?;
            response.text = skill
                .documents
                .get(path)
                .cloned()
                .ok_or("规则文档未读取。")?;
        } else {
            let path = match request {
                ContextRequest::File { path, .. } | ContextRequest::Symbol { path, .. } => path,
                _ => unreachable!(),
            };
            // Scanning has its own finite object cap; only requested lines become model evidence.
            let (revision, bytes) = snapshot
                .blob(path, MAX_SCANNED_OBJECT_BYTES, cancel)
                .await
                .map_err(|error| format!("{path}：{}", error.message))?;
            if bytes.contains(&0) {
                return Err(format!("{path}：二进制文件不能作为文本证据。"));
            }
            let content =
                String::from_utf8(bytes).map_err(|_| format!("{path}：证据不是 UTF-8 文本。"))?;
            let total_lines = content.lines().count();
            response.total_lines = Some(total_lines as u32);
            match request {
                ContextRequest::File {
                    start_line,
                    end_line,
                    ..
                } => {
                    if *start_line == 0
                        || end_line < start_line
                        || *start_line as usize > total_lines.max(1)
                    {
                        return Err(format!("{path}：行范围无效。"));
                    }
                    response.whole_file_read =
                        *start_line == 1 && *end_line as usize >= total_lines;
                }
                ContextRequest::Symbol { symbol, .. } => {
                    if symbol.is_empty() || symbol.len() > 256 || symbol.contains(['\n', '\r']) {
                        return Err(format!("{path}：检索符号必须为 1–256 字节单行字面文本。"));
                    }
                    response.searched_whole_file = true;
                }
                _ => unreachable!(),
            };
            for (index, text) in content.lines().enumerate() {
                if cancel.is_cancelled() {
                    return Err("审查已取消。".to_owned());
                }
                let number = index as u32 + 1;
                let selected = match request {
                    ContextRequest::File {
                        start_line,
                        end_line,
                        ..
                    } => (*start_line..=*end_line).contains(&number),
                    ContextRequest::Symbol { symbol, .. } => text.contains(symbol),
                    _ => false,
                };
                if !selected {
                    continue;
                }
                let line = format!("{number}: {text}\n");
                if response.text.len() + line.len() > MAX_READ_BYTES {
                    return Err(format!("{path}：证据超过 32 KiB，请缩小行范围或检索。"));
                }
                response.text.push_str(&line);
                if let Some(last) = response.sources.last_mut()
                    && last.end_line + 1 == number
                {
                    last.end_line = number;
                } else {
                    response.sources.push(ReviewEvidenceSource {
                        path: path.clone(),
                        revision: revision.clone(),
                        start_line: number,
                        end_line: number,
                    });
                }
            }
        }
        if response.text.len() > MAX_READ_BYTES
            || self.bytes + response.text.len() > MAX_EVIDENCE_BYTES
        {
            return Err("本批证据超过读取容量，请在未覆盖范围记录缺失上下文。".to_owned());
        }
        Ok(response)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::review_skill::ReviewSkillLoader;
    use crate::application::review_snapshot::tests::{git, repository};
    use crate::domain::review::ReviewSource;
    #[tokio::test]
    async fn frozen_ranges_and_literal_search_report_actual_coverage() {
        let dir = repository().await;
        std::fs::write(
            dir.path().join("module.py"),
            "import os\ndef helper():\n    return 1\nhelper()\n",
        )
        .unwrap();
        git(dir.path(), &["add", "module.py"]).await;
        std::fs::create_dir(dir.path().join("code-review-expert")).unwrap();
        std::fs::write(dir.path().join("code-review-expert/SKILL.md"), "policy").unwrap();
        let mut skill = ReviewSkillLoader::load(dir.path(), "", &[]).unwrap();
        let cancel = CancellationToken::new();
        let snapshot = ReviewSnapshot::capture(dir.path(), ReviewSource::Staged, &cancel)
            .await
            .unwrap();
        std::fs::write(dir.path().join("module.py"), "private unstaged").unwrap();
        let mut ledger = EvidenceLedger::default();
        let range = ledger
            .request(
                &snapshot,
                &mut skill,
                &ContextRequest::File {
                    path: "module.py".to_owned(),
                    start_line: 2,
                    end_line: 3,
                },
                &cancel,
            )
            .await;
        assert_eq!(range.status, "available");
        assert!(!range.whole_file_read);
        assert_eq!(range.total_lines, Some(4));
        assert_eq!(range.sources[0].start_line, 2);
        assert_eq!(range.sources[0].end_line, 3);
        assert!(!range.text.contains("import os"));
        let search = ledger
            .request(
                &snapshot,
                &mut skill,
                &ContextRequest::Symbol {
                    path: "module.py".to_owned(),
                    symbol: "helper(".to_owned(),
                },
                &cancel,
            )
            .await;
        assert!(search.searched_whole_file);
        assert!(!search.whole_file_read);
        assert_eq!(search.sources.len(), 2);
        assert!(!search.text.contains("private unstaged"));
        let full = ledger
            .request(
                &snapshot,
                &mut skill,
                &ContextRequest::File {
                    path: "module.py".to_owned(),
                    start_line: 1,
                    end_line: 100,
                },
                &cancel,
            )
            .await;
        assert!(full.whole_file_read);
    }
}
