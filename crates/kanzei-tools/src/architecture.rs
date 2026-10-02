//! architecture 工具:架构索引 `.kanzei/project/architecture/README.md` 的专用写通道。
//!
//! D-173 的根因不是权限太严,而是**权限严了却没有配套工具**:`.kanzei/project/*`
//! 对 write/edit 硬 deny,而架构索引没有任何专用工具,于是合法路径不可达,
//! 模型转向 shell 旁路。本工具把那条路补上,并且比裸 write 多三层硬门禁:
//!
//! 1. CAS(expected_hash):基于**读到的那一版**才能写,并发手改不会被静默覆盖。
//! 2. 写前校验:链接目标必须存在、被索引的设计文档必须是 snake_case、
//!    同一目标不得重复出现、磁盘上的设计文档不得漏索引。get/check 照常报出全部问题;
//!    update 只拒绝**本次新引入**的问题(新旧两版的问题键逐一比对,不含行号,D-755),
//!    存量问题不挡无关修改、写成功时作为警告回显;另外当前版里合规的条目不得删掉,
//!    空索引一律拒写——免得放宽后的门禁被借来把索引删空。
//! 3. 只认这一个文件:路径由引擎给定,输入里没有 path 参数可以指到别处。

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};

use crate::arch_diagram::{
    crates_mermaid, render_issue, scan_diagrams, workspace_crates, DIAGRAM_DIR, MAX_EDGES,
    MAX_NODES, SEMANTIC_CLASSES,
};
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
    /// | diagrams(只读:docs/architecture 下的架构图清单与 lint,附自动生成的 crate 依赖图源码)
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
             Validation rules: every link target must exist, indexed design docs must be \
             snake_case, no duplicate targets, and no design doc on disk may be missing from the \
             index. update refuses if the hash is stale, the content is empty, it drops an entry \
             that was valid, or it introduces a validation issue the current index does not \
             already have; pre-existing issues do not block an update (they are echoed back as \
             warnings).\n\
             Action `diagrams` (read-only) lists the architecture DIAGRAMS and lints them. \
             Conventions (docs/design/architecture_diagrams.md): one diagram per file \
             `{DIAGRAM_DIR}/NN_snake_name.md` (two-digit prefix = tab order); line 1 `# Title`, then \
             one short paragraph, then the FIRST ```mermaid fence is the diagram (edit it with plain \
             write/edit; GitHub renders it too). Write `flowchart LR` or `flowchart TB`; quote every \
             label: id[\"text\"], use <br/> for a second line (first line = name, second = file or \
             role); never use `end`/`graph`/`click`/`style`/`class` as a node id. NO colors: no \
             `style`, `classDef`, `linkStyle`, `%%{{init}}` or frontmatter `config` — theme and \
             layout are injected by kanzei. Emphasis only via the semantic classes {classes}. \
             Clickable nodes: `click <id> \"<project-relative path>[:line]\" \"<tooltip>\"`, or a \
             tracker id as target (`click n \"R-123\"`); the target must exist. Keep a diagram under \
             {MAX_NODES} nodes / {MAX_EDGES} edges — split by subsystem. The crate dependency graph \
             is generated from Cargo manifests (group = `[package.metadata.kanzei] group`, label = \
             `[package] description`) and is never written to disk. After editing run `diagrams` \
             again: errors carry file:line and a one-line fix; rendering in the app is the final \
             judge.",
            classes = SEMANTIC_CLASSES.map(|c| format!(":::{c}")).join(" "),
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
                serde_json::json!(["get", "check", "regenerate", "update", "diagrams"]),
            );
        }
        schema
    }

    /// 权限资源 = 子动作,读写可分别授权(get/check/regenerate/diagrams 只读,update 改盘)。
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
                let issues = validate(&root, &current).issues;
                ToolOutput::ok(format!(
                    "path: {ARCHITECTURE_REL}\nhash: {}\n{}\n---\n{current}",
                    content_hash(&current),
                    render_issues(&issues),
                ))
            }
            "check" => {
                let current = read_index(&path);
                let issues = validate(&root, &current).issues;
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
            // 图与 Cargo 清单是代码树里的文件:worktree 线(R-171)在自己的树里改图,自查必须读同一棵树
            // (edit/write/read 的相对路径按 ctx.cwd 解析),不能读主根。get/check/update 仍管 .kanzei 托管的索引。
            "diagrams" => diagrams_report(code_root(ctx)),
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
                // 空索引无条件拒写:当前版若本就为空,下面的逐键比对会把它当存量放过。
                if content.trim().is_empty() {
                    return ToolOutput::error(format!(
                        "REFUSING to write {ARCHITECTURE_REL}: {EMPTY_INDEX_MSG}\nNothing was written."
                    ));
                }
                // D-755:只拒绝本次新引入的问题。存量问题(如尚未改名的设计文档)不能把
                // 专用写通道整个锁死——它们照常由 get/check 报出,写成功时回显为警告。
                let before = validate(&root, &current);
                let after = validate(&root, &content);
                let introduced = introduced_issues(&before.issues, &after.issues);
                let dropped = dropped_entries(&before, &content, &introduced);
                if !introduced.is_empty() || !dropped.is_empty() {
                    let mut reasons = issue_lines(&introduced);
                    if !dropped.is_empty() {
                        reasons.push(format!(
                            "these entries are valid in the current index but gone from the new \
                             content: {} — keep them (edit the description/identity instead); an \
                             entry may only be removed once its target no longer exists",
                            dropped.join(", ")
                        ));
                    }
                    return ToolOutput::error(format!(
                        "REFUSING to write {ARCHITECTURE_REL}: the new content introduces problems \
                         the current index does not have.\n{}\n\
                         Nothing was written. Fix these and resubmit with the SAME expected_hash \
                         (pre-existing issues do not block and are not listed here; see `get`).",
                        reasons.join("\n"),
                    ));
                }
                if let Err(e) =
                    crate::atomic_file::write_atomic_cas(&path, &content, &expected, content_hash)
                {
                    return ToolOutput::error(e);
                }
                // D-398:architecture 专用写者写盘后记写日志(围栏收口归因凭据,此前零接入)。
                crate::record_write_log(ctx, ARCHITECTURE_REL, &path);
                let diff_lines = content.lines().count() as i64 - current.lines().count() as i64;
                let remaining = issue_lines(&after.issues);
                let validation = if remaining.is_empty() {
                    format!("validation: ok ({} indexed link(s))", links(&content).len())
                } else {
                    format!(
                        "validation: {} indexed link(s); WARNING: {} pre-existing issue(s) remain \
                         (already in the previous version, so they did not block this update):\n{}",
                        links(&content).len(),
                        remaining.len(),
                        remaining.join("\n"),
                    )
                };
                ToolOutput::ok(format!(
                    "updated {ARCHITECTURE_REL} ({} lines, {diff_lines:+} vs before)\nhash: {}\n\
                     {validation}",
                    content.lines().count(),
                    content_hash(&content),
                ))
                .with_display(serde_json::json!({
                    "kind": "diff",
                    "path": ARCHITECTURE_REL,
                    "before": current,
                    "after": content,
                }))
            }
            other => ToolOutput::error(format!(
                "unknown action `{other}`; valid: get | check | regenerate | update | diagrams"
            )),
        }
    }
}

/// 代码树根:worktree 线上是该线自己的工作树(ctx.cwd),没设 cwd 时退回项目根。
fn code_root(ctx: &ToolCtx) -> &Path {
    if ctx.cwd.as_os_str().is_empty() {
        &ctx.project_root
    } else {
        &ctx.cwd
    }
}

/// `diagrams` 动作:列出 docs/architecture 下每张图与 lint 结果,附自动生成的 crate 依赖图
/// 源码(约简版)作为新图的起点。有 error 时整体判错(同 check),只有警告照常成功。
/// `root` 是代码树根(见 code_root):图、click 目标与 Cargo 清单都按它读。
fn diagrams_report(root: &Path) -> ToolOutput {
    let docs = scan_diagrams(root);
    let errors: usize = docs.iter().map(|d| d.errors()).sum();
    let warnings = docs.iter().map(|d| d.issues.len()).sum::<usize>() - errors;
    let mut out = format!(
        "diagrams: {} file(s) in {DIAGRAM_DIR}/ · {errors} error(s) · {warnings} warning(s)\n",
        docs.len()
    );
    if docs.is_empty() {
        out.push_str(&format!(
            "No diagrams yet. Create `{DIAGRAM_DIR}/01_overview.md`: `# Title`, one paragraph, \
             then a ```mermaid fence (conventions are in this tool's description), and run \
             `diagrams` again.\n"
        ));
    }
    for doc in &docs {
        let title = if doc.title.is_empty() {
            "(no title)"
        } else {
            doc.title.as_str()
        };
        let status = if doc.issues.is_empty() {
            "ok".to_string()
        } else {
            format!("{} issue(s)", doc.issues.len())
        };
        out.push_str(&format!(
            "- {} 「{title}」 mermaid from line {}: {status}\n",
            doc.path, doc.source_line
        ));
        for issue in &doc.issues {
            out.push_str(&format!("  - {}\n", render_issue(&doc.path, issue)));
        }
    }
    if let Some(ws) = workspace_crates(root) {
        out.push_str(&format!(
            "---\ncrate dependency graph (generated from Cargo manifests, never on disk; {} \
             member(s), {} derivable edge(s) hidden). Reuse it as a starting point:\n```mermaid\n{}```\n",
            ws.members.len(),
            ws.hidden_transitive(),
            crates_mermaid(&ws, false)
        ));
    }
    if errors > 0 {
        ToolOutput::error(out)
    } else {
        ToolOutput::ok(out)
    }
}

fn read_index(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// 内容指纹(CAS 用)。规范化行尾后再哈希:换行风格不同不该算作并发改动。
/// pub(crate):conventions 工具复用同一套 CAS 原语(D-235),仓里不养第二份。
/// 写盘本体是 kanzei_base::atomic_file::write_atomic_cas(D-261 并轨,R-208 迁出),
/// 这里只留指纹。
pub(crate) fn content_hash(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

const EMPTY_INDEX_MSG: &str = "index is empty — an empty architecture index is never correct";

/// 校验问题的比较键(D-755):只含种类与目标,不含行号——条目挪行不算新问题,
/// update 靠它区分「存量问题」和「本次新引入的问题」。漏索引按文件逐条成键。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum IssueKey {
    Empty,
    /// 链接目标(原样)指到项目根之外。
    Escapes(String),
    /// 链接目标(原样)不存在。
    Dangling(String),
    /// 重复索引的目标(去掉 #锚点)。
    Duplicate(String),
    /// 被索引但不是 snake_case 的设计文档文件名。
    NotSnake(String),
    /// 磁盘上存在却没入索引的设计文档(项目根相对路径)。
    Missing(String),
}

#[derive(Debug, Clone)]
struct Issue {
    key: IssueKey,
    /// 逐条说明(含行号,文案沿用改造前)。Missing 留空:渲染时按文件聚成一行,
    /// 保持 get/check 的输出与改造前一致。
    message: String,
}

struct Validation {
    issues: Vec<Issue>,
    /// 合规条目:没有任何问题的本地链接,规范化的项目根相对路径 → 索引里的原样目标。
    compliant: BTreeMap<String, String>,
}

/// 问题渲染成行:逐条问题各占一行,漏索引的文档聚成末尾一行。
fn issue_lines(issues: &[Issue]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    for issue in issues {
        match &issue.key {
            IssueKey::Missing(doc) => missing.push(doc),
            _ => lines.push(issue.message.clone()),
        }
    }
    if !missing.is_empty() {
        lines.push(format!(
            "these design docs exist on disk but are NOT in the index: {} — every {DESIGN_DIR}/*.md \
             must appear exactly once",
            missing.join(", ")
        ));
    }
    lines
}

fn render_issues(issues: &[Issue]) -> String {
    let lines = issue_lines(issues);
    if lines.is_empty() {
        "validation: ok".to_string()
    } else {
        format!("validation: {} issue(s)\n{}", lines.len(), lines.join("\n"))
    }
}

/// `after` 里有、`before` 里没有的问题(按键计数比较:同键在新版里多出来的那几条也算新问题)。
fn introduced_issues(before: &[Issue], after: &[Issue]) -> Vec<Issue> {
    let mut budget: BTreeMap<&IssueKey, usize> = BTreeMap::new();
    for issue in before {
        *budget.entry(&issue.key).or_default() += 1;
    }
    after
        .iter()
        .filter(|issue| match budget.get_mut(&issue.key) {
            Some(left) if *left > 0 => {
                *left -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect()
}

/// 当前版里合规、新内容里却不再链接的条目(原样目标)。已由新增 Missing 点名的不重复报。
fn dropped_entries(before: &Validation, content: &str, introduced: &[Issue]) -> Vec<String> {
    let linked: BTreeSet<String> = links(content)
        .into_iter()
        .filter(|(_, target)| !is_external(target))
        .map(|(_, target)| project_rel(&target))
        .collect();
    let newly_missing: BTreeSet<String> = introduced
        .iter()
        .filter_map(|issue| match &issue.key {
            IssueKey::Missing(doc) => Some(kanzei_harness::permission::normalize_resource(doc)),
            _ => None,
        })
        .collect();
    before
        .compliant
        .iter()
        .filter(|(rel, _)| !linked.contains(*rel) && !newly_missing.contains(*rel))
        .map(|(_, target)| target.clone())
        .collect()
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
    project_rel(bare).starts_with("..")
}

/// 索引里的相对链接 → 规范化的项目根相对路径(去掉 #锚点),用于跨版本比对同一条目。
fn project_rel(target: &str) -> String {
    let bare = target.split('#').next().unwrap_or(target);
    let index_dir = ARCHITECTURE_REL.rsplit_once('/').map_or("", |(dir, _)| dir);
    kanzei_harness::permission::normalize_resource(&format!("{index_dir}/{bare}"))
}

fn validate(root: &Path, content: &str) -> Validation {
    let mut issues = Vec::new();
    let mut compliant = BTreeMap::new();
    if content.trim().is_empty() {
        issues.push(Issue {
            key: IssueKey::Empty,
            message: EMPTY_INDEX_MSG.into(),
        });
        return Validation { issues, compliant };
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
            issues.push(Issue {
                key: IssueKey::Escapes(target.clone()),
                message: format!(
                    "line {line_no}: link `{target}` escapes the project root — index only project files"
                ),
            });
            continue;
        }
        let absolute: PathBuf = base_dir.join(bare);
        if !absolute.exists() {
            issues.push(Issue {
                key: IssueKey::Dangling(target.clone()),
                message: format!("line {line_no}: link target does not exist: `{target}`"),
            });
            continue;
        }
        let mut ok = true;
        match seen.get(bare) {
            Some(first) => {
                ok = false;
                issues.push(Issue {
                    key: IssueKey::Duplicate(bare.to_string()),
                    message: format!(
                        "line {line_no}: duplicate index entry for `{bare}` (already at line {first})"
                    ),
                });
            }
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
                ok = false;
                issues.push(Issue {
                    key: IssueKey::NotSnake(file_name.clone()),
                    message: format!(
                        "line {line_no}: `{file_name}` is not snake_case — design docs use \
                         lower_snake_case.md (rename the file, then index it)"
                    ),
                });
            }
            indexed.insert(file_name);
        }
        if ok {
            compliant
                .entry(project_rel(bare))
                .or_insert_with(|| bare.to_string());
        }
    }

    // 漏索引逐个文件成键(D-755),渲染时再聚成一行。
    for rel in design_docs(root) {
        let name = rel.rsplit('/').next().unwrap_or(&rel);
        if !indexed.contains(name) {
            issues.push(Issue {
                key: IssueKey::Missing(rel),
                message: String::new(),
            });
        }
    }
    Validation { issues, compliant }
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
        let original = "# 架构\n\n\
                        - [`harness_m1.md`](../../../docs/design/harness_m1.md):a\n\
                        - [`memory_system.md`](../../../docs/design/memory_system.md):b\n";
        write_index(&root, original);
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let hash = content_hash(original);

        // 新漏索引 + 死链 + 重复 + 非 snake_case,都是本次新引入的,一次全部指出来。
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
            original
        );

        // 空索引无条件拒写。
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": "  \n", "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("index is empty"), "{}", out.content);
        std::fs::remove_dir_all(&root).ok();
    }

    fn hash_of(out: &ToolOutput) -> String {
        out.content
            .lines()
            .find_map(|l| l.strip_prefix("hash: "))
            .unwrap()
            .to_string()
    }

    /// D-755:存量的非 snake_case / 漏索引不挡无关修改,写后 get 照常报出;
    /// 新引入的非 snake_case 链接或新漏索引仍然拒写,只点名新增的那几条。
    #[tokio::test]
    async fn update_blocks_only_newly_introduced_issues() {
        let root = temp_project("baseline");
        for name in [
            "harness_m1.md",
            "memory_system.md",
            "oc-playback.md",
            "oc-voice.md",
            "orphan.md",
        ] {
            std::fs::write(root.join(DESIGN_DIR).join(name), "# x").unwrap();
        }
        let original = "# 架构\n\n\
                        - [`harness_m1.md`](../../../docs/design/harness_m1.md):Harness 六注册表。\n\
                        - [`memory_system.md`](../../../docs/design/memory_system.md):记忆。\n\
                        - [`oc-playback.md`](../../../docs/design/oc-playback.md):存量连字符名。\n";
        write_index(&root, original);
        let ctx = ToolCtx::new(root.clone(), root.clone());

        let got = ArchitectureTool
            .execute(json!({"action": "get"}), &ctx)
            .await;
        assert!(
            got.content.contains("validation: 2 issue(s)"),
            "{}",
            got.content
        );
        assert!(
            got.content.contains("`oc-playback.md` is not snake_case"),
            "{}",
            got.content
        );
        assert!(
            got.content
                .contains("NOT in the index: docs/design/oc-voice.md, docs/design/orphan.md"),
            "漏索引仍聚成一行报出: {}",
            got.content
        );
        let hash = hash_of(&got);

        // 只改无关行:存量问题不挡,写成功并以警告回显。
        let edited = original.replace("Harness 六注册表", "Harness 五注册表");
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": edited, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("WARNING: 2 pre-existing issue(s)"),
            "{}",
            out.content
        );
        assert!(out.content.contains("oc-playback.md"), "{}", out.content);
        assert_eq!(
            std::fs::read_to_string(root.join(ARCHITECTURE_REL)).unwrap(),
            edited
        );
        // 写后 get 仍报出全部存量问题。
        let got = ArchitectureTool
            .execute(json!({"action": "get"}), &ctx)
            .await;
        assert!(
            got.content.contains("validation: 2 issue(s)"),
            "{}",
            got.content
        );
        assert!(got.content.contains("not snake_case"), "{}", got.content);
        assert!(got.content.contains("orphan.md"), "{}", got.content);
        let hash = hash_of(&got);

        // 存量条目挪行不算新问题(键里不含行号)。
        let moved = "# 架构\n\n\
                     - [`oc-playback.md`](../../../docs/design/oc-playback.md):挪到最前。\n\
                     - [`harness_m1.md`](../../../docs/design/harness_m1.md):Harness 五注册表。\n\
                     - [`memory_system.md`](../../../docs/design/memory_system.md):记忆。\n";
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": moved, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let hash = content_hash(moved);

        // 新增一条非 snake_case 链接:拒写,只点名新增的那条。
        let added_bad = format!(
            "{moved}- [`oc-voice.md`](../../../docs/design/oc-voice.md):新加的连字符名。\n"
        );
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": added_bad, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("`oc-voice.md` is not snake_case"),
            "{}",
            out.content
        );
        assert!(
            !out.content.contains("`oc-playback.md` is not snake_case"),
            "存量问题不该被点名为拒写理由: {}",
            out.content
        );
        assert!(!out.content.contains("orphan.md"), "{}", out.content);

        // 新漏索引一份原本已入索引的文档:拒写。
        let dropped_doc = moved.replace(
            "- [`memory_system.md`](../../../docs/design/memory_system.md):记忆。\n",
            "",
        );
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": dropped_doc, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content
                .contains("NOT in the index: docs/design/memory_system.md —"),
            "只点名新漏的那份: {}",
            out.content
        );
        assert_eq!(
            std::fs::read_to_string(root.join(ARCHITECTURE_REL)).unwrap(),
            moved
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 放宽门禁后,当前版里合规的条目仍不得删掉(非设计目录的链接没有漏索引兜底)。
    #[tokio::test]
    async fn update_refuses_dropping_a_compliant_entry() {
        let root = temp_project("drop");
        std::fs::write(root.join(DESIGN_DIR).join("harness_m1.md"), "# x").unwrap();
        std::fs::write(root.join(DESIGN_DIR).join("Legacy-Doc.md"), "# y").unwrap();
        std::fs::write(root.join("docs/overview.md"), "# o").unwrap();
        let original = "# 架构\n\n\
                        - [`harness_m1.md`](../../../docs/design/harness_m1.md):基线。\n\
                        - [`overview.md`](../../../docs/overview.md):总览。\n\
                        - [`Legacy-Doc.md`](../../../docs/design/Legacy-Doc.md):存量问题。\n\
                        - [`gone.md`](../../../docs/design/gone.md):目标已删。\n";
        write_index(&root, original);
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let hash = content_hash(original);

        // 删掉合规条目 overview.md:拒写。
        let without_overview =
            original.replace("- [`overview.md`](../../../docs/overview.md):总览。\n", "");
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": without_overview, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("valid in the current index but gone"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("../../../docs/overview.md"),
            "{}",
            out.content
        );
        assert_eq!(
            std::fs::read_to_string(root.join(ARCHITECTURE_REL)).unwrap(),
            original
        );

        // 目标已不存在的死链(本身不合规)可以删。
        let without_dead = original.replace(
            "- [`gone.md`](../../../docs/design/gone.md):目标已删。\n",
            "",
        );
        let out = ArchitectureTool
            .execute(
                json!({"action": "update", "content": without_dead, "expected_hash": hash}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
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

    /// UI2-0926 #7:`diagrams` 只读动作——空目录给创建提示;有问题判错并给 file:行 + 修法;
    /// 改好后成功;是 Cargo 工作区时附上生成的 crate 图源码。描述里写明图的约定。
    #[tokio::test]
    async fn diagrams_action_lints_and_offers_the_crate_graph() {
        let root = temp_project("diagrams");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = ArchitectureTool
            .execute(json!({"action": "diagrams"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("No diagrams yet"), "{}", out.content);

        std::fs::create_dir_all(root.join("docs/architecture")).unwrap();
        let bad = "# 流程\n\n说明。\n\n```mermaid\nflowchart LR\n  a[\"甲\"] --> b[\"乙\"]\n  style a fill:#f00\n  click a \"docs/design/gone.md\"\n```\n";
        std::fs::write(root.join("docs/architecture/01_flow.md"), bad).unwrap();
        let out = ArchitectureTool
            .execute(json!({"action": "diagrams"}), &ctx)
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content
                .contains("docs/architecture/01_flow.md:8 [D4 error]"),
            "{}",
            out.content
        );
        assert!(
            out.content
                .contains("docs/architecture/01_flow.md:9 [D5 error]"),
            "{}",
            out.content
        );
        assert!(out.content.contains("修法"), "{}", out.content);

        let good = bad
            .replace("  style a fill:#f00\n", "")
            .replace("docs/design/gone.md", "docs/design/harness_m1.md");
        std::fs::write(root.join(DESIGN_DIR).join("harness_m1.md"), "# x").unwrap();
        std::fs::write(root.join("docs/architecture/01_flow.md"), good).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("crates/a/src")).unwrap();
        std::fs::write(
            root.join("crates/a/Cargo.toml"),
            "[package]\nname = \"a\"\ndescription = \"甲\"\n",
        )
        .unwrap();
        std::fs::write(root.join("crates/a/src/lib.rs"), "").unwrap();
        let out = ArchitectureTool
            .execute(json!({"action": "diagrams"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content
                .contains("01_flow.md 「流程」 mermaid from line 6: ok"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("crate dependency graph"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("```mermaid\nflowchart LR\n"),
            "{}",
            out.content
        );

        let schema = ArchitectureTool.input_schema();
        assert!(schema.to_string().contains("\"diagrams\""), "{schema}");
        let description = ArchitectureTool.description();
        for needle in [
            "docs/architecture/NN_snake_name.md",
            ":::focus",
            "click <id>",
            "classDef",
        ] {
            assert!(
                description.contains(needle),
                "描述缺约定 {needle}: {description}"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// 复核修复:worktree 线(cwd ≠ project_root)在自己的树里改图,`diagrams` 必须读这棵树——
    /// 主根上没有这张图;报出的问题行号来自 worktree 里的文件,click 目标也按 worktree 核对。
    #[tokio::test]
    async fn diagrams_action_reads_the_worktree_not_the_main_root() {
        let main = temp_project("diagrams-main");
        let worktree = temp_project("diagrams-wt");
        std::fs::create_dir_all(worktree.join("docs/architecture")).unwrap();
        std::fs::write(worktree.join("crates_only_here.rs"), "").unwrap();
        let doc = "# 线上的图\n\n说明。\n\n```mermaid\nflowchart LR\n  a[\"甲\"] --> b[\"乙\"]\n  click a \"crates_only_here.rs\"\n\n  style b fill:#f00\n```\n";
        std::fs::write(worktree.join("docs/architecture/03_line.md"), doc).unwrap();
        let ctx = ToolCtx::new(worktree.clone(), main.clone());
        let out = ArchitectureTool
            .execute(json!({"action": "diagrams"}), &ctx)
            .await;
        assert!(
            out.content.contains("1 file(s)")
                && out
                    .content
                    .contains("docs/architecture/03_line.md:10 [D4 error]"),
            "应扫到 worktree 里的图并给出该文件的行号:{}",
            out.content
        );
        assert!(
            !out.content.contains("目标文件不存在"),
            "click 目标只在 worktree 里,按主根核对会误报:{}",
            out.content
        );
        std::fs::remove_dir_all(&main).ok();
        std::fs::remove_dir_all(&worktree).ok();
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
