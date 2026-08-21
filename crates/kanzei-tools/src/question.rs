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
}

pub struct QuestionTool;

#[async_trait]
impl Tool for QuestionTool {
    fn name(&self) -> &'static str {
        "question"
    }

    fn description(&self) -> String {
        "Ask the user a structured question. Params: question; optional options, default and          multiple (multi-select). Each option is either a bare string or          {label, note} where `note` says what CHOOSING IT MEANS — give notes whenever the          option labels alone do not carry the consequence, which is most of the time for          design choices. Use this for clarification instead of guessing."
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
        ToolOutput::error("question must be handled by the interactive runner")
    }
}
