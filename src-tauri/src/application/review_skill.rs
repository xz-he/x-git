//! Repository-local review policy packages. Never executes package scripts.
use crate::domain::error::{BackendError, ErrorCode};
use crate::domain::review::{ReviewSkillInfo, ReviewSkillStatus};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_DOCUMENT_BYTES: usize = 32 * 1024;
const MAX_PACKAGE_BYTES: usize = 128 * 1024;
const MAX_DOCUMENTS: usize = 32;
const MAX_DEPTH: usize = 4;

pub struct ReviewSkillPackage {
    pub documents: BTreeMap<String, String>,
    root: PathBuf,
    directory: String,
    inventory: BTreeMap<String, String>,
    name: String,
    version: Option<String>,
}
impl ReviewSkillPackage {
    pub fn info(&self) -> ReviewSkillInfo {
        let mut hash = Sha256::new();
        for (path, content) in &self.documents {
            hash.update(path);
            hash.update([0]);
            hash.update(content);
            hash.update([0]);
        }
        ReviewSkillInfo {
            directory: self.directory.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            fingerprint: format!("{:x}", hash.finalize()),
            files: self.documents.keys().cloned().collect(),
        }
    }
    pub fn verify_unchanged(&self) -> Result<(), BackendError> {
        if inventory(&self.root, &self.directory)? != self.inventory {
            return Err(skill_error("审查技能在运行中发生变化，请重新开始审查。"));
        }
        Ok(())
    }
    pub fn load_reference(&mut self, path: &str) -> Result<(), BackendError> {
        self.verify_unchanged()?;
        safe_relative(path)?;
        let allowed = self.inventory.iter().any(|(source, body)| {
            references(body).iter().any(|reference| {
                resolve_reference(source, reference).is_ok_and(|resolved| resolved == path)
            })
        });
        if !allowed {
            return Err(skill_error("请求的文档不是技能声明的引用。"));
        }
        let previous = self.documents.clone();
        if let Err(error) = self.load_document(path, &[], &mut BTreeSet::new(), 0) {
            self.documents = previous;
            return Err(error);
        }
        Ok(())
    }
    pub fn text(&self) -> String {
        self.documents
            .iter()
            .map(|(path, content)| format!("### {path}\n{content}\n"))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn load_document(
        &mut self,
        path: &str,
        changed_paths: &[String],
        visiting: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<(), BackendError> {
        if visiting.contains(path) {
            return Err(skill_error("审查技能引用存在循环。"));
        }
        if self.documents.contains_key(path) {
            return Ok(());
        }
        if depth > MAX_DEPTH {
            return Err(skill_error("审查技能引用超过 4 层。"));
        }
        let content = self
            .inventory
            .get(path)
            .ok_or_else(|| skill_error(&format!("缺少必需的审查规则：{path}")))?
            .clone();
        visiting.insert(path.to_owned());
        for reference in references(&content) {
            let resolved = resolve_reference(path, &reference)?;
            // References in a document under `references/` are commonly written
            // either relative to that document (`detail.md`) or relative to the
            // skill root (`references/detail.md`). Accept the latter when the
            // document-relative candidate does not exist, without weakening
            // traversal or allowing paths outside the inventory.
            let resolved = if !self.inventory.contains_key(&resolved) {
                let root_relative = reference.replace('\\', "/");
                if self.inventory.contains_key(&root_relative) {
                    root_relative
                } else {
                    resolved
                }
            } else {
                resolved
            };
            if path == "SKILL.md" && !applicable(&resolved, changed_paths) {
                continue;
            }
            self.load_document(&resolved, changed_paths, visiting, depth + 1)?;
        }
        visiting.remove(path);
        self.documents.insert(path.to_owned(), content);
        Ok(())
    }
}

pub struct ReviewSkillLoader;
impl ReviewSkillLoader {
    pub fn load(
        root: &Path,
        configured: &str,
        paths: &[String],
    ) -> Result<ReviewSkillPackage, BackendError> {
        let directory = Self::discover(root, configured)?;
        let root = root
            .canonicalize()
            .map_err(|_| skill_error("无法访问仓库。"))?;
        let inventory = inventory(&root, &directory)?;
        let entry = inventory
            .get("SKILL.md")
            .ok_or_else(|| skill_error("缺少 SKILL.md。"))?;
        let name = entry
            .lines()
            .find_map(|line| {
                line.strip_prefix("name:")
                    .map(|value| value.trim().trim_matches(['\"', '\'']).to_owned())
            })
            .unwrap_or_else(|| directory.clone());
        let version = entry.lines().find_map(|line| {
            line.strip_prefix("version:")
                .or_else(|| line.strip_prefix("版本："))
                .map(|value| value.trim().trim_matches(['\"', '\'']).to_owned())
        });
        let mut package = ReviewSkillPackage {
            root,
            directory,
            inventory,
            documents: BTreeMap::new(),
            name,
            version,
        };
        package.load_document("SKILL.md", paths, &mut BTreeSet::new(), 0)?;
        if package.documents["SKILL.md"]
            .lines()
            .any(|line| line.starts_with("name:"))
            && matches!(
                package.name.as_str(),
                "code-review-expert" | "code-review-export"
            )
        {
            for rule in ["severity-guide", "false-positive-rules", "output-format"] {
                package.load_document(
                    &format!("references/{rule}.md"),
                    paths,
                    &mut BTreeSet::new(),
                    1,
                )?;
            }
            for rule in ["python-rules", "frontend-rules"] {
                let path = format!("references/{rule}.md");
                if applicable(&path, paths) {
                    package.load_document(&path, paths, &mut BTreeSet::new(), 1)?;
                }
            }
        }
        Ok(package)
    }
    pub fn status(root: &Path, configured: &str) -> ReviewSkillStatus {
        match Self::load(root, configured, &[]) {
            Ok(package) => ReviewSkillStatus {
                state: "ready".to_owned(),
                info: Some(package.info()),
                error: None,
            },
            Err(error) => {
                let state = if error.message.contains("同时存在") {
                    "ambiguous"
                } else if error.message.contains("未找到")
                    || error.message.contains("缺少 SKILL.md")
                {
                    "missing"
                } else {
                    "error"
                };
                ReviewSkillStatus {
                    state: state.to_owned(),
                    info: None,
                    error: Some(error),
                }
            }
        }
    }
    pub fn discover(root: &Path, configured: &str) -> Result<String, BackendError> {
        if !configured.trim().is_empty() {
            let directory = configured.trim().replace('\\', "/");
            safe_relative(&directory)?;
            if !root.join(&directory).join("SKILL.md").is_file() {
                return Err(skill_error("配置的审查技能缺少 SKILL.md。"));
            }
            return Ok(directory);
        }
        let found = ["code-review-expert", "code-review-export"]
            .into_iter()
            .filter(|name| root.join(name).join("SKILL.md").is_file())
            .collect::<Vec<_>>();
        match found.as_slice() {
            [name] => Ok((*name).to_owned()),
            [] => Err(skill_error(
                "未找到 code-review-expert/SKILL.md 或 code-review-export/SKILL.md。",
            )),
            _ => Err(skill_error(
                "同时存在两个审查技能，请设置仓库审查技能目录。",
            )),
        }
    }
}

fn skill_error(message: &str) -> BackendError {
    BackendError::new(ErrorCode::AiConfiguration, message)
}
fn safe_relative(value: &str) -> Result<(), BackendError> {
    if value.is_empty()
        || value.contains([':', '\0'])
        || value
            .replace('\\', "/")
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git") || part.eq_ignore_ascii_case("scripts"))
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(BackendError::new(
            ErrorCode::InvalidPath,
            "审查规则必须使用技能目录内的相对路径。",
        ));
    }
    Ok(())
}

fn applicable(path: &str, changed: &[String]) -> bool {
    match path.rsplit('/').next().unwrap_or(path) {
        "python-rules.md" => changed.iter().any(|path| path.ends_with(".py")),
        "frontend-rules.md" => changed.iter().any(|path| {
            [".js", ".ts", ".vue", ".jsx", ".tsx"]
                .iter()
                .any(|extension| path.ends_with(extension))
        }),
        "database-rules.md" | "configuration-rules.md" | "microservice-rules.md" => false,
        _ => true,
    }
}

fn references(content: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    for (start, end) in [('`', '`'), ('\"', '\"'), ('\'', '\''), ('(', ')')] {
        let mut rest = content;
        while let Some(left) = rest.find(start) {
            rest = &rest[left + start.len_utf8()..];
            let Some(right) = rest.find(end) else {
                break;
            };
            let candidate = rest[..right].trim().trim_matches(['<', '>']);
            let candidate = candidate.split('#').next().unwrap_or(candidate);
            if candidate.ends_with(".md")
                && !candidate.contains(char::is_whitespace)
                && !candidate.contains("://")
            {
                found.insert(candidate.to_owned());
            }
            rest = &rest[right + end.len_utf8()..];
        }
    }
    found.into_iter().collect()
}

fn resolve_reference(source: &str, reference: &str) -> Result<String, BackendError> {
    let reference = reference.replace('\\', "/");
    let reference = reference.strip_prefix("./").unwrap_or(&reference);
    safe_relative(reference)?;
    if !reference.ends_with(".md")
        || reference
            .split('/')
            .any(|part| part == "scripts" || part == ".git")
    {
        return Err(skill_error("只允许技能 Markdown 引用。"));
    }
    let parent = Path::new(source).parent().unwrap_or(Path::new(""));
    Ok(parent.join(reference).to_string_lossy().replace('\\', "/"))
}

fn inventory(root: &Path, directory: &str) -> Result<BTreeMap<String, String>, BackendError> {
    safe_relative(directory)?;
    let mut current = root.to_path_buf();
    for part in Path::new(directory).components() {
        current.push(part);
        reject_link(&current)?;
    }
    let skill_root = current
        .canonicalize()
        .map_err(|_| skill_error("无法访问审查技能目录。"))?;
    if !skill_root.starts_with(root) {
        return Err(skill_error("审查技能越出仓库。"));
    }
    let mut output = BTreeMap::new();
    let mut total = 0;
    collect_documents(&skill_root, &skill_root, 0, &mut output, &mut total)?;
    Ok(output)
}

fn reject_link(path: &Path) -> Result<std::fs::Metadata, BackendError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| skill_error("无法访问审查规则路径。"))?;
    #[cfg(windows)]
    let is_reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let is_reparse = false;
    if metadata.file_type().is_symlink() || is_reparse {
        return Err(skill_error("审查规则不允许链接或重解析点。"));
    }
    Ok(metadata)
}

fn collect_documents(
    root: &Path,
    directory: &Path,
    depth: usize,
    output: &mut BTreeMap<String, String>,
    total: &mut usize,
) -> Result<(), BackendError> {
    if depth > MAX_DEPTH {
        return Err(skill_error("审查技能目录超过 4 层。"));
    }
    for entry in std::fs::read_dir(directory).map_err(|_| skill_error("无法枚举审查规则。"))?
    {
        let entry = entry.map_err(|_| skill_error("无法枚举审查规则。"))?;
        if matches!(entry.file_name().to_str(), Some("scripts" | ".git")) {
            continue;
        }
        let path = entry.path();
        let metadata = reject_link(&path)?;
        if metadata.is_dir() {
            collect_documents(root, &path, depth + 1, output, total)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        if !metadata.is_file()
            || !path
                .canonicalize()
                .map_err(|_| skill_error("无法访问规则。"))?
                .starts_with(root)
        {
            return Err(skill_error("规则必须是技能目录内普通文件。"));
        }
        if output.len() >= MAX_DOCUMENTS {
            return Err(skill_error("审查技能超过 32 份文档。"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|_| skill_error("无法读取规则。"))?
            .take((MAX_DOCUMENT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| skill_error("无法读取规则。"))?;
        if bytes.len() > MAX_DOCUMENT_BYTES || *total + bytes.len() > MAX_PACKAGE_BYTES {
            return Err(BackendError::new(
                ErrorCode::AiContextTooLarge,
                "审查规则超过单文件 32 KiB 或总计 128 KiB 限制。",
            ));
        }
        *total += bytes.len();
        let content = String::from_utf8(bytes).map_err(|_| {
            BackendError::new(ErrorCode::UnsupportedEncoding, "审查规则必须为 UTF-8。")
        })?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| skill_error("规则越界。"))?
            .to_str()
            .ok_or_else(|| skill_error("规则路径必须为 UTF-8。"))?
            .replace('\\', "/");
        output.insert(relative, content);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn skill(root: &Path, directory: &str) {
        std::fs::create_dir_all(root.join(directory)).unwrap();
        std::fs::write(
            root.join(directory).join("SKILL.md"),
            "---\nname: sample\n---\npolicy",
        )
        .unwrap();
    }
    #[test]
    fn discovers_either_supported_directory() {
        for name in ["code-review-expert", "code-review-export"] {
            let root = tempfile::tempdir().unwrap();
            skill(root.path(), name);
            assert_eq!(ReviewSkillLoader::discover(root.path(), "").unwrap(), name);
        }
    }
    #[test]
    fn rejects_ambiguous_and_missing_without_fallback() {
        let root = tempfile::tempdir().unwrap();
        assert!(ReviewSkillLoader::discover(root.path(), "").is_err());
        skill(root.path(), "code-review-expert");
        skill(root.path(), "code-review-export");
        assert!(ReviewSkillLoader::discover(root.path(), "").is_err());
    }

    #[test]
    fn rejects_explicit_git_metadata_directory() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), ".git/policy");
        assert!(ReviewSkillLoader::load(root.path(), ".git/policy", &[]).is_err());
    }

    #[test]
    fn quoted_skill_name_still_requires_core_rules() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "code-review-expert");
        std::fs::write(
            root.path().join("code-review-expert/SKILL.md"),
            "---\nname: 'code-review-expert'\n---\npolicy",
        )
        .unwrap();
        assert!(ReviewSkillLoader::load(root.path(), "", &[]).is_err());
    }
    #[test]
    fn explicit_relative_directory_wins_without_fallback() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "rules/review");
        assert_eq!(
            ReviewSkillLoader::discover(root.path(), "rules/review").unwrap(),
            "rules/review"
        );
        assert!(ReviewSkillLoader::discover(root.path(), "missing").is_err());
        assert!(ReviewSkillLoader::discover(root.path(), "../outside").is_err());
    }
    #[test]
    fn loads_full_required_and_language_rules_and_detects_changes() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "code-review-expert");
        let dir = root.path().join("code-review-expert");
        std::fs::create_dir(dir.join("references")).unwrap();
        std::fs::write(dir.join("SKILL.md"), "---\nname: code-review-expert\n---\n版本：v2.2.1\n`references/severity-guide.md` `references/false-positive-rules.md` `references/output-format.md` `references/python-rules.md`").unwrap();
        for name in [
            "severity-guide",
            "false-positive-rules",
            "output-format",
            "python-rules",
        ] {
            std::fs::write(
                dir.join(format!("references/{name}.md")),
                "完整规则\n".repeat(900),
            )
            .unwrap();
        }
        let package = ReviewSkillLoader::load(root.path(), "", &["service.py".to_owned()]).unwrap();
        assert_eq!(package.documents.len(), 5);
        assert!(package.documents["references/python-rules.md"].len() > 6 * 1024);
        assert_eq!(package.info().version.as_deref(), Some("v2.2.1"));
        package.verify_unchanged().unwrap();
        std::fs::write(dir.join("references/severity-guide.md"), "changed").unwrap();
        assert!(package.verify_unchanged().is_err());
    }
    #[test]
    fn follows_relative_markdown_links_and_rejects_cycles_and_oversize() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "code-review-expert");
        let dir = root.path().join("code-review-expert");
        std::fs::create_dir(dir.join("references")).unwrap();
        std::fs::write(dir.join("SKILL.md"), "[Policy](references/base.md)").unwrap();
        std::fs::write(dir.join("references/base.md"), "[Details](detail.md)").unwrap();
        std::fs::write(dir.join("references/detail.md"), "complete").unwrap();
        let package = ReviewSkillLoader::load(root.path(), "", &[]).unwrap();
        assert_eq!(package.documents.len(), 3);
        std::fs::write(dir.join("references/detail.md"), "[Cycle](base.md)").unwrap();
        assert!(ReviewSkillLoader::load(root.path(), "", &[]).is_err());
        std::fs::write(dir.join("references/detail.md"), "x".repeat(32 * 1024 + 1)).unwrap();
        assert!(ReviewSkillLoader::load(root.path(), "", &[]).is_err());
    }
    #[test]
    fn missing_required_reference_and_non_utf8_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "code-review-expert");
        let entry = root.path().join("code-review-expert/SKILL.md");
        std::fs::write(&entry, "`references/severity-guide.md`").unwrap();
        assert!(ReviewSkillLoader::load(root.path(), "", &[]).is_err());
        std::fs::write(entry, [0xff]).unwrap();
        assert!(ReviewSkillLoader::load(root.path(), "", &[]).is_err());
    }

    #[test]
    fn accepts_root_relative_references_inside_reference_documents() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "code-review-expert");
        let directory = root.path().join("code-review-expert");
        std::fs::create_dir(directory.join("references")).unwrap();
        std::fs::write(
            directory.join("SKILL.md"),
            "`references/severity-guide.md` `references/false-positive-rules.md` `references/output-format.md`",
        )
        .unwrap();
        for name in ["severity-guide", "false-positive-rules", "output-format"] {
            std::fs::write(
                directory.join(format!("references/{name}.md")),
                if name == "severity-guide" {
                    "See `references/false-positive-rules.md`."
                } else {
                    "rule"
                },
            )
            .unwrap();
        }
        let package = ReviewSkillLoader::load(root.path(), "", &[]).unwrap();
        assert!(
            package
                .documents
                .contains_key("references/false-positive-rules.md")
        );
    }

    #[test]
    fn failed_on_demand_reference_does_not_leave_partial_rules_loaded() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "code-review-expert");
        let directory = root.path().join("code-review-expert");
        std::fs::create_dir(directory.join("references")).unwrap();
        std::fs::write(directory.join("SKILL.md"), "`references/database-rules.md`").unwrap();
        std::fs::write(
            directory.join("references/database-rules.md"),
            "`a.md` `missing.md`",
        )
        .unwrap();
        std::fs::write(directory.join("references/a.md"), "nested rule").unwrap();
        let mut package = ReviewSkillLoader::load(root.path(), "", &[]).unwrap();
        let before = package.info();
        assert!(
            package
                .load_reference("references/database-rules.md")
                .is_err()
        );
        assert_eq!(package.info(), before);
    }
}
