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

/// 记忆上下文提供者:给定触发文本与臂,返回该臂应注入的记忆文本。
///
/// core 不依赖 tools——检索实现(Current 用现有策略、Candidate 用新策略、
/// Oracle 用人工标定、Leave-One-Out 做单条消融、CompressionCF 做合并前后)
/// 由 CLI/桌面端把 kanzei-tools 记忆检索包装成此 trait 注入。
/// NoMemory 永远返回空(下界)。
pub trait MemoryContextProvider: Send + Sync {
    /// arm 对应的记忆注入文本;NoMemory 返回空字符串。
    fn context_for(&self, arm: &Arm, trigger: &str) -> String;
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
/// 生产实现包装 `LlmClient`(fast 档跑批);测试用固定响应 fake。
pub trait ReplayDecider: Send + Sync {
    fn decide(&self, question: &str, memory_context: &str) -> anyhow::Result<(String, u64)>;
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
    let context = memory.context_for(&arm, &question);
    let (text, tokens) = decider.decide(&question, &context)?;
    // 验收⑤结果落 memory_eval:每条记忆一个 arm 一行,同 case 可对照。
    // memory_id 为 case_id(六臂在同一 case 上对照,不是按条目消融时留空语义
    // 由 Leave-One-Out 臂的 provider 自行决定去掉哪条)。
    store.record_memory_eval(
        &case.case_id,
        &case.case_id,
        arm.label(),
        model,
        prompt_version,
        true, // success 语义(是否产出可行动作)由批3 J 判据细化,批2 先落"可跑"。
        1,
        case.tool_failures() as u64,
        0,
        tokens,
        None,
    )?;
    Ok(ReplayDecision {
        arm,
        case_id: case.case_id.clone(),
        text,
        tokens,
    })
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

#[cfg(test)]
mod eval_tests {
    use super::*;
    use crate::store::testutil::store;

    /// 固定响应的决策者:验证六臂机制,不依赖真实 LLM。
    /// 决策文本 = "行动" + 注入的记忆上下文——变量只有记忆,结果差异归因到记忆。
    struct FakeDecider;

    impl ReplayDecider for FakeDecider {
        fn decide(
            &self,
            _question: &str,
            memory_context: &str,
        ) -> anyhow::Result<(String, u64)> {
            Ok((
                format!("行动|{memory_context}"),
                memory_context.chars().count() as u64,
            ))
        }
    }

    /// 记忆提供者:除 NoMemory 外每臂都吐一行带臂名的文本,便于断言差异。
    struct LabelMemory;

    impl MemoryContextProvider for LabelMemory {
        fn context_for(&self, arm: &Arm, _trigger: &str) -> String {
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
