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
        "AUTO research 持久阶段。get 回读状态；survey_complete 提交调研；publish_map 提交带来源的方向地图并等待用户选题；define_mvp 定义可证伪方案；environment_ready 绑定成功基线；record_mvp 绑定真实结果；interpret 解读并 complete/iterate/pivot；request_input 记录资源或信息缺口。工件路径相对课题目录，所有写操作必传 get 返回的 revision。启动、选方向和恢复只由用户入口执行。".into()
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object", "properties": {
            "action": {"type":"string", "enum":["get", "survey_complete", "publish_map", "define_mvp", "environment_ready", "record_mvp", "interpret", "request_input"]},
            "topic":{"type":"string"}, "revision":{"type":"integer", "minimum":1},
            "artifact":{"type":"string", "description":"相对于当前课题的非空工件路径"},
            "directions":{"type":"array", "items": schemars::schema_for!(super::Direction)},
            "mvp": schemars::schema_for!(super::Mvp),
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
        let result = (|| {
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
            super::advance(&ctx.project_root, &topic, revision, &action, &input)
        })();
        match result {
            Ok(state) => {
                ToolOutput::ok(json!({"workflow":state, "guidance":state.guidance()}).to_string())
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
