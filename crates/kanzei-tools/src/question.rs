//! question 工具(R-029):由 runner 转发到统一 ask 通道，等待用户回答。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct QuestionInput {
    /// 要向用户提出的问题。
    question: String,
    /// 可选答案按钮；为空时使用文本输入。
    #[serde(default)]
    options: Vec<String>,
    /// 可选默认答案。
    #[serde(default)]
    default: Option<String>,
    /// 是否允许用户多选(默认 false:点一个选项即提交)。
    #[serde(default)]
    multiple: bool,
}

pub struct QuestionTool;

#[async_trait]
impl Tool for QuestionTool {
    fn name(&self) -> &'static str {
        "question"
    }

    fn description(&self) -> String {
        "Ask the user a structured question. Params: question; optional options, default and multiple (multi-select). Use for clarification instead of guessing.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(QuestionInput)).unwrap()
    }

    fn resources(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::error("question must be handled by the interactive runner")
    }
}
