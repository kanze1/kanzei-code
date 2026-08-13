//! 子代理域(R-155 B7):SubagentRuntime 运行时、task 工具 schema(task_spec)与
//! run_subagent(独立只读快照 + 空历史,ask 一律 Deny,run_once 递归经 dyn Box 断开)。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kanzei_harness::{AgentDef, HarnessSnapshot, ToolCtx};
use kanzei_llm::{LlmClient, Message, ReasoningEffort, Route, ToolSpec, Usage};
use tokio_util::sync::CancellationToken;

use super::event::{AskFuture, AskReply, AskRequest, AskResponse, RunEvent, TaskTrace};
use super::{run_once, AskPolicy, RunnerConfig};

/// R-174:运行中**可单条取消**的子代理注册表。id = 模型 task 调用 id 或编排角色名。
/// `cancel` 命中后 token 即触发,drive/phase_pipeline 的 select 分支以「被停」终态收尾,
/// 读槽由 run_subagent future drop 时 RAII 释放。None(测试/CLI 单运行)不支持中途单条停止。
#[derive(Default)]
pub struct TaskCancellations {
    inner: Mutex<HashMap<String, CancellationToken>>,
}

/// 取消注册表的 RAII 注册句柄。
///
/// `run_subagent` 外层有墙钟 timeout；future 被 timeout 丢弃时，await 之后的
/// 手工 unregister 永远不会执行，因此注册必须由这个句柄的 Drop 负责回收。
pub struct TaskCancellationGuard {
    registry: Arc<TaskCancellations>,
    id: String,
    token: CancellationToken,
}

impl TaskCancellations {
    pub fn register(self: &Arc<Self>, id: &str) -> TaskCancellationGuard {
        let token = CancellationToken::new();
        self.inner
            .lock()
            .unwrap()
            .insert(id.to_string(), token.clone());
        TaskCancellationGuard {
            registry: Arc::clone(self),
            id: id.to_string(),
            token,
        }
    }
    /// 取消一个子代理;返回该 id 当时是否在运行(不在 = 已结束/不存在)。
    pub fn cancel(&self, id: &str) -> bool {
        let token = self.inner.lock().unwrap().remove(id);
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }
    /// 子代理自然结束后清理注册(token 已失效或从未被 cancel);幂等。
    pub fn unregister(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
}

impl TaskCancellationGuard {
    fn token(&self) -> &CancellationToken {
        &self.token
    }
}

impl Drop for TaskCancellationGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.id);
    }
}

/// R-175 B2:后台子代理生命周期事件落库的回调类型。`Arc<dyn Fn>` 可 Clone,
/// 不破坏 SubagentRuntime 的 derive Clone;app 层(run.rs)把 record_live_trace_at_path
/// 包进闭包传入,drive.rs background 分支的 spawn 块在完成/失败/超时时调
/// sink(call_id, payload)写 session_events——绕开 SessionStore(rusqlite
/// Connection 非 Send)直接进 spawn 的限制。type 别名:避免 clippy type_complexity。
pub type BackgroundEventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// R-175 B3:子代理 transcript 暂存类型(进程内,按 id → 消息历史)。
/// type 别名:避免 clippy type_complexity。
pub type TranscriptStore =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<kanzei_llm::Message>>>>;

/// R-175 B4:后台子代理完成/失败/超时发通知回主对话的回调类型(call_id, status)。
/// type 别名:避免 clippy type_complexity。
pub type BackgroundNotificationSink = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// R-175 B5:重启可发现——从 session_events 回放,找出「上次未终结」的后台子代理。
///
/// 后台子代理的 task.lifecycle 事件(running / done / failed)经 background_events
/// sink 落 session_events。进程强杀后重开,内存注册表已丢,但事件轨迹还在:按 id
/// 聚合全部 task.lifecycle,若最后一个 state 是 `running`(无后续 done/failed),
/// 说明该子代理在进程死亡时仍在跑——给出确定处置(标失败,不放幽灵条目)。
///
/// 返回未终结子代理的 id 列表(去重、按首见顺序)。
pub fn pending_background_subagents(events: &[crate::store::StoredEvent]) -> Vec<String> {
    let mut latest: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for event in events {
        if event.event_type != "run.trace" {
            continue;
        }
        let Some(items) = event.payload.get("events").and_then(|e| e.as_array()) else {
            continue;
        };
        for item in items {
            if item.get("kind").and_then(|k| k.as_str()) != Some("task.lifecycle") {
                continue;
            }
            let Some(id) = item.get("id").and_then(|i| i.as_str()) else {
                continue;
            };
            let Some(state) = item.get("state").and_then(|s| s.as_str()) else {
                continue;
            };
            if !latest.contains_key(id) {
                order.push(id.to_string());
            }
            latest.insert(id.to_string(), state.to_string());
        }
    }
    order
        .into_iter()
        .filter(|id| latest.get(id).map(|s| s.as_str()) == Some("running"))
        .collect()
}

/// R-176 B4:写子代理改动台账——owner(子代理 id)→ 改动文件 + 首次记录时的原内容。
///
/// 采集点:run_subagent 在 writable=true 时拦截写工具(edit/write)的调用,把
/// 目标路径与**首次**记录时的文件内容存进来(后续对同一文件的再次写入不覆盖
/// 原内容快照——回滚要恢复到"这个子代理第一次碰它之前"的样子)。
///
/// 回滚语义(验收⑤):按 owner 恢复它改过的文件为原内容,只碰该 owner 的文件,
/// 不误伤其它写子代理与主代理的改动。文件原内容快照在内存,进程退出即丢
/// (跨 run 的归因/回滚不在本条——那是 git 的工作树层能力)。
#[derive(Default)]
pub struct SubagentChangeLog {
    inner: std::sync::Mutex<
        std::collections::HashMap<String, std::collections::BTreeMap<String, Vec<u8>>>,
    >,
}

impl SubagentChangeLog {
    /// 记录一次写工具调用:owner 首次触碰某文件时快照其当前内容。
    /// `project_root` 用于把相对路径解析成绝对路径;`path` 来自工具入参。
    pub fn record(&self, owner: &str, project_root: &std::path::Path, path: &str) {
        let mut inner = self.inner.lock().unwrap();
        let owner_map = inner.entry(owner.to_string()).or_default();
        if owner_map.contains_key(path) {
            return; // 已快照过,保持首次内容
        }
        let full = project_root.join(path.trim_start_matches(['/', '\\']));
        let content = std::fs::read(&full).unwrap_or_default();
        owner_map.insert(path.to_string(), content);
    }

    /// 该 owner 改过的文件(按首次记录顺序)。空 = 该子代理没碰过写工具。
    pub fn files_of(&self, owner: &str) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .get(owner)
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// 按 owner 回滚:把它改过的每个文件恢复为首次记录时的内容(写回快照)。
    /// 返回恢复的文件数。只碰该 owner 的文件——其它 owner 与主代理的改动
    /// 原样保留(验收⑤:单独回滚不误伤)。
    pub fn rollback(&self, owner: &str, project_root: &std::path::Path) -> usize {
        let files: Vec<(String, Vec<u8>)> = {
            let inner = self.inner.lock().unwrap();
            inner
                .get(owner)
                .map(|map| map.iter().map(|(p, c)| (p.clone(), c.clone())).collect())
                .unwrap_or_default()
        };
        let mut restored = 0usize;
        for (path, content) in &files {
            let full = project_root.join(path.trim_start_matches(['/', '\\']));
            if let Some(parent) = full.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&full, content).is_ok() {
                restored += 1;
            }
        }
        restored
    }
}

/// task 子代理运行时(R-004/R-012)。快照由调用方用 SubagentBase 组件构建,
/// 代码层面只含只读工具——子代理无人应答权限询问,必须做到零 ask。
#[derive(Clone)]
pub struct SubagentRuntime {
    pub snapshot: Arc<HarnessSnapshot>,
    pub agent: AgentDef,
    /// (route, model id):fast = 本地小模型跑机械检索。
    pub fast: (Route, String),
    /// primary = 主模型,给需要理解代码的任务。
    pub primary: (Route, String),
    /// 两条路由各自的服务档位(Codex Fast mode)。fast 与 primary 未必是同一供应商,
    /// 所以不能共用一个值——用哪条路由就带哪条的档位。
    pub fast_service_tier: Option<String>,
    pub primary_service_tier: Option<String>,
    pub max_tokens: u32,
    /// 单个子代理的墙钟上限(秒):本地模型多轮可能极慢,必须有界。
    pub timeout_secs: u64,
    /// 可调上限,随主运行链一起传下来。
    pub limits: kanzei_harness::config::Limits,
    /// R-171 批6:项目级协调器(可选)。Some 时子代理执行前申请读槽登记
    /// 「并行查」身份,结束 RAII 释放;None(纯 CLI 单运行/测试)不登记。
    /// R-176 B2:writable=true 时改为申请 **write_scope 写租约**(不继承主代理
    /// 租约,write_scope = 子代理代码树),同一棵树上的两个 writer 仍排队。
    pub coordinator:
        Option<std::sync::Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>>,
    /// R-176 B2:可写档位。true = 快照含写工具(由调用方用 WritableSubagentBase
    /// 构建),执行前必须自己 acquire_writer_lease,不得继承主代理租约;false =
    /// 只读勘察/复核(现状,读槽登记)。装配点必须与快照配套:可写快照 ⇔ writable=true。
    pub writable: bool,
    /// R-176 B3:写子代理的权限询问路由(可选)。writable=true 时把子代理内的
    /// 权限询问转发到这里(桌面端 = kz:ask 事件 + oneshot 回传,与主对话同通道);
    /// 询问发生在 acquire_writer_lease **之前**(验收③:用户拒绝后不得占用写租约)。
    /// None(只读子代理/CLI 无 UI)维持 ask 恒 Deny(现状,无人应答)。
    pub ask_router: Option<Arc<dyn Fn(AskRequest) -> AskFuture + Send + Sync>>,
    /// R-176 B4:写子代理改动台账(可选)。writable=true 且 Some 时,run_subagent
    /// 拦截写工具(edit/write)的调用,记录 owner(子代理 id)→ 改动文件 + 首次
    /// 记录时的原内容;回滚按 owner 恢复这些文件,不误伤其它(验收④⑤)。
    /// None(只读子代理/CLI 无 UI)不采集。
    pub change_log: Option<Arc<SubagentChangeLog>>,
    /// R-174:单条停止注册表(可选)。Some 时 drive/phase_pipeline 在子代理 future
    /// 上挂取消 token,`stop_task` 命令按 id 命中即取消;None(测试/CLI 单运行)不挂。
    pub cancellations: Option<Arc<TaskCancellations>>,
    /// R-175:后台模式。false(默认)= 轮内一次性调用:drive.rs 派发后等齐全部
    /// task 才继续;true = 后台化:派发即返回句柄,主代理本轮继续做别的,
    /// 子代理跨轮存活(注册表 + 持久化,见 R-175 内容①②)。
    pub background: bool,
    /// R-175 B1b:后台子代理完成结果的暂存(进程内)。background=true 时 spawn 的
    /// 子代理跑完把 ToolOutput 写进这里,主代理后续轮次按 id 查询;跨会话持久化
    /// 与通知回传在 B2/B3 承接。None = 非后台模式不使用。
    pub background_results: Option<
        std::sync::Arc<
            std::sync::Mutex<std::collections::HashMap<String, kanzei_harness::ToolOutput>>,
        >,
    >,
    /// R-175 B2:后台子代理生命周期事件落库的回调(跨轮注册表 / 事件可回放)。
    /// `Arc<dyn Fn>` 可 Clone,不破坏上面的 derive Clone;app 层(run.rs)把
    /// record_live_trace_at_path 包进闭包传入,drive.rs background 分支的 spawn
    /// 块在完成/失败/超时时调 sink(call_id, payload)写 session_events——
    /// 绕开 SessionStore(rusqlite Connection 非 Send)直接进 spawn 的限制。
    /// None = CLI/测试不落库。
    pub background_events: Option<BackgroundEventSink>,
    /// R-175 B3:子代理 transcript 持久化(进程内,按 id)。run_subagent 完成后把
    /// 完整消息历史存进这里,续跑入口按 id 恢复 prior(验收④:续跑请求里可见此前
    /// transcript,不是从空历史重开——对照现状 subagent.rs 的 `&[]`)。
    /// 跨会话持久化由 session_events 的 task.lifecycle + B5 注册表承接。
    /// None = 非后台模式不启用续跑。
    pub transcripts: Option<TranscriptStore>,
    /// R-175 B4:后台子代理完成/失败/超时**发通知回主对话**的回调(验收⑦:复用既有
    /// `agent_notifications` 表,不新造通道)。与 background_events 同理,SessionStore
    /// 非 Send 不能直接进 spawn——app 层(run.rs)把 append_notification_atomic 包进
    /// 闭包传入;sink(call_id, status)在 drive.rs spawn 块三终态(完成/失败/超时)
    /// 时调用,status ∈ done|failed|timeout。None = CLI/测试不通知。
    pub background_notifications: Option<BackgroundNotificationSink>,
}

pub(crate) fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Delegate a narrow read-only exploration task (find files, call \
                      sites, usages; read and summarize code) to a subagent with ONLY \
                      read/glob/grep. It cannot write/edit files, run bash, inspect or \
                      change git state, merge, or publish/release; those authority-bearing \
                      actions belong to the primary agent. Params: prompt (self-contained \
                      instruction saying exactly what to find and what to report back); \
                      optional model: \"fast\" (default, local model, mechanical searches) | \
                      \"primary\" (tasks needing code comprehension). Multiple task calls \
                      in one turn run in parallel."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task: what to find and exactly what to report back"
                },
                "model": {
                    "type": "string",
                    "enum": ["fast", "primary"],
                    "description": "fast = local small model (default); primary = main model"
                }
            },
            "required": ["prompt"]
        }),
    }
}

/// R-176 B2:写子代理自持 write_scope 写租约——不继承主代理租约、不绕过协调器
/// (验收②);非写子代理维持只读读槽(现状,验收⑦不受影响)。
///
/// 两者都是 RAII:drop 时回调协调器释放,任何收尾路径不留死锁。用枚举统一
/// 持有类型,避免 if/else 分支类型不同(WriterLease vs ReadPermit)。
/// 字段只需持有到函数结束(释放即生效),生产路径不读——dead_code 是 RAII 常态。
#[allow(dead_code)]
pub(crate) enum SubagentPermit {
    Writer(kanzei_harness::orchestration::WriterLease),
    Reader(kanzei_harness::orchestration::ReadPermit),
}

/// R-176 B2:档位 → 许可类型的映射(纯函数,测试可直接断言)。
/// writable=true → Writer(写租约);false → Reader(读槽)。
#[cfg(test)]
pub(crate) fn permit_kind(writable: bool) -> PermitKind {
    if writable {
        PermitKind::Writer
    } else {
        PermitKind::Reader
    }
}

/// R-176 验收③:权限询问结果 → 是否授予写租约。纯函数,测试断言「拒绝后不得
/// 占用租约」——只有 AllowOnce/AlwaysAllow 才放行取租约,Deny/Cancelled 一律拒绝。
pub(crate) fn writable_granted(decision: &AskResponse) -> bool {
    matches!(
        decision,
        AskResponse::Permission(AskReply::AllowOnce | AskReply::AlwaysAllow)
    )
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermitKind {
    Writer,
    Reader,
}

pub(crate) async fn acquire_subagent_permit(
    rt: &SubagentRuntime,
    ctx: &ToolCtx,
    parent_call_id: &str,
) -> Option<SubagentPermit> {
    let coord = rt.coordinator.as_ref()?;
    if rt.writable {
        coord
            .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
                // write_scope = 子代理的代码树(与主对话本轮一致:线 = worktree)。
                // 同树两个 writer 排队,跨树并行(R-182 口径)。
                write_scope: ctx.cwd.clone(),
                run_id: parent_call_id.to_string(),
                process_id: rt.agent.name.clone(),
                reason: format!("writable subagent `{}`", rt.agent.name),
            })
            .await
            .ok()
            .map(SubagentPermit::Writer)
    } else {
        // R-171 批6:只读子代理申请读槽登记并行身份,结束自动释放。
        // 读槽只登记不阻塞(wave 并行),与 writer 租约是两套互不干扰的机制。
        coord
            .acquire_read_slot(kanzei_harness::orchestration::ReadSlotRequest {
                project_root: ctx.project_root.clone(),
                run_id: parent_call_id.to_string(),
                process_id: rt.agent.name.clone(),
                agent_name: rt.agent.name.clone(),
            })
            .await
            .ok()
            .map(SubagentPermit::Reader)
    }
}

/// R-176 B4:从 ToolStart 事件提取写工具的 path 入参(edit/write 共用 `path` 字段)。
fn event_input_path(event: &RunEvent) -> Option<String> {
    match event {
        RunEvent::ToolStart { input, .. } => input
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// 跑一个子代理:独立的只读快照 + 空历史,结果文本即 tool result。
/// 子代理内 ask 一律 Deny(无人应答);run_once 递归经 dyn Box 断开无限类型。
/// 内部轮次/工具事件折叠成 TaskProgress 经 progress 通道上抛(UI 实时可见)。
pub(crate) async fn run_subagent(
    client: &LlmClient,
    rt: &SubagentRuntime,
    ctx: &ToolCtx,
    parent_call_id: &str,
    input: &serde_json::Value,
    progress: tokio::sync::mpsc::UnboundedSender<RunEvent>,
) -> kanzei_harness::ToolOutput {
    let prompt = ["prompt", "task", "instruction", "query"]
        .iter()
        .find_map(|k| input.get(k).and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();
    if prompt.is_empty() {
        return kanzei_harness::ToolOutput::error(
            "task requires a `prompt` string: a self-contained exploration instruction",
        );
    }
    let (route, model, service_tier) = match input.get("model").and_then(|v| v.as_str()) {
        Some("primary") => (&rt.primary.0, &rt.primary.1, &rt.primary_service_tier),
        _ => (&rt.fast.0, &rt.fast.1, &rt.fast_service_tier),
    };
    let config = RunnerConfig {
        model: model.clone(),
        max_tokens: rt.max_tokens,
        // 子代理是机械检索,不开思考:省钱且避免本地小模型不认该参数。
        reasoning: ReasoningEffort::Off,
        service_tier: service_tier.clone(),
        limits: rt.limits.clone(),
        // 子代理跑的是 fast 模型,窗口未必与主模型同源;这里不传上限,
        // 让它继续走撞墙后的被动恢复,不按主模型的预算误压。
        context_limit: None,
        // R-162:子代理是机械检索,不需要事件触发召回(记忆命中只面向主代理
        // 的失败瞬间决策;子代理注入会让同一失败双倍刷屏)。
        recall: None,
        // R-171:子代理是只读勘察/复核,不参与写仲裁,用默认执行策略。
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
        ask_policy: AskPolicy::NonInteractive,
    };
    let mut total_usage = Usage::default();
    // R-176 B4:写子代理改动台账——拦截写工具(edit/write)调用,记录 owner →
    // 改动文件 + 首次原内容。闭包捕获 Arc 克隆(非 'static,生命周期不逃逸)。
    let change_log = rt.change_log.clone();
    let change_root = ctx.project_root.clone();
    let change_owner = parent_call_id.to_string();
    let mut on_event = |event: RunEvent| {
        let text = match &event {
            RunEvent::TurnStart { step, max_steps } => Some(if *max_steps > 0 {
                format!("第 {step}/{max_steps} 轮")
            } else {
                format!("第 {step} 轮")
            }),
            RunEvent::ToolStart { name, summary, .. } => {
                let head: String = summary.chars().take(80).collect();
                Some(format!("{name} {head}"))
            }
            _ => None,
        };
        // R-176 B4:写子代理改动归因——写工具(edit/write)的 path 入参登记进台账,
        // 首次触碰时快照原内容。只有 writable 子代理带 change_log 才采集。
        if let Some(log) = change_log.as_ref() {
            if matches!(event, RunEvent::ToolStart { ref name, .. } if name == "edit" || name == "write")
            {
                if let Some(path) = event_input_path(&event) {
                    log.record(&change_owner, &change_root, &path);
                }
            }
        }
        let trace = match event {
            RunEvent::ToolStart {
                id,
                name,
                summary,
                input,
            } => Some(TaskTrace {
                child_id: id,
                phase: "start".into(),
                name,
                summary: Some(summary),
                ok: None,
                preview: None,
                display: None,
                // R-174:完整入参原文进 trace,面板/transcript 可展开复核「到底拿什么调的」。
                input: Some(input),
                usage: None,
            }),
            RunEvent::ToolEnd {
                id,
                name,
                ok,
                preview,
                display,
            } => Some(TaskTrace {
                child_id: id,
                phase: "end".into(),
                name,
                summary: None,
                ok: Some(ok),
                preview: Some(preview),
                display,
                input: None,
                usage: None,
            }),
            // R-174:子代理每轮 StepEnd 累计 token,以 phase="usage" 的 trace 上抛,
            // 前端据此刷新「累计 token」字段(transcript/面板共用同一数据源)。
            RunEvent::StepEnd { usage, .. } => {
                total_usage.input = total_usage.input.saturating_add(usage.input);
                total_usage.output = total_usage.output.saturating_add(usage.output);
                total_usage.reasoning = total_usage.reasoning.saturating_add(usage.reasoning);
                total_usage.cache_read = total_usage.cache_read.saturating_add(usage.cache_read);
                total_usage.cache_write = total_usage.cache_write.saturating_add(usage.cache_write);
                Some(TaskTrace {
                    child_id: parent_call_id.to_string(),
                    phase: "usage".into(),
                    name: String::new(),
                    summary: None,
                    ok: None,
                    preview: None,
                    display: None,
                    input: None,
                    usage: Some(total_usage),
                })
            }
            _ => None,
        };
        if let Some(text) = text {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text,
                trace: trace.clone(),
            });
        } else if trace.is_some() {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text: "子代理工具完成".into(),
                trace,
            });
        }
    };
    // R-176 B2/B3:写子代理自持 write_scope 写租约——不继承主代理租约、不绕过
    // 协调器(验收②)。**权限询问先于取租约**(验收③):先问用户"写子代理要获得
    // 写权,允许吗?",Allow 才 acquire_writer_lease;Deny 直接返回、协调器无
    // writer 占用。只读子代理维持读槽(现状,验收⑦不受影响)。
    // 两者都是 RAII:drop 时回调协调器释放,任何收尾路径不留死锁。
    let _lease = if rt.writable {
        // 询问先于租约:拒绝后不得占用写租约(设计不变量 6,验收③)。
        let decision = match rt.ask_router.as_ref() {
            Some(router) => {
                router(AskRequest::Permission {
                    action: "subagent-write".into(),
                    resource: ctx.cwd.display().to_string(),
                })
                .await
            }
            // 没有询问通道(CLI 无 UI):默认 Deny——宁可拒绝也不让看不见的
            // 子代理拿写权(风险条款:用户看不见的进程在改仓库)。
            None => AskResponse::Permission(AskReply::Deny),
        };
        let allowed = writable_granted(&decision);
        if !allowed {
            return kanzei_harness::ToolOutput::error(format!(
                "writable subagent `{}` was denied write permission by the user; \
                 no writer lease was acquired",
                rt.agent.name
            ));
        }
        acquire_subagent_permit(rt, ctx, parent_call_id).await
    } else {
        acquire_subagent_permit(rt, ctx, parent_call_id).await
    };
    // R-176 B3:写子代理的询问通道——有 ask_router 时把子代理内权限询问转发给
    // 用户(与主对话同 UI 通道);没有(只读子代理/CLI)维持恒 Deny(无人应答)。
    let mut ask = {
        let router = rt.ask_router.clone();
        move |request: AskRequest| -> AskFuture {
            match &router {
                Some(router) => router(request),
                None => Box::pin(async { AskResponse::Permission(AskReply::Deny) }),
            }
        }
    };
    // R-175 B3:续跑恢复——同一 id 再次调用 run_subagent 时,从 transcripts 恢复
    // 此前完整历史作为 prior(验收④:续跑请求里可见此前 transcript,不是从空历史
    // 重开)。首次派发(无历史)行为不变,仍从空 prior 开始。
    let prior: Vec<Message> = rt
        .transcripts
        .as_ref()
        .and_then(|store| store.lock().unwrap().get(parent_call_id).cloned())
        .unwrap_or_default();
    // run_once 本身返回 boxed future,递归的无限类型在其签名处已断开。
    let fut = run_once(
        client,
        route,
        &rt.snapshot,
        &rt.agent,
        &config,
        ctx,
        &prompt,
        None,
        &prior,
        None,
        &mut on_event,
        &mut ask,
    );
    // R-174:单条停止——注册表 Some 时注册本子代理的取消 token,`stop_task` 命中后
    // 立即触发 select 分支,以「被停」终态返回;读槽 `_read_permit` 随函数返回由 RAII
    // 释放。drive.rs 的 timeout 仍在外层墙钟兜底,二者正交(取消先到先得)。
    let cancellation = rt
        .cancellations
        .as_ref()
        .map(|reg| reg.register(parent_call_id));
    let mut fut = Box::pin(fut);
    let output = match &cancellation {
        Some(guard) => tokio::select! {
            biased;
            _ = guard.token().cancelled() => {
                let _ = progress.send(RunEvent::TaskProgress {
                    id: parent_call_id.to_string(),
                    text: "子代理已被停止".into(),
                    trace: Some(TaskTrace {
                        child_id: parent_call_id.to_string(),
                        phase: "cancelled".into(),
                        name: String::new(),
                        summary: None,
                        ok: None,
                        preview: None,
                        display: None,
                        input: None,
                        usage: None,
                    }),
                });
                kanzei_harness::ToolOutput::error(format!(
                    "subagent {parent_call_id} was stopped by the user"
                ))
            }
            result = &mut fut => match result {
                Ok(summary) => {
                    // R-175 B3:transcript 持久化——完成时把完整消息历史按 id 存进
                    // transcripts,续跑入口据此恢复 prior(验收④)。
                    if let Some(store) = rt.transcripts.as_ref() {
                        store
                            .lock()
                            .unwrap()
                            .insert(parent_call_id.to_string(), summary.messages.clone());
                    }
                    let text = if summary.text.trim().is_empty() {
                        "(subagent finished without a text answer)".to_string()
                    } else {
                        summary.text
                    };
                    kanzei_harness::ToolOutput::ok(text)
                }
                Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
            },
        },
        None => match fut.await {
            Ok(summary) => {
                // R-175 B3:同上,无取消注册表时也要存 transcript。
                if let Some(store) = rt.transcripts.as_ref() {
                    store
                        .lock()
                        .unwrap()
                        .insert(parent_call_id.to_string(), summary.messages.clone());
                }
                let text = if summary.text.trim().is_empty() {
                    "(subagent finished without a text answer)".to_string()
                } else {
                    summary.text
                };
                kanzei_harness::ToolOutput::ok(text)
            }
            Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
        },
    };
    // `cancellation` 的 Drop 负责正常、失败、取消和外层 timeout 的统一清理。
    output
}

/// R-173:**编排对象直接派发**的只读勘察/复核代理。
///
/// 与 `task` 工具那条路的区别只在**谁决定派谁**:task 由模型在轮内自行派发,
/// 本函数由阶段编排对象按固定角色表派发(设计文档「推荐勘察角色」)。二者走的是
/// 同一个 [`run_subagent`],所以只读白名单、`ask` 恒 Deny、读槽登记与 RAII 回收
/// **完全一致**——不存在"编排派的子代理走了另一条没人管的路"。
///
/// `agent_id` 同时用作读槽身份键(并行角色靠它区分,见 `ReadPermit::run_id`)。
pub async fn run_read_agent(
    client: &LlmClient,
    rt: &SubagentRuntime,
    ctx: &ToolCtx,
    agent_id: &str,
    prompt: &str,
    progress: tokio::sync::mpsc::UnboundedSender<RunEvent>,
) -> kanzei_harness::ToolOutput {
    run_subagent(
        client,
        rt,
        ctx,
        agent_id,
        &serde_json::json!({ "prompt": prompt }),
        progress,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::TaskCancellations;
    use std::sync::Arc;

    #[test]
    fn cancellation_guard_drop_清理注册表且终态停止返回未运行() {
        let registry = Arc::new(TaskCancellations::default());
        let guard = registry.register("timed-out-task");
        drop(guard);
        assert!(
            !registry.cancel("timed-out-task"),
            "外层 timeout 丢弃 future 后，死 token 不得继续可取消"
        );
    }

    /// R-175 B5 验收③:重启可发现——从 session_events 回放,找「running 无终态」的子代理;
    /// 有 done/failed 终态的不得残留(不留幽灵条目)。
    #[test]
    fn pending_background_subagents_只列running无终态_终态不残留() {
        let trace = |id: &str, state: &str| crate::store::StoredEvent {
            event_id: format!("ev_{id}_{state}"),
            session_id: "ses".into(),
            sequence: 1,
            event_type: "run.trace".into(),
            payload: serde_json::json!({
                "run_id": "r",
                "events": [{
                    "kind": "task.lifecycle",
                    "id": id,
                    "state": state,
                }],
                "partial": true,
            }),
            created_at: 1,
        };
        // 子代理 A:running 无终态(强杀时仍在跑)→ 必须列出。
        // 子代理 B:running → done(正常完成)→ 不得列出。
        // 子代理 C:running → failed(失败终态)→ 不得列出。
        let events = vec![
            trace("A", "running"),
            trace("B", "running"),
            trace("B", "done"),
            trace("C", "running"),
            trace("C", "failed"),
        ];
        let pending = super::pending_background_subagents(&events);
        assert_eq!(
            pending,
            vec!["A".to_string()],
            "只应列出 running 无终态的 A: {pending:?}"
        );
    }

    /// R-176 验收②:档位 → 许可类型映射——writable=true 走 writer lease、
    /// false 走 read slot。纯函数断言,不依赖完整 SubagentRuntime 构造。
    #[test]
    fn writable_maps_to_writer_permit_reader_maps_to_read_slot() {
        use super::{permit_kind, PermitKind};
        assert_eq!(permit_kind(true), PermitKind::Writer);
        assert_eq!(permit_kind(false), PermitKind::Reader);
    }

    /// R-176 验收③:权限询问先于取租约——拒绝/取消后**不得**授予写租约
    /// (设计不变量 6:用户拒绝后不得占用写租约);只有 AllowOnce/AlwaysAllow
    /// 才放行。纯函数断言顺序语义:询问结果 → 是否取租约。
    #[test]
    fn writable_granted_rejects_deny_and_cancel_allows_only_allow() {
        use super::{writable_granted, AskReply, AskResponse};
        assert!(
            !writable_granted(&AskResponse::Permission(AskReply::Deny)),
            "Deny 后不得取租约"
        );
        assert!(
            !writable_granted(&AskResponse::Cancelled),
            "取消后不得取租约"
        );
        assert!(
            writable_granted(&AskResponse::Permission(AskReply::AllowOnce)),
            "AllowOnce 可取租约"
        );
        assert!(
            writable_granted(&AskResponse::Permission(AskReply::AlwaysAllow)),
            "AlwaysAllow 可取租约"
        );
    }

    /// R-176 验收④:写子代理的改动可按 owner 归因——台账记录后,任一文件
    /// 能查到是哪个子代理 id 改的(owner → 文件清单)。
    #[test]
    fn change_log_attributes_files_to_owner() {
        use super::SubagentChangeLog;
        let log = SubagentChangeLog::default();
        let root = std::env::temp_dir().join(format!(
            "kz-change-attr-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "ORIGINAL-A").unwrap();
        std::fs::write(root.join("src/b.rs"), "ORIGINAL-B").unwrap();

        // 子代理 sub-A 改 a.rs;子代理 sub-B 改 b.rs。
        log.record("sub-A", &root, "src/a.rs");
        log.record("sub-B", &root, "src/b.rs");
        // sub-A 再改 a.rs(第二次不覆盖首次快照)。
        log.record("sub-A", &root, "src/a.rs");

        assert_eq!(
            log.files_of("sub-A"),
            vec!["src/a.rs".to_string()],
            "sub-A 只改过 a.rs"
        );
        assert_eq!(
            log.files_of("sub-B"),
            vec!["src/b.rs".to_string()],
            "sub-B 只改过 b.rs"
        );
        assert!(log.files_of("sub-none").is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-176 验收⑤:单个写子代理的改动可**单独回滚**——只恢复该 owner 改过的
    /// 文件,其它 owner 与主代理的改动原样保留(不误伤)。
    #[test]
    fn change_log_rollback_restores_only_owner_files() {
        use super::SubagentChangeLog;
        let log = SubagentChangeLog::default();
        let root = std::env::temp_dir().join(format!(
            "kz-change-rollback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "ORIGINAL-A").unwrap();
        std::fs::write(root.join("src/b.rs"), "ORIGINAL-B").unwrap();

        // 两个写子代理各自改自己的文件。
        log.record("sub-A", &root, "src/a.rs");
        log.record("sub-B", &root, "src/b.rs");
        // 子代理改完(写工具落盘)之后,主代理又改了一个无关文件。
        std::fs::write(root.join("src/a.rs"), "CHANGED-BY-A").unwrap();
        std::fs::write(root.join("src/b.rs"), "CHANGED-BY-B").unwrap();
        std::fs::write(root.join("src/main.rs"), "MAIN-OWN").unwrap();

        // 单独回滚 sub-A:只恢复 a.rs 为首次快照 ORIGINAL-A。
        assert_eq!(log.rollback("sub-A", &root), 1, "只恢复 A 的 1 个文件");
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "ORIGINAL-A",
            "A 的文件恢复到首次快照"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/b.rs")).unwrap(),
            "CHANGED-BY-B",
            "B 的改动不得被 A 的回滚误伤"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/main.rs")).unwrap(),
            "MAIN-OWN",
            "主代理的改动不得被 A 的回滚误伤"
        );

        // 回滚 sub-B:b.rs 也恢复,其它不受影响。
        assert_eq!(log.rollback("sub-B", &root), 1);
        assert_eq!(
            std::fs::read_to_string(root.join("src/b.rs")).unwrap(),
            "ORIGINAL-B"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
