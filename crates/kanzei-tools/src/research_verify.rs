//! R-277 批4：FACT 式引用核验与显式预算旋钮。
//!
//! 文献证据先经工具抓取为 topic source_text 全文，校验只读该全文而不是摘要字段；
//! 代码证据用 `file:line@commit` 通过 git show 读取并检查锚点邻域。校验结果落
//! verification.json，失败项保留以便重写对应分节。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};

use crate::docstore::{DocStore, SOURCES};
use crate::research_plan::{load_plan, PlanBudget, PlanStatus};
use crate::webfetch::{fetch_bytes, html_to_text};

const MAX_SOURCE_BYTES: usize = 3 * 1024 * 1024;

fn topic_dir(root: &Path, topic: &str) -> Result<PathBuf, String> {
    DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    Ok(root.join(".kanzei/research").join(topic))
}

fn valid_source_id(source_id: &str) -> bool {
    source_id.starts_with("S-")
        && source_id.len() > 2
        && source_id[2..].chars().all(|ch| ch.is_ascii_digit())
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建核验目录失败: {error}"))?;
    }
    let _lock = kanzei_base::atomic_file::lock_exclusive(path)
        .map_err(|error| format!("锁定核验工件失败: {error}"))?;
    kanzei_base::atomic_file::write_atomic(path, content)
        .map_err(|error| format!("写入核验工件失败: {error}"))
}

fn source_entries(
    root: &Path,
    topic: &str,
) -> Result<BTreeMap<String, crate::docstore::Entry>, String> {
    let store = DocStore::open_topic(root, &SOURCES, topic)
        .map_err(|error| format!("打开 topic sources 失败: {error}"))?;
    let entries = store
        .load()
        .map_err(|error| format!("读取 topic sources 失败: {error}"))?;
    Ok(entries
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect())
}

fn field(entry: &crate::docstore::Entry, key: &str) -> String {
    entry
        .fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

fn source_text_path(dir: &Path, source_id: &str) -> PathBuf {
    dir.join("source_text").join(format!("{source_id}.txt"))
}

fn budget_path(dir: &Path) -> PathBuf {
    dir.join("budget.json")
}

fn parse_positive_budget(input: &Value) -> Result<PlanBudget, String> {
    let max_rounds = input
        .get("max_rounds")
        .and_then(Value::as_u64)
        .ok_or_else(|| "budget_set 必须提供 max_rounds".to_string())?;
    let max_tokens = input
        .get("max_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| "budget_set 必须提供 max_tokens".to_string())?;
    let max_concurrency = input
        .get("max_concurrency")
        .and_then(Value::as_u64)
        .ok_or_else(|| "budget_set 必须提供 max_concurrency".to_string())?;
    if max_rounds == 0 || max_tokens == 0 || max_concurrency == 0 {
        return Err("预算旋钮必须全部为正数".into());
    }
    Ok(PlanBudget {
        max_rounds: u32::try_from(max_rounds).map_err(|_| "max_rounds 超出范围")?,
        max_tokens: u32::try_from(max_tokens).map_err(|_| "max_tokens 超出范围")?,
        max_concurrency: u32::try_from(max_concurrency).map_err(|_| "max_concurrency 超出范围")?,
    })
}

fn claim_keywords(claim: &Value) -> Result<Vec<String>, String> {
    let keywords = claim
        .get("keywords")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(|value| value.to_ascii_lowercase())
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if keywords.is_empty() {
        return Err("claim 必须提供 keywords，不能只凭自评通过".into());
    }
    Ok(keywords)
}

fn verify_literature(
    dir: &Path,
    entry: &crate::docstore::Entry,
    keywords: &[String],
    evidence_level: &str,
) -> Result<String, String> {
    let path = source_text_path(dir, &entry.id);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "{} 缺少正文全文缓存，摘要/要点字段不能支撑正文级 claim",
            entry.id
        )
    })?;
    let lower = text.to_ascii_lowercase();
    if evidence_level == "V2" || evidence_level == "V3" {
        let source_type = field(entry, "类型");
        if source_type.contains("摘要级") || source_type.contains("abstract") {
            return Err(format!(
                "{} 被标记为摘要级，不能支撑 {evidence_level} 正文 claim",
                entry.id
            ));
        }
    }
    let missing = keywords
        .iter()
        .filter(|keyword| !lower.contains(keyword.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "{} 正文全文缺少关键词: {}",
            entry.id,
            missing.join(", ")
        ));
    }
    Ok(format!(
        "{} 正文全文命中 {} 个关键词",
        entry.id,
        keywords.len()
    ))
}

fn parse_code_anchor(anchor: &str) -> Result<(&str, usize, &str), String> {
    let (path_and_line, commit) = anchor
        .rsplit_once('@')
        .ok_or_else(|| "代码证据锚必须是 file:line@commit".to_string())?;
    let (path, line) = path_and_line
        .rsplit_once(':')
        .ok_or_else(|| "代码证据锚必须是 file:line@commit".to_string())?;
    let line = line
        .parse::<usize>()
        .map_err(|_| "代码证据行号无效".to_string())?;
    if line == 0
        || path.is_empty()
        || path.starts_with('/')
        || path.contains("..")
        || !commit
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err("代码证据锚包含非法 path/line/commit".into());
    }
    Ok((path, line, commit))
}

fn verify_code(root: &Path, anchor: &str, keywords: &[String]) -> Result<String, String> {
    let (path, line, commit) = parse_code_anchor(anchor)?;
    let object = format!("{commit}:{path}");
    let output = Command::new("git")
        .current_dir(root)
        .args(["show", &object])
        .output()
        .map_err(|error| format!("git show 启动失败: {error}"))?;
    if !output.status.success() {
        return Err(format!("代码文件不存在于提交 {commit}: {path}"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let lines = text.lines().collect::<Vec<_>>();
    if line > lines.len() {
        return Err(format!("代码行不存在于提交 {commit}: {path}:{line}"));
    }
    let start = line.saturating_sub(2).max(1);
    let end = (line + 2).min(lines.len());
    let window = lines[start - 1..end].join("\n").to_ascii_lowercase();
    let missing = keywords
        .iter()
        .filter(|keyword| !window.contains(keyword.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "代码锚点邻域缺少语义关键词: {}",
            missing.join(", ")
        ));
    }
    Ok(format!(
        "{path}:{line}@{commit} 存在且锚点邻域命中 {} 个关键词",
        keywords.len()
    ))
}

fn claim_id(claim: &Value, index: usize) -> String {
    claim
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("claim-{}", index + 1))
}

pub struct ResearchVerifyTool;

#[async_trait]
impl Tool for ResearchVerifyTool {
    fn name(&self) -> &'static str {
        "research_verify"
    }

    fn description(&self) -> String {
        "FACT 引用核验与预算旋钮：capture_source 由工具抓取完整文献正文到 source_text；verify_claims 逐条检查 source refs、V0-V3、证据锚和 keywords，文献正文级必须命中全文、代码必须验证 file:line@commit；budget_set 在 loop 启动前写入显式预算，research_loop start 会实际消费。结果保存 verification.json。".into()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["capture_source", "verify_claims", "budget_get", "budget_set"] },
                "topic": { "type": "string", "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$" },
                "source_id": { "type": "string", "pattern": "^S-[0-9]+$" },
                "url": { "type": "string" },
                "claims": { "type": "array" },
                "max_rounds": { "type": "integer", "minimum": 1 },
                "max_tokens": { "type": "integer", "minimum": 1 },
                "max_concurrency": { "type": "integer", "minimum": 1 }
            },
            "required": ["action", "topic"],
            "additionalProperties": false
        })
    }

    fn resources(&self, input: &Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        vec![format!("write:{action}")]
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        let action = input.get("action").and_then(Value::as_str).unwrap_or("");
        let Some(topic) = input.get("topic").and_then(Value::as_str) else {
            return ToolOutput::needs_correction("MISSING_TOPIC", "research_verify 必须提供 topic");
        };
        let dir = match topic_dir(&ctx.project_root, topic) {
            Ok(dir) => dir,
            Err(error) => return ToolOutput::needs_correction("INVALID_TOPIC", error),
        };
        match action {
            "capture_source" => {
                let source_id = input.get("source_id").and_then(Value::as_str).unwrap_or("");
                let url = input.get("url").and_then(Value::as_str).unwrap_or("");
                if !valid_source_id(source_id)
                    || !(url.starts_with("http://") || url.starts_with("https://"))
                {
                    return ToolOutput::needs_correction(
                        "INVALID_SOURCE_CAPTURE",
                        "capture_source 必须提供有效 S-ID 和 http(s) URL",
                    );
                }
                let entries = match source_entries(&ctx.project_root, topic) {
                    Ok(entries) => entries,
                    Err(error) => return ToolOutput::error(error),
                };
                let Some(entry) = entries.get(source_id) else {
                    return ToolOutput::error(format!("source ref 不存在: {source_id}"));
                };
                let recorded_url = field(entry, "URL");
                if !recorded_url.is_empty() && recorded_url.trim() != url.trim() {
                    return ToolOutput::error(format!(
                        "source {source_id} 的 URL 不匹配：登记为 `{recorded_url}`，请求为 `{url}`"
                    ));
                }
                let fetched = match fetch_bytes(url, ctx, MAX_SOURCE_BYTES).await {
                    Ok(fetched) => fetched,
                    Err(error) => return ToolOutput::error(error),
                };
                if fetched.status >= 400 {
                    return ToolOutput::error(format!("正文抓取 HTTP {}: {url}", fetched.status));
                }
                let raw = String::from_utf8_lossy(&fetched.body);
                let text =
                    if fetched.content_type.contains("html") || raw.trim_start().starts_with('<') {
                        html_to_text(&raw)
                    } else {
                        raw.into_owned()
                    };
                if text.trim().is_empty() {
                    return ToolOutput::error("抓取正文为空，不能作为引用证据".to_string());
                }
                let path = source_text_path(&dir, source_id);
                match atomic_write(&path, &text) {
                    Ok(()) => ToolOutput::ok(json!({ "source_id": source_id, "url": url, "status": fetched.status, "body_chars": text.chars().count(), "stored": path.display().to_string() }).to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "verify_claims" => {
                let entries = match source_entries(&ctx.project_root, topic) {
                    Ok(entries) => entries,
                    Err(error) => return ToolOutput::error(error),
                };
                let claims = input
                    .get("claims")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if claims.is_empty() {
                    return ToolOutput::needs_correction(
                        "EMPTY_CLAIMS",
                        "verify_claims 必须提供 claims 数组",
                    );
                }
                let mut results = Vec::new();
                for (index, claim) in claims.iter().enumerate() {
                    let id = claim_id(claim, index);
                    let domain = claim.get("domain").and_then(Value::as_str).unwrap_or("");
                    let level = claim
                        .get("evidence_level")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let anchor = claim
                        .get("evidence_anchor")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let refs = claim
                        .get("source_ids")
                        .and_then(Value::as_array)
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let result = (|| -> Result<String, String> {
                        if !matches!(level, "V0" | "V1" | "V2" | "V3") {
                            return Err("evidence_level 必须是 V0/V1/V2/V3".into());
                        }
                        if anchor.trim().is_empty() || refs.is_empty() {
                            return Err("claim 必须提供 evidence_anchor 和 source_ids".into());
                        }
                        let keywords = claim_keywords(claim)?;
                        for source_id in &refs {
                            let entry = entries
                                .get(source_id)
                                .ok_or_else(|| format!("source ref 不存在: {source_id}"))?;
                            match domain {
                                "literature" | "文献" => {
                                    verify_literature(&dir, entry, &keywords, level)?;
                                }
                                "code" | "代码" => {
                                    verify_code(&ctx.project_root, anchor, &keywords)?;
                                }
                                _ => {
                                    return Err("domain 必须是 literature/文献 或 code/代码".into())
                                }
                            }
                        }
                        Ok(format!("{id} 通过: {}", refs.join(", ")))
                    })();
                    match result {
                        Ok(message) => {
                            results.push(json!({ "id": id, "passed": true, "message": message }))
                        }
                        Err(error) => {
                            results.push(json!({ "id": id, "passed": false, "error": error }))
                        }
                    }
                }
                let passed = results
                    .iter()
                    .filter(|result| result["passed"] == true)
                    .count();
                let report = json!({ "topic": topic, "claims": results, "passed": passed, "total": claims.len(), "all_passed": passed == claims.len() });
                let report_text =
                    serde_json::to_string_pretty(&report).unwrap_or_else(|_| report.to_string());
                if let Err(error) = atomic_write(&dir.join("verification.json"), &report_text) {
                    return ToolOutput::error(error);
                }
                if passed == claims.len() {
                    ToolOutput::ok(report.to_string())
                } else {
                    ToolOutput::error(report.to_string())
                }
            }
            "budget_get" => {
                let plan = match load_plan(&ctx.project_root, topic) {
                    Ok(Some(plan)) => plan,
                    Ok(None) => {
                        return ToolOutput::error(format!("topic `{topic}` 尚未创建研究计划"))
                    }
                    Err(error) => return ToolOutput::error(error),
                };
                let effective = if dir.join("budget.json").is_file() {
                    match std::fs::read_to_string(dir.join("budget.json"))
                        .ok()
                        .and_then(|text| serde_json::from_str::<PlanBudget>(&text).ok())
                    {
                        Some(budget) => budget,
                        None => return ToolOutput::error("budget.json 无效"),
                    }
                } else {
                    plan.budget
                };
                ToolOutput::ok(json!({ "topic": topic, "effective": effective, "override": dir.join("budget.json").is_file() }).to_string())
            }
            "budget_set" => {
                let plan = match load_plan(&ctx.project_root, topic) {
                    Ok(Some(plan)) => plan,
                    Ok(None) => {
                        return ToolOutput::error(format!("topic `{topic}` 尚未创建研究计划"))
                    }
                    Err(error) => return ToolOutput::error(error),
                };
                if plan.status != PlanStatus::Approved {
                    return ToolOutput::needs_confirmation(
                        "PLAN_NOT_APPROVED",
                        "预算旋钮也必须建立在已批准计划上",
                    );
                }
                if dir.join("loop.json").exists() {
                    return ToolOutput::error(
                        "检索环已启动，不能在运行中修改预算；请新建 topic 或先完成当前环"
                            .to_string(),
                    );
                }
                let budget = match parse_positive_budget(&input) {
                    Ok(budget) => budget,
                    Err(error) => return ToolOutput::needs_correction("INVALID_BUDGET", error),
                };
                let text = serde_json::to_string_pretty(&budget).unwrap_or_else(|_| "{}".into());
                match atomic_write(&budget_path(&dir), &text) {
                    Ok(()) => ToolOutput::ok(json!({ "topic": topic, "effective": budget, "applies_on": "next research_loop start" }).to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            _ => ToolOutput::needs_correction(
                "INVALID_ACTION",
                "action 只能是 capture_source/verify_claims/budget_get/budget_set",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research_loop::ResearchLoopTool;
    use crate::research_plan::{save_plan, PlanBudget, PlanNode, PlanNodeStatus, ResearchPlan};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kz-research-verify-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn source_doc(project: &Path, topic: &str, id: &str, title: &str, fields: &str) {
        let dir = project.join(".kanzei/research").join(topic);
        let text = format!("# Sources\n\n## {id} {title} [active]\n{fields}\n");
        atomic_write(&dir.join("sources.md"), &text).unwrap();
    }

    fn approved_plan(topic: &str) -> ResearchPlan {
        ResearchPlan {
            version: 1,
            topic: topic.into(),
            title: "预算测试".into(),
            status: PlanStatus::Approved,
            open_questions: Vec::new(),
            nodes: vec![PlanNode {
                id: "one".into(),
                title: "一项".into(),
                objective: "验证".into(),
                status: PlanNodeStatus::Pending,
                depends_on: Vec::new(),
                children: Vec::new(),
            }],
            budget: PlanBudget {
                max_rounds: 2,
                max_tokens: 100,
                max_concurrency: 1,
            },
            revision: 1,
        }
    }

    #[tokio::test]
    async fn literature_full_text_rejects_abstract_only_counterexample() {
        let project = root("literature");
        let ctx = ToolCtx::new(project.clone(), project.clone());
        source_doc(
            &project,
            "verify-smoke",
            "S-001",
            "CoALA",
            "- 类型: 文献(一手,正文级)\n- URL: https://example.test/coala\n",
        );
        let dir = project.join(".kanzei/research/verify-smoke");
        atomic_write(
            &source_text_path(&dir, "S-001"),
            "working episodic semantic procedural",
        )
        .unwrap();
        let tool = ResearchVerifyTool;
        let mismatch = tool
            .execute(
                json!({ "action": "capture_source", "topic": "verify-smoke", "source_id": "S-001", "url": "https://example.test/wrong" }),
                &ctx,
            )
            .await;
        assert!(mismatch.is_error);
        let claim = json!({ "id": "coala-memory", "domain": "literature", "evidence_level": "V2", "evidence_anchor": "S-001 正文 §2.3", "source_ids": ["S-001"], "keywords": ["working", "episodic", "semantic", "procedural"] });
        let passed = tool.execute(json!({ "action": "verify_claims", "topic": "verify-smoke", "claims": [claim.clone()] }), &ctx).await;
        assert!(!passed.is_error, "{}", passed.content);
        atomic_write(
            &source_text_path(&dir, "S-001"),
            "modular memory components",
        )
        .unwrap();
        let rejected = tool
            .execute(
                json!({ "action": "verify_claims", "topic": "verify-smoke", "claims": [claim] }),
                &ctx,
            )
            .await;
        assert!(rejected.is_error);
        std::fs::remove_dir_all(project).ok();
    }

    #[tokio::test]
    async fn budget_override_is_consumed_by_research_loop() {
        let project = root("budget");
        let ctx = ToolCtx::new(project.clone(), project.clone());
        save_plan(&project, &approved_plan("budget-smoke")).unwrap();
        let tool = ResearchVerifyTool;
        let set = tool.execute(json!({ "action": "budget_set", "topic": "budget-smoke", "max_rounds": 1, "max_tokens": 4, "max_concurrency": 1 }), &ctx).await;
        assert!(!set.is_error, "{}", set.content);
        ResearchLoopTool
            .execute(json!({ "action": "start", "topic": "budget-smoke" }), &ctx)
            .await;
        let state: Value = serde_json::from_str(
            &std::fs::read_to_string(project.join(".kanzei/research/budget-smoke/loop.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(state["max_tokens"], 4);
        std::fs::remove_dir_all(project).ok();
    }
}
