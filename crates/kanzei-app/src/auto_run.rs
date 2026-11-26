//! 自主推进(鞭挞)判定后端化(R-169):状态机在 `kanzei_harness::auto_run`,
//! 本模块负责桌面端的状态持有、控件同步命令与轮末判定入口——
//! 判定结果随 `kz:done` 事件带给前端,前端只执行(发下一条/NUDGE/停止显示),
//! 不再承载任何「该不该续跑」的机械判定(空转画像/连数/全部阻塞/无动作 NUDGE)。
//!
//! 设计见 docs/design/continue_prompt_dissection.md §4;状态机行为(判定顺序、
//! 七种停止场景)的权威单测在 harness 侧,本模块只是接线。

use std::path::Path;

use kanzei_harness::auto_run::{
    nudge_prompt, AutoRunAction, AutoRunCtx, AutoRunState, AutoStopReason,
};
use serde_json::json;
use tauri::State;

use crate::AppState;

/// R-322:agent 名 → 门禁强度默认值。
///
/// 只有 `dev-auto`(自主开发档)按 `Autonomous` 跑全套门禁;其余一律 `Paired`。
/// 判据是**有没有人在场**,不是能力大小:结伴开发、研究、只读分析都在用户
/// 盯着屏幕的情况下进行,Nudge / 验收核查轮插进来只是噪音。
///
/// 这是**默认值**,不是终值。用户覆盖(界面控件)接进来后,调用方应当先取用户
/// 显式选择,再回落本函数——所以这里不做任何缓存,也不读配置。
pub(crate) fn intensity_for_agent(agent: &str) -> kanzei_harness::HarnessIntensity {
    match agent {
        kanzei_harness::DEV_AUTO_AGENT => kanzei_harness::HarnessIntensity::Autonomous,
        _ => kanzei_harness::HarnessIntensity::Paired,
    }
}

/// 桌面端自主推进状态:控件输入(开关/暂停/本轮后停)由前端经
/// `auto_state_update` 同步,轮末判定由 run.rs 调用 `decide_auto_run`。
#[derive(Default)]
pub(crate) struct AutoRunController {
    pub(crate) state: AutoRunState,
    pub(crate) enabled: bool,
    /// R-322 B3:用户给定的停止条件(Claude Code `/goal` 的形状)。
    ///
    /// 挂上之后 loop 的停止规则整体换掉——不再由引擎猜「还有没有活干」,
    /// 而是**条件由用户给、达成与否由模型判**(经 `work handoff` 声明),
    /// 引擎只负责在达成前不让它散场,并在连续推不动时兜底。
    ///
    /// 达成(GoalMet)或判定不可达(GoalUnreachable)后**自动清除**:目标是
    /// 一次性意图,留着会在下一段无关对话里继续生效(D-111 同型教训)。
    pub(crate) goal: Option<String>,
}

/// 目标文本上限。超长条件既没法让模型稳定判定,也会每轮重发挤占上下文。
pub(crate) const MAX_GOAL_CHARS: usize = 500;

#[tauri::command]
pub fn auto_state_get(state: State<'_, AppState>, session_id: String) -> serde_json::Value {
    let mut controllers = state.auto_runs.lock().unwrap();
    let ctrl = controllers
        .entry(session_id.clone())
        .or_insert_with(|| AutoRunController {
            goal: crate::prefs::load_prefs()
                .conversation_goals
                .get(&session_id)
                .cloned(),
            ..Default::default()
        });
    json!({"enabled":ctrl.enabled, "paused":ctrl.state.paused, "goal":ctrl.goal})
}

pub(crate) fn persist_goal(session_id: &str, goal: Option<&str>) -> Result<(), String> {
    let _guard = crate::prefs::write_guard()?;
    let mut prefs = crate::prefs::load_prefs_for_write()?;
    if prefs.conversation_goals.get(session_id).map(String::as_str) == goal {
        return Ok(());
    }
    match goal {
        Some(text) => {
            prefs
                .conversation_goals
                .insert(session_id.to_string(), text.to_string());
        }
        None => {
            prefs.conversation_goals.remove(session_id);
        }
    }
    crate::prefs::save_prefs(&prefs)
}

/// 前端状态同步(开关/暂停/本轮后停)。
#[tauri::command]
pub fn auto_state_update(
    state: State<'_, AppState>,
    session_id: String,
    enabled: Option<bool>,
    paused: Option<bool>,
    stop_after_round: Option<bool>,
    goal: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut controllers = state.auto_runs.lock().unwrap();
    let ctrl = controllers.entry(session_id.clone()).or_default();
    // 空串 = 清除目标(前端清空输入框即撤销),非空则截断后挂上。
    if let Some(text) = goal {
        let goal = normalize_goal(&text);
        persist_goal(&session_id, goal.as_deref())?;
        ctrl.goal = goal;
    }
    apply_state_update(ctrl, enabled, paused, stop_after_round);
    Ok(json!({ "ok": true, "goal": ctrl.goal }))
}

/// 目标文本归一:去空白、空串视为「无目标」、超长截断。
pub(crate) fn normalize_goal(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_GOAL_CHARS).collect())
}

fn apply_state_update(
    ctrl: &mut AutoRunController,
    enabled: Option<bool>,
    paused: Option<bool>,
    stop_after_round: Option<bool>,
) {
    if let Some(v) = enabled {
        if !ctrl.enabled && v {
            // 开关重开应重新计数；否则 UI 已清零而引擎仍沿用旧轮数，状态投影会漂移。
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

/// D-583:熔断事件留痕——追加写一条 JSONL 到项目级审计文件。
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

/// D-583:本轮「真实进展」签名——对 Git 仓库取 HEAD + status + diff 内容哈希，
/// 非 Git 项目取项目树指纹，用于检测连续多轮无真实产出的空转。
pub fn progress_signature(project_root: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    if kanzei_tools::project_state::git_state_of(project_root).is_repo() {
        let git_out = |args: &[&str]| -> Vec<u8> {
            crate::state::hidden_command("git")
                .args(args)
                .current_dir(project_root)
                .output()
                .map(|o| o.stdout)
                .unwrap_or_default()
        };
        git_out(&["rev-parse", "HEAD"]).hash(&mut hasher);
        let status = git_out(&["status", "--porcelain"]);
        status.hash(&mut hasher);
        git_out(&["diff", "--no-ext-diff"]).hash(&mut hasher);
        git_out(&["diff", "--cached", "--no-ext-diff"]).hash(&mut hasher);
        // 未跟踪文件内容变更也要能被签名感知。
        for line in String::from_utf8_lossy(&status).lines() {
            if let Some(rel) = line.strip_prefix("?? ") {
                if let Ok(meta) = std::fs::metadata(project_root.join(rel.trim())) {
                    meta.len().hash(&mut hasher);
                    if let Ok(mtime) = meta.modified() {
                        mtime
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_nanos())
                            .hash(&mut hasher);
                    }
                }
            }
        }
    } else {
        kanzei_tools::project_state::tree_fingerprint(project_root).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// 只有交互模式的问题能进入等待用户状态。自动模式不因散文问号中断。
pub(crate) fn awaiting_user_for_round(
    missing_fact: bool,
    last_text: Option<&str>,
    autonomous: bool,
) -> bool {
    !autonomous && (missing_fact || last_text.is_some_and(ends_with_user_question))
}

pub fn ends_with_user_question(text: &str) -> bool {
    let tail = last_paragraph(&strip_fenced_code(text));
    let trimmed = tail
        .trim_end_matches(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '*' | '_' | '`' | '"' | '\'' | '”' | '’' | '」' | '』' | ')' | '）' | ']'
                )
        })
        .to_string();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    // 最后一句(按句末标点切)。
    let last_sentence = {
        let body = trimmed.trim_end_matches(['？', '?', '。', '.', '！', '!']);
        let cut = body
            .rfind(['。', '！', '!', '？', '?', '\n'])
            .map(|at| at + body[at..].chars().next().map_or(1, char::len_utf8))
            .unwrap_or(0);
        trimmed[cut..].trim().to_lowercase()
    };
    const OFFERS: &[&str] = &[
        "如有",
        "如果有问题",
        "有问题",
        "随时",
        "如需",
        "需要的话",
        "如果需要",
        "如果你需要",
        "feel free",
        "if you have",
        "if you need",
        "anytime",
        "happy to",
    ];
    if OFFERS.iter().any(|offer| last_sentence.contains(offer)) {
        return false;
    }
    if trimmed.ends_with('？') || trimmed.ends_with('?') {
        return true;
    }
    const REQUESTS: &[&str] = &[
        "请提供",
        "请确认",
        "请选择",
        "请告诉我",
        "请回复",
        "请决定",
        "请指定",
        "请补充",
        "请授权",
        "请你提供",
        "请你确认",
        "请你选择",
        "请您提供",
        "请您确认",
        "请您选择",
        "需要你提供",
        "需要你确认",
        "需要你决定",
        "需要你选择",
        "需要你授权",
        "需要您确认",
        "等你确认",
        "等你决定",
        "please provide",
        "please confirm",
        "please choose",
        "please select",
        "please let me know",
        "please tell me",
        "please reply",
        "please decide",
        "please advise",
        "let me know which",
        "let me know whether",
        "waiting for your",
    ];
    REQUESTS
        .iter()
        .any(|request| last_sentence.contains(request))
        || (REQUESTS.iter().any(|request| lower.contains(request)) && tail.lines().count() <= 3)
}

/// 本轮**最后一条**助手消息的正文(多段文本拼接)。那条消息带工具调用(轮中旁白,如「先确认 X
/// 是否存在?」+ 调工具)或没有正文(只有推理)时返回 None——不往前找更早的旁白:那不是收尾的话,
/// 拿来判「在问用户」会误停(复核 minor)。
pub fn last_assistant_text(messages: &[kanzei_llm::Message]) -> Option<String> {
    let message = messages
        .iter()
        .rev()
        .find(|message| message.role == kanzei_llm::Role::Assistant)?;
    if message
        .parts
        .iter()
        .any(|part| matches!(part, kanzei_llm::Part::ToolCall { .. }))
    {
        return None;
    }
    let text: Vec<&str> = message
        .parts
        .iter()
        .filter_map(|part| match part {
            kanzei_llm::Part::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
            _ => None,
        })
        .collect();
    (!text.is_empty()).then(|| text.join("\n\n"))
}

fn strip_fenced_code(text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn last_paragraph(text: &str) -> String {
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .collect();
    let Some(last) = paragraphs.last() else {
        return String::new();
    };
    let is_list = last.lines().all(|line| {
        let line = line.trim_start();
        line.starts_with("- ")
            || line.starts_with("* ")
            || line.split_once(['.', '、', ')']).is_some_and(|(head, _)| {
                !head.is_empty() && head.chars().all(|c| c.is_ascii_digit())
            })
    });
    if is_list && paragraphs.len() >= 2 {
        // 「你想选哪种?」+ 空行 + 选项列表:问题在列表前一段。
        return paragraphs[paragraphs.len() - 2].to_string();
    }
    last.to_string()
}

/// 判定结果序列化给前端:`{"type":"Continue"|"Nudge"|"NoContinue"|"Stop","prompt":...}`。
pub fn serialize_action(action: AutoRunAction) -> serde_json::Value {
    match action {
        AutoRunAction::Continue => json!({ "type": "Continue" }),
        AutoRunAction::Nudge => json!({ "type": "Nudge", "prompt": nudge_prompt() }),
        AutoRunAction::NoContinue => json!({ "type": "NoContinue" }),
        AutoRunAction::GoalPending => json!({ "type": "GoalPending" }),
        AutoRunAction::RetryAfterFailure { attempt } => json!({
            "type": "RetryAfterFailure",
            "attempt": attempt,
            "maxAttempts": kanzei_harness::auto_run::MAX_FAILED_ROUNDS,
            "delayMs": (15000u64 << attempt.saturating_sub(1).min(2)).min(60000),
        }),
        AutoRunAction::Stop(reason) => {
            let (reason_str, max) = match reason {
                AutoStopReason::Paused => ("Paused", None),
                AutoStopReason::StopAfterRound => ("StopAfterRound", None),
                AutoStopReason::NoAction => ("NoAction", None),
                AutoStopReason::ProfileMismatch => ("ProfileMismatch", None),
                AutoStopReason::RepeatedFailure(n) => ("RepeatedFailure", Some(n)),
                AutoStopReason::FatalError => ("FatalError", None),
                AutoStopReason::RateLimited => ("RateLimited", None),
                AutoStopReason::ZeroOutput(n) => ("ZeroOutput", Some(n)),
                AutoStopReason::ModelDeclaredDone => ("ModelDeclaredDone", None),
                AutoStopReason::GoalMet => ("GoalMet", None),
                AutoStopReason::GoalUnreachable(n) => ("GoalUnreachable", Some(n)),
                AutoStopReason::AwaitingUser => ("AwaitingUser", None),
                AutoStopReason::TokenBudgetExceeded { used, budget } => {
                    return json!({
                        "type": "Stop",
                        "reason": "TokenBudgetExceeded",
                        "usedTokens": used,
                        "budgetTokens": budget,
                    });
                }
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

#[cfg(test)]
mod tests {
    #[test]
    fn 自动问题不改变线路状态_目标仍由状态机判定() {
        use kanzei_harness::auto_run::{AutoRunCtx, AutoRunState};
        for goal_active in [false, true] {
            let round = |missing_fact| AutoRunCtx {
                halted: false,
                intensity: kanzei_harness::HarnessIntensity::Autonomous,
                completion: None,
                awaiting_user: super::awaiting_user_for_round(
                    missing_fact,
                    Some("请提供路径？"),
                    true,
                ),
                goal_active,
                steps: 3,
                tools: &[],
                auto_allowed: true,
                round_failure: None,
                progress_signature: "initial",
            };
            let mut with_question = AutoRunState::default();
            let mut without_question = AutoRunState::default();
            assert_eq!(
                with_question.decide(&round(true)),
                without_question.decide(&round(false)),
                "goal={goal_active}"
            );
            assert_eq!(with_question.rounds, without_question.rounds);
        }
    }

    #[test]
    fn 交互问题仍然等待真实回答() {
        assert!(super::awaiting_user_for_round(
            false,
            Some("需要分页吗？"),
            false
        ));
        assert!(super::awaiting_user_for_round(true, None, false));
        assert!(!super::awaiting_user_for_round(
            false,
            Some("已完成修改。"),
            false
        ));
    }
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

    #[test]
    fn 真实进展签名对代码改动与提交敏感() {
        let dir = temp_git_repo("basic");
        let baseline = super::progress_signature(&dir);
        assert_eq!(
            baseline,
            super::progress_signature(&dir),
            "什么都没变,签名必须稳定复现"
        );

        std::fs::write(dir.join("source.txt"), "changed").unwrap();
        let after_code_edit = super::progress_signature(&dir);
        assert_ne!(baseline, after_code_edit, "代码文件改动后签名必须变化");

        std::process::Command::new("git")
            .current_dir(&dir)
            .args(["commit", "-aqm", "code change"])
            .status()
            .unwrap();
        let after_commit = super::progress_signature(&dir);
        assert_ne!(after_code_edit, after_commit, "提交后签名必须再次变化");

        std::fs::remove_dir_all(&dir).ok();
    }

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
            halted: false,
            intensity: kanzei_harness::HarnessIntensity::Autonomous,
            completion: None,
            awaiting_user: false,
            goal_active: false,
            steps: 0,
            tools: &[],
            auto_allowed: true,
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
    fn 停机保留累计轮数_重新开启才清零() {
        let mut ctrl = AutoRunController {
            enabled: true,
            ..Default::default()
        };
        ctrl.state.rounds = 6;

        apply_state_update(&mut ctrl, Some(false), None, None);
        assert!(!ctrl.enabled);
        assert_eq!(ctrl.state.rounds, 6);

        ctrl.state.rounds = 3;
        apply_state_update(&mut ctrl, Some(true), None, None);
        assert!(ctrl.enabled);
        assert_eq!(ctrl.state.rounds, 0);
    }

    #[test]
    fn 不同控制器的轮数天然隔离() {
        let mut first = AutoRunController::default();
        let mut second = AutoRunController::default();
        first.state.rounds = 4;

        apply_state_update(&mut second, Some(true), None, None);
        assert_eq!(first.state.rounds, 4);
        assert_eq!(second.state.rounds, 0);
    }

    // ── 分区:工作目录管理(UI2-0926 #13)──

    /// 兜底判据的样本:「MD文件保存」现场的最后一段必须判真;陈述、反问后自答、客套邀请、
    /// 代码块里的问号必须判假。
    #[test]
    fn 最后一句在问用户_中英文样本() {
        use super::ends_with_user_question as asks;
        // 正例
        assert!(asks("我检查了工作目录:里面只有 .kanzei。\n\n请提供实际应用仓库的路径，或确认这个空目录就是目标项目，并准备好 Flutter/Dart 工具链。"));
        assert!(asks("B1 已完成。\n\n接下来用 Riverpod 还是 Bloc？"));
        assert!(asks(
            "两种方案各有取舍,你想选哪种?\n\n1. 本地 SQLite\n2. 纯文件"
        ));
        assert!(asks(
            "需要你确认是否允许我在 %LOCALAPPDATA% 安装 Flutter SDK。"
        ));
        assert!(asks("Setup is blocked on the toolchain. Please confirm whether I may install Flutter under %LOCALAPPDATA%."));
        assert!(asks("Which database should I use for the offline index?"));
        assert!(asks("**要不要我先把 B2 拆成两批？**"));
        // 反例
        assert!(!asks("已完成 R-001 B1，下一步补测试。"));
        assert!(!asks(
            "为什么会失败？原因是缓存没刷新,已修复并补了回归测试。"
        ));
        assert!(!asks("全部改完了。如有问题请告诉我。"));
        assert!(!asks("Done. Feel free to ask if you need anything else?"));
        assert!(!asks("示例:\n\n```\nwhat is this?\n```"));
        assert!(!asks("我确认了是否存在冲突:没有冲突,已提交。"));
        assert!(!asks(""));
    }

    /// 复核 minor:只看本轮最后一条助手消息,且它不能带工具调用。最后一条只有推理时,不得回头
    /// 拿更早的轮中旁白(「要先确认 X 吗?」+ 工具调用)去判「在问用户」。
    #[test]
    fn 最后一条助手消息才算收尾_带工具调用或只有推理都不算() {
        use kanzei_llm::{Message, Part, Role};
        let narration = Message {
            role: Role::Assistant,
            origin: kanzei_base::MessageOrigin::Unspecified,
            parts: vec![
                Part::Text {
                    text: "要先确认 X 吗？".into(),
                },
                Part::ToolCall {
                    id: "c1".into(),
                    name: "glob".into(),
                    input: serde_json::json!({ "pattern": "X*" }),
                },
            ],
        };
        let result = Message {
            role: Role::User,
            origin: kanzei_base::MessageOrigin::Human,
            parts: vec![Part::ToolResult {
                call_id: "c1".into(),
                content: "X.md".into(),
                is_error: false,
            }],
        };
        let reasoning_only = Message {
            role: Role::Assistant,
            origin: kanzei_base::MessageOrigin::Unspecified,
            parts: vec![Part::Reasoning {
                text: "done".into(),
                signature: None,
            }],
        };
        assert_eq!(
            super::last_assistant_text(&[narration.clone(), result.clone(), reasoning_only]),
            None,
            "最后一条只有推理:不回头读旁白"
        );
        assert_eq!(
            super::last_assistant_text(std::slice::from_ref(&narration)),
            None,
            "带工具调用的旁白不是收尾"
        );
        let closing = Message {
            role: Role::Assistant,
            origin: kanzei_base::MessageOrigin::Unspecified,
            parts: vec![Part::Text {
                text: "接下来用 Riverpod 还是 Bloc？".into(),
            }],
        };
        assert_eq!(
            super::last_assistant_text(&[narration, result, closing]).as_deref(),
            Some("接下来用 Riverpod 还是 Bloc？")
        );
    }

    #[test]
    fn 非_git_项目写代码后进展签名会变() {
        let dir = std::env::temp_dir().join(format!(
            "kz-sig-nogit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n",
        )
        .unwrap();
        if kanzei_tools::project_state::git_state_of(&dir).is_repo() {
            return;
        }
        let before = super::progress_signature(&dir);
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("lib/main.dart"), "void main() {}").unwrap();
        kanzei_tools::project_state::invalidate();
        let after = super::progress_signature(&dir);
        assert_ne!(before, after, "只写代码不动 tracker 也必须算进展");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 等用户与nudge序列化() {
        let v = super::serialize_action(kanzei_harness::auto_run::AutoRunAction::Stop(
            super::AutoStopReason::AwaitingUser,
        ));
        assert_eq!(v["type"], "Stop");
        assert_eq!(v["reason"], "AwaitingUser");

        let v = super::serialize_action(kanzei_harness::auto_run::AutoRunAction::Nudge);
        assert_eq!(v["type"], "Nudge");
        assert!(!v["prompt"].as_str().unwrap_or_default().is_empty());
    }

    /// R-322 B3:目标是**一次性意图**——空串即撤销,超长截断,前后空白不算内容。
    /// 归一化必须在后端做:前端有多个入口(输入框 change、切线回显、冷启动),
    /// 各写一份判空迟早漂开。
    #[test]
    fn conversation_goals_persist_in_isolated_home_and_clear_without_affecting_peers() {
        kanzei_base::test_support::isolated_test(
            module_path!(),
            "conversation_goals_persist_in_isolated_home_and_clear_without_affecting_peers",
            || {
                crate::prefs::failure_tests::with_home("conversation-goal", |_| {
                    super::persist_goal("one", Some("complete user goal")).unwrap();
                    super::persist_goal("two", Some("peer goal")).unwrap();
                    let restored = crate::prefs::load_prefs();
                    assert_eq!(restored.conversation_goals["one"], "complete user goal");
                    super::persist_goal("one", None).unwrap();
                    let cleared = crate::prefs::load_prefs();
                    assert!(!cleared.conversation_goals.contains_key("one"));
                    assert_eq!(cleared.conversation_goals["two"], "peer goal");
                });
            },
        );
    }

    #[test]
    fn 目标文本归一_空串撤销_超长截断() {
        use super::{normalize_goal, MAX_GOAL_CHARS};
        assert_eq!(normalize_goal(""), None);
        assert_eq!(
            normalize_goal(
                "   
	 "
            ),
            None,
            "纯空白视为撤销"
        );
        assert_eq!(
            normalize_goal("  全部测试通过  "),
            Some("全部测试通过".to_string()),
            "前后空白不算内容"
        );
        let long = "目".repeat(MAX_GOAL_CHARS + 50);
        let normalized = normalize_goal(&long).unwrap();
        assert_eq!(
            normalized.chars().count(),
            MAX_GOAL_CHARS,
            "超长按字符截断(不能按字节,会切断多字节码点)"
        );
    }
}
