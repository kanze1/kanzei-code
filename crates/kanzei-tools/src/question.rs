//! question 工具：runner 在交互模式询问用户，在自动模式持久化模型决定。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct QuestionInput {
    /// 交互模式中的必要问题，或自动模式中需要留档的决策主题。
    question: String,
    /// 可选答案。既吃裸字符串,也吃 `{"label": "...", "note": "选它意味着什么"}`
    /// (`description` 是 `note` 的别名)。为空时使用文本输入。
    #[serde(default)]
    options: Vec<serde_json::Value>,
    /// 交互界面的可选默认项，不作为自动模式的决定或用户回答。
    #[serde(default)]
    default: Option<String>,
    /// 是否允许用户多选(默认 false:点一个选项即提交)。
    #[serde(default)]
    multiple: bool,
    /// true：立即返回并继续独立工作，答案异步送回原会话/子任务。仅桌面端支持。
    #[serde(default)]
    background: bool,
    /// 自动模式：值得复核的重要决定。普通实现选择不要求登记。
    #[serde(default)]
    decision: Option<AutomaticDecision>,
    /// 调查后仍无法取得的必要外部事实，说明缺口和解除条件；不用于代替普通选择。
    #[serde(default)]
    missing_fact: Option<String>,
    /// 已存在的相关 Work Unit ID，供复核后定位受影响的验证。
    #[serde(default)]
    work_unit_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
struct AutomaticDecision {
    /// 在已有授权范围内作出的决定。
    answer: String,
    /// 给用户看的简短理由，不包含隐藏思维过程。
    rationale: String,
    /// 对方案、成本或交付的实质影响。
    impact: String,
    /// 实际采用的已确认偏好引用；没有引用时可省略。
    #[serde(default)]
    preference_refs: Vec<String>,
}

pub struct QuestionTool;

#[async_trait]
impl Tool for QuestionTool {
    // The runner records/forwards control-plane choices; it never edits user resources.
    fn read_only_support(&self) -> kanzei_harness::tool::ReadOnlySupport {
        kanzei_harness::tool::ReadOnlySupport::ControlPlane
    }
    fn name(&self) -> &'static str {
        "question"
    }

    fn description(&self) -> String {
        "交互模式询问必要信息；自动模式用 decision 记录值得复核的重要决定，或用 missing_fact 记录调查后仍缺少的必要事实。普通实现选择自主处理。决定留档后可继续工作，记录不构成用户回答或新增授权。"
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
