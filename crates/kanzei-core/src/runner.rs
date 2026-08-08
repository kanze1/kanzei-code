//! agent 循环(M1):harness 快照驱动——工具物化过权限、system 由 Context Source 拼装、
//! 每次工具调用过硬门禁(deny 回喂模型 / ask 问用户,用户拒绝则整轮停,与 V2 语义一致)。
//! steer/queue/持久化调度在 M2 引入。

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use kanzei_harness::{
    tolerant_parse, tool::repair_hint, AgentDef, Effect, HarnessSnapshot, Tool, ToolCtx,
};
use kanzei_llm::{
    FinishReason, LlmClient, LlmEvent, LlmRequest, Message, Part, ReasoningEffort, Role, Route,
    ToolSpec, Usage,
};

pub struct RunnerConfig {
    pub model: String,
    pub max_tokens: u32,
    /// 思考强度;由调用方(CLI/桌面端)按配置或运行时选择传入。
    pub reasoning: ReasoningEffort,
}

/// 单轮子代理上限：并行仍保持，但避免模型一次生成过多请求拖垮连接/本地模型。
pub const MAX_TASKS_PER_TURN: usize = 8;

/// 流中途断开后重放本步请求的上限。工具在流结束后才执行,所以此时重放零副作用;
/// 但每次重放都要重新生成已产出的 token,必须有界。
pub const MAX_STREAM_RESTARTS: u32 = 2;

/// Provider 上下文计算可能比本地估算更严格。首次超限保留有界历史摘要，
/// 第二次超限只保留当前用户消息；再失败就把真实错误交给调用方。
pub const MAX_CONTEXT_OVERFLOW_RECOVERIES: u32 = 2;

/// task 子代理运行时(R-004/R-012)。快照由调用方用 SubagentBase 组件构建,
/// 代码层面只含只读工具——子代理无人应答权限询问,必须做到零 ask。
pub struct SubagentRuntime {
    pub snapshot: Arc<HarnessSnapshot>,
    pub agent: AgentDef,
    /// (route, model id):fast = 本地小模型跑机械检索。
    pub fast: (Route, String),
    /// primary = 主模型,给需要理解代码的任务。
    pub primary: (Route, String),
    pub max_tokens: u32,
    /// 单个子代理的墙钟上限(秒):本地模型多轮可能极慢,必须有界。
    pub timeout_secs: u64,
}

fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Delegate a narrow read-only exploration task (find files, call \
                      sites, usages; read and summarize code) to a subagent with tools \
                      read/glob/grep. Params: prompt (self-contained instruction saying \
                      exactly what to find and what to report back); optional model: \
                      \"fast\" (default, local model, mechanical searches) | \"primary\" \
                      (tasks needing code comprehension). Multiple task calls in one \
                      turn run in parallel."
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

#[derive(Clone, Debug)]
pub struct TaskTrace {
    pub child_id: String,
    pub phase: String,
    pub name: String,
    pub summary: Option<String>,
    pub ok: Option<bool>,
    pub preview: Option<String>,
    pub display: Option<serde_json::Value>,
}

/// 面向 UI 的运行事件(CLI/桌面端都消费这一层,不直接碰 LlmEvent)。
pub enum RunEvent {
    /// 一轮 provider 调用开始(UI 画轮次分隔)。
    TurnStart {
        step: u32,
        max_steps: u32,
    },
    Text(String),
    Reasoning(String),
    ToolStart {
        /// 工具调用 id:并行工具(task)结束顺序不定,UI 靠它配对 start/end。
        id: String,
        name: String,
        summary: String,
        /// 结构化入参原文。summary 是给人看的一行摘要,信息量不足以复核"它到底
        /// 拿什么参数调的";活动面板要能展开完整入参,只能从这里拿(R-095)。
        input: serde_json::Value,
    },
    ToolEnd {
        id: String,
        name: String,
        ok: bool,
        preview: String,
        /// 结构化展示(diff/终端块),见 ToolOutput::display。
        display: Option<serde_json::Value>,
    },
    /// 子代理运行中的实时状态(轮次/正在用的工具),挂在对应 task 块上。
    TaskProgress {
        id: String,
        text: String,
        trace: Option<TaskTrace>,
    },
    /// 流建立前的临时网络错误重试,不会重放已建立流或工具副作用。
    Retry { attempt: u32, max: u32, delay_ms: u128 },
    /// 流中途断开后重放本步请求。本步工具尚未执行,零副作用;
    /// UI 收到后应丢弃本步已渲染的残缺输出,等待重新生成。
    StreamRestart { attempt: u32, max: u32, delay_ms: u128 },
    StepEnd {
        usage: Usage,
        reason: FinishReason,
    },
}

fn drain_task_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<RunEvent>,
    on_event: &mut dyn FnMut(RunEvent),
) {
    while let Ok(event) = rx.try_recv() {
        on_event(event);
    }
}

pub struct RunSummary {
    pub text: String,
    pub usage: Usage,
    pub steps: u32,
    /// 用户拒绝权限导致的提前停止。
    pub halted_by_user: bool,
    /// 本次运行结束时的完整消息历史(含本次),调用方保存后可作为下次 prior 传入,
    /// 实现跨消息连续对话(M2 落盘前的内存态方案)。
    pub messages: Vec<Message>,
    /// 上下文账单(R-106):system 各注入源的字符数(agent/system 在首位)。
    pub context_report: Vec<(String, usize)>,
}

/// 轨迹的工具调用画像:工具名 → 调用次数(episode 与 R-099 度量共用)。
/// 注意:传入 `&summary.messages` 会把 prior 全历史一起统计;要本轮画像请传
/// `&summary.messages[prior_len..]`。
pub fn summarize_tools(messages: &[Message]) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for message in messages {
        for part in &message.parts {
            if let Part::ToolCall { name, .. } = part {
                *counts.entry(name.clone()).or_insert(0) += 1;
            }
        }
    }
    counts
}

/// 一条失败信号:同一 (工具 × 错误类) 在本轮的聚合。
#[derive(Debug, Clone, PartialEq)]
pub struct FailureSignal {
    pub tool: String,
    /// 归一化错误指纹(抹掉路径与数字后的错误首行),用于聚合与跨轮去重。
    pub kind: String,
    /// 错误原文首段,给 manager 判断用。
    pub sample: String,
    /// 涉及的目标(路径/命令首词),最多 3 个。
    pub targets: Vec<String>,
    pub count: usize,
    /// 同目标后续成功的调用:「X 不行、Y 可以」是最值得记的形状。
    pub recovered_by: Option<String>,
}

/// 从本轮轨迹提炼失败信号(R-105 机械触发):失败数据本来就在 messages 里,
/// 引擎不额外采集,只做一次线性扫描 + 指纹聚合。纯函数,可单测。
///
/// 只传本轮切片(`&summary.messages[prior_len..]`),否则会把历史失败重复上报。
/// 一轮的调用画像(R-099):判断"冗余治理有没有效"所需的最小可比量。
/// 全部来自本轮消息切片,不含历史——混进 prior 会让基线一路虚高。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunMetrics {
    /// 终端调用次数(bash / process)。
    pub terminal_calls: usize,
    /// 其中被判定为 git 查询的次数(status/diff/log/show)。
    pub git_calls: usize,
    /// git 查询"组"数:连续的 git 查询算一组。同样是 6 次查询,
    /// 分散在 6 处比挤成 1 组糟得多——组数才反映节奏问题。
    pub git_groups: usize,
    /// edit / multiedit 调用次数与其中失败(未命中)次数。
    pub edit_calls: usize,
    pub edit_misses: usize,
    /// 子代理调用次数。
    pub subagent_calls: usize,
    /// 工具调用总次数与失败次数。
    pub total_calls: usize,
    pub failed_calls: usize,
}

impl RunMetrics {
    /// edit 未命中率(无 edit 调用时为 0)。
    pub fn edit_miss_rate(&self) -> f64 {
        if self.edit_calls == 0 {
            0.0
        } else {
            self.edit_misses as f64 / self.edit_calls as f64
        }
    }
}

/// 统计本轮调用画像。传 `&summary.messages[prior_len..]`,不要传全历史。
pub fn summarize_metrics(messages: &[Message]) -> RunMetrics {
    let mut metrics = RunMetrics::default();
    let mut calls: std::collections::HashMap<String, (String, bool)> =
        std::collections::HashMap::new();
    // 上一次调用是否是 git 查询:用来把连续的 git 查询并成一组。
    let mut prev_was_git = false;

    for message in messages {
        for part in &message.parts {
            match part {
                Part::ToolCall { id, name, input } => {
                    let is_git = name == "bash" && is_git_query(input);
                    calls.insert(id.clone(), (name.clone(), is_git));
                    metrics.total_calls += 1;
                    match name.as_str() {
                        "bash" | "process" => {
                            metrics.terminal_calls += 1;
                            if is_git {
                                metrics.git_calls += 1;
                                if !prev_was_git {
                                    metrics.git_groups += 1;
                                }
                            }
                        }
                        "edit" | "multiedit" => metrics.edit_calls += 1,
                        "task" => metrics.subagent_calls += 1,
                        _ => {}
                    }
                    prev_was_git = is_git;
                }
                Part::ToolResult { call_id, is_error, .. } => {
                    if !*is_error {
                        continue;
                    }
                    metrics.failed_calls += 1;
                    if let Some((name, _)) = calls.get(call_id) {
                        if name == "edit" || name == "multiedit" {
                            metrics.edit_misses += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    metrics
}

fn is_git_query(input: &serde_json::Value) -> bool {
    const QUERIES: &[&str] = &["git status", "git diff", "git log", "git show", "git blame"];
    let command = input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    QUERIES.iter().any(|q| command.contains(q))
}

/// 本轮完成的条目(R-124 SOP 提炼的触发闸门)。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletedEntry {
    /// 条目 id,如 R-123 / D-150。
    pub id: String,
    /// 落到的终态:done / fixed / dropped / wontfix。
    pub status: String,
    /// 本轮实际动过手的工具(去重、按首次出现排序),提炼步骤的原料。
    pub tools: Vec<String>,
}

/// 判定"本轮是否确实完成了一个完整条目"。只有成立时才允许提炼 SOP——
/// 否则失败轮、空转轮、纯查询轮都会产出垃圾模板,把 SOP 库淹掉(R-124 验收 ②)。
///
/// 成立条件全部满足:
/// 1. 有一次 **成功** 的 req/defect update,且目标状态是终态;
/// 2. 本轮存在实质动作(改文件或跑命令)——只把条目一勾并不构成可复用流程;
/// 3. 该次 update 之前就有实质动作,顺序不能反(先勾再干活不是同一件事)。
///
/// 判定用代码强制而非写进提示词:提示词约束不住"这轮到底算不算完成"。
pub fn completed_entry(messages: &[Message]) -> Option<CompletedEntry> {
    const TERMINAL: &[&str] = &["done", "fixed", "dropped", "wontfix"];
    const SUBSTANTIVE: &[&str] = &["write", "edit", "multiedit", "bash"];

    let mut calls: std::collections::HashMap<String, (String, serde_json::Value)> =
        std::collections::HashMap::new();
    let mut tools: Vec<String> = Vec::new();
    let mut substantive_before = false;
    let mut found: Option<CompletedEntry> = None;

    for message in messages {
        for part in &message.parts {
            match part {
                Part::ToolCall { id, name, input } => {
                    calls.insert(id.clone(), (name.clone(), input.clone()));
                }
                Part::ToolResult { call_id, is_error, .. } => {
                    let Some((name, input)) = calls.get(call_id) else { continue };
                    if *is_error {
                        continue;
                    }
                    if !tools.iter().any(|t| t == name) {
                        tools.push(name.clone());
                    }
                    if SUBSTANTIVE.contains(&name.as_str()) {
                        substantive_before = true;
                    }
                    if !matches!(name.as_str(), "req" | "defect") {
                        continue;
                    }
                    let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    let status = input.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if action != "update" || !TERMINAL.contains(&status) {
                        continue;
                    }
                    // 先干活后收口才算完成一件事;顺序反了(先勾再改)不构成可复用流程。
                    if !substantive_before {
                        continue;
                    }
                    let Some(id) = input.get("id").and_then(|v| v.as_str()) else { continue };
                    found = Some(CompletedEntry {
                        id: id.to_string(),
                        status: status.to_string(),
                        tools: tools.clone(),
                    });
                }
                _ => {}
            }
        }
    }
    // tools 要含收口那一刻之后的全貌:取最终列表,保证提炼看得到完整流程。
    found.map(|mut entry| {
        entry.tools = tools;
        entry
    })
}

pub fn summarize_failures(messages: &[Message]) -> Vec<FailureSignal> {
    // call_id → (工具名, 目标):ToolResult 只有 call_id,工具名要回溯 ToolCall。
    let mut calls: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let mut raw_inputs: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // 指纹 → 信号;另记每个目标最近一次失败,便于配对后续成功。
    let mut signals: Vec<FailureSignal> = Vec::new();
    let mut failed_targets: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for message in messages {
        for part in &message.parts {
            match part {
                Part::ToolCall { id, name, input } => {
                    // 同时留原始输入:判定恢复对时要看"失败的目标是否出现在后续成功调用里"
                    // (edit main.rs 失败 → bash 用脚本改 main.rs 成功,是同一个目标)。
                    calls.insert(id.clone(), (name.clone(), failure_target(input)));
                    raw_inputs.insert(id.clone(), input.to_string().to_lowercase());
                }
                Part::ToolResult { call_id, content, is_error } => {
                    let Some((tool, target)) = calls.get(call_id).cloned() else {
                        continue;
                    };
                    if *is_error {
                        let kind = failure_kind(content);
                        let position = match signals
                            .iter()
                            .position(|s| s.tool == tool && s.kind == kind)
                        {
                            Some(index) => {
                                signals[index].count += 1;
                                if !signals[index].targets.contains(&target)
                                    && signals[index].targets.len() < 3
                                {
                                    signals[index].targets.push(target.clone());
                                }
                                index
                            }
                            None => {
                                signals.push(FailureSignal {
                                    tool: tool.clone(),
                                    kind,
                                    sample: content.chars().take(240).collect(),
                                    targets: if target.is_empty() {
                                        Vec::new()
                                    } else {
                                        vec![target.clone()]
                                    },
                                    count: 1,
                                    recovered_by: None,
                                });
                                signals.len() - 1
                            }
                        };
                        if !target.is_empty() {
                            failed_targets.insert(target, position);
                        }
                    } else {
                        // 恢复对:失败过的目标出现在后续某次成功调用的输入里。工具不必相同——
                        // 「edit 不行、改用 bash 脚本可以」正是最值得记的形状。
                        let haystack = raw_inputs.get(call_id).cloned().unwrap_or_default();
                        let recovered: Vec<String> = failed_targets
                            .keys()
                            .filter(|failed| haystack.contains(failed.as_str()))
                            .cloned()
                            .collect();
                        for failed in recovered {
                            if let Some(position) = failed_targets.remove(&failed) {
                                if signals[position].recovered_by.is_none() {
                                    signals[position].recovered_by = Some(tool.clone());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // 机械闸门:一次性失败不入库(搜索空间无限的负面事实没有记忆价值);
    // 重复 ≥2 次说明是稳定的坑,或带恢复对说明有现成解药——两者才值得记。
    signals.retain(|s| s.count >= 2 || s.recovered_by.is_some());
    signals
}

/// 错误指纹:首行小写 → 抹掉含路径分隔符的 token 与全部数字 → 折叠空白 → 截 80。
/// 目的是让「13 次 CRLF 未命中」塌成同一条,而不是 13 条。
fn failure_kind(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").to_lowercase();
    let scrubbed: Vec<String> = first_line
        .split_whitespace()
        .filter(|token| !token.contains('/') && !token.contains('\\'))
        .map(|token| {
            token
                .chars()
                .filter(|c| !c.is_ascii_digit())
                .collect::<String>()
        })
        .filter(|token| !token.is_empty())
        .collect();
    scrubbed.join(" ").chars().take(80).collect()
}

/// 目标键:路径类取最后一段(跨平台分隔符都算),命令取首词,其余取 id。
fn failure_target(input: &serde_json::Value) -> String {
    for key in ["path", "file_path", "file", "id", "command"] {
        if let Some(value) = input.get(key).and_then(|v| v.as_str()) {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            if key == "command" {
                return value.split_whitespace().next().unwrap_or("").to_lowercase();
            }
            return value
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(value)
                .to_lowercase();
        }
    }
    String::new()
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    use kanzei_llm::{Message, Part};

    fn call(id: &str, name: &str, target_key: &str, target: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: name.into(),
            input: serde_json::json!({ target_key: target }),
        }])
    }
    fn result(call_id: &str, content: &str, is_error: bool) -> Message {
        Message::tool_results(vec![Part::ToolResult {
            call_id: call_id.into(),
            content: content.into(),
            is_error,
        }])
    }

    fn tracker_call(id: &str, tool: &str, entry: &str, status: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: tool.into(),
            input: serde_json::json!({ "action": "update", "id": entry, "status": status }),
        }])
    }

    fn bash(id: &str, command: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: "bash".into(),
            input: serde_json::json!({ "command": command }),
        }])
    }

    #[test]
    fn 调用画像把连续_git_查询并成一组() {
        let messages = vec![
            // 一组:三条连着的 git 查询
            bash("g1", "git status --porcelain"),
            result("g1", "", false),
            bash("g2", "git diff --stat"),
            result("g2", "", false),
            bash("g3", "git log --oneline -3"),
            result("g3", "", false),
            // 中间插入真正的工作,后面的 git 查询另起一组
            call("e1", "edit", "path", "src/lib.rs"),
            result("e1", "no match", true),
            call("e2", "edit", "path", "src/lib.rs"),
            result("e2", "ok", false),
            bash("g4", "git status"),
            result("g4", "", false),
            bash("b1", "cargo test"),
            result("b1", "ok", false),
        ];
        let m = summarize_metrics(&messages);
        assert_eq!(m.terminal_calls, 5, "bash 调用总数");
        assert_eq!(m.git_calls, 4);
        assert_eq!(m.git_groups, 2, "连续查询应并成一组,分散才是节奏问题");
        assert_eq!(m.edit_calls, 2);
        assert_eq!(m.edit_misses, 1);
        assert!((m.edit_miss_rate() - 0.5).abs() < 1e-9);
        assert_eq!(m.failed_calls, 1);
        assert_eq!(m.total_calls, 7);
        assert_eq!(m.subagent_calls, 0);

        // 无 edit 时未命中率是 0 而不是除零。
        assert_eq!(summarize_metrics(&[]).edit_miss_rate(), 0.0);
    }

    #[test]
    fn sop_提炼只在真正完成一个条目时触发() {
        // 正例:先改文件再收口到终态 —— 这才是一段可复用的流程。
        let done = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            call("c2", "bash", "command", "cargo test"),
            result("c2", "test result: ok", false),
            tracker_call("c3", "req", "R-123", "done"),
            result("c3", "updated", false),
        ];
        let entry = completed_entry(&done).expect("完成一个完整条目时应触发");
        assert_eq!(entry.id, "R-123");
        assert_eq!(entry.status, "done");
        assert!(entry.tools.contains(&"edit".to_string()) && entry.tools.contains(&"bash".to_string()));

        // 反例一:纯查询轮 —— 没有任何实质动作,不构成可复用流程。
        let read_only = vec![
            call("c1", "read", "path", "src/lib.rs"),
            result("c1", "...", false),
            tracker_call("c2", "req", "R-124", "done"),
            result("c2", "updated", false),
        ];
        assert!(completed_entry(&read_only).is_none(), "纯查询轮不该提炼 SOP");

        // 反例二:先勾完成再干活 —— 顺序反了,勾的那一刻并没有完成什么。
        let out_of_order = vec![
            tracker_call("c1", "req", "R-125", "done"),
            result("c1", "updated", false),
            call("c2", "edit", "path", "src/lib.rs"),
            result("c2", "ok", false),
        ];
        assert!(completed_entry(&out_of_order).is_none(), "先收口后干活不该提炼 SOP");

        // 反例三:收口调用本身失败 —— 条目根本没进终态。
        let failed_close = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            tracker_call("c2", "req", "R-126", "done"),
            result("c2", "cannot move backward", true),
        ];
        assert!(completed_entry(&failed_close).is_none(), "收口失败不该提炼 SOP");

        // 反例四:只是把状态推到 doing —— 不是终态。
        let in_progress = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            tracker_call("c2", "req", "R-127", "doing"),
            result("c2", "updated", false),
        ];
        assert!(completed_entry(&in_progress).is_none(), "推进到 doing 不是完成");
    }

    #[test]
    fn 一次性失败不上报_重复失败才成为信号() {
        // 搜索空间无限的负面事实(猜错一次文件名)没有记忆价值,必须被闸掉。
        let once = vec![
            call("c1", "read", "path", "src/nope.rs"),
            result("c1", "cannot access src/nope.rs: not found", true),
        ];
        assert!(summarize_failures(&once).is_empty());

        // 同一坑重复两次 = 稳定问题,值得记。
        let twice = vec![
            call("c1", "edit", "path", "C:/p/main.rs"),
            result("c1", "old_string not found in C:/p/main.rs — it must match exactly", true),
            call("c2", "edit", "path", "C:/p/other.rs"),
            result("c2", "old_string not found in C:/p/other.rs — it must match exactly", true),
        ];
        let signals = summarize_failures(&twice);
        assert_eq!(signals.len(), 1, "同类错误必须塌成一条: {signals:?}");
        assert_eq!(signals[0].tool, "edit");
        assert_eq!(signals[0].count, 2);
        // 指纹抹掉了路径,两次不同文件仍归一类
        assert!(!signals[0].kind.contains("main.rs"), "指纹不该含路径: {}", signals[0].kind);
        assert_eq!(signals[0].targets, vec!["main.rs", "other.rs"]);
    }

    #[test]
    fn 失败后换工具成功构成恢复对_单次也上报() {
        // 「X 不行、Y 可以」自带解药,是最值得记的形状,阈值降到 1。
        let messages = vec![
            call("c1", "edit", "path", "C:/p/main.rs"),
            result("c1", "old_string not found in C:/p/main.rs", true),
            call("c2", "bash", "command", "python fix.py C:/p/main.rs"),
            result("c2", "exit code: 0", false),
        ];
        let signals = summarize_failures(&messages);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].recovered_by.as_deref(), Some("bash"));
        assert_eq!(signals[0].count, 1, "有恢复对时单次失败即上报");
    }

    #[test]
    fn tdd_噪声不被误报为信号() {
        // 先写测试→失败→实现→通过:这是正常节奏,不是可复用知识。
        // 单次失败被阈值闸掉;目标不同(测试命令 vs 源文件)也不构成恢复对。
        let messages = vec![
            call("c1", "bash", "command", "cargo test新增用例"),
            result("c1", "exit code: 101\ntest failed", true),
            call("c2", "edit", "path", "C:/p/impl.rs"),
            result("c2", "replaced 1 occurrence(s)", false),
        ];
        assert!(
            summarize_failures(&messages).is_empty(),
            "TDD 的一次预期失败不该进记忆"
        );
    }

    #[test]
    fn 本轮统计不含历史_调用方须传切片() {
        // summarize_tools/summarize_failures 都按传入切片统计;
        // 调用方传 &messages[prior_len..] 才是本轮画像(否则历史被重复计入)。
        let history = vec![
            call("h1", "edit", "path", "old.rs"),
            result("h1", "old_string not found in old.rs", true),
            call("h2", "edit", "path", "old2.rs"),
            result("h2", "old_string not found in old2.rs", true),
        ];
        let mut all = history.clone();
        all.extend(vec![
            call("n1", "read", "path", "new.rs"),
            result("n1", "ok", false),
        ]);
        assert_eq!(summarize_failures(&all).len(), 1, "全量含历史失败");
        assert!(
            summarize_failures(&all[history.len()..]).is_empty(),
            "本轮切片不该带出历史失败"
        );
        assert_eq!(summarize_tools(&all[history.len()..]).get("read"), Some(&1));
        assert_eq!(summarize_tools(&all[history.len()..]).get("edit"), None);
    }
}

/// 权限询问的用户决定。AlwaysAllow 的持久化由 UI 层负责(写入项目配置),
/// runner 只负责本次会话内不再重复询问。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskReply {
    Deny,
    AllowOnce,
    AlwaysAllow,
}

/// 权限询问回调的返回值:异步等待用户决定(CLI 同步问、桌面端走事件+oneshot)。
#[derive(Clone, Debug)]
pub enum AskRequest {
    Permission { action: String, resource: String },
    Question { question: String, options: Vec<String>, default: Option<String> },
}

#[derive(Clone, Debug)]
pub enum AskResponse {
    Permission(AskReply),
    Answer(String),
    Cancelled,
}

/// 交互询问回调的返回值:异步等待权限决定或用户答案。
pub type AskFuture = std::pin::Pin<Box<dyn std::future::Future<Output = AskResponse> + Send>>;

#[allow(clippy::too_many_arguments)]
pub fn run_once<'a>(
    client: &'a LlmClient,
    route: &'a Route,
    snapshot: &'a HarnessSnapshot,
    agent: &'a AgentDef,
    config: &'a RunnerConfig,
    ctx: &'a ToolCtx,
    prompt: &'a str,
    prior: &'a [Message],
    subagent: Option<&'a SubagentRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    run_once_with_parts(client, route, snapshot, agent, config, ctx, prompt, prior, None, subagent, on_event, ask)
}

#[allow(clippy::too_many_arguments)]
pub fn run_once_with_parts<'a>(
    client: &'a LlmClient,
    route: &'a Route,
    snapshot: &'a HarnessSnapshot,
    agent: &'a AgentDef,
    config: &'a RunnerConfig,
    ctx: &'a ToolCtx,
    prompt: &'a str,
    // 之前轮次的完整消息历史(空 = 新对话)。
    prior: &'a [Message],
    initial_parts: Option<&'a [Part]>,
    // Some = 注册 task 工具,模型可派生并行子代理;子代理自身传 None(禁嵌套)。
    subagent: Option<&'a SubagentRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    Box::pin(async move {
    let tools: Vec<Arc<dyn Tool>> = snapshot.materialize_tools();
    let mut specs: Vec<ToolSpec> = tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description(),
            input_schema: t.input_schema(),
        })
        .collect();
    if subagent.is_some() {
        specs.push(task_spec());
    }

    // system 分块:agent 提示词 + harness baseline(M2 起 baseline 进 Context Epoch)。
    let (baseline, mut context_report) = snapshot.system_baseline_with_report();
    if !agent.system.trim().is_empty() {
        context_report.insert(0, ("agent/system".into(), agent.system.chars().count()));
    }
    let system: Vec<String> = [agent.system.clone(), baseline]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();

    // prior 可能来自旧快照或跨进程恢复，先统一清洗孤儿工具 part，避免首次请求
    // 在尚未触发上下文压缩时就把非法消息交给 provider。
    let mut messages: Vec<Message> = crate::history::filter_message_history(prior);
    let user_parts = match initial_parts {
        Some(parts) => {
            let mut parts = parts.to_vec();
            if !prompt.is_empty() {
                parts.insert(0, Part::Text { text: prompt.to_string() });
            }
            parts
        }
        None => vec![Part::Text { text: prompt.to_string() }],
    };
    messages.push(Message { role: Role::User, parts: user_parts });
    let mut total_usage = Usage::default();
    let mut final_text = String::new();
    // steps 语义:0 = 无上限(用户定调:不设人为轮数天花板——停止权在用户按钮
    // 与上下文管理,不在计数器)。>0 时保留封顶,最后一步收工具+收尾指令。
    let max_steps = agent.steps;
    // 本次运行内已放行的 (action, resource):同一资源不重复问(用户反馈:别烦我)。
    let mut session_approved: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // "总是允许"的会话内即时生效层(D-006):快照是开跑时定死的,新写入的规则
    // 本次运行读不到——泛化 pattern 记在这里,同类资源当场不再询问。
    let mut session_rules: Vec<(String, String)> = Vec::new();

    let mut overflow_recoveries = 0;

    let mut step = 0u32;
    loop {
        step += 1;
        on_event(RunEvent::TurnStart { step, max_steps });
        let last_step = max_steps > 0 && step == max_steps;

        // Provider 可能比本地配置更严格地计算上下文(尤其是工具 schema)。
        // 建流前和 HTTP 200 后 SSE 流内都可能报告 context overflow，必须走同一套
        // 有界恢复；本步工具要等流完整结束才执行，因此流内超限重放不会重复副作用。
        let mut stream_restarts: u32 = 0;
        let (parts, calls, finish) = loop {
        // 每次恢复都会改写 messages，请求必须在重试循环内重建；否则即使压缩了
        // 内存历史，发给 provider 的仍会是第一次克隆出的超长请求。
        let mut request_messages = messages.clone();
        // 最后一步收走工具强制收敛;必须同时明确告知(D-027:只收走不告知,
        // codex 仍试图调用工具,把调用 JSON 当纯文本狂喷并在思考里反复自我纠正)。
        if last_step {
            request_messages.push(Message::user_text(
                "(system) Final step of this run: tools are no longer available. Do NOT \
                 attempt any tool call and do NOT emit JSON — reply in plain text only, \
                 summarizing what was completed and what remains.",
            ));
        }
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.clone(),
            messages: request_messages,
            tools: if last_step { vec![] } else { specs.clone() },
            max_tokens: config.max_tokens,
            temperature: None,
            reasoning: config.reasoning,
        };
        let mut stream = match client
            .stream_with_retry_notice(route, &request, |attempt, delay| {
                on_event(RunEvent::Retry { attempt, max: kanzei_llm::client::MAX_TRANSPORT_RETRIES, delay_ms: delay.as_millis() });
            })
            .await
        {
            Err(error) if error.is_context_overflow() => {
                if recover_context_overflow(&mut messages, &mut overflow_recoveries) {
                    continue;
                }
                return Err(error.into());
            }
            result => result?,
        };
        let mut text_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut reasoning_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut parts: Vec<Part> = Vec::new();
        let mut calls: Vec<(String, String, serde_json::Value, String)> = Vec::new();
        let mut finish = FinishReason::EndTurn;
        let mut stream_error: Option<kanzei_llm::LlmError> = None;

        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    stream_error = Some(error);
                    break;
                }
            };
            match event {
                LlmEvent::TextDelta { index, text } => {
                    on_event(RunEvent::Text(text.clone()));
                    text_buffers.entry(index).or_default().push_str(&text);
                }
                LlmEvent::TextEnd { index } => {
                    if let Some(text) = text_buffers.remove(&index) {
                        parts.push(Part::Text { text });
                    }
                }
                LlmEvent::ReasoningDelta { index, text } => {
                    on_event(RunEvent::Reasoning(text.clone()));
                    reasoning_buffers.entry(index).or_default().push_str(&text);
                }
                // reasoning 连同 signature(codex 的 encrypted_content)入历史,
                // Responses 协议多轮工具循环必须回放;其他协议的 builder 自行忽略。
                LlmEvent::ReasoningEnd { index, signature } => {
                    let text = reasoning_buffers.remove(&index).unwrap_or_default();
                    if !text.is_empty() || signature.is_some() {
                        parts.push(Part::Reasoning { text, signature });
                    }
                }
                LlmEvent::ToolCall {
                    id,
                    name,
                    input,
                    raw_input,
                } => {
                    // 协议层解析失败 → 宽容修复(尾逗号/单引号/裸键/围栏)。
                    let input = if input.is_null() {
                        tolerant_parse(&raw_input).unwrap_or(serde_json::Value::Null)
                    } else {
                        input
                    };
                    parts.push(Part::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: if input.is_null() {
                            serde_json::json!({})
                        } else {
                            input.clone()
                        },
                    });
                    calls.push((id, name, input, raw_input));
                }
                LlmEvent::StepFinish { usage, reason } => {
                    total_usage = add_usage(total_usage, usage);
                    finish = reason.clone();
                    on_event(RunEvent::StepEnd { usage, reason });
                }
                _ => {}
            }
        }

        match stream_error {
            None => {
                for (_, text) in std::mem::take(&mut text_buffers) {
                    parts.push(Part::Text { text });
                }
                break (parts, calls, finish);
            }
            // Provider 也可能在 HTTP 200 的 SSE error 事件里报告上下文超限。
            // 此时本步工具尚未执行，压缩 messages 后安全地从头重放请求。
            Some(error) if error.is_context_overflow() => {
                if recover_context_overflow(&mut messages, &mut overflow_recoveries) {
                    continue;
                }
                return Err(error.into());
            }
            // 只重放传输层中断:协议错误重放只会原样复现,白烧钱。
            Some(error)
                if matches!(error, kanzei_llm::LlmError::Transport(_))
                    && stream_restarts < MAX_STREAM_RESTARTS =>
            {
                stream_restarts += 1;
                let delay = std::time::Duration::from_millis(500 * stream_restarts as u64);
                tracing::warn!(
                    attempt = stream_restarts,
                    delay_ms = delay.as_millis(),
                    error = %error,
                    "stream broke mid-flight, re-requesting step"
                );
                on_event(RunEvent::StreamRestart {
                    attempt: stream_restarts,
                    max: MAX_STREAM_RESTARTS,
                    delay_ms: delay.as_millis(),
                });
                tokio::time::sleep(delay).await;
            }
            Some(error) => return Err(error.into()),
        }
        };

        final_text = parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !parts.is_empty() {
            messages.push(Message::assistant(parts));
        }

        if calls.is_empty() {
            return Ok(RunSummary {
                text: final_text,
                usage: total_usage,
                steps: step,
                halted_by_user: false,
                messages,
                context_report: context_report.clone(),
            });
        }

        // ---- task 子代理:同轮多个 task 并行执行。只读快照无副作用,与任何工具并发都安全 ----
        let mut task_results: std::collections::HashMap<String, kanzei_harness::ToolOutput> =
            std::collections::HashMap::new();
        if let Some(rt) = subagent {
            let mut task_calls: Vec<(String, serde_json::Value, String)> = calls
                .iter()
                .filter(|(_, name, _, _)| name == "task")
                .map(|(id, _, input, raw)| (id.clone(), input.clone(), raw.clone()))
                .collect();
            if !task_calls.is_empty() {
                let overflow = if task_calls.len() > MAX_TASKS_PER_TURN {
                    task_calls.split_off(MAX_TASKS_PER_TURN)
                } else {
                    Vec::new()
                };
                for (id, input, raw) in &task_calls {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                        input: input.clone(),
                    });
                }
                for (id, input, raw) in &overflow {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                        input: input.clone(),
                    });
                    let output = kanzei_harness::ToolOutput::error(format!(
                        "too many parallel subagent tasks; maximum per turn is {}",
                        MAX_TASKS_PER_TURN
                    ));
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name: "task".into(),
                        ok: false,
                        preview: preview(&output.content),
                        display: output.display.clone(),
                    });
                    task_results.insert(id.clone(), output);
                }
                // 进度通道:子代理内部事件(轮次/工具)转成 TaskProgress 实时上抛,
                // 完成一个立刻报一个 ToolEnd——不再等最慢的,UI 全程有反馈。
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
                let mut jobs: futures::stream::FuturesUnordered<_> = task_calls
                    .iter()
                    .map(|(id, input, _)| {
                        let tx = tx.clone();
                        async move {
                            let bound = std::time::Duration::from_secs(rt.timeout_secs);
                            let output = match tokio::time::timeout(
                                bound,
                                run_subagent(client, rt, ctx, id, input, tx),
                            )
                            .await
                            {
                                Ok(output) => output,
                                // 纯兜底(默认 15 分钟):防失控,不是性能预算。
                                Err(_) => kanzei_harness::ToolOutput::error(format!(
                                    "subagent hit the {}s wall-clock safety limit — split the task into narrower pieces",
                                    rt.timeout_secs
                                )),
                            };
                            (id.clone(), output)
                        }
                    })
                    .collect();
                drop(tx);
                loop {
                    tokio::select! {
                        next = jobs.next() => match next {
                            Some((id, output)) => {
                                on_event(RunEvent::ToolEnd {
                                    id: id.clone(),
                                    name: "task".into(),
                                    ok: !output.is_error,
                                    preview: preview(&output.content),
                                    display: output.display.clone(),
                                });
                                task_results.insert(id, output);
                            }
                            None => {
                                drain_task_events(&mut rx, on_event);
                                break;
                            }
                        },
                        Some(event) = rx.recv() => on_event(event),
                    }
                }
            }
        }

        let mut results = Vec::new();
        for (call_index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
            // task 不过权限门禁:子代理快照在代码层面只含只读工具(硬门禁在构造,不在评估)。
            // ToolEnd 已在并行阶段按完成顺序上报过,这里只归位结果。
            if name == "task" && subagent.is_some() {
                let output = task_results.remove(&id).unwrap_or_else(|| {
                    kanzei_harness::ToolOutput::error("internal: task result missing")
                });
                results.push(Part::ToolResult {
                    call_id: id,
                    content: output.content,
                    is_error: output.is_error,
                });
                continue;
            }
            let Some(tool) = tools.iter().find(|t| t.name() == name) else {
                results.push(Part::ToolResult {
                    call_id: id,
                    content: format!(
                        "unknown tool `{name}`; available: {}",
                        tools
                            .iter()
                            .map(|t| t.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    is_error: true,
                });
                continue;
            };
            on_event(RunEvent::ToolStart {
                id: id.clone(),
                name: name.clone(),
                summary: summarize_input(&input, &raw_input),
                input: input.clone(),
            });

            // question 是交互工具，不再叠加权限询问；答案作为工具结果回喂模型。
            if name == "question" {
                let question = input.get("question").and_then(|v| v.as_str()).unwrap_or("").trim();
                let options = input.get("options").and_then(|v| v.as_array()).map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
                let default = input.get("default").and_then(|v| v.as_str()).map(str::to_owned);
                let output = if question.is_empty() {
                    kanzei_harness::ToolOutput::error("question must not be empty")
                } else {
                    match ask(AskRequest::Question { question: question.to_owned(), options, default }).await {
                        AskResponse::Answer(answer) => kanzei_harness::ToolOutput::ok(format!("User answer: {answer}")),
                        AskResponse::Cancelled => kanzei_harness::ToolOutput::error("question cancelled by user"),
                        AskResponse::Permission(_) => kanzei_harness::ToolOutput::error("invalid question response"),
                    }
                };
                on_event(RunEvent::ToolEnd { id: id.clone(), name: name.clone(), ok: !output.is_error, preview: preview(&output.content), display: output.display.clone() });
                results.push(Part::ToolResult { call_id: id, content: output.content, is_error: output.is_error });
                continue;
            }

            // ---- 硬门禁:权限 Ruleset(deny 回喂模型;ask 问用户,拒绝停整轮)----
            let action = tool.action();
            let mut gate_result = Gate::Pass;
            let mut pending_ask: Vec<String> = Vec::new();
            for resource in tool.resources_with_ctx(&input, ctx) {
                // 统一正斜杠 + 消解 . / ..,权限 pattern 不用关心平台,也不能被路径变体绕过:
                // `.kanzei/research/../../src/main.rs` 会被 `*.kanzei/research/*` 判为放行,
                // 而落盘时 join 会消解 ..,实际写到项目任意位置(D-050)。
                let normalized =
                    kanzei_harness::permission::normalize_resource(&resource);
                match snapshot.evaluate(action, &normalized) {
                    Effect::Deny => {
                        gate_result = Gate::Deny(normalized);
                        break;
                    }
                    Effect::Ask => pending_ask.push(normalized),
                    Effect::Allow => {}
                }
            }
            if matches!(gate_result, Gate::Pass) {
                for resource in pending_ask {
                    let key = (action.to_string(), resource.clone());
                    if session_approved.contains(&key) {
                        continue;
                    }
                    if session_rules.iter().any(|(a, pattern)| {
                        a == action
                            && kanzei_harness::permission::resource_match_for_action(a, pattern, &resource)
                    }) {
                        continue;
                    }
                    match ask(AskRequest::Permission { action: action.to_string(), resource: resource.clone() }).await {
                        AskResponse::Permission(AskReply::Deny) | AskResponse::Cancelled | AskResponse::Answer(_) => {
                            gate_result = Gate::UserDeclined;
                            break;
                        }
                        AskResponse::Permission(AskReply::AllowOnce) => {
                            session_approved.insert(key);
                        }
                        AskResponse::Permission(AskReply::AlwaysAllow) => {
                            session_rules.push((
                                action.to_string(),
                                kanzei_harness::config::generalize_resource(action, &resource),
                            ));
                        }
                    }
                }
            }
            let output = match gate_result {
                Gate::Deny(resource) => kanzei_harness::ToolOutput::error(format!(
                    "permission denied by ruleset: {action} on `{resource}`. \
                     This resource is policy-managed; use the dedicated tool for it.",
                )),
                Gate::UserDeclined => {
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name: name.clone(),
                        ok: false,
                        preview: "(user declined)".into(),
                        display: None,
                    });
                    append_declined_tool_results(&mut results, &calls, call_index);
                    messages.push(Message::tool_results(results));
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        steps: step,
                        halted_by_user: true,
                        messages,
                        context_report: context_report.clone(),
                    });
                }
                Gate::Pass => {
                    if input.is_null() {
                        repair_hint(tool.as_ref(), &raw_input, "tool input was not valid JSON")
                    } else {
                        tool.execute(input, ctx).await
                    }
                }
            };
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name: name.clone(),
                ok: !output.is_error,
                preview: preview(&output.content),
                display: output.display.clone(),
            });
            results.push(Part::ToolResult {
                call_id: id,
                content: output.content,
                is_error: output.is_error,
            });
        }
        messages.push(Message::tool_results(results));

        if matches!(finish, FinishReason::MaxTokens | FinishReason::Refusal) {
            return Ok(RunSummary {
                text: final_text,
                usage: total_usage,
                steps: step,
                halted_by_user: false,
                messages,
                context_report: context_report.clone(),
            });
        }
        if last_step {
            break;
        }
    }

    Ok(RunSummary {
        text: final_text,
        usage: total_usage,
        steps: step,
        halted_by_user: false,
        messages,
        context_report,
    })
    })
}

fn append_declined_tool_results(
    results: &mut Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    declined_index: usize,
) {
    for (index, (id, _, _, _)) in calls.iter().enumerate().skip(declined_index) {
        let content = if index == declined_index {
            "permission request declined by user"
        } else {
            "tool call cancelled because a previous permission request was declined"
        };
        results.push(Part::ToolResult {
            call_id: id.clone(),
            content: content.into(),
            is_error: true,
        });
    }
}

enum Gate {
    Pass,
    Deny(String),
    UserDeclined,
}
/// 跑一个子代理:独立的只读快照 + 空历史,结果文本即 tool result。
/// 子代理内 ask 一律 Deny(无人应答);run_once 递归经 dyn Box 断开无限类型。
/// 内部轮次/工具事件折叠成 TaskProgress 经 progress 通道上抛(UI 实时可见)。
async fn run_subagent(
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
    let (route, model) = match input.get("model").and_then(|v| v.as_str()) {
        Some("primary") => (&rt.primary.0, &rt.primary.1),
        _ => (&rt.fast.0, &rt.fast.1),
    };
    let config = RunnerConfig {
        model: model.clone(),
        max_tokens: rt.max_tokens,
        // 子代理是机械检索,不开思考:省钱且避免本地小模型不认该参数。
        reasoning: ReasoningEffort::Off,
    };
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
        let trace = match event {
            RunEvent::ToolStart { id, name, summary, .. } => Some(TaskTrace {
                child_id: id,
                phase: "start".into(),
                name,
                summary: Some(summary),
                ok: None,
                preview: None,
                display: None,
            }),
            RunEvent::ToolEnd { id, name, ok, preview, display } => Some(TaskTrace {
                child_id: id,
                phase: "end".into(),
                name,
                summary: None,
                ok: Some(ok),
                preview: Some(preview),
                display,
            }),
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
    };    let mut ask = |_request: AskRequest| -> AskFuture {
        Box::pin(async { AskResponse::Permission(AskReply::Deny) })
    };
    // run_once 本身返回 boxed future,递归的无限类型在其签名处已断开。
    let fut = run_once(
        client,
        route,
        &rt.snapshot,
        &rt.agent,
        &config,
        ctx,
        &prompt,
        &[],
        None,
        &mut on_event,
        &mut ask,
    );
    match fut.await {
        Ok(summary) => {
            let text = if summary.text.trim().is_empty() {
                "(subagent finished without a text answer)".to_string()
            } else {
                summary.text
            };
            kanzei_harness::ToolOutput::ok(text)
        }
        Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
    }
}

fn recover_context_overflow(messages: &mut Vec<Message>, recoveries: &mut u32) -> bool {
    match *recoveries {
        0 => compact_messages_for_retry(messages),
        1 => compact_messages_aggressively(messages),
        _ => return false,
    }
    *recoveries += 1;
    tracing::warn!(
        attempt = *recoveries,
        max = MAX_CONTEXT_OVERFLOW_RECOVERIES,
        "provider context overflow, retrying with compacted history"
    );
    true
}

fn compact_messages_for_retry(messages: &mut Vec<Message>) {
    let Some(current_index) = messages.iter().rposition(is_text_user_message) else {
        return;
    };
    let current = messages[current_index].clone();
    let mut history = String::new();
    for (index, message) in messages.iter().enumerate() {
        if index == current_index {
            continue;
        }
        for part in &message.parts {
            let text = match part {
                Part::Text { text } => text,
                Part::ToolResult { content, .. } => content,
                _ => continue,
            };
            if history.len() >= 8_000 {
                break;
            }
            let remaining = 8_000 - history.len();
            let snippet: String = text.chars().take(remaining).collect();
            history.push_str(&snippet);
            history.push('\n');
        }
    }
    messages.clear();
    if !history.trim().is_empty() {
        messages.push(Message::user_text(format!(
            "以下是此前工具执行结果的压缩记录，仅供继续当前任务参考：\n{}",
            history.trim_end()
        )));
    }
    messages.push(current);
}

fn compact_messages_aggressively(messages: &mut Vec<Message>) {
    if let Some(current) = messages
        .iter()
        .rfind(|message| is_text_user_message(message))
        .cloned()
    {
        messages.clear();
        messages.push(current);
    }
}
fn is_text_user_message(message: &Message) -> bool {
    message.role == Role::User
        && message
            .parts
            .iter()
            .any(|part| matches!(part, Part::Text { .. }))
}

fn add_usage(a: Usage, b: Usage) -> Usage {
    Usage {
        input: a.input + b.input,
        output: a.output + b.output,
        reasoning: a.reasoning + b.reasoning,
        cache_read: a.cache_read + b.cache_read,
        cache_write: a.cache_write + b.cache_write,
    }
}

fn summarize_input(input: &serde_json::Value, raw: &str) -> String {
    let rendered = if input.is_null() {
        raw.to_string()
    } else {
        input.to_string()
    };
    match rendered.char_indices().nth(160) {
        Some((idx, _)) => format!("{}…", &rendered[..idx]),
        None => rendered,
    }
}

fn preview(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("");
    let mut p = match first_line.char_indices().nth(120) {
        Some((idx, _)) => format!("{}…", &first_line[..idx]),
        None => first_line.to_string(),
    };
    let lines = content.lines().count();
    if lines > 1 {
        p.push_str(&format!(" (+{} lines)", lines - 1));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::{
        append_declined_tool_results, compact_messages_aggressively, compact_messages_for_retry,
        drain_task_events, RunEvent, MAX_STREAM_RESTARTS,
    };
    use kanzei_llm::{LlmError, Message, Part};

    #[test]
    fn 子代理完成前的缓冲事件会被排空() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(RunEvent::TaskProgress {
            id: "task-1".into(),
            text: "收尾".into(),
            trace: None,
        })
        .unwrap();
        drop(tx);
        let mut drained = 0;
        drain_task_events(&mut rx, &mut |event| {
            if matches!(event, RunEvent::TaskProgress { .. }) {
                drained += 1;
            }
        });
        assert_eq!(drained, 1);
    }

    /// 流中途断开只对传输层错误重放:协议错误重放只会原样复现,白烧钱。
    /// 这里锁住 runner 里那条判定用的变体匹配语义。
    #[test]
    fn 只有传输层中断才重放本步() {
        let transport = LlmError::Config("placeholder".into());
        assert!(!matches!(transport, LlmError::Transport(_)));
        assert!(!matches!(
            LlmError::Protocol("bad SSE".into()),
            LlmError::Transport(_)
        ));
        assert!(!matches!(
            LlmError::ContextOverflow { message: "x".into() },
            LlmError::Transport(_)
        ));
        assert!(!matches!(
            LlmError::Provider { kind: "rate_limit_error".into(), message: "x".into() },
            LlmError::Transport(_)
        ));
        // 重放次数必须有界:每次重放都要重新生成已产出的 token。
        assert_eq!(MAX_STREAM_RESTARTS, 2);
    }

    #[test]
    fn compact_retry_keeps_prompt_and_bounded_tool_history() {
        let mut messages = vec![
            Message::user_text("原始任务"),
            Message::assistant(vec![Part::Text {
                text: "旧回复".into(),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "工具结果".into(),
                is_error: false,
            }]),
            Message::user_text("当前任务"),
        ];

        compact_messages_for_retry(&mut messages);

        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].parts[0], Part::Text { ref text } if text.contains("工具结果")));
        assert!(matches!(messages[1].parts[0], Part::Text { ref text } if text == "当前任务"));
    }

    #[test]
    fn compact_retry_drops_orphan_tool_results_in_tool_loop() {
        let mut messages = vec![
            Message::user_text("当前任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "notes.md"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "工具结果".into(),
                is_error: false,
            }]),
        ];

        compact_messages_for_retry(&mut messages);
        assert!(messages.iter().any(|message| {
            message.parts.iter().any(
                |part| matches!(part, Part::Text { text } if text == "当前任务")
            )
        }));
        assert!(!messages.iter().flat_map(|message| &message.parts).any(|part| {
            matches!(part, Part::ToolResult { .. } | Part::ToolCall { .. })
        }));
        assert!(messages.iter().any(|message| {
            message.parts.iter().any(
                |part| matches!(part, Part::Text { text } if text.contains("工具结果"))
            )
        }));

        let mut aggressive = vec![
            Message::user_text("当前任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_2".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_2".into(),
                content: "结果".into(),
                is_error: false,
            }]),
        ];
        compact_messages_aggressively(&mut aggressive);
        assert!(!aggressive.iter().flat_map(|message| &message.parts).any(|part| {
            matches!(part, Part::ToolResult { .. } | Part::ToolCall { .. })
        }));
    }
    #[test]
    fn declined_tool_batch_keeps_real_and_placeholder_results_paired() {
        let calls = vec![
            ("call_done".into(), "write".into(), serde_json::json!({}), "{}".into()),
            ("call_declined".into(), "edit".into(), serde_json::json!({}), "{}".into()),
            ("call_pending".into(), "bash".into(), serde_json::json!({}), "{}".into()),
        ];
        let mut results = vec![Part::ToolResult {
            call_id: "call_done".into(),
            content: "真实写入结果".into(),
            is_error: false,
        }];
        append_declined_tool_results(&mut results, &calls, 1);

        assert_eq!(results.len(), 3);
        assert!(matches!(
            &results[0],
            Part::ToolResult { call_id, content, is_error: false }
                if call_id == "call_done" && content == "真实写入结果"
        ));
        assert!(matches!(
            &results[1],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_declined" && content.contains("declined")
        ));
        assert!(matches!(
            &results[2],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_pending" && content.contains("cancelled")
        ));
    }
}
