use async_trait::async_trait;
use serde_json::Value;

/// 工具执行上下文。M1 起会带 session/agent/权限句柄。
#[derive(Debug, Clone)]
pub struct ToolCtx {
    pub cwd: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        ToolOutput { content: content.into(), is_error: false }
    }

    pub fn error(content: impl Into<String>) -> Self {
        ToolOutput { content: content.into(), is_error: true }
    }
}

/// 工具契约:描述保持一句话级别(系统提示词预算红线),规则靠代码强制。
/// 执行失败返回 is_error 输出回喂模型(修复回路),不向用户抛异常。
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    /// 描述可动态生成(如 bash 按实际选中的 shell 生成语法提示)。
    fn description(&self) -> String;
    fn input_schema(&self) -> Value;
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput;
}

/// 输入解析失败时给模型的纠错反馈(设计红线 1:不崩溃、告知正确格式)。
pub fn repair_hint(tool: &dyn Tool, raw_input: &str, problem: &str) -> ToolOutput {
    ToolOutput::error(format!(
        "Invalid input for tool `{}`: {problem}\nYour raw input was: {}\nExpected JSON schema:\n{}\nRetry the tool call with corrected JSON.",
        tool.name(),
        truncate(raw_input, 500),
        tool.input_schema(),
    ))
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}
