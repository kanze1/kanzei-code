use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema(由 harness 用 schemars 生成)。
    pub input_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
    },
    /// 图片内容,以 base64 编码传输,不含 data: 前缀。
    Image {
        media_type: String,
        data: String,
    },
    /// PDF/其它文档内容,以 base64 编码传输。
    Document {
        media_type: String,
        data: String,
    },
    Reasoning {
        text: String,
        signature: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        call_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Message {
            role: Role::User,
            parts: vec![Part::Text { text: text.into() }],
        }
    }

    pub fn assistant(parts: Vec<Part>) -> Self {
        Message {
            role: Role::Assistant,
            parts,
        }
    }

    /// 工具结果按 Anthropic 语义以 user 角色回传。
    pub fn tool_results(parts: Vec<Part>) -> Self {
        Message {
            role: Role::User,
            parts,
        }
    }
}

/// 思考强度:三种协议的表达方式不同(anthropic 是 token 预算,openai 系是 effort 档位),
/// 统一成一档一档的强度,由各协议自己翻译成原生参数。
/// 默认 Off = 完全不发思考参数,与未支持该能力时的行为一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            ReasoningEffort::Off => "off",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }

    /// 未知取值一律回落 Off:配置里写错档位不该让请求带上意外参数。
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" => ReasoningEffort::Low,
            "medium" | "mid" => ReasoningEffort::Medium,
            "high" | "max" => ReasoningEffort::High,
            _ => ReasoningEffort::Off,
        }
    }

    pub fn enabled(self) -> bool {
        self != ReasoningEffort::Off
    }

    /// Anthropic 的 thinking 用 token 预算表达;返回 None 表示不开启。
    /// 下限 1024 是 API 硬要求。
    pub fn budget_tokens(self) -> Option<u32> {
        match self {
            ReasoningEffort::Off => None,
            ReasoningEffort::Low => Some(4096),
            ReasoningEffort::Medium => Some(12288),
            ReasoningEffort::High => Some(24576),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
    /// system prompt 分块:agent 提示词 + harness baseline(Context Epoch 内字节不变)。
    pub system: Vec<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    /// 思考强度;Off 时各协议不发任何思考相关参数。
    pub reasoning: ReasoningEffort,
    /// 服务档位;仅 Responses 协议在设置为 priority 时发送。
    pub service_tier: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 配置里写错档位不该让请求带上意外参数:未知值一律回落 Off。
    #[test]
    fn effort_parse_falls_back_to_off() {
        assert_eq!(ReasoningEffort::parse("low"), ReasoningEffort::Low);
        assert_eq!(ReasoningEffort::parse(" HIGH "), ReasoningEffort::High);
        assert_eq!(ReasoningEffort::parse("max"), ReasoningEffort::High);
        assert_eq!(ReasoningEffort::parse("medium"), ReasoningEffort::Medium);
        assert_eq!(ReasoningEffort::parse("off"), ReasoningEffort::Off);
        assert_eq!(ReasoningEffort::parse(""), ReasoningEffort::Off);
        assert_eq!(ReasoningEffort::parse("很高"), ReasoningEffort::Off);
        assert_eq!(ReasoningEffort::default(), ReasoningEffort::Off);
        assert!(!ReasoningEffort::Off.enabled());
        assert!(ReasoningEffort::Low.enabled());
        // 预算必须满足 Anthropic 的 1024 下限
        for effort in [
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
        ] {
            assert!(effort.budget_tokens().unwrap() >= 1024);
        }
        assert!(ReasoningEffort::Off.budget_tokens().is_none());
    }
}
