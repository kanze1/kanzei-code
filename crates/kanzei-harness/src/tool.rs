use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具终态的机器可读分类。
///
/// `is_error` 仍保留给 provider 协议：需要模型修正/确认的调用必须继续以 error
/// 回喂，不能让模型误以为已经落盘。`outcome` 服务于 UI、轨迹、指标与记忆，避免
/// 把安全拒绝、no-op 和真实执行故障揉成同一种“失败”。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOutcome {
    Success,
    NoOp,
    NeedsCorrection,
    NeedsConfirmation,
    Failed,
}

impl ToolOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::NoOp => "noop",
            Self::NeedsCorrection => "needs_correction",
            Self::NeedsConfirmation => "needs_confirmation",
            Self::Failed => "failed",
        }
    }

    pub fn is_expected_rejection(self) -> bool {
        matches!(
            self,
            Self::NoOp | Self::NeedsCorrection | Self::NeedsConfirmation
        )
    }
}

/// 工具执行上下文。
#[derive(Debug, Clone)]
pub struct ToolCtx {
    pub cwd: std::path::PathBuf,
    /// 项目根(.kanzei/.git 所在);工作区文档(requirements/defects/sources)挂在这下面。
    pub project_root: std::path::PathBuf,
    /// R-171:实际代码工作树键(worktree 隔离时与 project_root 不同)。
    /// 单写语义下 worktree_key 只用于工具内冲突,project_write_key 才是跨进程仲裁键。
    pub worktree_key: Option<String>,
    /// R-171:规范化项目主根(写租约仲裁键)。默认与 project_root 一致;
    /// worktree 场景由调用方显式设置为规范化主根。
    pub project_write_key: Option<String>,
    /// R-171:租约归属与审计身份。
    pub run_id: Option<String>,
    pub process_id: Option<String>,
    /// 本轮唯一的取活队列优先级。`work` 工具只读此值，模型不能在调用参数里改模式。
    pub work_priority: crate::auto_run::WorkPriority,
}

impl Default for ToolCtx {
    fn default() -> Self {
        ToolCtx {
            cwd: std::path::PathBuf::new(),
            project_root: std::path::PathBuf::new(),
            worktree_key: None,
            project_write_key: None,
            run_id: None,
            process_id: None,
            work_priority: crate::auto_run::WorkPriority::DefectFirst,
        }
    }
}

impl ToolCtx {
    /// R-141:显式主根绑定——**不做任何根发现**。
    ///
    /// `cwd` 是实际代码工作树,`project_root` 是项目身份真源(`.kanzei` 托管文档、
    /// state.db、记忆)。worktree 线两者不同,调用方必须自己说清楚。
    pub fn new(cwd: std::path::PathBuf, project_root: std::path::PathBuf) -> Self {
        ToolCtx {
            cwd,
            project_root,
            worktree_key: None,
            project_write_key: None,
            run_id: None,
            process_id: None,
            work_priority: crate::auto_run::WorkPriority::DefectFirst,
        }
    }

    /// 从 cwd 发现式解析项目根。**只允许出现在进程/IPC 入口**——CLI 启动、
    /// Tauri command 的第一行、一次性查询。**线路径(runner/drive、子代理、
    /// 桌面端 run/processes/subagents)调用它是 bug**:根必须在入口解析一次,
    /// 之后全程显式携带。
    ///
    /// 为什么钉这么死(R-141 立的护栏,别拆):
    /// - D-170:发现式根解析让两个项目串了身份,项目级状态互相污染。
    /// - D-170 的 worktree 变体:`.kanzei/project/*.md` 被 git 跟踪,
    ///   `git worktree add` 会把它们 checkout 成**分支副本**;而 worktree 里的
    ///   `.git` 是文件不是目录。于是 `discover_project_root` 在 worktree 内
    ///   第一层就命中副本自己的 `.kanzei` 立即返回,线拿到一套过期的
    ///   需求/缺陷/记忆,tracker 写入落在分支副本上。
    ///   worktree 又建在主根的兄弟目录,向上走也回不到主根。
    pub fn discovering(cwd: std::path::PathBuf) -> Self {
        let project_root =
            crate::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
        ToolCtx::new(cwd, project_root)
    }

    pub fn with_identity(
        mut self,
        worktree_key: String,
        project_write_key: String,
        run_id: String,
        process_id: String,
    ) -> Self {
        self.worktree_key = Some(worktree_key);
        self.project_write_key = Some(project_write_key);
        self.run_id = Some(run_id);
        self.process_id = Some(process_id);
        self
    }

    pub fn with_work_priority(mut self, work_priority: crate::auto_run::WorkPriority) -> Self {
        self.work_priority = work_priority;
        self
    }

    /// 工具级并发锁键 = **代码树**,不是项目根。
    ///
    /// R-141:缺省回退到 `cwd` 而非 `project_root`。显式主根绑定后,同一项目的
    /// N 棵 worktree 的 `project_root` 完全相同,拿它当工具锁键会把互不相干的
    /// 两棵树串死。锁键的真源是工具真实作用的那棵树——bash 用
    /// `ctx.cwd.join(workdir)` 定执行目录,git 用 `ensure_repository(&ctx.cwd)`。
    /// 跨进程写仲裁是另一把键,见 [`ToolCtx::project_write_key`]。
    pub fn worktree_concurrency_key(&self) -> String {
        let key = self
            .worktree_key
            .clone()
            .unwrap_or_else(|| self.cwd.display().to_string());
        format!("worktree:{}", key.replace('\\', "/").to_lowercase())
    }

    /// 写仲裁键:缺省回退到 project_root(未显式绑定身份时与旧行为一致)。
    pub fn project_write_key(&self) -> String {
        self.project_write_key
            .clone()
            .unwrap_or_else(|| self.project_root.display().to_string())
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

/// 工具随结果回喂模型的图片(R-249)。与 `kanzei_llm::Part::Image` 同形:
/// `data` 是 base64,**不含** `data:` 前缀。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolImage {
    pub media_type: String,
    pub data: String,
}

/// Durable 原文外置后的引用。内容本身不再放进事件/模型消息，只保留可回读元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolArtifact {
    pub artifact_id: String,
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
    pub retrieval_hint: String,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    /// UI/轨迹/指标使用的结构化终态；与 provider 的 `is_error` 正交。
    pub outcome: ToolOutcome,
    /// 稳定错误码，供恢复策略和离线评估使用；人类文案可独立演进。
    pub code: Option<&'static str>,
    /// 面向 UI 的结构化展示(diff/终端块等),模型看不到,只给人看。
    /// 形如 {"kind":"diff","path":...,"diff":...} / {"kind":"terminal",...}。
    pub display: Option<serde_json::Value>,
    /// R-249:随工具结果一并回喂模型的图片。空 vec = 纯文本结果,与既有行为逐字节一致。
    /// 投递形态是「同一条 tool_results 消息里,ToolResult 之后追加 Part::Image」——
    /// 协议层已有通用 Image 映射(anthropic/openai/openai_responses),无需改协议。
    pub images: Vec<ToolImage>,
    /// D-349:大结果完整原文写入 durable artifact 后的可恢复引用。
    pub artifact: Option<ToolArtifact>,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        ToolOutput {
            content: content.into(),
            is_error: false,
            outcome: ToolOutcome::Success,
            code: None,
            display: None,
            images: Vec::new(),
            artifact: None,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        ToolOutput {
            content: content.into(),
            is_error: true,
            outcome: ToolOutcome::Failed,
            code: None,
            display: None,
            images: Vec::new(),
            artifact: None,
        }
    }

    pub fn noop(code: &'static str, content: impl Into<String>) -> Self {
        Self::rejected(ToolOutcome::NoOp, code, content)
    }

    pub fn needs_correction(code: &'static str, content: impl Into<String>) -> Self {
        Self::rejected(ToolOutcome::NeedsCorrection, code, content)
    }

    pub fn needs_confirmation(code: &'static str, content: impl Into<String>) -> Self {
        Self::rejected(ToolOutcome::NeedsConfirmation, code, content)
    }

    pub fn failed(code: &'static str, content: impl Into<String>) -> Self {
        ToolOutput {
            content: content.into(),
            is_error: true,
            outcome: ToolOutcome::Failed,
            code: Some(code),
            display: None,
            images: Vec::new(),
            artifact: None,
        }
    }

    fn rejected(outcome: ToolOutcome, code: &'static str, content: impl Into<String>) -> Self {
        debug_assert!(outcome.is_expected_rejection());
        ToolOutput {
            content: content.into(),
            // provider 仍须把它当成需要处理的结果，避免 no-op/保护门禁被当完成。
            is_error: true,
            outcome,
            code: Some(code),
            display: None,
            images: Vec::new(),
            artifact: None,
        }
    }

    /// 回喂模型的内容携带稳定机器头；UI 继续使用无头的 `content`。
    pub fn model_content(&self) -> String {
        if !self.outcome.is_expected_rejection() {
            return self.content.clone();
        }
        match self.code {
            Some(code) => format!(
                "[tool_outcome={} code={code}]\n{}",
                self.outcome.as_str(),
                self.content
            ),
            None => format!("[tool_outcome={}]\n{}", self.outcome.as_str(), self.content),
        }
    }

    pub fn with_display(mut self, display: serde_json::Value) -> Self {
        self.display = Some(display);
        self
    }

    /// R-249:挂上随结果回喂模型的图片。
    pub fn with_images(mut self, images: Vec<ToolImage>) -> Self {
        self.images = images;
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
    let mut message = format!("Invalid input for tool `{}`: {problem}", tool.name());
    if let Some(field) = missing_field_name(problem) {
        message.push_str(&format!(
            "\n缺少必填参数 `{field}`。\nExample (one line): {}",
            missing_field_example(tool.name(), field)
        ));
    }
    message.push_str(&format!(
        "\nYour raw input was: {}\nExpected JSON schema:\n{}\nRetry the tool call with corrected JSON.",
        truncate(raw_input, 500),
        tool.input_schema()
    ));
    ToolOutput::needs_correction("INVALID_TOOL_INPUT", message)
}

fn missing_field_name(problem: &str) -> Option<&str> {
    let marker = "missing field `";
    let start = problem.find(marker)? + marker.len();
    let rest = &problem[start..];
    let end = rest.find('`')?;
    Some(&rest[..end])
}

fn missing_field_example(tool_name: &str, field: &str) -> String {
    match tool_name {
        "read" | "symbols" => r#"{"path":"src/lib.rs"}"#.into(),
        "edit" => r#"{"path":"src/lib.rs","old_string":"old","new_string":"new"}"#.into(),
        "insert" => {
            r#"{"path":"src/lib.rs","anchor":"ANCHOR","content":"new\n","position":"after"}"#.into()
        }
        "memory_search" => r#"{"query":"search terms"}"#.into(),
        _ => format!(r#"{{"{field}":"..."}}"#),
    }
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolConcurrency, ToolCtx, ToolOutcome, ToolOutput};
    use std::path::{Path, PathBuf};

    /// 复刻 `git worktree add` 之后的真实磁盘形态:
    /// 主根与 worktree 是**兄弟目录**(见 kanzei-app/src/processes.rs 的
    /// `parent.join(".kanzei-worktree-{name}")`),且 worktree 里躺着一份被 git
    /// checkout 出来的 `.kanzei` 副本。worktree 的 `.git` 是文件不是目录,
    /// 所以发现式取根只会撞上那份副本。
    fn worktree_fixture(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "kz-toolctx-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(base.join("main").join(".kanzei").join("project")).unwrap();
        for tree in [".kanzei-worktree-a", ".kanzei-worktree-b"] {
            std::fs::create_dir_all(base.join(tree).join(".kanzei").join("project")).unwrap();
            // 真 worktree 的 .git 是文件(gitdir: 指针),不是目录。
            std::fs::write(
                base.join(tree).join(".git"),
                "gitdir: ../main/.git/worktrees/x",
            )
            .unwrap();
        }
        base
    }

    fn cleanup(base: &Path) {
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn structured_outcome_keeps_provider_error_and_machine_code_separate() {
        let output = ToolOutput::needs_correction("EDIT_ANCHOR_NOT_FOUND", "re-read target");
        assert!(output.is_error, "模型必须继续看到需要纠正的 tool error");
        assert_eq!(output.outcome, ToolOutcome::NeedsCorrection);
        assert_eq!(output.code, Some("EDIT_ANCHOR_NOT_FOUND"));
        assert_eq!(
            output.model_content(),
            "[tool_outcome=needs_correction code=EDIT_ANCHOR_NOT_FOUND]\nre-read target"
        );
    }

    #[test]
    fn worktree_内运行时_project_root_必须等于主根() {
        let base = worktree_fixture("root");
        let main = base.join("main");
        let worktree = base.join(".kanzei-worktree-a");

        // 危害前提:发现式取根在 worktree 内必然命中分支副本,拿不到主根。
        // 这条断言在,后来人改 discover_project_root 时就知道自己动了什么。
        assert_eq!(
            crate::config::discover_project_root(&worktree),
            Some(worktree.clone()),
            "worktree 内的 .kanzei 副本是目录、.git 是文件,discover 第一层就命中副本"
        );
        assert_ne!(
            crate::config::discover_project_root(&worktree),
            Some(main.clone())
        );

        // 显式绑定后,项目身份恒为主根,代码树仍是 worktree。
        let ctx = ToolCtx::new(worktree.clone(), main.clone());
        assert_eq!(ctx.project_root, main);
        assert_eq!(ctx.cwd, worktree);

        cleanup(&base);
    }

    #[test]
    fn 两个_worktree_实例锁键必须不同_写仲裁键必须相同() {
        let base = worktree_fixture("keys");
        let main = base.join("main");
        let main_key = main.display().to_string();
        let identity = |tree: PathBuf, run: &str| {
            let tree_key = tree.display().to_string();
            ToolCtx::new(tree, main.clone()).with_identity(
                tree_key,
                main_key.clone(),
                run.into(),
                "p1".into(),
            )
        };
        let a = identity(base.join(".kanzei-worktree-a"), "run_a");
        let b = identity(base.join(".kanzei-worktree-b"), "run_b");

        // 工具级锁键按代码树分开:两棵树的写工具不该互相排队。
        assert_ne!(a.worktree_concurrency_key(), b.worktree_concurrency_key());
        assert!(!ToolConcurrency::write_worktree(&a)
            .conflicts_with(&ToolConcurrency::write_worktree(&b)));
        // 跨进程写仲裁键仍是同一个主根:主根 .kanzei 的写入必须串行。
        assert_eq!(a.project_write_key(), b.project_write_key());
        assert_eq!(a.project_write_key(), main_key);

        cleanup(&base);
    }

    #[test]
    fn 未显式设身份时锁键回退到代码树而非项目根() {
        // 回退分支的护栏:回退到 project_root 会让同项目的两棵树拿到同一把键,
        // 互不相干的 worktree 立刻撞锁。别改回去。
        let main = PathBuf::from("/repo/main");
        let a = ToolCtx::new(PathBuf::from("/repo/.kanzei-worktree-a"), main.clone());
        let b = ToolCtx::new(PathBuf::from("/repo/.kanzei-worktree-b"), main.clone());
        assert_eq!(a.project_root, b.project_root);
        assert_ne!(a.worktree_concurrency_key(), b.worktree_concurrency_key());
        // 同一棵树的两个上下文仍然共键(写工具照常互斥)。
        let a2 = ToolCtx::new(PathBuf::from("/repo/.kanzei-worktree-a"), main);
        assert_eq!(a.worktree_concurrency_key(), a2.worktree_concurrency_key());
    }

    #[test]
    fn 同一棵树的锁键对大小写与分隔符稳定() {
        let main = PathBuf::from(r"C:\repo\main");
        let upper = ToolCtx::new(PathBuf::from(r"C:\repo\Wt"), main.clone());
        let lower = ToolCtx::new(PathBuf::from("c:/repo/wt"), main);
        assert_eq!(upper.worktree_concurrency_key(), "worktree:c:/repo/wt");
        assert_eq!(
            upper.worktree_concurrency_key(),
            lower.worktree_concurrency_key()
        );
    }

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
