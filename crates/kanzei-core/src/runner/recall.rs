//! 工具失败后的记忆线索组装。运行器负责错误分类、预算、去重和后续失败观察。
//! 检索与遥测通过 RecallPolicy 注入；共享运行装配选择 kanzei-memory 的实现。
//! 召回线索与原始工具错误分别表达，未完成的检索不伪装成零命中。

use std::collections::{HashMap, HashSet};

use kanzei_llm::Part;

use super::metrics::{failure_kind, failure_target, is_expected_tool_rejection};

/// 一次工具失败触发的召回请求。RecallWatch 构造它,policy 决定怎么查。
#[derive(Debug, Clone)]
pub struct RecallTrigger {
    /// 失败的工具名(edit/bash/read…)。
    pub tool: String,
    /// 归一化错误指纹(failure_kind 输出,抹掉路径与数字)。
    pub kind: String,
    /// 错误原文首段(供 BM25 构 query)。
    pub sample: String,
    /// 失败目标(路径尾段/命令首词,可能为空)。
    pub target: String,
    /// 本轮内同 (tool, kind) 已失败次数(含本次)。≥2 时 policy 应走 ReRetrieve。
    pub failure_count: usize,
    /// 同一 (tool, kind) 上一轮检索返回的原始候选，供 ReRetrieve 机械排除。
    /// 首次触发为空；只表示同一运行内的检索事实，不跨运行猜测历史身份。
    pub previous_retrieved_ids: Vec<String>,
}

/// 一条记忆定位与适用线索，不携带执行授权。
#[derive(Debug, Clone)]
pub struct RecallHit {
    pub id: String,
    pub category: String,
    /// 检索描述与适用线索，不是本次执行指令。
    pub description: String,
    pub status: String,
    pub source: String,
    /// 实际命中的检索层级。由 Retriever 填写，遥测必须原样记录，不能由
    /// 触发次数或其它旁路字段反推。
    pub policy_action: String,
}

/// 每个失败结果追加的记忆线索预算，包含说明与省略回执。
const RECALL_HINT_BUDGET: usize = 3_000;

fn render_hints(retrieved: &[RecallHit], seen: &HashSet<String>) -> (String, Vec<RecallHit>) {
    let mut unique = HashSet::new();
    let candidates: Vec<_> = retrieved
        .iter()
        .filter(|hit| !seen.contains(&hit.id) && unique.insert(hit.id.clone()))
        .collect();
    if candidates.is_empty() {
        return (String::new(), Vec::new());
    }
    let mut text =
        String::from("\n<failure-memory-hints>\n以下为历史线索，是否回读或采用由当前任务决定。\n");
    let ending = |omitted| {
        if omitted == 0 {
            "</failure-memory-hints>".to_string()
        } else {
            format!("预算未列出 {omitted} 条线索，可按需检索。\n</failure-memory-hints>")
        }
    };
    let mut used = text.chars().count() + ending(candidates.len()).chars().count();
    let (mut included, mut omitted) = (Vec::new(), 0);
    for hit in candidates {
        let line = format!(
            "[记忆线索 {} | {}]\n适用线索: {}\n记录状态: {} · 来源: {}\n",
            hit.id, hit.category, hit.description, hit.status, hit.source
        );
        let chars = line.chars().count();
        if used + chars <= RECALL_HINT_BUDGET {
            text.push_str(&line);
            used += chars;
            included.push(hit.clone());
        } else {
            omitted += 1;
        }
    }
    text.push_str(&ending(omitted));
    (text, included)
}

/// 一次注入的轮末结局:注入后同 (tool, kind) 是否停止复发。
/// 只描述余下轮次的失败观察，不能据此证明采用或收益。
#[derive(Debug, Clone)]
pub struct RecallOutcome {
    pub memory_id: String,
    pub tool: String,
    pub kind: String,
    /// true = 注入后到轮末同 (tool,kind) 未再失败；false = 再次失败。
    pub failure_not_repeated: bool,
}

/// 运行级终态观察。Completed 只说明引擎结束，不证明任务成功或收益。
/// Unknown 表示提前返回时未提交明确结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallRunOutcome {
    Completed,
    Halted,
    Unknown,
}

/// 召回策略接口(core 侧定义,CLI/桌面端注入 kanzei-tools 实现)。
/// core 不依赖 tools:检索/遥测/超时降级全部在实现侧,本域只做触发判定、
/// 去重与注入格式。
pub trait RecallPolicy: Send + Sync {
    /// 给定失败触发返回线索，读取失败、超时与不可用保持为 Err。
    /// 实现负责检索，本域负责预算、去重与组装。
    fn retrieve(&self, trigger: &RecallTrigger) -> Result<Vec<RecallHit>, String>;

    /// 每次实际触发落遥测(trigger/action/延迟)。空实现 = 不落库。
    /// `retrieved` 保留检索结果或错误，`injected` 是实际进入本次结果文本的子集。
    /// 成功结果包含同轮已注入而被去重的候选，错误不作为零命中记录。
    /// 正常零命中也记录，以区分检索覆盖缺口与未执行的检索。
    fn record_trigger(
        &self,
        _trigger: &RecallTrigger,
        _retrieved: Result<&[RecallHit], &str>,
        _injected: &[RecallHit],
        _elapsed_ms: u64,
        _run_id: Option<&str>,
    ) {
    }

    /// 轮末回收每条注入后的失败复发观察。空实现 = 不落库。
    fn record_outcomes(&self, _outcomes: &[RecallOutcome]) {}

    /// 同时交付运行终态与失败观察；收益评估由独立证据提供。
    fn record_outcome_evidence(&self, outcomes: &[RecallOutcome], _run_outcome: RecallRunOutcome) {
        self.record_outcomes(outcomes);
    }
}

/// 事件触发召回状态按单次运行持有，轮与轮之间不复用。挂在工具结果回喂前,就地追加 Packet 文本,不阻断。
/// 策略经引用借用(不拥有)——drive.rs 从 &RunnerConfig 借出,不取走。
pub struct RecallWatch<'a> {
    /// 失败计数:(tool, kind) → 本轮次数。
    failures: HashMap<(String, String), usize>,
    /// 已注入条目 id(同轮同条目只注入一次,防刷屏——设计 §3.3 按条目去重,
    /// 不按 (tool,kind),否则同类失败换了个措辞就重复注入)。
    injected: HashSet<String>,
    /// 上一轮每个失败桶的原始检索候选，供重复失败重查排除。
    last_retrieved: HashMap<(String, String), Vec<String>>,
    /// 待判定的注入结局:(memory_id, tool, kind, 注入时该 kind 的失败次数)。
    /// finish 时记录同类失败是否重现及运行终态；Drop 以 Unknown 终态兜底。
    pending: Vec<(String, String, String, usize)>,
    /// 注入的策略(检索 + 遥测)。
    policy: Option<&'a dyn RecallPolicy>,
    /// 显式 finish 后禁止 Drop 重复落库；Drop 仍作为未来提前返回路径的安全兜底。
    finished: bool,
    run_id: Option<String>,
}

impl<'a> RecallWatch<'a> {
    pub fn new(policy: Option<&'a dyn RecallPolicy>) -> Self {
        Self {
            failures: HashMap::new(),
            injected: HashSet::new(),
            last_retrieved: HashMap::new(),
            pending: Vec::new(),
            policy,
            finished: false,
            run_id: None,
        }
    }

    pub fn with_run_id(mut self, run_id: Option<String>) -> Self {
        self.run_id = run_id;
        self
    }

    /// 在 RunSummary 构造前提交终态；Drop 使用 Unknown，且不重复落库。
    pub fn finish(&mut self, run_outcome: RecallRunOutcome) {
        if self.finished {
            return;
        }
        self.finished = true;
        let Some(policy) = &self.policy else {
            return;
        };
        if self.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        let outcomes: Vec<RecallOutcome> = pending
            .into_iter()
            .map(|(memory_id, tool, kind, at_count)| {
                let now = self
                    .failures
                    .get(&(tool.clone(), kind.clone()))
                    .copied()
                    .unwrap_or(0);
                RecallOutcome {
                    memory_id,
                    tool,
                    kind,
                    failure_not_repeated: now <= at_count,
                }
            })
            .collect();
        policy.record_outcome_evidence(&outcomes, run_outcome);
    }

    /// 在整步工具结果回喂前调用:`results` 与 `calls` 按下标一一对应
    /// 调用与结果按下标对齐；只追加召回线索，不改原始结果语义。
    /// 无 policy 时是纯 no-op(空转成本 = 一次 Option 判空)。
    pub fn note_step(
        &mut self,
        calls: &[(String, String, serde_json::Value, String)],
        results: &mut [Part],
    ) {
        let Some(policy) = &self.policy else {
            return;
        };
        debug_assert_eq!(calls.len(), results.len(), "工具调用与结果按下标一一对应");
        for (index, (_, name, input, _)) in calls.iter().enumerate() {
            let Some(Part::ToolResult {
                content, is_error, ..
            }) = results.get_mut(index)
            else {
                continue;
            };
            if !*is_error {
                continue;
            }
            if is_expected_tool_rejection(content) {
                continue;
            }
            let kind = failure_kind(content);
            let target = failure_target(input);
            let count = {
                let entry = self
                    .failures
                    .entry((name.clone(), kind.clone()))
                    .or_insert(0);
                *entry += 1;
                *entry
            };
            let trigger = RecallTrigger {
                tool: name.clone(),
                kind: kind.clone(),
                sample: content.chars().take(240).collect(),
                target,
                failure_count: count,
                previous_retrieved_ids: self
                    .last_retrieved
                    .get(&(name.clone(), kind.clone()))
                    .cloned()
                    .unwrap_or_default(),
            };
            let started = std::time::Instant::now();
            let retrieved = match policy.retrieve(&trigger) {
                Ok(hits) => hits,
                Err(error) => {
                    tracing::warn!(%error, tool=%trigger.tool, "failure memory retrieval unavailable");
                    policy.record_trigger(
                        &trigger,
                        Err(&error),
                        &[],
                        started.elapsed().as_millis() as u64,
                        self.run_id.as_deref(),
                    );
                    content
                        .push_str("\n[失败记忆检索未完成；不能据此判断没有匹配，错误见运行日志。]");
                    continue;
                }
            };
            self.last_retrieved.insert(
                (trigger.tool.clone(), trigger.kind.clone()),
                retrieved.iter().map(|hit| hit.id.clone()).collect(),
            );
            let elapsed = started.elapsed().as_millis() as u64;
            // 同轮同条目只注入一次:已注入的 id 从注入集丢弃,但保留在 retrieved
            // 里落遥测——"检索到但没再注入"与"根本没检索到"是两种漏斗事实。
            let (hint_text, hits) = render_hints(&retrieved, &self.injected);
            for hit in &hits {
                self.injected.insert(hit.id.clone());
            }
            // miss 也落遥测(retrieved/injected 皆空):没有 miss 记录就永远
            // 看不见"高频失败但零记忆覆盖"的缺口。
            policy.record_trigger(
                &trigger,
                Ok(&retrieved),
                &hits,
                elapsed,
                self.run_id.as_deref(),
            );
            content.push_str(&hint_text);
            // 只有实际列出的线索进入注入与后续观察集合。
            for hit in &hits {
                self.pending.push((
                    hit.id.clone(),
                    trigger.tool.clone(),
                    trigger.kind.clone(),
                    trigger.failure_count,
                ));
            }
        }
    }
}

impl Drop for RecallWatch<'_> {
    /// 未来新增提前返回路径的安全兜底：未显式 finish 时不伪造最终完成结局。
    fn drop(&mut self) {
        self.finish(RecallRunOutcome::Unknown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 可编程假策略:按触发返回固定命中。
    struct FakePolicy {
        hits: Vec<RecallHit>,
    }

    impl RecallPolicy for FakePolicy {
        fn retrieve(&self, _trigger: &RecallTrigger) -> Result<Vec<RecallHit>, String> {
            Ok(self.hits.clone())
        }
    }

    /// 记录型策略:捕获每次 record_trigger 的 (retrieved, injected) 条数与
    /// 轮末 outcomes,验证 miss 落库与 ACTION_CHANGED 对账。
    struct RecordingPolicy {
        hits: Vec<RecallHit>,
        triggers: std::sync::Arc<std::sync::Mutex<Vec<(usize, usize)>>>,
        outcomes: std::sync::Arc<std::sync::Mutex<Vec<RecallOutcome>>>,
    }

    impl RecallPolicy for RecordingPolicy {
        fn retrieve(&self, _trigger: &RecallTrigger) -> Result<Vec<RecallHit>, String> {
            Ok(self.hits.clone())
        }
        fn record_trigger(
            &self,
            _trigger: &RecallTrigger,
            retrieved: Result<&[RecallHit], &str>,
            injected: &[RecallHit],
            _elapsed_ms: u64,
            _run_id: Option<&str>,
        ) {
            self.triggers
                .lock()
                .unwrap()
                .push((retrieved.unwrap().len(), injected.len()));
        }
        fn record_outcomes(&self, outcomes: &[RecallOutcome]) {
            self.outcomes.lock().unwrap().extend(outcomes.to_vec());
        }
    }

    fn hit(id: &str, category: &str) -> RecallHit {
        RecallHit {
            id: id.into(),
            category: category.into(),
            description: "先 read 重建 old_string 再重试".into(),
            status: "active".into(),
            source: "episode E-1 步 3".into(),
            policy_action: "fingerprint".into(),
        }
    }

    #[test]
    fn budget_and_deduplication_record_only_visible_hints_without_changing_the_failure() {
        let mut large = hit("M-large", "sop");
        large.description = "过长历史线索".repeat(RECALL_HINT_BUDGET);
        let policy = RecordingPolicy {
            hits: vec![
                large,
                hit("M-short", "fact"),
                hit("M-short", "fact"),
                hit("M-other", "sop"),
            ],
            triggers: Default::default(),
            outcomes: Default::default(),
        };
        let mut watch = RecallWatch::new(Some(&policy));
        let calls = vec![(
            "call".into(),
            "edit".into(),
            serde_json::json!({"path":"src/lib.rs"}),
            String::new(),
        )];
        let original = "old_string not found";
        let mut results = vec![Part::ToolResult {
            call_id: "call".into(),
            content: original.into(),
            is_error: true,
        }];
        watch.note_step(&calls, &mut results);
        let Part::ToolResult {
            content, is_error, ..
        } = &results[0]
        else {
            panic!("tool result");
        };
        assert!(*is_error && content.starts_with(original));
        assert!(content[original.len()..].chars().count() <= RECALL_HINT_BUDGET);
        assert!(content.contains("预算未列出 1 条线索"));
        assert!(content.contains("适用线索:") && !content.contains("行动:"));
        assert_eq!(content.matches("[记忆线索 M-short").count(), 1);
        assert!(!watch.injected.contains("M-large"));
        assert_eq!(*policy.triggers.lock().unwrap(), vec![(4, 2)]);
    }

    #[test]
    fn retrieval_errors_remain_distinct_from_no_matches_and_preserve_the_original_tool_error() {
        struct Unavailable;
        impl RecallPolicy for Unavailable {
            fn retrieve(&self, _: &RecallTrigger) -> Result<Vec<RecallHit>, String> {
                Err("source read failed".into())
            }
            fn record_trigger(
                &self,
                _: &RecallTrigger,
                retrieved: Result<&[RecallHit], &str>,
                injected: &[RecallHit],
                _: u64,
                _: Option<&str>,
            ) {
                assert_eq!(retrieved.unwrap_err(), "source read failed");
                assert!(injected.is_empty());
            }
        }
        let mut watch = RecallWatch::new(Some(&Unavailable));
        let calls = vec![(
            "call".into(),
            "read".into(),
            serde_json::json!({"path":"missing.rs"}),
            String::new(),
        )];
        let original = "no such file: missing.rs";
        let mut results = vec![Part::ToolResult {
            call_id: "call".into(),
            content: original.into(),
            is_error: true,
        }];
        watch.note_step(&calls, &mut results);
        let Part::ToolResult {
            content, is_error, ..
        } = &results[0]
        else {
            panic!("tool result");
        };
        assert!(*is_error && content.starts_with(original));
        assert!(content.contains("失败记忆检索未完成"));
        assert!(watch.injected.is_empty() && watch.pending.is_empty());
    }

    #[test]
    fn 工具失败触发注入_同轮同条目只注入一次() {
        // R-162 验收④:同一运行内同条目只注入一次。
        let policy = FakePolicy {
            hits: vec![hit("M-009", "sop")],
        };
        let mut watch = RecallWatch::new(Some(&policy));
        let calls = vec![(
            "c1".into(),
            "edit".into(),
            serde_json::json!({ "path": "src/lib.rs" }),
            "".into(),
        )];
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: "old_string not found in src/lib.rs".into(),
            is_error: true,
        }];
        // 第一次失败:注入 Packet。
        watch.note_step(&calls, &mut results);
        let Part::ToolResult { content, .. } = &results[0] else {
            panic!("expected ToolResult");
        };
        assert!(content.contains("[记忆线索 M-009 | sop]"), "{content}");
        assert!(
            content.contains("适用线索: 先 read 重建 old_string 再重试"),
            "{content}"
        );
        assert!(content.contains("来源: episode E-1 步 3"), "{content}");

        // 同轮第二次同类失败:同条目不重复注入(即使错误措辞略变,去重按条目 id)。
        let mut results2 = vec![Part::ToolResult {
            call_id: "c2".into(),
            content: "old_string not found in src/lib.rs again".into(),
            is_error: true,
        }];
        watch.note_step(&calls, &mut results2);
        let Part::ToolResult { content: c2, .. } = &results2[0] else {
            panic!("expected ToolResult");
        };
        assert_eq!(
            c2, "old_string not found in src/lib.rs again",
            "同轮同条目不得重复注入: {c2}"
        );
        assert!(!c2.contains("记忆线索 M-009"));
    }

    #[test]
    fn 非失败结果不触发_成功工具结果无注入() {
        let policy = FakePolicy {
            hits: vec![hit("M-009", "sop")],
        };
        let mut watch = RecallWatch::new(Some(&policy));
        let calls = vec![(
            "c1".into(),
            "read".into(),
            serde_json::json!({ "path": "src/lib.rs" }),
            "".into(),
        )];
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: "ok".into(),
            is_error: false,
        }];
        watch.note_step(&calls, &mut results);
        let Part::ToolResult { content, .. } = &results[0] else {
            panic!("expected ToolResult");
        };
        assert_eq!(content, "ok", "成功结果不得触发召回");
    }

    #[test]
    fn 受控拒绝不触发失败记忆召回() {
        let policy = FakePolicy {
            hits: vec![hit("M-009", "sop")],
        };
        let mut watch = RecallWatch::new(Some(&policy));
        let calls = vec![(
            "c1".into(),
            "edit".into(),
            serde_json::json!({ "path": "src/lib.rs" }),
            "".into(),
        )];
        let original = "[tool_outcome=needs_correction code=EDIT_ANCHOR_NOT_FOUND]\n请重读锚点";
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: original.into(),
            is_error: true,
        }];
        watch.note_step(&calls, &mut results);
        let Part::ToolResult { content, .. } = &results[0] else {
            panic!("expected ToolResult");
        };
        assert_eq!(content, original, "保护门禁不得召回失败记忆或追加 Packet");
    }

    #[test]
    fn 无策略时纯no_op_不panic() {
        let mut watch = RecallWatch::new(None);
        let calls = vec![(
            "c1".into(),
            "edit".into(),
            serde_json::json!({ "path": "a.rs" }),
            "".into(),
        )];
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: "boom".into(),
            is_error: true,
        }];
        watch.note_step(&calls, &mut results);
        let Part::ToolResult { content, .. } = &results[0] else {
            panic!("expected ToolResult");
        };
        assert_eq!(content, "boom", "无策略必须原样保留结果文本");
    }

    #[test]
    fn miss也落遥测_去重后零注入也留痕() {
        // R-161 修复:此前命中为空直接 continue,miss 从不落库——trigger
        // precision/recall 与"高频失败零覆盖"缺口分析都不可能。现在三种事实
        // 都要可区分:命中且注入 / 检索到但同轮已注入(去重) / 彻底 miss。
        let triggers = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let outcomes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let policy = RecordingPolicy {
            hits: vec![hit("M-009", "sop")],
            triggers: triggers.clone(),
            outcomes: outcomes.clone(),
        };
        {
            let mut watch = RecallWatch::new(Some(&policy));
            let calls = vec![(
                "c1".into(),
                "edit".into(),
                serde_json::json!({ "path": "src/lib.rs" }),
                "".into(),
            )];
            let mut results = vec![Part::ToolResult {
                call_id: "c1".into(),
                content: "old_string not found".into(),
                is_error: true,
            }];
            watch.note_step(&calls, &mut results); // 命中并注入:(1,1)
            let mut results2 = vec![Part::ToolResult {
                call_id: "c2".into(),
                content: "old_string not found again".into(),
                is_error: true,
            }];
            watch.note_step(&calls, &mut results2); // 检索到但去重:(1,0)
        }
        assert_eq!(
            triggers.lock().unwrap().clone(),
            vec![(1, 1), (1, 0)],
            "检索到但去重的触发必须留痕(retrieved=1, injected=0)"
        );

        // 彻底 miss:策略返回空,也必须留痕 (0,0)。
        let triggers2 = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let policy_empty = RecordingPolicy {
            hits: vec![],
            triggers: triggers2.clone(),
            outcomes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        {
            let mut watch = RecallWatch::new(Some(&policy_empty));
            let calls = vec![(
                "c1".into(),
                "bash".into(),
                serde_json::json!({ "command": "cargo test" }),
                "".into(),
            )];
            let mut results = vec![Part::ToolResult {
                call_id: "c1".into(),
                content: "some new failure".into(),
                is_error: true,
            }];
            watch.note_step(&calls, &mut results);
        }
        assert_eq!(
            triggers2.lock().unwrap().clone(),
            vec![(0, 0)],
            "miss 必须落遥测,否则记忆缺口不可见"
        );
    }

    #[test]
    fn 轮末对账_注入后复发judged_false_未复发judged_true() {
        // ACTION_CHANGED 机械口径:注入后同 (tool,kind) 失败计数再涨 → false;
        // 到轮末没再失败 → true。正常路径由 finish 提交，未显式收尾时 Drop 兜底。
        let outcomes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let policy = RecordingPolicy {
            hits: vec![hit("M-009", "sop")],
            triggers: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            outcomes: outcomes.clone(),
        };
        {
            let mut watch = RecallWatch::new(Some(&policy));
            let calls = vec![(
                "c1".into(),
                "edit".into(),
                serde_json::json!({ "path": "src/lib.rs" }),
                "".into(),
            )];
            let mut results = vec![Part::ToolResult {
                call_id: "c1".into(),
                content: "old_string not found".into(),
                is_error: true,
            }];
            watch.note_step(&calls, &mut results); // 注入(count=1)
                                                   // 同类复发:错误原文必须同 kind(failure_kind 按原文归一),
                                                   // 换措辞会落进另一个 (tool,kind) 桶,对账就测不到复发。
            let mut results2 = vec![Part::ToolResult {
                call_id: "c2".into(),
                content: "old_string not found".into(),
                is_error: true,
            }];
            watch.note_step(&calls, &mut results2); // 同类复发(count=2)
        } // drop → 对账
        let got = outcomes.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].memory_id, "M-009");
        assert!(!got[0].failure_not_repeated, "注入后同类失败复发: {got:?}");

        let outcomes2 = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let policy2 = RecordingPolicy {
            hits: vec![hit("M-009", "sop")],
            triggers: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            outcomes: outcomes2.clone(),
        };
        {
            let mut watch = RecallWatch::new(Some(&policy2));
            let calls = vec![(
                "c1".into(),
                "edit".into(),
                serde_json::json!({ "path": "src/lib.rs" }),
                "".into(),
            )];
            let mut results = vec![Part::ToolResult {
                call_id: "c1".into(),
                content: "old_string not found".into(),
                is_error: true,
            }];
            watch.note_step(&calls, &mut results); // 注入后不再失败
        }
        let got2 = outcomes2.lock().unwrap().clone();
        assert_eq!(got2.len(), 1);
        assert!(
            got2[0].failure_not_repeated,
            "注入后到轮末未再记录同类失败: {got2:?}"
        );
    }

    type FailureObservationLog = std::sync::Arc<std::sync::Mutex<Vec<(usize, Vec<String>)>>>;

    #[test]
    #[allow(non_snake_case)]
    fn 失败计数累加_供ReRetrieve判重() {
        // 同 (tool, kind) 第三次失败时 failure_count 应到 3——policy 据此决定
        // ReRetrieve 换 query。用 Arc 共享计数验证 retrieve 收到的 failure_count 递增。
        struct CountingPolicy {
            seen: FailureObservationLog,
        }
        impl RecallPolicy for CountingPolicy {
            fn retrieve(&self, trigger: &RecallTrigger) -> Result<Vec<RecallHit>, String> {
                self.seen.lock().unwrap().push((
                    trigger.failure_count,
                    trigger.previous_retrieved_ids.clone(),
                ));
                if trigger.failure_count == 1 {
                    Ok(vec![hit("M-009", "sop")])
                } else {
                    Ok(vec![])
                }
            }
        }
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let policy = CountingPolicy { seen: seen.clone() };
        let mut watch = RecallWatch::new(Some(&policy));
        let calls = vec![(
            "c1".into(),
            "bash".into(),
            serde_json::json!({ "command": "cargo build" }),
            "".into(),
        )];
        for i in 0..3 {
            let mut results = vec![Part::ToolResult {
                call_id: format!("c{i}"),
                content: "error[E0425]: cannot find value".into(),
                is_error: true,
            }];
            watch.note_step(&calls, &mut results);
        }
        let observations = seen.lock().unwrap().clone();
        let counts: Vec<usize> = observations.iter().map(|(count, _)| *count).collect();
        assert_eq!(counts, vec![1, 2, 3], "失败计数必须随同类失败递增");
        assert!(observations[0].1.is_empty(), "首次召回没有上一轮候选");
        assert_eq!(
            observations[1].1,
            vec!["M-009".to_string()],
            "重复失败必须收到上一轮原始候选"
        );
    }
}
