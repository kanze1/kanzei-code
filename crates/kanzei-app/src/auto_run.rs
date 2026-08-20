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

/// D-403 后续:失败轮该不该进自动重试判定,判据是**鞭挞武装了没有**,不是「已经跑了几轮」。
/// 原判据 `ctrl.state.rounds == 0` 想表达「手动单轮,用户在场,错误直接返回」,但 rounds 在
/// 任何 Stop(含 RepeatedFailure)与手动发送时都归零——于是「停摆后手动发一句『继续』」
/// 恢复的那一轮又落回 rounds==0:再断一次网就没有退避重试,链条静默断掉(正是用户
/// 2026-08-17 现场的下一步)。鞭挞开着就该重试;没开时保持原行为。
pub fn should_retry_failed_round(ctrl: &AutoRunController) -> bool {
    ctrl.enabled
}

/// 轮末判定入口:未开启时直接 NoContinue(不续跑),否则交给 harness 状态机。
/// 判定不在此处实现——它只转发,防止前端判定逻辑换个位置再长回来。
pub fn decide_auto_run(ctrl: &mut AutoRunController, ctx: AutoRunCtx) -> AutoRunAction {
    if !ctrl.enabled {
        return AutoRunAction::NoContinue;
    }
    ctrl.state.decide(&ctx)
}

// R-144 的 closed_count_this_round(扫 summary.messages 数成功 close)已在 D-654
// 移除:它扫的是全历史消息,历史轮的 close 每轮重复计入,verify_every_n 节律被
// 刷穿;且轮中上下文压缩会改写消息列表,消息口径本身不可靠。现口径为事件收口,
// 见 run/events/mod.rs MetricsSink(ToolStart 登记 close 意图,ToolEnd ok 计数)。

/// backlog 判定(R-169):实现已下沉 kanzei-tools::tracker::backlog_status,
/// 桌面端与 CLI 共用同一实现(D-229 架构债消除)。此处只做转发,不留第二份逻辑。
pub fn backlog_status(project_root: &Path) -> BacklogStatus {
    kanzei_tools::tracker::backlog_status(project_root)
}

/// D-583 验收①后半:「点名阻塞清单」——熔断多半发生在活动条目卡在外部阻塞
/// (权限环境/待用户决策/真实冲突)时,诊断必须直接列出卡住的条目与阻塞原文,
/// 不能让人再去翻 defects.md/requirements.md 逐条找。只扫 WIP(doing/fixing)
/// 且「阻塞」字段非空的条目——已经 todo/open 的排队条目不算「正在卡住」。
pub fn blocked_wip_summary(project_root: &Path) -> Vec<String> {
    use kanzei_tools::docstore::{DocStore, DEFECTS, REQUIREMENTS};
    let mut out = Vec::new();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let Ok(entries) = DocStore::open(project_root, kind).load() else {
            continue;
        };
        for entry in entries {
            if !matches!(entry.status.as_str(), "doing" | "fixing") {
                continue;
            }
            let blocked = entry
                .fields
                .iter()
                .find(|(key, _)| key == "阻塞")
                .map(|(_, value)| value.trim())
                .unwrap_or_default();
            if !blocked.is_empty() {
                out.push(format!("{}: {blocked}", entry.id));
            }
        }
    }
    out
}

/// D-583:熔断事件留痕——单纯的一次性 kz:done UI 事件会随会话滚动沉底,人工事后
/// 想查「这个会话到底哪一轮触发过熔断」找不到独立证据。追加写一条 JSONL 到项目级
/// 审计文件,与 D-566 的 cleanup-log.jsonl、D-567 的连续失败持久化同一手法:
/// 只增不改,坏一行不影响其余行,不需要额外的读—改—写并发保护。
pub fn record_zero_output_alert(project_root: &Path, session_id: &str, progress_signature: &str) {
    let path = project_root.join(".kanzei/project/auto-run-alerts.jsonl");
    let recorded_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let line = json!({
        "type": "zero_output_circuit_breaker",
        "session_id": session_id,
        "rounds": kanzei_harness::auto_run::ZERO_OUTPUT_ROUND_LIMIT,
        "progress_signature": progress_signature,
        "blocked": blocked_wip_summary(project_root),
        "recorded_at_ms": recorded_at_ms,
    });
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// D-583:本轮「真实进展」签名——(HEAD、代码 worktree hash、tracker 文档内容)三者
/// 拼在一起哈希。与 has_progress_tools(工具名画像)互补:那个防「整轮不调用任何
/// 工具」,这个防「调用了 bash/read 等工具但状态什么也没真的变」(复诵证据清单、
/// 反复读同一批文件这类看起来在干活、实际空转的场景——R-306 现场空转 10 轮
/// 才被人工发现)。
///
/// `repo_observation` 的 (head, worktree_hash) 刻意排除 `.kanzei/**`(它是给证据锚
/// 新鲜度用的,tracker 独立提交不该让代码证据变 stale),所以这里再显式并入
/// defects.md/requirements.md 的原始字节,三段一起才覆盖「文件改动/提交/tracker
/// 实质字段变化」三个验收点。哈希只用于本进程内逐轮比较,不落盘不跨进程,
/// 用标准库 DefaultHasher 足够,不必对齐 work.rs 的 fnv1a64 格式。
pub fn progress_signature(project_root: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let observation = kanzei_tools::work::repo_observation(project_root);
    let mut hasher = DefaultHasher::new();
    observation.observed_head.hash(&mut hasher);
    observation.observed_worktree_hash.hash(&mut hasher);
    for rel in [
        ".kanzei/project/defects.md",
        ".kanzei/project/requirements.md",
    ] {
        std::fs::read(project_root.join(rel))
            .unwrap_or_default()
            .hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
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
                // D-583:连续 N 轮真实进展签名未变(max 槽复用为触发轮数,与
                // RepeatedFailure 同口径)。
                AutoStopReason::ZeroOutput(n) => ("ZeroOutput", Some(n)),
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

/// 2026-08-20 现场:本地 ollama 兼容端点把"请求体没有 user 消息"这种确定性
/// 客户端错误也包成了 HTTP 500(标准做法应是 4xx)。这类错误的请求体内容不会
/// 因为等待重试而改变,原样重试只会原样再失败一次——判成瞬态纯粹是白烧一轮
/// 退避等待外加一次注定失败的调用。只精确识别这一个已验证的死循环特征,不做
/// 通用的"猜错误语义"规则(会引入误判真实瞬态 500 的风险)。
fn is_malformed_request_body(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("no user query") || lower.contains("no user message")
}

pub fn is_transient_run_error(error: &anyhow::Error) -> bool {
    let Some(llm) = llm_error_in(error) else {
        return false;
    };
    match llm {
        kanzei_llm::LlmError::RateLimited { .. } | kanzei_llm::LlmError::Transport(_) => true,
        kanzei_llm::LlmError::Http { status, body } => {
            matches!(status, 500 | 502 | 503 | 504 | 529) && !is_malformed_request_body(body)
        }
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

    fn temp_git_repo(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-d583-sig-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(&dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Kanzei Test"]);
        std::fs::write(dir.join("source.txt"), "source").unwrap();
        std::fs::write(dir.join(".kanzei/project/defects.md"), "# Defects\n").unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n",
        )
        .unwrap();
        git(&["add", "-A"]);
        git(&["commit", "--quiet", "-m", "init"]);
        dir
    }

    /// D-583:真实进展签名对三类真实动作都要敏感——①代码文件改动;②commit;
    /// ③tracker 文档内容改动(哪怕还没 commit)。`repo_observation` 本身显式排除
    /// `.kanzei/**`(避免 tracker 独立提交让证据锚变 stale),所以③必须靠
    /// progress_signature 自己并入 defects.md/requirements.md 字节才覆盖得到——
    /// 这条测试专门守住这一点,防止未来重构把这段拼接删掉。
    #[test]
    fn 真实进展签名对代码改动提交与tracker文档改动均敏感() {
        let dir = temp_git_repo("basic");
        let baseline = super::progress_signature(&dir);
        assert_eq!(
            baseline,
            super::progress_signature(&dir),
            "什么都没变,签名必须稳定复现"
        );

        // ①未提交的代码改动应改变签名(repo_observation 的 worktree_hash 会变)。
        std::fs::write(dir.join("source.txt"), "changed").unwrap();
        let after_code_edit = super::progress_signature(&dir);
        assert_ne!(baseline, after_code_edit, "代码文件改动后签名必须变化");

        // ②commit 应再次改变签名(HEAD 变化)。
        std::process::Command::new("git")
            .current_dir(&dir)
            .args(["commit", "-aqm", "code change"])
            .status()
            .unwrap();
        let after_commit = super::progress_signature(&dir);
        assert_ne!(after_code_edit, after_commit, "提交后签名必须再次变化");

        // ③只改 tracker 文档(不提交)也必须改变签名——repo_observation 本身对
        // `.kanzei/**` 视而不见,这一步专门验证 progress_signature 自己补上了这块。
        std::fs::write(
            dir.join(".kanzei/project/defects.md"),
            "# Defects\n\n## D-1 test\n",
        )
        .unwrap();
        let after_tracker_edit = super::progress_signature(&dir);
        assert_ne!(
            after_commit, after_tracker_edit,
            "tracker 文档改动未被 repo_observation 捕捉,必须靠本函数自己的拼接补上"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-583 验收①后半:「点名阻塞清单」只收 WIP(doing/fixing)且「阻塞」字段
    /// 非空的条目——排队中的 todo/open 与阻塞字段空着的活动条目都不算「正在卡住」。
    #[test]
    fn 阻塞清单只收wip且阻塞字段非空的条目() {
        let dir = temp_git_repo("blocked");
        std::fs::write(
            dir.join(".kanzei/project/defects.md"),
            "# Defects\n\n\
             ## D-001 卡住的缺陷 [fixing] (medium)\n\
             - 阻塞: 等用户确认线上环境\n\n\
             ## D-002 正常推进的缺陷 [fixing] (medium)\n\
             - 阻塞: \n\n\
             ## D-003 排队中的缺陷 [open] (low)\n\
             - 阻塞: 这条不该出现,状态不是 wip\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n\
             ## R-001 卡住的需求 [doing]\n\
             - 阻塞: 等 review\n",
        )
        .unwrap();

        let summary = super::blocked_wip_summary(&dir);
        assert_eq!(summary.len(), 2, "{summary:?}");
        assert!(summary
            .iter()
            .any(|s| s.starts_with("D-001: 等用户确认线上环境")));
        assert!(summary.iter().any(|s| s.starts_with("R-001: 等 review")));
        assert!(!summary.iter().any(|s| s.starts_with("D-002")));
        assert!(!summary.iter().any(|s| s.starts_with("D-003")));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-583 验收③:熔断事件必须留痕可审计——追加进 auto-run-alerts.jsonl,
    /// 每行独立(只增不改),多次触发各自成行不覆盖,且带上阻塞清单(验收①)。
    #[test]
    fn 熔断事件写入审计文件_多次触发各自成行() {
        let dir = temp_git_repo("alert");
        let alert_path = dir.join(".kanzei/project/auto-run-alerts.jsonl");
        assert!(!alert_path.exists(), "前置:审计文件尚不存在");

        super::record_zero_output_alert(&dir, "session-1", "sig-a");
        let content = std::fs::read_to_string(&alert_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["type"], "zero_output_circuit_breaker");
        assert_eq!(parsed["session_id"], "session-1");
        assert_eq!(parsed["progress_signature"], "sig-a");
        assert_eq!(
            parsed["rounds"],
            kanzei_harness::auto_run::ZERO_OUTPUT_ROUND_LIMIT
        );
        assert!(parsed["blocked"].is_array(), "{parsed}");

        super::record_zero_output_alert(&dir, "session-2", "sig-b");
        let content = std::fs::read_to_string(&alert_path).unwrap();
        assert_eq!(content.lines().count(), 2, "第二次触发追加而非覆盖");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-403 后续:失败轮进不进重试判定只看鞭挞是否武装。第一轮(rounds==0)断网、
    /// 以及「停摆后手动发继续」恢复的那一轮(同样 rounds==0)都必须能退避重试。
    #[test]
    fn 失败轮是否重试_只看鞭挞武装_与已跑轮数无关() {
        let mut ctrl = AutoRunController {
            enabled: true,
            ..Default::default()
        };
        assert!(
            super::should_retry_failed_round(&ctrl),
            "鞭挞开着、第一轮就断网也必须走退避重试(手动发继续恢复的那轮同样是 rounds==0)"
        );
        ctrl.state.rounds = 5;
        assert!(super::should_retry_failed_round(&ctrl));
        ctrl.enabled = false;
        assert!(
            !super::should_retry_failed_round(&ctrl),
            "没开鞭挞:手动单轮保持原行为,错误直接返回"
        );
        ctrl.state.rounds = 0;
        assert!(!super::should_retry_failed_round(&ctrl));
    }

    /// 闸门放行后,第一轮的瞬态失败必须得到 RetryAfterFailure{attempt:1}(而不是无声无息)。
    #[test]
    fn 武装后第一轮瞬态失败_得到退避重试而非静默() {
        let mut ctrl = AutoRunController {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(ctrl.state.rounds, 0, "前置:第一轮尚未计数");
        let ctx = kanzei_harness::auto_run::AutoRunCtx {
            backlog: kanzei_harness::auto_run::BacklogStatus::Workable,
            halted: false,
            steps: 0,
            tools: &[],
            auto_allowed: true,
            closed_this_round: 0,
            verify_every_n: 0,
            round_failure: Some(kanzei_harness::auto_run::RoundFailure::Transient),
            progress_signature: "",
        };
        assert_eq!(
            super::decide_auto_run(&mut ctrl, ctx),
            kanzei_harness::auto_run::AutoRunAction::RetryAfterFailure { attempt: 1 }
        );
    }

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

    /// 真实服务端过载(无请求体格式线索)仍按原规则判瞬态,不能被新判据误伤。
    #[test]
    fn 无请求体线索的http_500仍判瞬态() {
        let error = anyhow::Error::new(kanzei_llm::LlmError::Http {
            status: 500,
            body: "internal server error".into(),
        })
        .context("run failed");
        assert!(super::is_transient_run_error(&error));
    }

    /// 2026-08-20 现场:本地 ollama 兼容端点把"请求体没有 user 消息"这种确定性
    /// 客户端错误包成 HTTP 500。这类错误重试注定原样失败,不该判瞬态——否则
    /// 鞭挞会白烧一轮退避等待外加一次注定失败的调用才升级为致命。
    #[test]
    fn 请求体缺user消息的http_500判为致命不重试() {
        let error = anyhow::Error::new(kanzei_llm::LlmError::Http {
            status: 500,
            body: r#"{"error":{"message":"no user query found in messages","type":"api_error","param":null,"code":null}}"#.into(),
        })
        .context("run failed");
        assert!(
            !super::is_transient_run_error(&error),
            "确定性请求体错误不该被当瞬态重试"
        );
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
