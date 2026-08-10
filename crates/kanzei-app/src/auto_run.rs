//! 自主推进(鞭挞)判定后端化(R-169):状态机在 `kanzei_harness::auto_run`,
//! 本模块负责桌面端的状态持有、控件同步命令与轮末判定入口——
//! 判定结果随 `kz:done` 事件带给前端,前端只执行(发下一条/NUDGE/停止显示),
//! 不再承载任何「该不该续跑」的机械判定(空转画像/连数/全部阻塞/无动作 NUDGE)。
//!
//! 设计见 docs/design/continue_prompt_dissection.md §4;状态机行为(判定顺序、
//! 七种停止场景)的权威单测在 harness 侧,本模块只是接线。

use std::path::Path;
use std::sync::Mutex;

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
    enabled: Option<bool>,
    paused: Option<bool>,
    stop_after_round: Option<bool>,
    max_rounds: Option<u32>,
) -> serde_json::Value {
    let mut ctrl = state.auto_run.lock().unwrap();
    if let Some(v) = enabled {
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
    json!({ "ok": true })
}

/// 手动发送/用户操作时归零连数(原前端 sendText 非 auto 分支)。
#[tauri::command]
pub fn auto_state_reset(state: State<'_, AppState>) -> serde_json::Value {
    state.auto_run.lock().unwrap().state.reset();
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
        AutoRunAction::NoContinue => json!({ "type": "NoContinue" }),
        AutoRunAction::Stop(reason) => {
            let (reason_str, max) = match reason {
                AutoStopReason::AllBlocked => ("AllBlocked", None),
                AutoStopReason::BacklogEmpty => ("BacklogEmpty", None),
                AutoStopReason::Paused => ("Paused", None),
                AutoStopReason::StopAfterRound => ("StopAfterRound", None),
                AutoStopReason::MaxRounds(max) => ("MaxRounds", Some(max)),
                AutoStopReason::NoAction => ("NoAction", None),
            };
            let mut v = json!({ "type": "Stop", "reason": reason_str });
            if let Some(max) = max {
                v["max"] = json!(max);
            }
            v
        }
    }
}

pub fn work_priority_enum(v: &str) -> WorkPriority {
    if v == "requirement-first" {
        WorkPriority::RequirementFirst
    } else {
        WorkPriority::DefectFirst
    }
}
