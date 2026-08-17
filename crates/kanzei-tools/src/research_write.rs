//! R-277 批3：大纲、分节写作与 LaTeX 编译回环。
//!
//! 写作阶段只消费已收敛的 research_loop 状态；outline 先行、分节单次落盘，
//! paper.tex 通过 R-273 的 compile_latex 编译，失败后才允许有限次 repair。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::docstore::DocStore;
use crate::latex_tool::compile_latex;

const MAX_REPAIR_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutlineSection {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchOutline {
    pub version: u32,
    pub topic: String,
    pub title: String,
    pub sections: Vec<OutlineSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompileState {
    pub attempts: u32,
    pub status: String,
    pub diagnostics: String,
}

fn topic_dir(root: &Path, topic: &str) -> Result<PathBuf, String> {
    DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    Ok(root.join(".kanzei/research").join(topic))
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建写作目录失败: {error}"))?;
    }
    let _lock = kanzei_base::atomic_file::lock_exclusive(path)
        .map_err(|error| format!("锁定写作工件失败: {error}"))?;
    kanzei_base::atomic_file::write_atomic(path, content)
        .map_err(|error| format!("写入写作工件失败: {error}"))
}

fn read_loop_status(root: &Path, topic: &str) -> Result<(String, String), String> {
    let path = topic_dir(root, topic)?.join("loop.json");
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("读取检索环状态失败: {error}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("检索环状态 JSON 无效: {error}"))?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "检索环状态缺少 status".to_string())?;
    let phase = value
        .get("phase")
        .and_then(Value::as_str)
        .ok_or_else(|| "检索环状态缺少 phase".to_string())?;
    Ok((status.into(), phase.into()))
}

fn require_writing_ready(root: &Path, topic: &str) -> Result<(), String> {
    let (status, phase) = read_loop_status(root, topic)?;
    if matches!(status.as_str(), "ready_to_write" | "budget_exhausted") && phase == "synthesize" {
        Ok(())
    } else {
        Err(format!(
            "检索环当前状态 {status} / {phase}，尚未收敛到写作阶段"
        ))
    }
}

fn parse_outline(input: &Value, topic: &str) -> Result<ResearchOutline, String> {
    let title = input
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let sections = input
        .get("sections")
        .and_then(Value::as_array)
        .ok_or_else(|| "write_outline 必须提供 sections 数组".to_string())?;
    if title.is_empty() || sections.is_empty() {
        return Err("write_outline 必须提供非空 title 和 sections".into());
    }
    let mut parsed = Vec::with_capacity(sections.len());
    for item in sections {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("").trim();
        let section_title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let objective = item
            .get("objective")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let source_ids = item
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
        if id.is_empty()
            || section_title.is_empty()
            || objective.is_empty()
            || source_ids.is_empty()
        {
            return Err("每个 outline section 必须有 id/title/objective/source_ids".into());
        }
        if !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(format!("section id `{id}` 含非法字符"));
        }
        if parsed
            .iter()
            .any(|section: &OutlineSection| section.id == id)
        {
            return Err(format!("outline section id 重复: {id}"));
        }
        parsed.push(OutlineSection {
            id: id.into(),
            title: section_title.into(),
            objective: objective.into(),
            source_ids,
        });
    }
    Ok(ResearchOutline {
        version: 1,
        topic: topic.into(),
        title: title.into(),
        sections: parsed,
    })
}

fn outline_markdown(outline: &ResearchOutline) -> String {
    let mut text = format!("# {}\n\n", outline.title);
    for (index, section) in outline.sections.iter().enumerate() {
        text.push_str(&format!(
            "## {}. {}\n\n- id: {}\n- objective: {}\n- source_ids: {}\n\n",
            index + 1,
            section.title,
            section.id,
            section.objective,
            section.source_ids.join(", ")
        ));
    }
    text
}

fn load_outline(dir: &Path) -> Result<ResearchOutline, String> {
    let text = std::fs::read_to_string(dir.join("outline.json"))
        .map_err(|error| format!("读取 outline.json 失败: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("outline.json 无效: {error}"))
}

fn load_compile_state(dir: &Path) -> Result<Option<CompileState>, String> {
    let path = dir.join("compile.json");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取 compile.json 失败: {error}"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("compile.json 无效: {error}"))
}

fn compile_and_record(dir: &Path, previous: Option<CompileState>) -> ToolOutput {
    let attempts = previous.map_or(0, |state| state.attempts).saturating_add(1);
    let (ok, diagnostics) = compile_latex(dir, "paper.tex");
    let state = CompileState {
        attempts,
        status: if ok { "passed".into() } else { "failed".into() },
        diagnostics: diagnostics.clone(),
    };
    let state_text = match serde_json::to_string_pretty(&state) {
        Ok(text) => text,
        Err(error) => return ToolOutput::error(format!("序列化 compile.json 失败: {error}")),
    };
    if let Err(error) = atomic_write(&dir.join("compile.json"), &state_text) {
        return ToolOutput::error(error);
    }
    let summary = json!({
        "status": state.status,
        "attempts": state.attempts,
        "diagnostics": state.diagnostics,
        "paper": dir.join("paper.tex").display().to_string(),
    })
    .to_string();
    if ok {
        ToolOutput::ok(summary)
    } else {
        ToolOutput::error(summary)
    }
}

pub struct ResearchWriteTool;

#[async_trait]
impl Tool for ResearchWriteTool {
    fn name(&self) -> &'static str {
        "research_write"
    }

    fn description(&self) -> String {
        "研究写作流水线：write_outline 先落 outline.md/json；write_section 按 outline 顺序单次生成带 source_ids 的 .tex 分节；assemble_paper 组装 paper.tex；compile_paper 通过 R-273 LaTeX 通道编译并记录 compile.json；repair_paper 仅在失败且未超过 3 次时修复并重编译。".into()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["write_outline", "write_section", "assemble_paper", "compile_paper", "repair_paper"] },
                "topic": { "type": "string", "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$" },
                "title": { "type": "string" },
                "sections": { "type": "array" },
                "section_id": { "type": "string" },
                "content": { "type": "string" }
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
            return ToolOutput::needs_correction("MISSING_TOPIC", "research_write 必须提供 topic");
        };
        let dir = match topic_dir(&ctx.project_root, topic) {
            Ok(dir) => dir,
            Err(error) => return ToolOutput::needs_correction("INVALID_TOPIC", error),
        };
        match action {
            "write_outline" => {
                if let Err(error) = require_writing_ready(&ctx.project_root, topic) {
                    return ToolOutput::error(error);
                }
                if dir.join("outline.json").exists() || dir.join("outline.md").exists() {
                    return ToolOutput::error("outline 已存在；写作阶段不允许重复生成".to_string());
                }
                let outline = match parse_outline(&input, topic) {
                    Ok(outline) => outline,
                    Err(error) => return ToolOutput::needs_correction("INVALID_OUTLINE", error),
                };
                let json_text = match serde_json::to_string_pretty(&outline) {
                    Ok(text) => text,
                    Err(error) => return ToolOutput::error(format!("序列化 outline 失败: {error}")),
                };
                if let Err(error) = atomic_write(&dir.join("outline.json"), &json_text) {
                    return ToolOutput::error(error);
                }
                if let Err(error) = atomic_write(&dir.join("outline.md"), &outline_markdown(&outline)) {
                    return ToolOutput::error(error);
                }
                ToolOutput::ok(json!({ "topic": topic, "outline": "outline.md", "sections": outline.sections }).to_string())
            }
            "write_section" => {
                let outline = match load_outline(&dir) {
                    Ok(outline) => outline,
                    Err(error) => return ToolOutput::error(error),
                };
                let section_id = input.get("section_id").and_then(Value::as_str).unwrap_or("").trim();
                let content = input.get("content").and_then(Value::as_str).unwrap_or("").trim();
                let Some(section) = outline.sections.iter().find(|item| item.id == section_id) else {
                    return ToolOutput::needs_correction("UNKNOWN_SECTION", "section_id 不在 outline 中");
                };
                if content.is_empty() {
                    return ToolOutput::needs_correction("EMPTY_SECTION", "section content 不能为空");
                }
                let section_path = dir.join("sections").join(format!("{}.tex", section.id));
                if section_path.exists() {
                    return ToolOutput::error("该 section 已生成；不允许重复写入".to_string());
                }
                let text = format!("% source_ids: {}\n{}\n", section.source_ids.join(", "), content);
                match atomic_write(&section_path, &text) {
                    Ok(()) => ToolOutput::ok(json!({ "section_id": section.id, "path": section_path.display().to_string(), "source_ids": section.source_ids }).to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "assemble_paper" => {
                let outline = match load_outline(&dir) {
                    Ok(outline) => outline,
                    Err(error) => return ToolOutput::error(error),
                };
                let paper_path = dir.join("paper.tex");
                if paper_path.exists() {
                    return ToolOutput::error("paper.tex 已存在；使用 compile_paper 或失败后的 repair_paper".to_string());
                }
                let mut paper = format!("\\documentclass{{article}}\n\\title{{{}}}\n\\begin{{document}}\n\\maketitle\n", outline.title);
                for section in &outline.sections {
                    let section_path = dir.join("sections").join(format!("{}.tex", section.id));
                    if !section_path.is_file() {
                        return ToolOutput::error(format!("section `{}` 尚未生成，不能组装 paper.tex", section.id));
                    }
                    paper.push_str(&format!("\\input{{sections/{}}}\n", section.id));
                }
                paper.push_str("\\end{document}\n");
                match atomic_write(&paper_path, &paper) {
                    Ok(()) => ToolOutput::ok(json!({ "paper": paper_path.display().to_string(), "sections": outline.sections.len() }).to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "compile_paper" => {
                if !dir.join("paper.tex").is_file() {
                    return ToolOutput::error("paper.tex 不存在，请先 assemble_paper".to_string());
                }
                let previous = match load_compile_state(&dir) {
                    Ok(state) => state,
                    Err(error) => return ToolOutput::error(error),
                };
                compile_and_record(&dir, previous)
            }
            "repair_paper" => {
                let previous = match load_compile_state(&dir) {
                    Ok(Some(state)) => state,
                    Ok(None) => return ToolOutput::error("尚无失败的 LaTeX 编译记录".to_string()),
                    Err(error) => return ToolOutput::error(error),
                };
                if previous.status != "failed" {
                    return ToolOutput::error("只有最近一次编译失败后才能 repair_paper".to_string());
                }
                if previous.attempts >= MAX_REPAIR_ATTEMPTS {
                    return ToolOutput::error("LaTeX 修复次数已达上限 3，停止回环并保留诊断".to_string());
                }
                let content = input.get("content").and_then(Value::as_str).unwrap_or("").trim();
                if content.is_empty() {
                    return ToolOutput::needs_correction("EMPTY_REPAIR", "repair_paper 必须提供修复后的完整 paper.tex 内容");
                }
                if let Err(error) = atomic_write(&dir.join("paper.tex"), content) {
                    return ToolOutput::error(error);
                }
                compile_and_record(&dir, Some(previous))
            }
            _ => ToolOutput::needs_correction("INVALID_ACTION", "action 只能是 write_outline/write_section/assemble_paper/compile_paper/repair_paper"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research_loop::ResearchLoopTool;
    use crate::research_plan::{
        save_plan, PlanBudget, PlanNode, PlanNodeStatus, PlanStatus, ResearchPlan,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kz-research-write-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn approved_plan(topic: &str) -> ResearchPlan {
        ResearchPlan {
            version: 1,
            topic: topic.into(),
            title: "写作测试".into(),
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
                max_rounds: 1,
                max_tokens: 100,
                max_concurrency: 1,
            },
            revision: 1,
        }
    }

    async fn ready_ctx() -> (PathBuf, ToolCtx) {
        let project = root();
        save_plan(&project, &approved_plan("write-smoke")).unwrap();
        let ctx = ToolCtx::new(project.clone(), project.clone());
        ResearchLoopTool
            .execute(json!({ "action": "start", "topic": "write-smoke" }), &ctx)
            .await;
        let state_path = project.join(".kanzei/research/write-smoke/loop.json");
        let mut state: Value =
            serde_json::from_str(&std::fs::read_to_string(&state_path).unwrap()).unwrap();
        state["status"] = Value::String("ready_to_write".into());
        state["phase"] = Value::String("synthesize".into());
        atomic_write(&state_path, &serde_json::to_string_pretty(&state).unwrap()).unwrap();
        (project, ctx)
    }

    #[tokio::test]
    async fn outline_precedes_single_write_sections_and_paper_assembly() {
        let (project, ctx) = ready_ctx().await;
        let tool = ResearchWriteTool;
        let before = tool.execute(json!({ "action": "write_section", "topic": "write-smoke", "section_id": "intro", "content": "x" }), &ctx).await;
        assert!(before.is_error);
        let outline_input = json!({ "action": "write_outline", "topic": "write-smoke", "title": "测试论文", "sections": [
            { "id": "intro", "title": "引言", "objective": "交代问题", "source_ids": ["S-1"] },
            { "id": "method", "title": "方法", "objective": "说明方法", "source_ids": ["S-2"] }
        ]});
        assert!(!tool.execute(outline_input.clone(), &ctx).await.is_error);
        assert!(tool.execute(outline_input, &ctx).await.is_error);
        for (id, content) in [("intro", "引言内容"), ("method", "方法内容")] {
            let output = tool.execute(json!({ "action": "write_section", "topic": "write-smoke", "section_id": id, "content": content }), &ctx).await;
            assert!(!output.is_error, "{}", output.content);
        }
        let paper = tool
            .execute(
                json!({ "action": "assemble_paper", "topic": "write-smoke" }),
                &ctx,
            )
            .await;
        assert!(!paper.is_error, "{}", paper.content);
        let text = std::fs::read_to_string(project.join(".kanzei/research/write-smoke/paper.tex"))
            .unwrap();
        assert!(text.find("sections/intro").unwrap() < text.find("sections/method").unwrap());
        std::fs::remove_dir_all(project).ok();
    }

    #[tokio::test]
    async fn compile_failure_is_recorded_and_repair_is_bounded() {
        let (project, ctx) = ready_ctx().await;
        let tool = ResearchWriteTool;
        tool.execute(json!({ "action": "write_outline", "topic": "write-smoke", "title": "错误论文", "sections": [{ "id": "bad", "title": "坏节", "objective": "触发诊断", "source_ids": ["S-1"] }] }), &ctx).await;
        tool.execute(json!({ "action": "write_section", "topic": "write-smoke", "section_id": "bad", "content": "\\badcommand" }), &ctx).await;
        tool.execute(
            json!({ "action": "assemble_paper", "topic": "write-smoke" }),
            &ctx,
        )
        .await;
        let compile = tool
            .execute(
                json!({ "action": "compile_paper", "topic": "write-smoke" }),
                &ctx,
            )
            .await;
        assert!(compile.is_error);
        let state: CompileState = serde_json::from_str(
            &std::fs::read_to_string(project.join(".kanzei/research/write-smoke/compile.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(state.status, "failed");
        assert_eq!(state.attempts, 1);
        let repair = tool.execute(json!({ "action": "repair_paper", "topic": "write-smoke", "content": "\\documentclass{article}\\begin{document}ok\\end{document}" }), &ctx).await;
        let state: CompileState = serde_json::from_str(
            &std::fs::read_to_string(project.join(".kanzei/research/write-smoke/compile.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(state.attempts, 2);
        assert!(!repair.content.is_empty());
        std::fs::remove_dir_all(project).ok();
    }
}
