//! 自主推进(鞭挞)判定后端化(R-169):状态机在 `kanzei_harness::auto_run`,
//! 本模块负责桌面端的状态持有、控件同步命令与轮末判定入口——
//! 判定结果随 `kz:done` 事件带给前端,前端只执行(发下一条/NUDGE/停止显示),
//! 不再承载任何「该不该续跑」的机械判定(空转画像/连数/全部阻塞/无动作 NUDGE)。
//!
//! 设计见 docs/design/continue_prompt_dissection.md §4;状态机行为(判定顺序、
//! 七种停止场景)的权威单测在 harness 侧,本模块只是接线。

use std::path::Path;

use kanzei_harness::auto_run::{
    nudge_prompt, AutoRunAction, AutoRunCtx, AutoRunState, AutoStopReason, BacklogStatus,
    WorkPriority,
};
use serde_json::json;
use tauri::State;

use crate::AppState;

/// 桌面端自主推进状态:控件输入(开关/暂停/本轮后停/上限)由前端经
/// `auto_state_update` 同步,轮末判定由 run.rs 调用 `decide_auto_run`。
#[derive(Default)]
pub(crate) struct AutoRunController {
    pub(crate) state: AutoRunState,
    pub(crate) enabled: bool,
}

/// 前端控件变化同步(开关/暂停/本轮后停/连数上限)。
#[tauri::command]
pub fn auto_state_update(
    state: State<'_, AppState>,
    session_id: String,
    enabled: Option<bool>,
    paused: Option<bool>,
    stop_after_round: Option<bool>,
    max_rounds: Option<u32>,
) -> serde_json::Value {
    let mut controllers = state.auto_runs.lock().unwrap();
    let ctrl = controllers.entry(session_id).or_default();
    apply_state_update(ctrl, enabled, paused, stop_after_round, max_rounds);
    json!({ "ok": true })
}

fn apply_state_update(
    ctrl: &mut AutoRunController,
    enabled: Option<bool>,
    paused: Option<bool>,
    stop_after_round: Option<bool>,
    max_rounds: Option<u32>,
) {
    if let Some(v) = enabled {
        if ctrl.enabled != v {
            // 开关重开应重新计数；否则 UI 已清零而引擎仍沿用旧轮数，会立刻误触上限。
            ctrl.state.reset();
        }
        ctrl.enabled = v;
    }
    if let Some(v) = paused {
        ctrl.state.paused = v;
    }
    if let Some(v) = stop_after_round {
        ctrl.state.stop_after_round = v;
    }
    if let Some(v) = max_rounds {
        ctrl.state.max_rounds = v.clamp(1, 100);
    }
}

/// 手动发送/用户操作时归零连数(原前端 sendText 非 auto 分支)。
#[tauri::command]
pub fn auto_state_reset(state: State<'_, AppState>, session_id: String) -> serde_json::Value {
    state
        .auto_runs
        .lock()
        .unwrap()
        .entry(session_id)
        .or_default()
        .state
        .reset();
    json!({ "ok": true })
}

/// 轮末判定入口:未开启时直接 NoContinue(不续跑),否则交给 harness 状态机。
/// 判定不在此处实现——它只转发,防止前端判定逻辑换个位置再长回来。
pub fn decide_auto_run(ctrl: &mut AutoRunController, ctx: AutoRunCtx) -> AutoRunAction {
    if !ctrl.enabled {
        return AutoRunAction::NoContinue;
    }
    ctrl.state.decide(&ctx)
}

/// R-144:统计本轮实际**成功关闭**的条目数。只数 req/defect 工具调用中
/// action=close 且对应 ToolResult 非 error 的——「调用了 close」不等于「关闭成功」,
/// 被门禁拦下的 close(R-228/R-229 等)不算数,否则核查节律会被失败调用刷阈值。
/// 配对方式:先收集 close 调用的 call_id,遇到对应 ToolResult 且 !is_error 时 +1。
pub fn closed_count_this_round(summary: &kanzei_core::RunSummary) -> u32 {
    use kanzei_llm::Part;
    use std::collections::BTreeSet;
    let mut close_ids: BTreeSet<String> = BTreeSet::new();
    let mut done: BTreeSet<String> = BTreeSet::new();
    let mut count = 0u32;
    for message in &summary.messages {
        for part in &message.parts {
            match part {
                Part::ToolCall { id, name, input } => {
                    let is_close = matches!(name.as_str(), "req" | "defect")
                        && input.get("action").and_then(serde_json::Value::as_str) == Some("close");
                    if is_close {
                        close_ids.insert(id.clone());
                    }
                }
                Part::ToolResult {
                    call_id, is_error, ..
                } if close_ids.contains(call_id) && !done.contains(call_id) => {
                    done.insert(call_id.clone());
                    count += u32::from(!is_error);
                }
                _ => {}
            }
        }
    }
    count
}

/// backlog 判定(R-169):实现已下沉 kanzei-tools::tracker::backlog_status,
/// 桌面端与 CLI 共用同一实现(D-229 架构债消除)。此处只做转发,不留第二份逻辑。
pub fn backlog_status(project_root: &Path) -> BacklogStatus {
    kanzei_tools::tracker::backlog_status(project_root)
}

/// 判定结果序列化给前端:`{"type":"Continue"|"Nudge"|"NoContinue"|"Stop","prompt":...}`。
/// Nudge 文案由引擎生成(nudge_prompt),前端不持模板。
pub fn serialize_action(action: AutoRunAction, work_priority: WorkPriority) -> serde_json::Value {
    match action {
        AutoRunAction::Continue => json!({ "type": "Continue" }),
        AutoRunAction::Nudge => json!({ "type": "Nudge", "prompt": nudge_prompt(work_priority) }),
        // R-144:核查轮——前端收到后把核查指令作为下一轮输入发回(与 Nudge 同款
        // 机制),主代理用只读 task 子代理(read/glob/grep)核对最近关闭条目的
        // 验收证据与真实调用方;发现问题生成候选缺陷或退回依据。核查指令由引擎
        // 生成(harness verify_prompt),前端不持模板——与 nudge_prompt 同一哲学。
        AutoRunAction::VerifyRound => json!({
            "type": "VerifyRound",
            "prompt": kanzei_harness::auto_run::verify_prompt(),
        }),
        AutoRunAction::NoContinue => json!({ "type": "NoContinue" }),
        // D-403:瞬态失败退避重试——退避时长在此换算(attempt1=15s,attempt2=30s,
        // 封顶 60s),引擎不持时钟;前端只按 delayMs 定时,不再造第二套退避表。
        AutoRunAction::RetryAfterFailure { attempt } => json!({
            "type": "RetryAfterFailure",
            "attempt": attempt,
            "maxAttempts": kanzei_harness::auto_run::MAX_FAILED_ROUNDS,
            "delayMs": (15000u64 << attempt.saturating_sub(1).min(2)).min(60000),
        }),
        AutoRunAction::Stop(reason) => {
            let (reason_str, max) = match reason {
                AutoStopReason::AllBlocked => ("AllBlocked", None),
                AutoStopReason::BacklogEmpty => ("BacklogEmpty", None),
                AutoStopReason::Paused => ("Paused", None),
                AutoStopReason::StopAfterRound => ("StopAfterRound", None),
                AutoStopReason::MaxRounds(max) => ("MaxRounds", Some(max)),
                AutoStopReason::NoAction => ("NoAction", None),
                AutoStopReason::ProfileMismatch => ("ProfileMismatch", None),
                // D-403:连续瞬态失败停摆(max 槽复用为失败次数)/致命错误立即停。
                AutoStopReason::RepeatedFailure(n) => ("RepeatedFailure", Some(n)),
                AutoStopReason::FatalError => ("FatalError", None),
                AutoStopReason::RateLimited => ("RateLimited", None),
            };
            let mut v = json!({ "type": "Stop", "reason": reason_str });
            if let Some(max) = max {
                v["max"] = json!(max);
            }
            v
        }
    }
}

/// D-403:失败轮的瞬态/致命分类——按 anyhow 链里的 LlmError 变体判定。
/// 瞬态 = 限流/过载(RateLimited)、传输中断(Transport)、服务端 5xx;
/// 致命 = 认证/参数 4xx、配置/协议错、上下文溢出(压缩轨道已尽力,重试同样溢出)、
/// 以及非 LlmError 的失败(内部错误,重试只会原样复现)。
fn llm_error_in(error: &anyhow::Error) -> Option<&kanzei_llm::LlmError> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<kanzei_llm::LlmError>())
}

/// 判断错误链中是否有 provider 限流错误。`run_task` 会给底层错误增加上下文，
/// 不能只检查 anyhow 最外层。
pub fn is_rate_limited_run_error(error: &anyhow::Error) -> bool {
    llm_error_in(error).is_some_and(kanzei_llm::LlmError::is_rate_limited)
}

pub fn is_transient_run_error(error: &anyhow::Error) -> bool {
    let Some(llm) = llm_error_in(error) else {
        return false;
    };
    match llm {
        kanzei_llm::LlmError::RateLimited { .. } | kanzei_llm::LlmError::Transport(_) => true,
        kanzei_llm::LlmError::Http { status, .. } => matches!(status, 500 | 502 | 503 | 504 | 529),
        _ => false,
    }
}

pub fn work_priority_enum(v: &str) -> WorkPriority {
    if v == "requirement-first" {
        WorkPriority::RequirementFirst
    } else {
        WorkPriority::DefectFirst
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_state_update, AutoRunController};

    #[test]
    fn 限流错误链会被识别为自动推进的立即停止() {
        let error = anyhow::Error::new(kanzei_llm::LlmError::RateLimited {
            status: 429,
            kind: Some("rate_limit_error".into()),
            body: "slow down".into(),
            retry_after: Some(30),
        })
        .context("run failed");

        assert!(super::is_rate_limited_run_error(&error));
        assert!(super::is_transient_run_error(&error));
    }

    #[test]
    fn 开关切换会清空本会话旧轮数_上限保持边界约束() {
        let mut ctrl = AutoRunController {
            enabled: true,
            ..Default::default()
        };
        ctrl.state.rounds = 6;

        apply_state_update(&mut ctrl, Some(false), None, None, Some(0));
        assert!(!ctrl.enabled);
        assert_eq!(ctrl.state.rounds, 0);
        assert_eq!(ctrl.state.max_rounds, 1);

        ctrl.state.rounds = 3;
        apply_state_update(&mut ctrl, Some(true), None, None, Some(500));
        assert!(ctrl.enabled);
        assert_eq!(ctrl.state.rounds, 0);
        assert_eq!(ctrl.state.max_rounds, 100);
    }

    #[test]
    fn 不同控制器的轮数天然隔离() {
        let mut first = AutoRunController::default();
        let mut second = AutoRunController::default();
        first.state.rounds = 4;

        apply_state_update(&mut second, Some(true), None, None, Some(2));
        assert_eq!(first.state.rounds, 4);
        assert_eq!(second.state.rounds, 0);
        assert_eq!(second.state.max_rounds, 2);
    }

    /// R-144:closed_count_this_round 只数「action=close 且结果非 error」的 req/defect
    /// 调用——被门禁拦下的 close 不算(否则核查节律被失败调用刷阈值)。
    #[test]
    fn closed计数_只数成功的close调用_失败与其它工具不计() {
        use kanzei_llm::{Message, Part};
        let msg = |parts: Vec<Part>| Message {
            role: kanzei_llm::Role::User,
            parts,
        };
        let summary = kanzei_core::RunSummary {
            text: String::new(),
            usage: kanzei_llm::Usage::default(),
            last_input_tokens: None,
            steps: 4,
            halted_by_user: false,
            messages: vec![
                msg(vec![Part::ToolCall {
                    id: "c1".into(),
                    name: "req".into(),
                    input: serde_json::json!({"action": "close", "id": "R-001"}),
                }]),
                msg(vec![Part::ToolResult {
                    call_id: "c1".into(),
                    content: "closed".into(),
                    is_error: false,
                }]),
                msg(vec![Part::ToolCall {
                    id: "c2".into(),
                    name: "defect".into(),
                    input: serde_json::json!({"action": "close", "id": "D-001"}),
                }]),
                msg(vec![Part::ToolResult {
                    call_id: "c2".into(),
                    content: "门禁拒绝".into(),
                    is_error: true,
                }]),
                msg(vec![Part::ToolCall {
                    id: "c3".into(),
                    name: "req".into(),
                    input: serde_json::json!({"action": "update", "id": "R-002"}),
                }]),
                msg(vec![Part::ToolResult {
                    call_id: "c3".into(),
                    content: "updated".into(),
                    is_error: false,
                }]),
            ],
            context_report: vec![],
            overflow_traces: vec![],
        };
        // c1 成功 close → 计 1;c2 close 失败 → 不计;c3 是 update → 不计。
        assert_eq!(super::closed_count_this_round(&summary), 1);
    }

    /// R-144:VerifyRound 序列化必须携带引擎生成的核查指令 prompt(前端据此发回
    /// 核查轮输入),不能是空壳。
    #[test]
    fn verifyround序列化_携带核查指令prompt() {
        let v = super::serialize_action(
            kanzei_harness::auto_run::AutoRunAction::VerifyRound,
            super::WorkPriority::DefectFirst,
        );
        assert_eq!(v["type"], "VerifyRound");
        let prompt = v["prompt"].as_str().unwrap_or("");
        assert!(prompt.contains("验收核查"), "{prompt}");
        assert!(prompt.contains("只读"), "{prompt}");
    }
}
