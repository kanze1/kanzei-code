//! architecture 工具:架构索引 `.kanzei/project/architecture/README.md` 的专用写通道。
//!
//! D-173 的根因不是权限太严,而是**权限严了却没有配套工具**:`.kanzei/project/*`
//! 对 write/edit 硬 deny,而架构索引没有任何专用工具,于是合法路径不可达,
//! 模型转向 shell 旁路。本工具把那条路补上,并且比裸 write 多三层硬门禁:
//!
//! 1. CAS(expected_hash):基于**读到的那一版**才能写,并发手改不会被静默覆盖。
//! 2. 写后即校验:链接目标必须存在、被索引的设计文档必须是 snake_case、
//!    同一目标不得重复出现、磁盘上的设计文档不得漏索引——校验不过直接拒写。
//! 3. 只认这一个文件:路径由引擎给定,输入里没有 path 参数可以指到别处。

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

/// 架构索引(相对项目根)。
pub const ARCHITECTURE_REL: &str = ".kanzei/project/architecture/README.md";
/// 索引应当覆盖的设计文档目录(相对项目根)。
const DESIGN_DIR: &str = "docs/design";
const MAX_INDEX_BYTES: usize = 1024 * 1024;

#[derive(Deserialize, JsonSchema)]
struct ArchitectureInput {
    /// get(读全文+hash) | check(只校验) | regenerate(按磁盘生成草稿,不写盘) | update(整文件替换)
    action: String,
    /// update 必填:索引全文
    #[serde(default)]
    content: Option<String>,
    /// update 必填:上一次 get/check 返回的 hash(并发写入保护)
    #[serde(default)]
    expected_hash: Option<String>,
}

pub struct ArchitectureTool;

#[async_trait]
impl Tool for ArchitectureTool {
    fn name(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> String {
        format!(
            "Read and maintain the architecture index `{ARCHITECTURE_REL}` (the ONLY write \
             channel for it — write/edit are denied there). Actions: get (full text + hash + \
             validation), check (validation only), regenerate (draft index rebuilt from the \
             files actually in {DESIGN_DIR}/, keeping existing descriptions — returned for you \
             to review, NOT written), update (content + expected_hash from the last get). \
             update refuses if the hash is stale or if validation fails: every link target must \
             exist, indexed design docs must be snake_case, no duplicate targets, and no design \
             doc on disk may be missing from the index."
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(ArchitectureInput)).unwrap();
        if let Some(action) = schema
            .pointer_mut("/properties/action")
            .and_then(|v| v.as_object_mut())
        {
            action.insert(
                "enum".into(),
                serde_json::json!(["get", "check", "regenerate", "update"]),
            );
        }
        schema
    }

    /// 权限资源 = 子动作,读写可分别授权(get/check/regenerate 只读,update 改盘)。
    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["action"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        match input["action"].as_str() {
            Some("update") => ToolConcurrency::write_worktree(ctx),
            _ => ToolConcurrency::shared_worktree(ctx),
        }
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: ArchitectureInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let root = ctx.project_root.clone();
        let path = root.join(ARCHITECTURE_REL);

        match input.action.as_str() {
            "get" => {
                let current = read_index(&path);
                let issues = validate(&root, &current);
                ToolOutput::ok(format!(
                    "path: {ARCHITECTURE_REL}\nhash: {}\n{}\n---\n{current}",
                    content_hash(&current),
                    render_issues(&issues),
                ))
            }
            "check" => {
                let current = read_index(&path);
                let issues = validate(&root, &current);
                let report = format!(
                    "path: {ARCHITECTURE_REL}\nhash: {}\n{}",
                    content_hash(&current),
                    render_issues(&issues),
                );
                if issues.is_empty() {
                    ToolOutput::ok(report)
                } else {
                    ToolOutput::error(report)
                }
            }
            "regenerate" => {
                let current = read_index(&path);
                ToolOutput::ok(format!(
                    "Draft only — NOT written. Review it, fix the categories/descriptions, then \
                     submit with `update` (expected_hash: {}).\n---\n{}",
                    content_hash(&current),
                    regenerate_draft(&root, &current),
                ))
            }
            "update" => {
                let Some(content) = input.content else {
                    return ToolOutput::error("`content` (full index text) is required for update");
                };
                if content.len() > MAX_INDEX_BYTES {
                    return ToolOutput::error(format!(
                        "architecture index is {} bytes; limit is {MAX_INDEX_BYTES}",
                        content.len()
                    ));
                }
                let current = read_index(&path);
                let current_hash = content_hash(&current);
                let Some(expected) = input.expected_hash else {
                    return ToolOutput::error(format!(
                        "`expected_hash` is required for update — call `get` first and pass the \
                         hash it returned. Current hash: {current_hash}"
                    ));
                };
                if expected != current_hash {
                    return ToolOutput::error(format!(
                        "stale expected_hash `{expected}`; the file is now `{current_hash}` \
                         (someone edited {ARCHITECTURE_REL} since your last read). Re-run `get`, \
                         merge your change onto the current text, and update again — do NOT \
                         resubmit the old text."
                    ));
                }
                let issues = validate(&root, &content);
                if !issues.is_empty() {
                    return ToolOutput::error(format!(
                        "REFUSING to write {ARCHITECTURE_REL}: the new content does not validate.\n{}\n\
                         Nothing was written. Fix these and resubmit with the SAME expected_hash.",
                        issues.join("\n"),
                    ));
                }
                if let Err(e) = replace_recoverably(&path, &content, &expected) {
                    return ToolOutput::error(e);
                }
                let diff_lines = content.lines().count() as i64 - current.lines().count() as i64;
                ToolOutput::ok(format!(
                    "updated {ARCHITECTURE_REL} ({} lines, {diff_lines:+} vs before)\nhash: {}\n\
                     validation: ok ({} indexed link(s))",
                    content.lines().count(),
                    content_hash(&content),
                    links(&content).len(),
                ))
                .with_display(serde_json::json!({
                    "kind": "diff",
                    "path": ARCHITECTURE_REL,
                    "before": current,
                    "after": content,
                }))
            }
            other => ToolOutput::error(format!(
                "unknown action `{other}`; valid: get | check | regenerate | update"
            )),
        }
    }
}

fn read_index(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// 同目录临时文件 + 可恢复替换。写临时文件后再次核对 CAS，避免校验期间的外部修改
/// 被覆盖；Windows 不能原子覆盖已有目标，因此先保留 backup，替换失败会恢复原件。
/// pub(crate):conventions 工具复用同一套 CAS 原语(D-235),仓里不养第二份。
pub(crate) fn replace_recoverably(
    path: &Path,
    content: &str,
    expected_hash: &str,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp = parent.join(format!(".architecture-{}-{nonce}.tmp", std::process::id()));
    let backup = parent.join(format!(".architecture-{}-{nonce}.bak", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| format!("cannot create {}: {e}", temp.display()))?;
    use std::io::Write;
    if let Err(error) = file
        .write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
    {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("cannot persist {}: {error}", temp.display()));
    }
    drop(file);

    let live = read_index(path);
    let live_hash = content_hash(&live);
    if live_hash != expected_hash {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "stale expected_hash `{expected_hash}` during final write; the file is now `{live_hash}`. Nothing was replaced. Re-run `get`."
        ));
    }

    let had_target = path.exists();
    if had_target {
        std::fs::rename(path, &backup)
            .map_err(|e| format!("cannot preserve {} before replacement: {e}", path.display()))?;
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        if had_target {
            let _ = std::fs::rename(&backup, path);
        }
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "cannot replace {}: {error}; original was restored",
            path.display()
        ));
    }
    if had_target {
        let _ = std::fs::remove_file(&backup);
    }
    Ok(())
}

/// 内容指纹(CAS 用)。规范化行尾后再哈希:换行风格不同不该算作并发改动。
/// pub(crate):conventions 工具复用同一套 CAS 原语(D-235),仓里不养第二份。
pub(crate) fn content_hash(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn render_issues(issues: &[String]) -> String {
    if issues.is_empty() {
        "validation: ok".to_string()
    } else {
        format!(
            "validation: {} issue(s)\n{}",
            issues.len(),
            issues.join("\n")
        )
    }
}

/// Markdown 行内链接 `[text](target)`,返回 (行号, 目标)。
/// 目标里的反引号/尖括号包装先剥掉(索引里写成 [`x.md`](...) 是常态)。
fn links(content: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (line_no, line) in content.lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '[' {
                i += 1;
                continue;
            }
            let Some(close) = (i + 1..chars.len()).find(|&j| chars[j] == ']') else {
                break;
            };
            if chars.get(close + 1) != Some(&'(') {
                i = close + 1;
                continue;
            }
            let Some(end) = (close + 2..chars.len()).find(|&j| chars[j] == ')') else {
                break;
            };
            let target: String = chars[close + 2..end].iter().collect();
            let target = target.trim().trim_matches('<').trim_matches('>').trim();
            if !target.is_empty() {
                out.push((line_no + 1, target.to_string()));
            }
            i = end + 1;
        }
    }
    out
}

fn is_external(target: &str) -> bool {
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with('#')
}

/// snake_case 文件名:小写字母/数字/下划线,不以下划线开头。
fn is_snake_case_file(name: &str) -> bool {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    !stem.is_empty()
        && !stem.starts_with('_')
        && stem
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// 磁盘上的设计文档(相对项目根,正斜杠),按名字排序。
fn design_docs(root: &Path) -> Vec<String> {
    let dir = root.join(DESIGN_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.ends_with(".md")
                .then(|| format!("{DESIGN_DIR}/{name}"))
        })
        .collect();
    out.sort();
    out
}

/// 链接是否指到项目根之外。用与权限同一套 `..` 消解逻辑,避免两处语义打架(D-050)。
fn escapes_project_root(target: &str) -> bool {
    let bare = target.split('#').next().unwrap_or(target);
    if bare.starts_with('/') || bare.starts_with('\\') || bare.contains(':') {
        return true;
    }
    let index_dir = ARCHITECTURE_REL.rsplit_once('/').map_or("", |(dir, _)| dir);
    kanzei_harness::permission::normalize_resource(&format!("{index_dir}/{bare}")).starts_with("..")
}

fn validate(root: &Path, content: &str) -> Vec<String> {
    let mut issues = Vec::new();
    if content.trim().is_empty() {
        issues.push("index is empty — an empty architecture index is never correct".into());
        return issues;
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut indexed: BTreeSet<String> = BTreeSet::new();
    let base_dir = root.join(ARCHITECTURE_REL);
    let base_dir = base_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());

    for (line_no, target) in links(content) {
        if is_external(&target) {
            continue;
        }
        let bare = target.split('#').next().unwrap_or(&target);
        if escapes_project_root(&target) {
            issues.push(format!(
                "line {line_no}: link `{target}` escapes the project root — index only project files"
            ));
            continue;
        }
        let absolute: PathBuf = base_dir.join(bare);
        if !absolute.exists() {
            issues.push(format!(
                "line {line_no}: link target does not exist: `{target}`"
            ));
            continue;
        }
        match seen.get(bare) {
            Some(first) => issues.push(format!(
                "line {line_no}: duplicate index entry for `{bare}` (already at line {first})"
            )),
            None => {
                seen.insert(bare.to_string(), line_no);
            }
        }
        let file_name = Path::new(bare)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if bare.contains(DESIGN_DIR) {
            if !is_snake_case_file(&file_name) {
                issues.push(format!(
                    "line {line_no}: `{file_name}` is not snake_case — design docs use \
                     lower_snake_case.md (rename the file, then index it)"
                ));
            }
            indexed.insert(file_name);
        }
    }

    let missing: Vec<String> = design_docs(root)
        .into_iter()
        .filter(|rel| {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            !indexed.contains(name)
        })
        .collect();
    if !missing.is_empty() {
        issues.push(format!(
            "these design docs exist on disk but are NOT in the index: {} — every {DESIGN_DIR}/*.md \
             must appear exactly once",
            missing.join(", ")
        ));
    }
    issues
}

/// 按磁盘实况生成索引草稿:已有条目沿用原描述行,新文档留 TODO 待归类。
fn regenerate_draft(root: &Path, current: &str) -> String {
    let described: BTreeMap<String, String> = current
        .lines()
        .filter_map(|line| {
            let (_, target) = links(line).into_iter().next()?;
            let name = target.split('#').next()?.rsplit('/').next()?.to_string();
            Some((name, line.trim_end().to_string()))
        })
        .collect();
    let mut out = String::from("## 当前索引(草稿:请按现行基线/评审中/历史/规范重新分节)\n\n");
    for rel in design_docs(root) {
        let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        match described.get(&name) {
            Some(line) => {
                out.push_str(line);
                out.push('\n');
            }
            None => out.push_str(&format!(
                "- [`{name}`](../../../{DESIGN_DIR}/{name}):TODO 补一句话说明并归类。\n"
            )),
        }
    }
    let stale: Vec<&String> = described
        .keys()
        .filter(|name| {
            !design_docs(root)
                .iter()
                .any(|rel| rel.ends_with(name.as_str()))
        })
        .collect();
    if !stale.is_empty() {
        out.push_str(&format!(
            "\n(索引里这些条目在 {DESIGN_DIR}/ 已不存在,草稿中已删除:{})\n",
            stale
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;
    use serde_json::json;

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-architecture-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project/architecture")).unwrap();
        std::fs::create_dir_all(dir.join(DESIGN_DIR)).unwrap();
        dir
    }

    fn write_index(root: &Path, body: &str) {
        std::fs::write(root.join(ARCHITECTURE_REL), body).unwrap();
    }

    #[tokio::test]
    async fn get_returns_hash_and_update_requires_it() {
        let root = temp_project("cas");
        std::fs::write(root.join(DESIGN_DIR).join("harness_m1.md"), "# x").unwrap();
        write_index(
            &root,
            "# 架构\n\n- [`harness_m1.md`](../../../docs/design/harness_m1.md):基线。\n",
        );
        let ctx = ToolCtx::new(root.clone(), root.clone());

        let got = ArchitectureTool
            .execute(json!({"action": "get"}), &ctx)
            .await;
        assert!(!got.is_error, "{}", got.content);
        assert!(got.content.contains("validation: ok"), "{}", got.content);
        let hash = got
            .content
            .lines()
            .find_map(|l| l.strip_prefix("hash: "))
            .unwrap()
            .to_string();

        // 缺 expected_hash:拒绝。
        let out = ArchitectureTool
            .execute(json!({"action": "update", "content": "# 架构\n"}), &ctx)
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("expected_hash"), "{}", out.content);

        // 陈旧 hash:拒绝,且不落盘。
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": "# 架构\n", "expected_hash": "0000000000000000"}),
                &ctx,
            )
            .await;
        assert!(out.is_error);
        assert!(
            out.content.contains("stale expected_hash"),
            "{}",
            out.content
        );

        // 正确 hash + 合法内容:写入成功。
        let next =
            "# 架构\n\n- [`harness_m1.md`](../../../docs/design/harness_m1.md):基线(改过)。\n";
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": next, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(
            std::fs::read_to_string(root.join(ARCHITECTURE_REL)).unwrap(),
            next
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn update_refuses_broken_links_duplicates_and_missing_docs() {
        let root = temp_project("validate");
        std::fs::write(root.join(DESIGN_DIR).join("harness_m1.md"), "# x").unwrap();
        std::fs::write(root.join(DESIGN_DIR).join("memory_system.md"), "# y").unwrap();
        write_index(&root, "# 架构\n");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let hash = content_hash("# 架构\n");

        // 漏索引 + 死链 + 重复 + 非 snake_case,一次全部指出来。
        std::fs::write(root.join(DESIGN_DIR).join("BadName.md"), "# z").unwrap();
        let bad = "# 架构\n\n\
                   - [`harness_m1.md`](../../../docs/design/harness_m1.md):a\n\
                   - [`harness_m1.md`](../../../docs/design/harness_m1.md):重复\n\
                   - [`BadName.md`](../../../docs/design/BadName.md):大写\n\
                   - [`gone.md`](../../../docs/design/gone.md):死链\n";
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": bad, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        for expected in [
            "duplicate index entry",
            "does not exist",
            "not snake_case",
            "memory_system.md",
        ] {
            assert!(
                out.content.contains(expected),
                "缺少校验项 {expected}: {}",
                out.content
            );
        }
        // 拒写就是真的没写。
        assert_eq!(
            std::fs::read_to_string(root.join(ARCHITECTURE_REL)).unwrap(),
            "# 架构\n"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn regenerate_lists_disk_truth_without_writing() {
        let root = temp_project("regen");
        std::fs::write(root.join(DESIGN_DIR).join("harness_m1.md"), "# x").unwrap();
        std::fs::write(root.join(DESIGN_DIR).join("new_doc.md"), "# y").unwrap();
        let original =
            "# 架构\n\n- [`harness_m1.md`](../../../docs/design/harness_m1.md):已有说明。\n";
        write_index(&root, original);
        let ctx = ToolCtx::new(root.clone(), root.clone());

        let out = ArchitectureTool
            .execute(json!({"action": "regenerate"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("已有说明"),
            "既有描述要沿用: {}",
            out.content
        );
        assert!(
            out.content.contains("new_doc.md"),
            "新文档要出现: {}",
            out.content
        );
        assert!(
            out.content.contains("TODO"),
            "新文档要留待归类: {}",
            out.content
        );
        // 草稿不落盘。
        assert_eq!(
            std::fs::read_to_string(root.join(ARCHITECTURE_REL)).unwrap(),
            original
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn snake_case_rule() {
        assert!(is_snake_case_file("memory_system.md"));
        assert!(is_snake_case_file("r059_mobile_agent.md"));
        assert!(!is_snake_case_file("MemorySystem.md"));
        assert!(!is_snake_case_file("memory-system.md"));
        assert!(!is_snake_case_file("_hidden.md"));
    }

    #[test]
    fn link_extraction_handles_backticked_labels() {
        let found = links("- [`a.md`](../x/a.md):说明 [b](b.md) 尾巴");
        assert_eq!(
            found,
            vec![(1, "../x/a.md".to_string()), (1, "b.md".to_string())]
        );
    }
}
