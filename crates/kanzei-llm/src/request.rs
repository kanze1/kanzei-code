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
    Text { text: String },
    Reasoning { text: String, signature: Option<String> },
    ToolCall { id: String, name: String, input: Value },
    ToolResult { call_id: String, content: String, is_error: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Message { role: Role::User, parts: vec![Part::Text { text: text.into() }] }
    }

    pub fn assistant(parts: Vec<Part>) -> Self {
        Message { role: Role::Assistant, parts }
    }

    /// 工具结果按 Anthropic 语义以 user 角色回传。
    pub fn tool_results(parts: Vec<Part>) -> Self {
        Message { role: Role::User, parts }
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
}
