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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
/// Off = 省略档位并使用模型默认;None = 在模型支持时明确请求无思考。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    #[default]
    /// Omit effort settings and use provider/model defaults.
    Off,
    /// Explicitly request no reasoning where the model supports it.
    None,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            ReasoningEffort::Off => "off",
            ReasoningEffort::None => "none",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::XHigh => "xhigh",
            ReasoningEffort::Max => "max",
        }
    }

    /// 未知取值一律回落 Off:配置里写错档位不该让请求带上意外参数。
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" => ReasoningEffort::Low,
            "medium" | "mid" => ReasoningEffort::Medium,
            "high" => ReasoningEffort::High,
            "none" => ReasoningEffort::None,
            "xhigh" => ReasoningEffort::XHigh,
            "max" => ReasoningEffort::Max,
            _ => ReasoningEffort::Off,
        }
    }

    pub fn enabled(self) -> bool {
        !matches!(self, ReasoningEffort::Off | ReasoningEffort::None)
    }

    /// Anthropic 的 thinking 用 token 预算表达;返回 None 表示不开启。
    /// 下限 1024 是 API 硬要求。
    pub fn budget_tokens(self) -> Option<u32> {
        match self {
            ReasoningEffort::Off => None,
            ReasoningEffort::None => None,
            ReasoningEffort::Low => Some(4096),
            ReasoningEffort::Medium => Some(12288),
            ReasoningEffort::High => Some(24576),
            ReasoningEffort::XHigh => Some(32768),
            ReasoningEffort::Max => Some(49152),
        }
    }
}

/// OpenAI-compatible `reasoning_effort` is only understood by reasoning model families.
/// Unknown/custom model IDs stay untouched rather than receiving a parameter that can make
/// an otherwise valid request fail with HTTP 400.
pub(crate) fn supports_openai_reasoning_effort(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.contains("gpt-5")
        || model.contains("gpt-6")
        || model.contains("gpt-oss")
        || ["o1", "o3", "o4"].iter().any(|family| {
            model
                .split('/')
                .next_back()
                .unwrap_or(&model)
                .starts_with(family)
        })
        || model.contains("deepseek-r1")
        || model.contains("deepseek-reasoner")
        || model.contains("reasoner")
}

pub(crate) fn openai_reasoning_effort(
    model: &str,
    effort: ReasoningEffort,
) -> Option<&'static str> {
    if !supports_openai_reasoning_effort(model) {
        return None;
    }
    let model = model.trim().to_ascii_lowercase();
    let is_gpt56_or_6 = model.contains("gpt-5.6") || model.contains("gpt-6");
    match effort {
        ReasoningEffort::Off => None,
        ReasoningEffort::None if supports_openai_none_effort(&model) => Some("none"),
        ReasoningEffort::None if model_family_is_gpt6_astra(&model) => Some("low"),
        ReasoningEffort::None => None,
        ReasoningEffort::Low => Some("low"),
        ReasoningEffort::Medium => Some("medium"),
        ReasoningEffort::High => Some("high"),
        ReasoningEffort::XHigh if is_gpt56_or_6 => Some("xhigh"),
        ReasoningEffort::XHigh => Some("high"),
        ReasoningEffort::Max if is_gpt56_or_6 => Some("max"),
        ReasoningEffort::Max => Some("high"),
    }
}

fn supports_openai_none_effort(model: &str) -> bool {
    model.contains("gpt-5.6") || (model.contains("gpt-6") && !model_family_is_gpt6_astra(model))
}

fn model_family_is_gpt6_astra(model: &str) -> bool {
    model
        .split('/')
        .next_back()
        .unwrap_or(model)
        .starts_with("gpt-6-astra")
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
    /// 思考强度;Off 时各协议不指定档位,使用模型默认。
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
        assert_eq!(ReasoningEffort::parse("xhigh"), ReasoningEffort::XHigh);
        assert_eq!(ReasoningEffort::parse("max"), ReasoningEffort::Max);
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
