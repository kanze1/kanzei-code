//! 只读 task 工具的描述与参数；角色枚举取自本轮实际名册。

use kanzei_llm::ToolSpec;

/// 可选角色取自实际名册；有多个角色时提供选择参数。
pub(crate) fn task_spec_for(agent_names: &[String]) -> ToolSpec {
    let mut spec = task_spec();
    if agent_names.len() > 1 {
        spec.input_schema["properties"]["agent"] = serde_json::json!({
            "type": "string",
            "enum": agent_names,
            "description": "选择本次名册中的只读角色；explore 侧重检索，plan 侧重分析与规划。省略时使用名册首项。"
        });
    }
    spec
}

pub(crate) fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "将独立的只读检索或分析任务委派给子 Agent，返回调查结果。可读取文件、符号和 Git 状态、差异与历史；不提供写文件、执行 shell 或递归委派的能力。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "子任务目标、调查范围、必要上下文和预期结果。"
                },
                "description": {
                    "type": "string",
                    "description": "供用户识别此子任务的简短标签。"
                },
                "model": {
                    "type": "string",
                    "enum": ["fast", "primary"],
                    "description": "配置中的 fast 或 primary 模型档位；省略时使用所选角色的模型档位。"
                },
                "schema": {
                    "type": "object",
                    "description": "可选的结果 JSON Schema，支持 type、required、properties、items、enum，用于约束结构化返回结果。"
                }
            },
            "required": ["prompt"]
        }),
    }
}
