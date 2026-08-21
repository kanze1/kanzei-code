//! 运行事件域(R-253 批5,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:事件归约是「把 RunEvent 投影到各消费者」的独立变更理由——`build_event_
//! handler` 对每个事件同时做 UI 投影/typed 持久化/trace 落库/指标累计/运行时状态,
//! `build_ask_handler` 把权限/提问请求经 kz:ask 发给前端并挂 pending asks 表。
//! 两者与装配/执行/落库正交:加一个事件类型或改投影方式,不必读懂整个运行主链路
//! (照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):⑦R-143 的两个 `AtomicBool` 有 swap 语义(`round_pending.swap(false)`
//! 是「取并清」,不是「读」)——拆 sink 时这对状态整体归 `MetricsSink`,不分开。
//! ⑧D-361 的 `subagent_tools` 是跨模块状态:这里边跑边收,run_task 轮末合并进
//! tools_vec 供鞭挞判定——`MetricsSink` 持有同一个 `Arc`,run_task 仍可读回。
//! ⑩`UiEventSink` 的 emit 闭包捕获 window + session_id,经 `Arc<dyn Fn>` 收纳。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use kanzei_core::{AskFuture, RunEvent};
use serde_json::json;
use tauri::Emitter;
use tokio::sync::oneshot;

use crate::{
    record_live_trace, record_live_trace_at_path, typed_events, with_session_id, LiveRun,
    PendingAsk,
};

use super::{now_ms, subagent_round_tool, TRACE_INPUT_KEEP_CHARS};

/// UI 事件发射通道(window.emit + with_session_id 的闭包)。
type EmitFn = Arc<dyn Fn(&str, serde_json::Value) -> tauri::Result<()> + Send + Sync>;

/// R-253 批8:UI 投影 sink——把 RunEvent 投影为前端可见的 kz:* 事件。
/// 持有 emit 闭包(捕获 window + session_id);纯 UI 事件只落这一个 sink。
pub(crate) struct UiEventSink {
    emit: EmitFn,
    session_id: String,
    run_id: String,
}

impl UiEventSink {
    pub(crate) fn new(
        emit_event: impl Fn(&str, serde_json::Value) -> tauri::Result<()> + Send + Sync + 'static,
        session_id: String,
        run_id: String,
    ) -> Self {
        Self {
            emit: Arc::new(emit_event),
            session_id,
            run_id,
        }
    }

    fn emit(&self, name: &str, payload: serde_json::Value) -> tauri::Result<()> {
        let legacy_result = (self.emit)(name, payload.clone());
        let structured_result = match crate::experience_events::from_legacy(
            name,
            &self.session_id,
            Some(self.run_id.clone()),
            payload,
            now_ms().max(0) as u64,
        ) {
            Ok(event) => {
                // R-299 契约扫描(ipc-event-smoke)只认「emit 调用 + 字面量事件名」的
                // 源码形态,经字段间接调用的常量名它看不见;名称真源仍是
                // kanzei-core::EXPERIENCE_EVENT_NAME,由 debug 断言钉住一致。
                debug_assert_eq!(
                    crate::experience_events::EXPERIENCE_EVENT_NAME,
                    "kz:experience"
                );
                let emit_event = &self.emit;
                emit_event("kz:experience", event.into_value())
            }
            Err(error) => {
                tracing::warn!(%error, event = name, "experience event adaptation failed");
                Ok(())
            }
        };
        legacy_result.and(structured_result)
    }
}

/// R-253 批8:typed 事件 sink——把 RunEvent 投影为 typed_events(TypedEventWriter)。
pub(crate) struct TypedEventSink {
    writer: Arc<Mutex<typed_events::TypedEventWriter>>,
}

impl TypedEventSink {
    pub(crate) fn new(writer: Arc<Mutex<typed_events::TypedEventWriter>>) -> Self {
        Self { writer }
    }
    fn turn_started(&self, step: u32, max_steps: u32) {
        self.writer.lock().unwrap().turn_started(step, max_steps);
    }
    fn push_text(&self, text: &str) {
        self.writer.lock().unwrap().push_text(text);
    }
    fn assistant_committed(&self, step: u32, message: kanzei_llm::Message) {
        self.writer
            .lock()
            .unwrap()
            .assistant_committed(step, message);
    }
    fn tool_results_committed(&self, step: u32, message: kanzei_llm::Message) {
        self.writer
            .lock()
            .unwrap()
            .tool_results_committed(step, message);
    }
    fn stream_restarted(&self) {
        self.writer.lock().unwrap().stream_restarted();
    }
}

/// R-253 批8:trace sink——把 RunEvent 投影为 live 画像与 run.trace 增量落库。
pub(crate) struct TraceSink {
    live: Arc<Mutex<LiveRun>>,
    state_path: PathBuf,
    session_id: String,
    run_id: String,
    /// D-374:本 run 期间复用同一条连接。
    ///
    /// 原实现每条 RunEvent 都走 `SessionStore::open`,而一次 open 不是"打开个文件"那么
    /// 便宜:create_dir_all + Connection::open + busy_timeout/journal_mode/synchronous
    /// 三个 pragma + migrate 的建表批与版本查询 + housekeeping 的节流时间戳查询。
    /// 在本仓 132MB 的主库上实测约 4.3ms/次;历史 48,582 条 run.trace 折合约 210 秒,
    /// 全部花在反复打开一个刚刚关掉的文件上。
    ///
    /// 为什么是 `Mutex` 而不是直接持有:`rusqlite::Connection` 是 Send 但**不是** Sync,
    /// 而 EventSink 要求 Sync(原注释"事件回调需要 Send + Sync,不能捕获 rusqlite 连接"
    /// 说的就是这一条)。`Mutex<Connection>` 补上 Sync,连接因此可以跨事件存活。
    /// 并发事件从"各开一条连接在 SQLite 层用 busy_timeout 抢"变成"在这把锁上排队",
    /// 单行 insert 的临界区是微秒级,严格优于原状。
    ///
    /// 打不开就留 None,逐事件回落到原来的短开连接路径——轨迹落库失败不打断模型运行,
    /// 与原语义一致。
    store: Mutex<Option<kanzei_core::SessionStore>>,
}

impl TraceSink {
    pub(crate) fn new(
        live: Arc<Mutex<LiveRun>>,
        state_path: PathBuf,
        session_id: String,
        run_id: String,
    ) -> Self {
        let store = kanzei_core::SessionStore::open(&state_path)
            .map_err(|error| {
                tracing::warn!(
                    target: "kanzei::run",
                    path = %state_path.display(),
                    %error,
                    "轨迹连接打开失败,本轮回落到逐事件短开连接"
                );
            })
            .ok();
        Self {
            live,
            state_path,
            session_id,
            run_id,
            store: Mutex::new(store),
        }
    }
    fn record_transaction_budget_extension(
        &self,
        step: u32,
        base_max_steps: u32,
        extension_steps: u32,
    ) -> bool {
        let store = self.store.lock().unwrap();
        let Some(store) = store.as_ref() else {
            return false;
        };
        let already_recorded = store
            .list_events_by_type(&self.session_id, 0, "run.transaction_budget_extended")
            .map(|events| {
                events
                    .iter()
                    .any(|event| event.payload["run_id"].as_str() == Some(self.run_id.as_str()))
            })
            .unwrap_or(true);
        if already_recorded {
            return false;
        }
        store
            .append_event(
                &self.session_id,
                "run.transaction_budget_extended",
                &json!({
                    "run_id": self.run_id,
                    "step": step,
                    "base_max_steps": base_max_steps,
                    "extension_steps": extension_steps,
                    "reason": "tests_passed_files_staged_commit_tracker_anchor_only",
                }),
            )
            .is_ok()
    }

    fn note_event(&self) {
        self.live.lock().unwrap().note_event();
    }

    fn record(&self, payload: serde_json::Value) {
        let store = self.store.lock().unwrap();
        let persisted = match store.as_ref() {
            Some(store) => record_live_trace(store, &self.session_id, &self.live, payload.clone()),
            None => {
                drop(store);
                record_live_trace_at_path(
                    &self.state_path,
                    &self.session_id,
                    &self.live,
                    payload.clone(),
                )
            }
        };
        if !persisted {
            crate::record_unpersisted_artifact(&self.state_path, &self.session_id, &payload);
        }
    }
    fn note_step(&self, step: u32) {
        let mut live = self.live.lock().unwrap();
        live.steps = live.steps.max(step);
    }
    fn add_usage(&self, usage: &kanzei_llm::Usage) {
        let mut live = self.live.lock().unwrap();
        live.input_tokens += usage.input;
        live.output_tokens += usage.output;
    }
}

/// R-253 批8:metrics sink——commit 检测位(R-143)与子代理工具画像(D-361)。
/// ⑦:两个 `AtomicBool` 的 swap 语义必须整体归本 sink;⑧:`subagent_tools` 是
/// 跨模块状态,run_task 轮末仍要读,故持有同一个 `Arc`。
pub(crate) struct MetricsSink {
    tool_started: Arc<Mutex<HashMap<String, std::time::Instant>>>,
    round_committed: Arc<std::sync::atomic::AtomicBool>,
    round_pending: Arc<std::sync::atomic::AtomicBool>,
    subagent_tools: Arc<Mutex<std::collections::BTreeSet<String>>>,
    /// D-654:主轮真实执行过的工具名——鞭挞画像的真源。轮中上下文压缩会把
    /// `summary.messages` 结构性删短,按 `prior.len()` 切片会把本轮真实调用切掉
    /// (画像切空 → 误判无动作 → Nudge/Stop(NoAction)),事件流不受消息改写影响。
    round_tools: Arc<Mutex<std::collections::BTreeSet<String>>>,
    /// D-654 同因:req/defect close 的成功计数也改事件收口——原
    /// `closed_count_this_round` 扫的是全历史 `summary.messages`,历史 close 每轮
    /// 重复计入,verify_every_n 节律被刷穿。ToolStart 登记 close 意图,ToolEnd
    /// ok=true 才计数,语义与原「调用 close 且 ToolResult 非 error」一致。
    pending_closes: Mutex<std::collections::HashSet<String>>,
    round_closed: Arc<std::sync::atomic::AtomicU32>,
    /// R-322(#7):本轮模型是否用 `work handoff` 显式声明任务完成、交还控制权。
    /// 与 close 计数同一收口方式(ToolStart 登记意图 → ToolEnd ok 才置位):
    /// 被权限拦下或执行失败的 handoff 不算数据。
    pending_handoffs: Mutex<std::collections::HashSet<String>>,
    round_handoff: Arc<std::sync::atomic::AtomicBool>,
    /// R-319:显式收尾事务状态。只由真实工具结果推进，不接受模型自报。
    transaction_calls: Mutex<HashMap<String, (String, serde_json::Value)>>,
    tests_passed: std::sync::atomic::AtomicBool,
    tests_failed: std::sync::atomic::AtomicBool,
    files_staged: std::sync::atomic::AtomicBool,
    stage_failed: std::sync::atomic::AtomicBool,
    commit_pending: std::sync::atomic::AtomicBool,
    source_edited: std::sync::atomic::AtomicBool,
    unexpected_tool: std::sync::atomic::AtomicBool,
    approval_seen: std::sync::atomic::AtomicBool,
    extension_used: std::sync::atomic::AtomicBool,
}

impl MetricsSink {
    pub(crate) fn new(
        tool_started: Arc<Mutex<HashMap<String, std::time::Instant>>>,
        committed_this_round: Arc<std::sync::atomic::AtomicBool>,
        pending_commit_call: Arc<std::sync::atomic::AtomicBool>,
        subagent_tools: Arc<Mutex<std::collections::BTreeSet<String>>>,
        round_tools: Arc<Mutex<std::collections::BTreeSet<String>>>,
        round_closed: Arc<std::sync::atomic::AtomicU32>,
        round_handoff: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            tool_started,
            round_committed: committed_this_round,
            round_pending: pending_commit_call,
            subagent_tools,
            round_tools,
            pending_closes: Mutex::new(std::collections::HashSet::new()),
            round_closed,
            pending_handoffs: Mutex::new(std::collections::HashSet::new()),
            round_handoff,
            transaction_calls: Mutex::new(HashMap::new()),
            tests_passed: std::sync::atomic::AtomicBool::new(false),
            tests_failed: std::sync::atomic::AtomicBool::new(false),
            files_staged: std::sync::atomic::AtomicBool::new(false),
            stage_failed: std::sync::atomic::AtomicBool::new(false),
            commit_pending: std::sync::atomic::AtomicBool::new(false),
            source_edited: std::sync::atomic::AtomicBool::new(false),
            unexpected_tool: std::sync::atomic::AtomicBool::new(false),
            approval_seen: std::sync::atomic::AtomicBool::new(false),
            extension_used: std::sync::atomic::AtomicBool::new(false),
        }
    }
    /// R-143:git commit 调用意图登记(成功与否由 ToolEnd ok 收口)。
    fn note_commit_intent(&self, name: &str, input: &serde_json::Value) {
        if name == "git" && input["action"].as_str() == Some("commit") {
            self.round_pending
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    /// D-173:按 id 记工具开始时刻,供 ToolEnd 算耗时。
    fn note_tool_started(&self, id: &str) {
        self.tool_started
            .lock()
            .unwrap()
            .insert(id.to_string(), std::time::Instant::now());
    }
    /// D-654:主轮工具画像按事件收集(名字进画像;调了就算,成败不论——与原
    /// 消息画像里 ToolCall 出现即计入的语义一致)。close 意图同步登记,等 ToolEnd 收口。
    fn note_round_tool(&self, id: &str, name: &str, input: &serde_json::Value) {
        self.round_tools.lock().unwrap().insert(name.to_string());
        self.transaction_calls
            .lock()
            .unwrap()
            .insert(id.to_string(), (name.to_string(), input.clone()));
        if matches!(name, "edit" | "insert" | "write") {
            self.source_edited
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if !matches!(name, "git" | "req" | "defect" | "work" | "test_record") {
            self.unexpected_tool
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let is_close = matches!(name, "req" | "defect")
            && input.get("action").and_then(serde_json::Value::as_str) == Some("close");
        if is_close {
            self.pending_closes.lock().unwrap().insert(id.to_string());
        }
        // R-322:handoff 意图登记,等 ToolEnd ok 收口。
        if name == "work"
            && input.get("action").and_then(serde_json::Value::as_str) == Some("handoff")
        {
            self.pending_handoffs.lock().unwrap().insert(id.to_string());
        }
    }
    /// R-143:git commit 结束后解析提交结果;返回该工具耗时(取并清开始时刻)。
    fn resolve_tool_end(&self, id: &str, name: &str, ok: bool) -> Option<u128> {
        let transaction_call = self.transaction_calls.lock().unwrap().remove(id);
        if let Some((call_name, input)) = transaction_call {
            if call_name == "test_record" {
                let passed = ok && input["status"].as_str() == Some("passed");
                if passed {
                    self.tests_passed
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                } else {
                    // R-319/D-679:失败是本轮事务的永久 taint；后续补跑成功也不能
                    // 把「发生过失败」伪装成从未失败。
                    self.tests_failed
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            if call_name == "git" && input["action"].as_str() == Some("stage") {
                if ok {
                    self.files_staged
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    self.commit_pending
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                } else {
                    self.stage_failed
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            if call_name == "git" && input["action"].as_str() == Some("commit") && ok {
                self.files_staged
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                self.commit_pending
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }
        // D-654:close 调用成功才计入本轮关闭数(被门禁拦下的 close 不算,
        // 否则核查节律被失败调用刷阈值——与原 closed_count_this_round 判据一致)。
        if self.pending_closes.lock().unwrap().remove(id) && ok {
            self.round_closed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // R-322:handoff 成功执行才算模型真的声明了完成。
        if self.pending_handoffs.lock().unwrap().remove(id) && ok {
            self.round_handoff
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if name == "git" {
            if ok
                && self
                    .round_pending
                    .swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                self.round_committed
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            } else if !ok {
                self.round_pending
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }
        self.tool_started
            .lock()
            .unwrap()
            .remove(id)
            .map(|at| at.elapsed().as_millis())
    }
    /// D-361:子代理内部完成的工具名上卷进本轮画像。
    fn note_subagent_tool(&self, name: &str) {
        self.subagent_tools.lock().unwrap().insert(name.to_string());
    }
}

impl MetricsSink {
    /// R-319:仅在最后一步且三个确定性事实齐备时授予一次 2 步收尾延长。
    /// 事务状态一旦被源码编辑、审批或失败测试污染，永不自动恢复。
    fn maybe_extend_transaction(&self, step: u32, max_steps: u32) -> bool {
        if max_steps == 0 || step < max_steps {
            return false;
        }
        self.tests_passed.load(std::sync::atomic::Ordering::Relaxed)
            && self
                .commit_pending
                .load(std::sync::atomic::Ordering::Relaxed)
            && !self.tests_failed.load(std::sync::atomic::Ordering::Relaxed)
            && !self.stage_failed.load(std::sync::atomic::Ordering::Relaxed)
            && !self
                .unexpected_tool
                .load(std::sync::atomic::Ordering::Relaxed)
            && !self
                .source_edited
                .load(std::sync::atomic::Ordering::Relaxed)
            && !self
                .approval_seen
                .load(std::sync::atomic::Ordering::Relaxed)
            && !self
                .extension_used
                .swap(true, std::sync::atomic::Ordering::Relaxed)
    }

    fn note_approval(&self) {
        self.approval_seen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// R-253 批8:构造 run_task 的 RunEvent 处理器闭包——按投影拆成四个 sink 后 fanout。
/// D-173 可观测性:主代理工具调用实时转发 UI 并按 id 记开始时刻;R-143:git commit
/// 检测位在 ToolStart(action=commit)/ToolEnd(ok=true) 置位/提升;轨迹与 typed writer
/// 增量落库。加一个 RunEvent 变体时,按它要投影到的消费者(UI/typed/trace/metrics)
/// 只改对应 sink 的方法与这一个 arm 的 fanout 调用——不需要再读懂五种投影的揉合。
pub(crate) fn build_event_handler(
    ui: UiEventSink,
    typed: TypedEventSink,
    trace: TraceSink,
    metrics: MetricsSink,
) -> impl FnMut(RunEvent) {
    move |event: RunEvent| {
        // 每个真实 RunEvent 都刷新协作快照的进展时钟；UI 不靠纯轮询时间猜死活。
        trace.note_event();
        let _ = match event {
            RunEvent::TurnStart {
                step,
                max_steps,
                budget_extension,
            } => {
                if metrics.maybe_extend_transaction(step, max_steps)
                    && trace.record_transaction_budget_extension(step, max_steps, 2)
                {
                    budget_extension.store(2, std::sync::atomic::Ordering::Relaxed);
                    trace.record(json!({
                        "kind": "transaction_budget.extended",
                        "step": step,
                        "baseMaxSteps": max_steps,
                        "extensionSteps": 2,
                        "reason": "tests_passed_files_staged_commit_tracker_anchor_only",
                        "at": now_ms(),
                    }));
                }
                trace.note_step(step);
                trace.record(json!({ "kind": "turn.started", "step": step, "at": now_ms() }));
                typed.turn_started(step, max_steps);
                ui.emit("kz:turn", json!({ "step": step, "maxSteps": max_steps }))
            }
            RunEvent::Text(text) => {
                typed.push_text(&text);
                ui.emit("kz:text", json!({ "text": text }))
            }
            RunEvent::Reasoning(text) => ui.emit("kz:reasoning", json!({ "text": text })),
            RunEvent::AssistantMessageCommitted { step, message } => {
                typed.assistant_committed(step, message);
                Ok(())
            }
            RunEvent::ToolStart {
                id,
                name,
                summary,
                input,
            } => {
                metrics.note_commit_intent(&name, &input);
                metrics.note_tool_started(&id);
                metrics.note_round_tool(&id, &name, &input);
                trace.record(json!({
                    "kind": "tool.started", "id": id, "name": name,
                    "summary": summary, "at": now_ms(),
                }));
                ui.emit(
                    "kz:tool-start",
                    json!({ "id": id, "name": name, "summary": summary, "input": input }),
                )
            }
            // 执行中的增量输出:只转发给 UI 实时追加,不进 trace——回放时
            // ToolEnd 的完整输出就是终态,逐段进度落盘只会把轨迹撑爆。
            RunEvent::ToolProgress { id, chunk } => {
                ui.emit("kz:tool-progress", json!({ "id": id, "chunk": chunk }))
            }
            RunEvent::ToolEnd {
                id,
                name,
                ok,
                outcome,
                code,
                preview,
                display,
                artifact,
            } => {
                let duration_ms = metrics.resolve_tool_end(&id, &name, ok);
                trace.record(json!({
                    "kind": "tool.completed", "id": id, "name": name, "ok": ok,
                    "outcome": outcome, "code": code,
                    "durationMs": duration_ms, "at": now_ms(),
                    "preview": preview,
                    "artifact": artifact,
                    // D-349:终态只留紧凑 preview 与可恢复 artifact 元数据,不复制完整原文。
                    "error": (!ok).then(|| preview.chars().take(400).collect::<String>()),
                }));
                ui.emit(
                    "kz:tool-end",
                    json!({ "id": id, "name": name, "ok": ok, "outcome": outcome, "code": code, "preview": preview, "display": display, "artifact": artifact }),
                )
            }
            RunEvent::ToolResultsCommitted { step, message } => {
                typed.tool_results_committed(step, message);
                Ok(())
            }
            // 轮内主动压缩:UI 要看得见"什么时候让的路、让掉了多少",
            // 否则历史突然变短只会被当成 bug(D-176)。
            RunEvent::ContextCompacted {
                before_tokens,
                after_tokens,
                budget_tokens,
                limit_tokens,
                dropped_messages,
            } => {
                trace.record(json!({
                    "kind": "context.compacted", "before": before_tokens, "after": after_tokens,
                    "budget": budget_tokens, "limit": limit_tokens,
                    "dropped": dropped_messages, "at": now_ms(),
                }));
                ui.emit(
                    "kz:status",
                    json!({
                        "stage": "压缩",
                        "detail": format!(
                            "上下文约 {}k 已达 {}k 预算线(上限 {}k),就地压缩为 {}k,裁掉 {dropped_messages} 条历史",
                            before_tokens / 1000, budget_tokens / 1000,
                            limit_tokens / 1000, after_tokens / 1000
                        ),
                    }),
                )
            }
            // R-236 B4:L0 机械清理——先于 LLM 纪要,零幻觉;轨迹留档让
            // 「压缩触发频率下降」可度量。
            RunEvent::ContextPruned {
                cleared_results,
                before_tokens,
                after_tokens,
            } => {
                trace.record(json!({
                    "kind": "context.pruned", "cleared": cleared_results,
                    "before": before_tokens, "after": after_tokens, "at": now_ms(),
                }));
                ui.emit(
                    "kz:status",
                    json!({
                        "stage": "压缩",
                        "detail": format!(
                            "已机械清理 {cleared_results} 条旧工具结果({}k → {}k token),未动 LLM 纪要",
                            before_tokens / 1000, after_tokens / 1000
                        ),
                    }),
                )
            }
            RunEvent::PermissionResolved {
                tool_call_id,
                action,
                resource,
                decision,
                source,
                ..
            } => {
                metrics.note_approval();
                trace.record(json!({
                    "kind": "permission.resolved", "id": tool_call_id, "action": action,
                    "resource": resource, "decision": decision, "source": source, "at": now_ms(),
                }));
                ui.emit(
                    "kz:permission-resolved",
                    json!({
                        "tool_call_id": tool_call_id,
                        "action": action,
                        "resource": resource,
                        "decision": decision,
                        "source": source,
                    }),
                )
            }
            // 子代理实时状态:挂到对应 task 块的进度行,并附带可展开的子工具轨迹。
            RunEvent::TaskProgress {
                id,
                text,
                trace: task_trace,
            } => {
                // D-361:子代理内部完成的工具调用,名字上卷进本轮画像。主轮画像只切
                // 主 conversation 的消息(轮末 summarize_tools),子代理的 read/grep/edit
                // 全在它自己的消息里——主轮看得见的只有一次 task 调用,而 task 本身在
                // NON_PROGRESS_TOOLS 里。不上卷的话「整轮把活派给子代理」在鞭挞眼里
                // 等于什么都没干:第一轮 Nudge、第二轮 Stop(NoAction),越守规矩地委派
                // 越快自停,停止原因还误报成「空转」。
                if let Some(name) = task_trace.as_ref().and_then(subagent_round_tool) {
                    metrics.note_subagent_tool(name);
                }
                // UI 实时事件保留完整入参(transcript 数据源,R-174);
                // 落库副本把入参截断到上限,避免大入参撑爆 run.trace(D-297 验收③)。
                let ui_payload = json!({
                    "id": id,
                    "text": text,
                    "trace": task_trace.as_ref().map(|item| json!({
                        "child_id": item.child_id,
                        "phase": item.phase,
                        "name": item.name,
                        "summary": item.summary,
                        "ok": item.ok,
                        "outcome": item.outcome,
                        "code": item.code,
                        "preview": item.preview,
                        "artifact": item.artifact,
                        "display": item.display,
                        "input": item.input,
                        "usage": item.usage,
                        "text": item.text,
                    })),
                });
                let stored_payload = match &task_trace {
                    Some(item) => json!({
                        "id": id,
                        "text": text,
                        "trace": json!({
                            "child_id": item.child_id,
                            "phase": item.phase,
                            "name": item.name,
                            "summary": item.summary,
                            "ok": item.ok,
                            "outcome": item.outcome,
                            "code": item.code,
                            "preview": item.preview,
                            "artifact": item.artifact,
                            "display": item.display,
                            "input": item.input.as_ref().map(|input| {
                                let text = serde_json::to_string(input).unwrap_or_default();
                                let kept: String =
                                    text.chars().take(TRACE_INPUT_KEEP_CHARS).collect();
                                json!(kept)
                            }),
                            "usage": item.usage,
                            "text": item.text,
                        }),
                    }),
                    None => ui_payload.clone(),
                };
                trace.record(stored_payload);
                ui.emit("kz:task-progress", ui_payload)
            }
            RunEvent::Retry {
                attempt,
                max,
                delay_ms,
            } => ui.emit(
                "kz:status",
                json!({ "stage": "重试", "detail": format!("网络请求暂时失败,第 {attempt}/{max} 次重试,等待 {delay_ms}ms") }),
            ),
            // 本步工具尚未执行,重放零副作用;前端需丢弃本步已渲染的残缺输出。
            RunEvent::StreamRestart {
                attempt,
                max,
                delay_ms,
            } => {
                typed.stream_restarted();
                ui.emit(
                    "kz:stream-restart",
                    json!({
                    "attempt": attempt,
                    "max": max,
                    "delayMs": delay_ms,
                    "detail": format!("连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms"),
                    }),
                )
            }
            // 每步累计:停止时 episode 才有真实 token 数,而不是写个 0 冒充。
            RunEvent::StepEnd { usage, .. } => {
                trace.add_usage(&usage);
                ui.emit(
                    "kz:step",
                    json!({
                        "input": usage.input, "output": usage.output,
                        "cacheRead": usage.cache_read, "cacheWrite": usage.cache_write,
                    }),
                )
            }
        };
    }
}

/// R-202 批2:构造 run_task 的权限/提问询问处理器闭包(原 run_task 内联的 ask)。
/// 请求经 kz:ask 事件发给前端,应答挂 pending asks 表等待 answer_ask 回填。
pub(crate) fn build_ask_handler(
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    ask_seq: Arc<AtomicU64>,
    ask_source: &'static str,
    window: &tauri::Window,
    project_root: PathBuf,
    session_id: String,
) -> impl FnMut(kanzei_core::AskRequest) -> AskFuture {
    let ask_window = window.clone();
    let ask_root = project_root;
    let ask_session_id = session_id;
    move |request: kanzei_core::AskRequest| -> AskFuture {
        let (sender, receiver) = oneshot::channel();
        let id = ask_seq.fetch_add(1, Ordering::SeqCst);
        let (action, resource, payload) = match &request {
            kanzei_core::AskRequest::Permission { action, resource } => (
                action.clone(),
                resource.clone(),
                json!({ "kind": "permission", "id": id, "action": action, "resource": resource, "remember": kanzei_harness::config::generalize_resource(action, resource) }),
            ),
            kanzei_core::AskRequest::Question {
                question,
                options,
                default,
                multiple,
            } => (
                "question".into(),
                question.clone(),
                json!({ "kind": "question", "id": id, "question": question, "options": options, "default": default, "multiple": multiple }),
            ),
        };
        let payload = with_session_id(payload, &ask_session_id);
        let payload = match payload {
            serde_json::Value::Object(mut object) => {
                object.insert("source".into(), json!(ask_source));
                serde_json::Value::Object(object)
            }
            other => other,
        };
        asks.lock().unwrap().insert(
            id,
            PendingAsk {
                sender,
                request,
                action,
                resource,
                project_root: ask_root.clone(),
                session_id: ask_session_id.clone(),
            },
        );
        let _ = ask_window.emit("kz:ask", payload);
        // D-388:approval 建立时发手机系统通知(验收⑥「息屏收到 approval 通知」)。
        // 尽力而为:无推送桥只记诊断不阻塞。
        let (notify_title, notify_body) = match &asks.lock().unwrap()[&id].request {
            kanzei_core::AskRequest::Permission { action, resource } => (
                "kanzei 需要批准".to_string(),
                format!("{action}: {resource}"),
            ),
            kanzei_core::AskRequest::Question { question, .. } => {
                ("kanzei 询问".to_string(), question.clone())
            }
        };
        if let Ok(message) = crate::mobile_notify::notify_mobile(&notify_title, &notify_body) {
            tracing::debug!("{message}");
        }
        Box::pin(async move {
            receiver
                .await
                .unwrap_or(kanzei_core::AskResponse::Cancelled)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-374 机械判据:轨迹落库在一次 run 里**只开一条连接**。
    ///
    /// 断言的是 `SessionStore::open` 的调用次数,不是"读代码看起来复用了"。原实现
    /// 每条 RunEvent 开一条(N 条事件 = N+ 次 open,132MB 库上 ~4.3ms/次);把
    /// `record` 换回 `record_live_trace_at_path` 这条判据立刻红。
    #[test]
    fn 轨迹落库整轮只开一条连接() {
        let dir = std::env::temp_dir().join(format!(
            "kz-trace-conn-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state_path = dir.join("state.db");
        {
            let store = kanzei_core::SessionStore::open(&state_path).unwrap();
            store.create_session("ses_trace", "C:/proj", None).unwrap();
        }

        let live = Arc::new(Mutex::new(LiveRun::default()));
        live.lock().unwrap().begin("run-1", "in-1", "头", "p", "m");

        let before = kanzei_core::store_open_count(&state_path);
        let sink = TraceSink::new(
            live,
            state_path.clone(),
            "ses_trace".into(),
            "run-trace-test".into(),
        );
        const EVENTS: usize = 20;
        for index in 0..EVENTS {
            sink.record(json!({ "kind": "test.event", "i": index }));
        }
        let opened = kanzei_core::store_open_count(&state_path) - before;
        assert_eq!(
            opened, 1,
            "{EVENTS} 条轨迹事件开了 {opened} 条连接:逐事件 open 的成本又回来了(D-374)"
        );

        // 复用连接不得以少写事件为代价:事件必须全部落库。
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        let events = store
            .list_events_by_type("ses_trace", 0, "run.trace")
            .unwrap();
        assert_eq!(events.len(), EVENTS, "轨迹事件落库条数不符");
        drop(store);
        drop(sink);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn mk_metrics_sink() -> (
        MetricsSink,
        Arc<Mutex<std::collections::BTreeSet<String>>>,
        Arc<std::sync::atomic::AtomicU32>,
    ) {
        mk_metrics_sink_full().0
    }

    #[allow(clippy::type_complexity)]
    fn mk_metrics_sink_full() -> (
        (
            MetricsSink,
            Arc<Mutex<std::collections::BTreeSet<String>>>,
            Arc<std::sync::atomic::AtomicU32>,
        ),
        Arc<std::sync::atomic::AtomicBool>,
    ) {
        let round_tools = Arc::new(Mutex::new(std::collections::BTreeSet::new()));
        let round_closed = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let round_handoff = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sink = MetricsSink::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(Mutex::new(std::collections::BTreeSet::new())),
            round_tools.clone(),
            round_closed.clone(),
            round_handoff.clone(),
        );
        ((sink, round_tools, round_closed), round_handoff)
    }

    /// R-322(#7):handoff 必须成功执行才算模型声明了完成——被拦下/失败的调用
    /// 不得让引擎误以为模型交还了控制权(与 D-654 的 close 计数同一收口口径)。
    #[test]
    fn handoff成功才置位_失败不算声明() {
        let ((sink, _, _), handoff) = mk_metrics_sink_full();
        sink.note_round_tool("h1", "work", &json!({"action": "handoff"}));
        sink.resolve_tool_end("h1", "work", false);
        assert!(
            !handoff.load(std::sync::atomic::Ordering::Relaxed),
            "失败的 handoff 不得置位"
        );

        sink.note_round_tool("h2", "work", &json!({"action": "handoff"}));
        sink.resolve_tool_end("h2", "work", true);
        assert!(
            handoff.load(std::sync::atomic::Ordering::Relaxed),
            "成功的 handoff 必须置位"
        );
    }

    /// work 的其它动作不得被误判成 handoff。
    #[test]
    fn work其它动作不置位handoff() {
        let ((sink, _, _), handoff) = mk_metrics_sink_full();
        for action in ["next", "claim", "complete", "checkpoint"] {
            sink.note_round_tool("x", "work", &json!({"action": action}));
            sink.resolve_tool_end("x", "work", true);
        }
        assert!(!handoff.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn 事务延长只在收尾事实齐备时触发且只触发一次() {
        let (sink, _, _) = mk_metrics_sink();
        sink.note_round_tool("test", "test_record", &json!({"status": "passed"}));
        sink.resolve_tool_end("test", "test_record", true);
        sink.note_round_tool("stage", "git", &json!({"action": "stage"}));
        sink.resolve_tool_end("stage", "git", true);
        assert!(sink.maybe_extend_transaction(32, 32));
        assert!(!sink.maybe_extend_transaction(32, 32));
    }

    #[test]
    fn 事务延长失败后补跑成功仍永久拒绝() {
        let (failed_test, _, _) = mk_metrics_sink();
        failed_test.note_round_tool("t1", "test_record", &json!({"status": "failed"}));
        failed_test.resolve_tool_end("t1", "test_record", false);
        failed_test.note_round_tool("t2", "test_record", &json!({"status": "passed"}));
        failed_test.resolve_tool_end("t2", "test_record", true);
        failed_test.note_round_tool("s1", "git", &json!({"action": "stage"}));
        failed_test.resolve_tool_end("s1", "git", true);
        assert!(!failed_test.maybe_extend_transaction(32, 32));

        let (failed_stage, _, _) = mk_metrics_sink();
        failed_stage.note_round_tool("t", "test_record", &json!({"status": "passed"}));
        failed_stage.resolve_tool_end("t", "test_record", true);
        failed_stage.note_round_tool("s1", "git", &json!({"action": "stage"}));
        failed_stage.resolve_tool_end("s1", "git", false);
        failed_stage.note_round_tool("s2", "git", &json!({"action": "stage"}));
        failed_stage.resolve_tool_end("s2", "git", true);
        assert!(!failed_stage.maybe_extend_transaction(32, 32));
    }

    #[test]
    fn 事务延长遇到失败源码编辑或审批时拒绝() {
        let (failed_test, _, _) = mk_metrics_sink();
        failed_test.note_round_tool("test", "test_record", &json!({"status": "failed"}));
        failed_test.resolve_tool_end("test", "test_record", false);
        failed_test.note_round_tool("stage", "git", &json!({"action": "stage"}));
        failed_test.resolve_tool_end("stage", "git", true);
        assert!(!failed_test.maybe_extend_transaction(32, 32));

        let (edited, _, _) = mk_metrics_sink();
        edited.note_round_tool("test", "test_record", &json!({"status": "passed"}));
        edited.resolve_tool_end("test", "test_record", true);
        edited.note_round_tool("stage", "git", &json!({"action": "stage"}));
        edited.resolve_tool_end("stage", "git", true);
        edited.note_round_tool("edit", "edit", &json!({"path": "src/lib.rs"}));
        assert!(!edited.maybe_extend_transaction(32, 32));

        let (approved, _, _) = mk_metrics_sink();
        approved.note_round_tool("test", "test_record", &json!({"status": "passed"}));
        approved.resolve_tool_end("test", "test_record", true);
        approved.note_round_tool("stage", "git", &json!({"action": "stage"}));
        approved.resolve_tool_end("stage", "git", true);
        approved.note_approval();
        assert!(!approved.maybe_extend_transaction(32, 32));
    }

    /// R-319 B3:扩展事件必须按 run_id 去重，重启恢复或重复回调不得重复记账。
    #[test]
    fn 事务延长事件按run_id去重() {
        let dir = std::env::temp_dir().join(format!(
            "kz-transaction-budget-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state_path = dir.join("state.db");
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        store.create_session("ses_budget", "C:/proj", None).unwrap();
        drop(store);
        let live = Arc::new(Mutex::new(LiveRun::default()));
        live.lock()
            .unwrap()
            .begin("run-budget", "in-1", "头", "p", "m");
        let sink = TraceSink::new(
            live,
            state_path.clone(),
            "ses_budget".into(),
            "run-budget".into(),
        );
        assert!(sink.record_transaction_budget_extension(32, 32, 2));
        assert!(!sink.record_transaction_budget_extension(32, 32, 2));
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        assert_eq!(
            store
                .list_events_by_type("ses_budget", 0, "run.transaction_budget_extended")
                .unwrap()
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// D-654 核心回归:鞭挞的工具画像走事件真源,不经过 `summary.messages` 切片。
    /// 轮中上下文压缩把消息列表结构性删短后,`&messages[prior.len()..]` 会把本轮
    /// 真实的 edit/bash 全部切掉(画像空 → 误判「连续两轮无实质动作」自停);
    /// 事件流逐次收集,画像与消息列表的任何改写(压缩/prune/trim)彻底解耦。
    #[test]
    fn 主轮工具画像走事件真源_与消息切片解耦() {
        let (sink, round_tools, _) = mk_metrics_sink();
        for (id, name) in [("c1", "read"), ("c2", "edit"), ("c3", "bash")] {
            sink.note_round_tool(id, name, &json!({}));
        }
        let names: Vec<String> = round_tools.lock().unwrap().iter().cloned().collect();
        assert_eq!(names, ["bash", "edit", "read"]);
        assert!(
            kanzei_harness::auto_run::has_progress_tools(&names),
            "事件画像里有 edit/bash,本轮必须判为有实质动作"
        );
    }

    /// D-654:close 计数事件收口——ToolStart 登记意图,ToolEnd ok=true 才 +1。
    /// 被门禁拦下的 close(ok=false)与非 close 动作(update)都不计,判据与原
    /// closed_count_this_round(「调用 close 且 ToolResult 非 error」)一致;
    /// 差别是只收本轮事件,历史轮的 close 不会再被每轮重复计入刷穿 verify 节律。
    #[test]
    fn close计数事件收口_成功才计_失败与update不计() {
        let (sink, _, round_closed) = mk_metrics_sink();
        sink.note_round_tool("c1", "req", &json!({"action": "close", "id": "R-001"}));
        sink.note_round_tool("c2", "defect", &json!({"action": "close", "id": "D-001"}));
        sink.note_round_tool("c3", "req", &json!({"action": "update", "id": "R-002"}));
        let _ = sink.resolve_tool_end("c1", "req", true);
        let _ = sink.resolve_tool_end("c2", "defect", false);
        let _ = sink.resolve_tool_end("c3", "req", true);
        assert_eq!(
            round_closed.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "c1 成功 close 计 1;c2 被拦不计;c3 是 update 不计"
        );
    }
}
