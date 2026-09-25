use std::sync::Arc;

use async_trait::async_trait;
use kanzei_harness::{
    refreshing_source, Component, HarnessDraft, ProfileKind, ResolveCtx, Tool, ToolConcurrency,
    ToolCtx, ToolOutput,
};
use serde_json::{json, Value};

#[derive(Default)]
pub struct ResearchWorkflowTool {
    pub topic: Option<String>,
}

#[async_trait]
impl Tool for ResearchWorkflowTool {
    fn name(&self) -> &'static str {
        "research_workflow"
    }
    fn description(&self) -> String {
        "AUTO research 全流程：get 回读流程及 recovery（剩余实验预算、active_runs、待 record_full 的 pending_full_results）；survey_complete/publish_map 调研地图并等待用户选题；define_mvp/prepare_compute/environment_ready 准备环境与基线；record_mvp/interpret 最小验证；define_full/record_full 完整实验；submit_analysis 综合分析；paper_init 生成或导入模板；submit_paper/review_paper 检查数值与引用；compile_paper 真正编译并交付 PDF。request_input 记录缺口。路径相对课题，写操作必传 revision；用户选题与预算不可由模型更改。完整实验 params_text 需含 experiment_id/role/seed；compute 返回实验必须使用的环境与 Python 路径。".into()
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object", "properties": {
            "action": {"type":"string", "enum":["get", "survey_complete", "publish_map", "define_mvp", "prepare_compute", "environment_ready", "record_mvp", "interpret", "define_full", "record_full", "submit_analysis", "paper_init", "submit_paper", "review_paper", "compile_paper", "request_input"]},
            "topic":{"type":"string"}, "revision":{"type":"integer", "minimum":1},
            "artifact":{"type":"string", "description":"survey_complete/publish_map/environment_ready/interpret/submit_analysis 必填。先用 write 创建非空文件，再传相对于课题的路径（如 survey.md，不带 .kanzei/research/<topic>/ 前缀）。"},
            "directions":{"type":"array", "items": schemars::schema_for!(super::Direction)},
            "mvp": schemars::schema_for!(super::Mvp),
            "compute": schemars::schema_for!(super::compute::ComputeSpec),
            "full_plan": {"type":"object", "properties":{
                "protocol":{"type":"string","description":"课题内已写好的完整实验协议路径，例如 full-protocol.md。"},
                "experiments":{"type":"array","minItems":6,"maxItems":100,"items":schemars::schema_for!(super::ExperimentSpec)}
            },"required":["protocol","experiments"]},
            "full_results":{"type":"array","items":schemars::schema_for!(super::FullResult)},
            "reuse_results":{"type":"array","items":schemars::schema_for!(super::FullResult),"description":"define_full 时显式复用已归档且定义、环境未变的成功结果；新矩阵至少有一个新实验项"},
            "claims":{"type":"array","items":schemars::schema_for!(super::paper::Claim)},
            "title":{"type":"string"},"template_path":{"type":"string"},"template_source":{"type":"string"},
            "baseline_result":{"type":"string"}, "result_ids":{"type":"array", "items":{"type":"string"}},
            "verdict":{"type":"string", "enum":["supported", "rejected", "inconclusive"]},
            "next":{"type":"string", "enum":["complete", "iterate", "pivot"]}, "reason":{"type":"string"}
        }, "required":["action", "topic"]})
    }
    fn resources(&self, input: &Value) -> Vec<String> {
        let action = input["action"].as_str().unwrap_or("unknown");
        vec![format!(
            "{}:{action}",
            if action == "get" { "read" } else { "write" }
        )]
    }
    fn concurrency(&self, _: &Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::WorktreeWrite(ctx.worktree_concurrency_key())
    }
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        let result = async {
            let topic = super::required(&input, "topic")?;
            if self.topic.as_ref().is_some_and(|bound| bound != &topic) {
                return Err("本会话只能推进绑定的研究课题".into());
            }
            let action = super::required(&input, "action")?;
            if action == "get" {
                return super::load(&ctx.project_root, &topic)?
                    .ok_or_else(|| "尚未启动 AUTO research，请由用户在课题概览启动".into());
            }
            let revision = input["revision"].as_u64().ok_or("必须传当前 revision")?;
            match action.as_str() {
                "prepare_compute" => {
                    let spec = serde_json::from_value(input["compute"].clone())
                        .map_err(|e| e.to_string())?;
                    super::compute::prepare(&ctx.project_root, &topic, revision, spec).await
                }
                "paper_init" => {
                    super::paper::initialize(&ctx.project_root, &topic, revision, &input)
                }
                "compile_paper" => super::paper::compile(&ctx.project_root, &topic, revision).await,
                _ => super::advance(&ctx.project_root, &topic, revision, &action, &input),
            }
        }
        .await;
        match result {
            Ok(state) => {
                let mut view = serde_json::to_value(&state).unwrap();
                view["history_count"] = json!(state.history.len());
                view["history"] = json!(state.history.iter().rev().take(2).collect::<Vec<_>>());
                if let Some(snapshot) = view
                    .pointer_mut("/compute/snapshot")
                    .and_then(Value::as_object_mut)
                {
                    snapshot.remove("packages");
                }
                if let Some(rounds) = view["full_rounds"].as_array_mut() {
                    for round in rounds {
                        if let Some(snapshot) = round
                            .pointer_mut("/compute/snapshot")
                            .and_then(Value::as_object_mut)
                        {
                            snapshot.remove("packages");
                        }
                    }
                }
                let recovery = super::lifecycle::recovery(&ctx.project_root, &state)
                    .unwrap_or_else(|error| json!({"error":error}));
                ToolOutput::ok(json!({"workflow":view, "recovery":recovery,"guidance":state.guidance(),"full_history_file":format!(".kanzei/research/{}/workflow.json",state.topic)}).to_string())
            }
            Err(error) => ToolOutput::needs_correction("RESEARCH_WORKFLOW", error),
        }
    }
}

/// Refresh the topic's checkpoint each model step, including after compression or restart.
pub struct ResearchWorkflowContext(pub String);
impl Component for ResearchWorkflowContext {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Research {
            return Ok(());
        }
        let topic = self.0.clone();
        draft.tools.insert(
            "research_workflow",
            Arc::new(ResearchWorkflowTool {
                topic: Some(topic.clone()),
            }),
        );
        draft.context.insert(
            "research/workflow",
            refreshing_source(
                "research/workflow",
                move |ctx: &ResolveCtx| match super::load(&ctx.project_root, &topic) {
                    Ok(Some(state)) => Some(state.guidance()),
                    Ok(None) => None,
                    Err(error) => Some(format!(
                        "AUTO research 状态读取失败，请停止自动推进并报告：{error}"
                    )),
                },
            ),
        );
        Ok(())
    }
}
