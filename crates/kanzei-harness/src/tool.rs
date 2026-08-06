use async_trait::async_trait;
use serde_json::Value;

/// 工具执行上下文。
#[derive(Debug, Clone)]
pub struct ToolCtx {
    pub cwd: std::path::PathBuf,
    /// 项目根(.kanzei/.git 所在);工作区文档(requirements/defects/sources)挂在这下面。
    pub project_root: std::path::PathBuf,
}

impl ToolCtx {
    pub fn new(cwd: std::path::PathBuf) -> Self {
        let project_root =
            crate::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
        ToolCtx { cwd, project_root }
    }
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    /// 面向 UI 的结构化展示(diff/终端块等),模型看不到,只给人看。
    /// 形如 {"kind":"diff","path":...,"diff":...} / {"kind":"terminal",...}。
    pub display: Option<serde_json::Value>,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        ToolOutput {
            content: content.into(),
            is_error: false,
            display: None,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        ToolOutput {
            content: content.into(),
            is_error: true,
            display: None,
        }
    }

    pub fn with_display(mut self, display: serde_json::Value) -> Self {
        self.display = Some(display);
        self
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
    /// 权限动作名(默认=工具名);拦截器以 (action, resource) 查 Ruleset。
    fn action(&self) -> &'static str {
        self.name()
    }
    /// 从输入提取受权限约束的资源(路径/命令等)。默认 "*" = 只按 action 粒度管。
    fn resources(&self, _input: &Value) -> Vec<String> {
        vec!["*".into()]
    }
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
