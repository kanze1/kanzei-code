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

    pub fn worktree_concurrency_key(&self) -> String {
        format!(
            "worktree:{}",
            self.project_root
                .display()
                .to_string()
                .replace('\\', "/")
                .to_lowercase()
        )
    }
}

/// 工具调用的并发契约，独立于权限资源。默认 Exclusive：旧工具未显式审计前
/// 绝不自动并行；Shared/WorktreeWrite 只有 key 相同且至少一方写时才冲突。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolConcurrency {
    Shared(String),
    WorktreeWrite(String),
    Exclusive,
}

impl ToolConcurrency {
    pub fn shared_worktree(ctx: &ToolCtx) -> Self {
        Self::Shared(ctx.worktree_concurrency_key())
    }

    pub fn write_worktree(ctx: &ToolCtx) -> Self {
        Self::WorktreeWrite(ctx.worktree_concurrency_key())
    }

    pub fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Exclusive, _) | (_, Self::Exclusive) => true,
            (Self::Shared(_), Self::Shared(_)) => false,
            (Self::Shared(left), Self::WorktreeWrite(right))
            | (Self::WorktreeWrite(left), Self::Shared(right))
            | (Self::WorktreeWrite(left), Self::WorktreeWrite(right)) => left == right,
        }
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
    /// 需要把执行上下文纳入权限资源时覆写；默认保持旧工具契约。
    fn resources_with_ctx(&self, input: &Value, _ctx: &ToolCtx) -> Vec<String> {
        self.resources(input)
    }
    /// 批内执行冲突声明。权限资源不能兼任锁键；未审计工具默认全局独占。
    fn concurrency(&self, _input: &Value, _ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::Exclusive
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

#[cfg(test)]
mod tests {
    use super::ToolConcurrency;

    #[test]
    fn concurrency_conflicts_are_keyed_and_conservative() {
        let read_a = ToolConcurrency::Shared("worktree:a".into());
        let read_b = ToolConcurrency::Shared("worktree:b".into());
        let write_a = ToolConcurrency::WorktreeWrite("worktree:a".into());
        let write_b = ToolConcurrency::WorktreeWrite("worktree:b".into());
        assert!(!read_a.conflicts_with(&read_a));
        assert!(!read_a.conflicts_with(&read_b));
        assert!(read_a.conflicts_with(&write_a));
        assert!(!read_a.conflicts_with(&write_b));
        assert!(write_a.conflicts_with(&write_a));
        assert!(!write_a.conflicts_with(&write_b));
        assert!(ToolConcurrency::Exclusive.conflicts_with(&read_a));
        assert!(write_a.conflicts_with(&ToolConcurrency::Exclusive));
    }
}
