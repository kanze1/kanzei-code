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
