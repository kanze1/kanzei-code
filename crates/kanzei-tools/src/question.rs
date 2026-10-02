//! question 工具：runner 在交互模式询问用户，在自动模式持久化模型决定。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct QuestionInput {
    /// 要向用户提出的问题。
    question: String,
    /// 可选答案。既吃裸字符串,也吃 `{"label": "...", "note": "选它意味着什么"}`
    /// (`description` 是 `note` 的别名)。为空时使用文本输入。
    #[serde(default)]
    options: Vec<serde_json::Value>,
    /// 可选默认答案。
    #[serde(default)]
    default: Option<String>,
    /// 是否允许用户多选(默认 false:点一个选项即提交)。
    #[serde(default)]
    multiple: bool,
    /// true：立即返回并继续独立工作，答案异步送回原会话/子任务。仅桌面端支持。
    #[serde(default)]
    background: bool,
    /// 自动模式：自己作出的决定。记录后继续执行；不是用户回答。
    #[serde(default)]
    decision: Option<AutomaticDecision>,
    /// 仅用于确实无法获取的外部事实（如凭证、真实路径）；普通设计选择用 decision。
    #[serde(default)]
    missing_fact: Option<String>,
    /// 已存在的相关 Work Unit ID，供复核后定位受影响的验证。
    #[serde(default)]
    work_unit_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct AutomaticDecision {
    answer: String,
    /// 给用户看的简短理由，不包含隐藏思维过程。
    rationale: String,
    impact: String,
    #[serde(default)]
    preference_refs: Vec<String>,
}

pub struct QuestionTool;

#[async_trait]
impl Tool for QuestionTool {
    fn name(&self) -> &'static str {
        "question"
    }

    fn description(&self) -> String {
        "Structured choice and decision log. In interactive mode ask the user (question, options, default, multiple). In autonomous mode decide ordinary choices yourself, based on the user's instructions and known preferences: supply decision {answer, rationale, impact, preference_refs} and optionally work_unit_id. The runner persists it before you continue; this is an agent decision, never a user answer or authorization. If an external fact truly cannot be obtained, supply missing_fact, block only the dependent work and continue independent work. Do not use missing_fact for ordinary design choices. Options accept strings or {label, note}."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(QuestionInput)).unwrap();
        // schemars 对 `Vec<serde_json::Value>` 只能给出「任意值数组」,模型据此
        // 不知道 {label, note} 这条路存在。这里显式写出两种形态——schema 是模型
        // 唯一的契约来源,描述里说了而 schema 里没有,等于没说。
        schema["properties"]["options"]["items"] = serde_json::json!({
            "anyOf": [
                { "type": "string", "description": "Option label with no extra explanation" },
                {
                    "type": "object",
                    "required": ["label"],
                    "properties": {
                        "label": { "type": "string" },
                        "note": {
                            "type": "string",
                            "description": "What choosing this option means or implies"
                        }
                    }
                }
            ]
        });
        schema
    }

    fn resources(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::error(
            "question must be handled by the runner's decision or interaction channel",
        )
    }
}
