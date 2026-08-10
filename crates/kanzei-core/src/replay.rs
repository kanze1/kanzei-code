//! 回放评估台数据层(R-163 批1):从历史 run.trace 解析可回放的案例。
//!
//! 目标:六臂对照量化每条记忆的决策价值,原料是历史真实轨迹——
//! 但**回放绝不真执行外部工具**(验收④)。本模块只做两件事:
//!
//! 1. [`parse_trace_payload`]:把 session_events 里 `run.trace` 的 payload_json
//!    解析成结构化 [`ReplayCase`](工具调用序列 + 每步成败 + 失败原文)。
//!    数据源是引擎已落库的轨迹(run.trace 含 tool.started/tool.completed,
//!    失败时 error 字段带原文;episodes 的 overflow_json 带失败样本)。
//! 2. [`recorded_tool_results`]:把一个 case 的步骤转成回放用的 `Part::ToolResult`
//!    列表——成功步骤给占位文本,失败步骤透传 error 原文,is_error 保留。
//!    整个路径不构造任何 `Tool` 实例,更不调用 `execute`,从结构上杜绝副作用。
//!
//! 六臂 runner(批2)消费这里的 case 与录制结果:LLM 真调,工具回放,
//! 对比不同记忆策略下的行为差异。

use serde_json::Value;

/// 一个历史工具调用及其结果(从 run.trace 解析)。
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayStep {
    /// 工具名(edit/bash/req/read…)。
    pub tool: String,
    /// 输入摘要(tool.started 的 summary 原文,通常是被调参数的 JSON)。
    pub input: String,
    /// 调用是否成功(tool.completed 的 ok 字段)。
    pub ok: bool,
    /// 失败时的错误原文(tool.completed 的 error 字段;成功时 None)。
    pub error: Option<String>,
    /// 调用耗时毫秒(tool.completed 的 durationMs)。
    pub duration_ms: u64,
}

/// 一条可回放的历史案例:一次运行中一个 turn 的工具调用序列。
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayCase {
    /// 案例标识:来源事件 id(如 `trace-<session>-<seq>`)。
    pub case_id: String,
    /// 该轮 outcome(completed/failed/halted,来自 payload 顶层)。
    pub outcome: String,
    /// 按调用顺序的工具步骤。
    pub steps: Vec<ReplayStep>,
}

impl ReplayCase {
    /// 该案例里失败的工具调用数。
    pub fn tool_failures(&self) -> usize {
        self.steps.iter().filter(|s| !s.ok).count()
    }
}

/// 解析一条 `run.trace` payload_json → [`ReplayCase`]。
///
/// 结构(与 kanzei-app 写入侧对齐):顶层 `{"events":[...], "outcome": "..."}`,
/// events 是 `turn.started` / `tool.started` / `tool.completed` 的混合数组;
/// 同一调用按 `id` 配对(started 记 name/summary,completed 记 ok/error/durationMs)。
/// 宽容解析:缺字段/未知 kind 的事件跳过,不报错(回放原料残缺时退而求其次)。
pub fn parse_trace_payload(payload: &str, case_id: &str) -> Option<ReplayCase> {
    let root: Value = serde_json::from_str(payload).ok()?;
    let events = root.get("events")?.as_array()?;
    // tool.id → (name, input, started_index),配对 completed。
    let mut pending: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let mut steps: Vec<ReplayStep> = Vec::new();
    for event in events {
        let kind = event.get("kind")?.as_str()?;
        match kind {
            "tool.started" => {
                let id = event.get("id")?.as_str()?;
                let name = event.get("name")?.as_str()?.to_string();
                let input = event
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                pending.insert(id.to_string(), (name, input));
            }
            "tool.completed" => {
                let id = event.get("id")?.as_str()?;
                let Some((name, input)) = pending.remove(id) else {
                    continue; // 缺配对的 completed(数据截断),跳过。
                };
                let ok = event.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                let error = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let duration_ms = event
                    .get("durationMs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                steps.push(ReplayStep {
                    tool: name,
                    input,
                    ok,
                    error,
                    duration_ms,
                });
            }
            _ => {} // turn.started 等只做节奏标记,回放不需要。
        }
    }
    let outcome = root
        .get("outcome")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    Some(ReplayCase {
        case_id: case_id.to_string(),
        outcome,
        steps,
    })
}

/// 把 case 步骤转成回放用的 `Part::ToolResult` 列表。
///
/// 与真实工具执行的唯一区别在这里:**不构造 Tool、不调用 execute**。
/// 成功步骤给 `[recorded ok]` 占位;失败步骤透传 error 原文并标 is_error。
/// 调用方(批2 的六臂 runner)把这些结果回喂 LLM,让模型在下一轮
/// 依据"录制的工具结果"继续决策——而非真实副作用。
pub fn recorded_tool_results(case: &ReplayCase) -> Vec<kanzei_llm::Part> {
    case.steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let content = match (&step.ok, &step.error) {
                (true, _) => format!("[recorded ok] {}", step.tool),
                (false, Some(err)) => format!("[recorded error] {}\n{err}", step.tool),
                (false, None) => format!("[recorded error] {}", step.tool),
            };
            kanzei_llm::Part::ToolResult {
                call_id: format!("recorded-{}-{}", case.case_id, index),
                content: content.into(),
                is_error: !step.ok,
            }
        })
        .collect()
}

/// 六臂对照(R-163 内容②,设计文档 §4):
/// A NoMemory(下界)/ B Current(现状)/ C Candidate(新策略)/
/// D Oracle(人工标定正确记忆 = 上界)/ E Leave-One-Out(单条消融)/
/// F CompressionCF(合并前后对照)。
///
/// 每臂的差异只在"注入什么记忆上下文",决策主体(LLM)与回放数据完全相同——
/// 这是对照实验的公共底座:变量只有记忆,结果差异才能归因到记忆本身。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arm {
    NoMemory,
    Current,
    Candidate,
    Oracle,
    LeaveOneOut,
    CompressionCF,
}

impl Arm {
    /// 六臂的固定顺序(报告与落库都按此序)。
    pub fn all() -> [Arm; 6] {
        [
            Arm::NoMemory,
            Arm::Current,
            Arm::Candidate,
            Arm::Oracle,
            Arm::LeaveOneOut,
            Arm::CompressionCF,
        ]
    }

    /// 落 memory_eval 的 arm 名(小写下划线,稳定契约)。
    pub fn label(&self) -> &'static str {
        match self {
            Arm::NoMemory => "nomemory",
            Arm::Current => "current",
            Arm::Candidate => "candidate",
            Arm::Oracle => "oracle",
            Arm::LeaveOneOut => "leave_one_out",
            Arm::CompressionCF => "compression_cf",
        }
    }
}

/// 记忆上下文提供者:给定回放案例与臂,返回该臂应注入的记忆文本。
///
/// core 不依赖 tools——检索实现(Current 用现有策略、Candidate 用新策略、
/// Oracle 用人工标定/自动事后正确做法、Leave-One-Out 做单条消融、
/// CompressionCF 做合并前后)由 CLI/桌面端把 kanzei-tools 记忆检索
/// 包装成此 trait 注入。NoMemory 永远返回空(下界)。
pub trait MemoryContextProvider: Send + Sync {
    /// arm 对应的记忆注入文本;NoMemory 返回空字符串。
    /// 接收整个 case:Oracle 等臂需要从 case 里提取失败后成功步骤,
    /// 而非只依赖 trigger 文本。
    fn context_for(&self, arm: &Arm, case: &ReplayCase) -> String;
}

/// 一次回放决策的产物:某臂在某 case 上的 LLM 决策文本与 token 消耗。
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayDecision {
    pub arm: Arm,
    pub case_id: String,
    /// LLM 输出的决策文本(下一步怎么做)。
    pub text: String,
    /// 本次决策的 token 成本(产出 token 数)。
    pub tokens: u64,
}

/// 决策者:给定决策问题与记忆上下文,产出决策文本与 token 数。
/// 生产实现包装 `LlmClient`(fast 档跑批,异步流);测试用固定响应 fake。
/// 用显式 BoxFuture 而非 async_trait,避免给 core 增加主依赖。
pub trait ReplayDecider: Send + Sync {
    fn decide<'a>(
        &'a self,
        question: &'a str,
        memory_context: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<(String, u64)>> + Send + 'a>,
    >;
}

/// Oracle 臂的自动近似:从 case 里失败步骤**之后**的成功工具调用,
/// 合成"事后正确做法"记忆(上界的自动化版本;人工标定条目可覆盖)。
/// 无后续成功步骤时返回空(该 case 的 Oracle 臂与 NoMemory 同权)。
pub fn oracle_text_from_case(case: &ReplayCase) -> String {
    let Some(failed_idx) = case.steps.iter().position(|s| !s.ok) else {
        return String::new();
    };
    let mut recovered = Vec::new();
    for step in &case.steps[failed_idx + 1..] {
        if step.ok {
            recovered.push(format!("{} {}", step.tool, step.input));
        }
    }
    if recovered.is_empty() {
        return String::new();
    }
    format!(
        "[oracle] 工具 `{}` 失败后,实际成功的做法是:\n- {}",
        case.steps[failed_idx].tool,
        recovered.join("\n- ")
    )
}

/// 从 case 构造决策问题:第一个失败步骤的"该怎么做"追问。
/// 没有失败步骤时退化为"本轮怎么做"。
pub fn question_for_case(case: &ReplayCase) -> String {
    if let Some(failed) = case.steps.iter().find(|s| !s.ok) {
        let err = failed.error.as_deref().unwrap_or("(无错误文本)");
        format!(
            "工具 `{}` 调用失败: {}\n请给出下一步行动。",
            failed.tool, err
        )
    } else {
        format!("本轮没有失败步骤(outcome={})。请给出下一步行动。", case.outcome)
    }
}

/// 跑单个臂:构造问题 → 取记忆上下文 → 决策 → 落 memory_eval。
/// 返回该臂的决策产物(供批3的对照报告聚合)。
pub async fn run_single_arm(
    case: &ReplayCase,
    arm: Arm,
    memory: &dyn MemoryContextProvider,
    decider: &dyn ReplayDecider,
    store: &crate::store::SessionStore,
    model: &str,
    prompt_version: &str,
) -> anyhow::Result<ReplayDecision> {
    let question = question_for_case(case);
    let context = memory.context_for(&arm, case);
    let (text, tokens) = decider.decide(&question, &context).await?;
    // 验收⑤结果落 memory_eval:每条记忆一个 arm 一行,同 case 可对照。
    // memory_id 为 case_id(六臂在同一 case 上对照,不是按条目消融时留空语义
    // 由 Leave-One-Out 臂的 provider 自行决定去掉哪条)。
    let decision = ReplayDecision {
        arm,
        case_id: case.case_id.clone(),
        text,
        tokens,
    };
    // J 判据(批3)驱动落库:success = 是否产出可行动作(terminal 成功代理)。
    let score = score_decision(case, &decision);
    store.record_memory_eval(
        &case.case_id,
        &case.case_id,
        arm.label(),
        model,
        prompt_version,
        score.has_action,
        case.steps.len() as u64,
        case.tool_failures() as u64,
        score.retry_signal as u64,
        tokens,
        None,
    )?;
    Ok(decision)
}

/// 六臂全跑同一 case,返回各臂决策(顺序与 [`Arm::all`] 一致)。
pub async fn run_arms(
    case: &ReplayCase,
    memory: &dyn MemoryContextProvider,
    decider: &dyn ReplayDecider,
    store: &crate::store::SessionStore,
    model: &str,
    prompt_version: &str,
) -> anyhow::Result<Vec<ReplayDecision>> {
    let mut decisions = Vec::with_capacity(6);
    for arm in Arm::all() {
        decisions.push(
            run_single_arm(case, arm, memory, decider, store, model, prompt_version).await?,
        );
    }
    Ok(decisions)
}

// ---------------------------------------------------------------------------
// 批3:J 判据分层 + 对照报告(验收③)
// ---------------------------------------------------------------------------

/// J 判据分层评分(R-163 内容②设计 §5):对一次回放决策打分。
///
/// 分层的意图是从"决策质量"反推记忆价值——C≪D 说明触发/检索问题,
/// C≈D 仍败则内容/utilization 问题。本评分是**可自动计算的代理**:
///
/// 1. `has_action`(terminal 成功代理):决策是否给出实质可行动作
///    (非空、非空转——含动作词且不全是"无法操作"类退避)。
/// 2. `repeats_failed_tool`(工具失败数方向,负信号):决策重提了 case 里
///    已失败的工具——大概率再次失败,记忆没起作用。
/// 3. `retry_signal`(重试方向,负信号):决策文本出现"重试/再试/retry"。
/// 4. `tokens`:决策成本,六臂同 case 对比时作为效率维度。
#[derive(Debug, Clone, PartialEq)]
pub struct JScore {
    pub has_action: bool,
    pub repeats_failed_tool: bool,
    pub retry_signal: bool,
    pub tokens: u64,
}

/// 动作词启发集:命中任一即视为"给出了动作"(terminal 成功代理)。
const ACTION_WORDS: &[&str] = &[
    "read",
    "edit",
    "bash",
    "git",
    "grep",
    "glob",
    "req",
    "defect",
    "memory",
    "查看",
    "读取",
    "修改",
    "运行",
    "调用",
    "搜索",
    "执行",
    "重试",
    "改用",
    "尝试",
];

/// 空转词:只有这些词(或很短)视为未给出动作。
const EVASION_WORDS: &[&str] = &["无法", "不能", "不知道", "无权限", "抱歉"];

/// 对一次决策按 J 判据分层评分。
pub fn score_decision(case: &ReplayCase, decision: &ReplayDecision) -> JScore {
    let text = decision.text.trim();
    let has_action = {
        let len = text.chars().count();
        let has_action_word = ACTION_WORDS.iter().any(|w| text.contains(w));
        let has_evasion = EVASION_WORDS.iter().any(|w| text.contains(w));
        len >= 4 && has_action_word && !has_evasion
    };
    // 负信号:决策文本里出现 case 中失败的工具名(词边界避免误伤)。
    let repeats_failed_tool = case
        .steps
        .iter()
        .filter(|s| !s.ok)
        .any(|s| {
            let tool = s.tool.trim();
            tool.len() >= 3 && text.split(|c: char| !c.is_alphanumeric()).any(|w| w == tool)
        });
    let retry_signal = ["重试", "再试", "retry", "重新执行"]
        .iter()
        .any(|w| text.contains(w));
    JScore {
        has_action,
        repeats_failed_tool,
        retry_signal,
        tokens: decision.tokens,
    }
}

/// 单臂在多个 case 上的聚合评分(报告的一行)。
#[derive(Debug, Clone)]
pub struct ArmSummary {
    pub arm: Arm,
    /// 参与聚合的 case 数。
    pub cases: usize,
    /// has_action 命中的 case 数。
    pub with_action: usize,
    /// 重提失败工具(负信号)的 case 数。
    pub repeats_failed: usize,
    /// 重试信号(负信号)的 case 数。
    pub retry: usize,
    /// 决策 token 总和。
    pub total_tokens: u64,
}

/// 把一组 case 的六臂决策聚合成各臂汇总(顺序与 [`Arm::all`] 一致)。
pub fn summarize(
    cases: &[ReplayCase],
    decisions: &[Vec<ReplayDecision>],
) -> Vec<ArmSummary> {
    debug_assert_eq!(cases.len(), decisions.len());
    let mut per_arm: std::collections::HashMap<Arm, Vec<JScore>> =
        std::collections::HashMap::new();
    for (case, arm_decisions) in cases.iter().zip(decisions) {
        for decision in arm_decisions {
            per_arm
                .entry(decision.arm)
                .or_default()
                .push(score_decision(case, decision));
        }
    }
    Arm::all()
        .iter()
        .map(|arm| {
            let scores = per_arm.get(arm).cloned().unwrap_or_default();
            ArmSummary {
                arm: *arm,
                cases: scores.len(),
                with_action: scores.iter().filter(|s| s.has_action).count(),
                repeats_failed: scores.iter().filter(|s| s.repeats_failed_tool).count(),
                retry: scores.iter().filter(|s| s.retry_signal).count(),
                total_tokens: scores.iter().map(|s| s.tokens).sum(),
            }
        })
        .collect()
}

/// 渲染六臂对照报告(验收③:产出 NoMemory vs Current vs Oracle 对照)。
///
/// 输出 Markdown 表格 + 三条关键差距注释:
/// 1. NoMemory→Current:记忆是否带来可行动作的提升(内容/注入价值)。
/// 2. Current→Oracle:现状与上界的差距(检索/触发损失)。
/// 3. 负信号(repeats_failed/retry):哪一臂最容易把模型带回失败路径。
pub fn render_report(
    cases: &[ReplayCase],
    decisions: &[Vec<ReplayDecision>],
    model: &str,
) -> String {
    let summaries = summarize(cases, decisions);
    let mut out = String::new();
    out.push_str(&format!(
        "## 六臂对照报告(case={}, model={})\n\n",
        cases.len(),
        model
    ));
    out.push_str("| 臂 | case | 有动作 | 重提失败工具 | 重试信号 | 总token |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for s in &summaries {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            s.arm.label(),
            s.cases,
            s.with_action,
            s.repeats_failed,
            s.retry,
            s.total_tokens
        ));
    }
    // 差距注释:按臂索引取 NoMemory=0, Current=1, Oracle=3。
    let get = |arm: Arm| summaries.iter().find(|s| s.arm == arm);
    let no_memory = get(Arm::NoMemory).map(|s| s.with_action).unwrap_or(0);
    let current = get(Arm::Current).map(|s| s.with_action).unwrap_or(0);
    let oracle = get(Arm::Oracle).map(|s| s.with_action).unwrap_or(0);
    let current_repeat = get(Arm::Current).map(|s| s.repeats_failed).unwrap_or(0);
    let oracle_repeat = get(Arm::Oracle).map(|s| s.repeats_failed).unwrap_or(0);
    out.push_str("\n### 差距注释\n\n");
    out.push_str(&format!(
        "- NoMemory→Current 有动作: {no_memory} → {current}(记忆注入带来的增量)\n"
    ));
    out.push_str(&format!(
        "- Current→Oracle 有动作: {current} → {oracle}(上界差距 = 检索/触发损失)\n"
    ));
    out.push_str(&format!(
        "- 重提失败工具: Current {current_repeat} vs Oracle {oracle_repeat}(记忆把模型拉回失败路径的程度)\n"
    ));
    out
}


#[cfg(test)]
mod eval_tests {
    use super::*;
    use crate::store::testutil::store;

    /// 固定响应的决策者:验证六臂机制,不依赖真实 LLM。
    /// 决策文本 = "行动" + 注入的记忆上下文——变量只有记忆,结果差异归因到记忆。
    struct FakeDecider;

    impl ReplayDecider for FakeDecider {
        fn decide<'a>(
            &'a self,
            _question: &'a str,
            memory_context: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<(String, u64)>> + Send + 'a>,
        > {
            Box::pin(async move {
                Ok((
                    format!("行动|{memory_context}"),
                    memory_context.chars().count() as u64,
                ))
            })
        }
    }

    /// 记忆提供者:除 NoMemory 外每臂都吐一行带臂名的文本,便于断言差异。
    struct LabelMemory;

    impl MemoryContextProvider for LabelMemory {
        fn context_for(&self, arm: &Arm, _case: &ReplayCase) -> String {
            match arm {
                Arm::NoMemory => String::new(),
                _ => format!("[memory-{}] 该做的行动", arm.label()),
            }
        }
    }

    const SAMPLE: &str = r#"{
      "events": [
        {"at": 1, "id": "a", "kind": "tool.started", "name": "edit", "summary": "{}"},
        {"at": 2, "durationMs": 5, "error": "old_string not found", "id": "a", "kind": "tool.completed", "name": "edit", "ok": false}
      ],
      "outcome": "failed"
    }"#;

    #[tokio::test]
    async fn 六臂各自可跑并落memory_eval() {
        // 验收①:六臂在同一 case 上各自跑完并落库,arm 名是稳定契约。
        let store = store();
        let case = parse_trace_payload(SAMPLE, "case-arms").unwrap();
        let memory = LabelMemory;
        let decider = FakeDecider;
        let decisions = run_arms(&case, &memory, &decider, &store, "fake", "v1")
            .await
            .unwrap();
        assert_eq!(decisions.len(), 6);
        assert_eq!(
            decisions.iter().map(|d| d.arm.label()).collect::<Vec<_>>(),
            vec![
                "nomemory",
                "current",
                "candidate",
                "oracle",
                "leave_one_out",
                "compression_cf"
            ]
        );
        // 变量只有记忆:NoMemory 决策文本最短(无注入),其它臂都带各自记忆文本。
        assert_eq!(decisions[0].text, "行动|");
        for d in &decisions[1..] {
            assert!(
                d.text.contains(&format!("[memory-{}]", d.arm.label())),
                "臂 {} 的决策必须带自己的记忆上下文: {}",
                d.arm.label(),
                d.text
            );
        }
        // 全部落 memory_eval:arm 名齐全。
        let rows: Vec<(String, String)> = store
            .connection
            .prepare("SELECT arm, replay_case FROM memory_eval ORDER BY arm")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows.len(), 6);
        for (arm, replay_case) in rows {
            assert_eq!(replay_case, "case-arms");
            assert!(Arm::all().iter().any(|a| a.label() == arm), "未知臂: {arm}");
        }
    }

    #[test]
    fn 决策问题取第一个失败步骤的原文() {
        let case = parse_trace_payload(SAMPLE, "q").unwrap();
        let q = question_for_case(&case);
        assert!(q.contains("edit"), "{q}");
        assert!(q.contains("old_string not found"), "{q}");
        // 无失败步骤时退化文案。
        let ok_case = parse_trace_payload(
            r#"{"events":[{"id":"a","kind":"tool.started","name":"git","summary":"{}"},
                {"id":"a","kind":"tool.completed","name":"git","ok":true}],"outcome":"completed"}"#,
            "q2",
        )
        .unwrap();
        assert!(question_for_case(&ok_case).contains("没有失败步骤"));
    }

    #[test]
    fn arm_label契约稳定_与设计文档六臂一致() {
        assert_eq!(
            Arm::all()
                .iter()
                .map(|a| a.label())
                .collect::<Vec<_>>(),
            vec![
                "nomemory",
                "current",
                "candidate",
                "oracle",
                "leave_one_out",
                "compression_cf"
            ]
        );
    }

    #[test]
    fn oracle自动合成失败后的成功做法_无后续则空() {
        // 失败后紧跟成功步骤 → 合成"事后正确做法"。
        let case = parse_trace_payload(
            r#"{"events":[
                {"id":"a","kind":"tool.started","name":"edit","summary":"{}"},
                {"id":"a","kind":"tool.completed","name":"edit","ok":false,"error":"old_string not found"},
                {"id":"b","kind":"tool.started","name":"read","summary":"{\"path\":\"src/main.rs\"}"},
                {"id":"b","kind":"tool.completed","name":"read","ok":true}
            ],"outcome":"completed"}"#,
            "o1",
        )
        .unwrap();
        let oracle = oracle_text_from_case(&case);
        assert!(oracle.contains("[oracle]"), "{oracle}");
        assert!(oracle.contains("edit"), "{oracle}");
        assert!(oracle.contains("read"), "{oracle}");
        // 失败后没有成功步骤 → 空(Oracle 与 NoMemory 同权)。
        let no_recover = parse_trace_payload(SAMPLE, "o2").unwrap();
        assert!(oracle_text_from_case(&no_recover).is_empty());
        // 无失败步骤 → 空。
        let ok_case = parse_trace_payload(
            r#"{"events":[{"id":"a","kind":"tool.started","name":"git","summary":"{}"},
                {"id":"a","kind":"tool.completed","name":"git","ok":true}],"outcome":"completed"}"#,
            "o3",
        )
        .unwrap();
        assert!(oracle_text_from_case(&ok_case).is_empty());
    }

    // ---- 批3:J 判据分层 + 对照报告 ----

    #[test]
    fn J判据_有动作重提失败工具重试信号分别识别() {
        let case = parse_trace_payload(SAMPLE, "j1").unwrap();
        // 有动作 + 重提失败工具(edit)+ 重试信号 → 全负信号命中但 has_action。
        let d = ReplayDecision {
            arm: Arm::Current,
            case_id: "j1".into(),
            text: "我重试 edit,先读取文件再修改。".into(),
            tokens: 9,
        };
        let s = score_decision(&case, &d);
        assert!(s.has_action, "含动作词(读取/修改/重试)且无空转词");
        assert!(s.repeats_failed_tool, "重提了失败工具 edit");
        assert!(s.retry_signal, "含'重试'");
        assert_eq!(s.tokens, 9);
        // 空转:无动作词/短文本 → 无动作。
        let evasive = ReplayDecision {
            arm: Arm::NoMemory,
            case_id: "j1".into(),
            text: "抱歉,我无法处理。".into(),
            tokens: 3,
        };
        let s2 = score_decision(&case, &evasive);
        assert!(!s2.has_action, "空转词 + 无动作词 → 无动作");
        assert!(!s2.repeats_failed_tool, "未重提工具");
        // 动作词命中但避开失败工具:改用 read 而非 edit。
        let good = ReplayDecision {
            arm: Arm::Oracle,
            case_id: "j1".into(),
            text: "先用 read 查看目标文件确认实际内容。".into(),
            tokens: 8,
        };
        let s3 = score_decision(&case, &good);
        assert!(s3.has_action);
        assert!(!s3.repeats_failed_tool, "改用 read,不重提失败工具 edit");
        assert!(!s3.retry_signal);
    }

    #[test]
    fn 对照报告汇总并渲染NoMemoryCurrentOracle差距() {
        let case = parse_trace_payload(SAMPLE, "r1").unwrap();
        // 模拟两个 case 的六臂决策:NoMemory 全部空转、Current 一半、
        // Oracle 全部有动作且不重提失败工具。
        let mut decisions: Vec<Vec<ReplayDecision>> = Vec::new();
        for i in 0..2 {
            let mut arm_decisions = Vec::new();
            for arm in Arm::all() {
                let text = match (arm, i) {
                    (Arm::NoMemory, _) => "抱歉,无法处理。".into(),
                    (Arm::Oracle, _) => "先 read 查看文件实际内容再修改。".into(),
                    (Arm::Current, 0) => "先 read 查看文件。".into(),
                    (Arm::Current, 1) => "抱歉,无法处理。".into(),
                    _ => "read 文件后 edit 修改。".into(),
                };
                arm_decisions.push(ReplayDecision {
                    arm,
                    case_id: format!("r{}", i + 1),
                    text,
                    tokens: 10,
                });
            }
            decisions.push(arm_decisions);
        }
        let cases = vec![case.clone(), case];
        let report = render_report(&cases, &decisions, "fake");
        // 六臂都有一行。
        for arm in Arm::all() {
            assert!(report.contains(&format!("| {} |", arm.label())), "{report}");
        }
        // 差距注释:NoMemory 0 → Current 1 → Oracle 2。
        assert!(report.contains("NoMemory→Current 有动作: 0 → 1"), "{report}");
        assert!(report.contains("Current→Oracle 有动作: 1 → 2"), "{report}");
        assert!(report.contains("重提失败工具: Current 0 vs Oracle 0"), "{report}");
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 与 kanzei-app/src/run.rs 写入侧同构的 run.trace payload:
    /// 混合 turn.started / tool.started / tool.completed,失败带 error 原文。
    const SAMPLE: &str = r#"{
      "events": [
        {"at": 1786343405100, "kind": "turn.started", "step": 1},
        {"at": 1786343422302, "id": "call_00_x", "kind": "tool.started", "name": "git", "summary": "{\"action\":\"status\"}"},
        {"at": 1786343422302, "id": "call_01_y", "kind": "tool.started", "name": "edit", "summary": "{\"path\":\"src/main.rs\"}"},
        {"at": 1786343422420, "durationMs": 117, "error": null, "id": "call_00_x", "kind": "tool.completed", "name": "git", "ok": true},
        {"at": 1786343422421, "durationMs": 15, "error": "old_string not found", "id": "call_01_y", "kind": "tool.completed", "name": "edit", "ok": false}
      ],
      "outcome": "failed"
    }"#;

    #[test]
    fn 解析run_trace_按id配对并透传失败原文() {
        let case = parse_trace_payload(SAMPLE, "trace-test").expect("样例应可解析");
        assert_eq!(case.case_id, "trace-test");
        assert_eq!(case.outcome, "failed");
        assert_eq!(case.steps.len(), 2);
        // 按调用顺序:git 先、edit 后。
        assert_eq!(case.steps[0].tool, "git");
        assert!(case.steps[0].ok);
        assert_eq!(case.steps[0].error, None);
        assert_eq!(case.steps[1].tool, "edit");
        assert!(!case.steps[1].ok);
        assert_eq!(case.steps[1].error.as_deref(), Some("old_string not found"));
        assert_eq!(case.steps[1].duration_ms, 15);
        assert_eq!(case.tool_failures(), 1);
    }

    #[test]
    fn 录制回放不真执行外部工具_合成结果透传成败() {
        // 验收④:回放路径不得执行任何外部工具——本测试只有 parse + 合成,
        // 全程没有 Tool 实例;断言结果文本是占位/原文透传,is_error 保留。
        let case = parse_trace_payload(SAMPLE, "trace-test").unwrap();
        let parts = recorded_tool_results(&case);
        assert_eq!(parts.len(), 2);
        match &parts[0] {
            kanzei_llm::Part::ToolResult {
                call_id,
                content,
                is_error,
            } => {
                assert!(call_id.starts_with("recorded-trace-test-0"));
                assert!(content.contains("[recorded ok]"), "{content}");
                assert!(!is_error);
            }
            other => panic!("预期 ToolResult,得到 {other:?}"),
        }
        match &parts[1] {
            kanzei_llm::Part::ToolResult {
                content, is_error, ..
            } => {
                assert!(content.contains("[recorded error]"), "{content}");
                assert!(content.contains("old_string not found"), "{content}");
                assert!(*is_error, "失败步骤必须保持 is_error=true");
            }
            other => panic!("预期 ToolResult,得到 {other:?}"),
        }
    }

    #[test]
    fn 宽容解析_缺配对的completed与未知kind不崩() {
        let payload = r#"{
          "events": [
            {"at": 1, "id": "a", "kind": "tool.started", "name": "read", "summary": "{}"},
            {"at": 2, "kind": "mystery", "x": 1}
          ],
          "outcome": "completed"
        }"#;
        let case = parse_trace_payload(payload, "t").unwrap();
        // read 有 started 无 completed → 被丢弃;unknown kind 跳过。
        assert!(case.steps.is_empty());
        assert_eq!(case.outcome, "completed");
    }

    #[test]
    fn 坏payload返回None_不panic() {
        assert!(parse_trace_payload("not json", "t").is_none());
        assert!(parse_trace_payload("{}", "t").is_none());
        assert!(parse_trace_payload("[]", "t").is_none());
    }
}
