//! 事件触发召回域(R-162 B3):RecallWatch 挂在工具结果回喂钩位,与
//! RedundancyWatch 同款(redundancy.note_step 先例,drive.rs 工具结果回喂前
//! 就地调用)。区别在 RecallWatch 不携带检索实现——core 不依赖 tools,
//! 检索与遥测经 RecallPolicy trait 注入,由 CLI/桌面端把 kanzei-tools 的
//! 记忆检索包装成 policy 传入 RunnerConfig。
//!
//! 语义(R-162 内容④/⑤):
//! - 工具失败 → 错误分类(failure_kind/failure_target,复用 metrics 共享函数)→
//!   交给 policy.retrieve 查指纹;命中则把 Memory Packet 追加进该 ToolResult 文本
//!   (与 [冗余提醒] 同机制),不阻断主循环。
//! - 同一运行内同条目只注入一次(watcher 内去重,防刷屏)。
//! - 每次触发经 policy.record_trigger 落 recall_events(trigger/action/延迟)。
//! - 重复失败(同 tool+kind ≥2)换 query 重查(policy 负责换词,本域只传失败计数)。

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
}

/// 一次命中:要注入的 Memory Packet 内容。
#[derive(Debug, Clone)]
pub struct RecallHit {
    pub id: String,
    pub category: String,
    pub action: String,
    pub status: String,
    pub source: String,
    /// 实际命中的检索层级。由 Retriever 填写，遥测必须原样记录，不能由
    /// 触发次数或其它旁路字段反推。
    pub policy_action: String,
}

/// 一次注入的轮末结局:注入后同 (tool, kind) 是否停止复发。
/// 这是 ACTION_CHANGED 漏斗段的机械口径——"注入了但没拦住复发"与
/// "注入后失败停止"在遥测里必须可区分,否则利用率只能凭感觉。
#[derive(Debug, Clone)]
pub struct RecallOutcome {
    pub memory_id: String,
    pub tool: String,
    pub kind: String,
    /// true = 注入后到轮末同 (tool,kind) 未再失败(行为改变);false = 又失败了。
    pub changed: bool,
}

/// 召回策略接口(core 侧定义,CLI/桌面端注入 kanzei-tools 实现)。
/// core 不依赖 tools:检索/遥测/超时降级全部在实现侧,本域只做触发判定、
/// 去重与注入格式。
pub trait RecallPolicy: Send + Sync {
    /// 给定失败触发,返回要注入的 Packet。实现侧负责 Tier0 fingerprint →
    /// Tier1 BM25、超时降级、ReRetrieve 换 query;本域不感知检索细节。
    fn retrieve(&self, trigger: &RecallTrigger) -> Vec<RecallHit>;

    /// 每次实际触发落遥测(trigger/action/延迟)。空实现 = 不落库。
    /// `retrieved` 是检索原始结果(含同轮已注入而被去重的),`injected` 是真进
    /// 本次结果文本的子集——两者都可为空,**miss 也必须留痕**,否则
    /// trigger precision/recall 永远算不出来、记忆缺口(高频失败无覆盖)不可见。
    fn record_trigger(
        &self,
        _trigger: &RecallTrigger,
        _retrieved: &[RecallHit],
        _injected: &[RecallHit],
        _elapsed_ms: u64,
    ) {
    }

    /// 轮末回收:每条注入的行为改变判定(ACTION_CHANGED)。空实现 = 不落库。
    fn record_outcomes(&self, _outcomes: &[RecallOutcome]) {}
}

/// R-162 事件触发召回 watcher:状态按单次运行持有(与 RedundancyWatch 同规,
/// 轮与轮之间不复用)。挂在工具结果回喂前,就地追加 Packet 文本,不阻断。
/// 策略经引用借用(不拥有)——drive.rs 从 &RunnerConfig 借出,不取走。
pub struct RecallWatch<'a> {
    /// 失败计数:(tool, kind) → 本轮次数。
    failures: HashMap<(String, String), usize>,
    /// 已注入条目 id(同轮同条目只注入一次,防刷屏——设计 §3.3 按条目去重,
    /// 不按 (tool,kind),否则同类失败换了个措辞就重复注入)。
    injected: HashSet<String>,
    /// 待判定的注入结局:(memory_id, tool, kind, 注入时该 kind 的失败次数)。
    /// Drop 时对账——若失败计数没再涨,判定 ACTION_CHANGED 成立。
    pending: Vec<(String, String, String, usize)>,
    /// 注入的策略(检索 + 遥测)。
    policy: Option<&'a dyn RecallPolicy>,
}

impl<'a> RecallWatch<'a> {
    pub fn new(policy: Option<&'a dyn RecallPolicy>) -> Self {
        Self {
            failures: HashMap::new(),
            injected: HashSet::new(),
            pending: Vec::new(),
            policy,
        }
    }

    /// 在整步工具结果回喂前调用:`results` 与 `calls` 按下标一一对应
    /// (与 redundancy::note_step 同一不变式)。只追加、不改 is_error。
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
                kind,
                sample: content.chars().take(240).collect(),
                target,
                failure_count: count,
            };
            let started = std::time::Instant::now();
            let retrieved = policy.retrieve(&trigger);
            let elapsed = started.elapsed().as_millis() as u64;
            // 同轮同条目只注入一次:已注入的 id 从注入集丢弃,但保留在 retrieved
            // 里落遥测——"检索到但没再注入"与"根本没检索到"是两种漏斗事实。
            let hits: Vec<RecallHit> = retrieved
                .iter()
                .filter(|hit| !self.injected.contains(&hit.id))
                .cloned()
                .collect();
            for hit in &hits {
                self.injected.insert(hit.id.clone());
            }
            // miss 也落遥测(retrieved/injected 皆空):没有 miss 记录就永远
            // 看不见"高频失败但零记忆覆盖"的缺口。
            policy.record_trigger(&trigger, &retrieved, &hits, elapsed);
            if hits.is_empty() {
                continue;
            }
            // 追加 Packet 文本(与 [冗余提醒] 同机制:就地进结果文本,模型可见)。
            for hit in &hits {
                self.pending.push((
                    hit.id.clone(),
                    trigger.tool.clone(),
                    trigger.kind.clone(),
                    trigger.failure_count,
                ));
                content.push_str(&format!(
                    "\n[记忆命中 {id} | {category}]\n\
                     行动: {action}\n\
                     状态: {status} · 来源: {source}",
                    id = hit.id,
                    category = hit.category,
                    action = hit.action,
                    status = hit.status,
                    source = hit.source,
                ));
            }
        }
    }
}

impl Drop for RecallWatch<'_> {
    /// 轮末对账(所有退出路径统一覆盖,含错误提前返回):每条注入按
    /// "注入后同 (tool,kind) 失败计数是否再涨"机械判定 ACTION_CHANGED,
    /// 交给 policy 落库。无注入或无策略时零成本。
    fn drop(&mut self) {
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
                    changed: now <= at_count,
                }
            })
            .collect();
        policy.record_outcomes(&outcomes);
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
        fn retrieve(&self, _trigger: &RecallTrigger) -> Vec<RecallHit> {
            self.hits.clone()
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
        fn retrieve(&self, _trigger: &RecallTrigger) -> Vec<RecallHit> {
            self.hits.clone()
        }
        fn record_trigger(
            &self,
            _trigger: &RecallTrigger,
            retrieved: &[RecallHit],
            injected: &[RecallHit],
            _elapsed_ms: u64,
        ) {
            self.triggers
                .lock()
                .unwrap()
                .push((retrieved.len(), injected.len()));
        }
        fn record_outcomes(&self, outcomes: &[RecallOutcome]) {
            self.outcomes.lock().unwrap().extend(outcomes.to_vec());
        }
    }

    fn hit(id: &str, category: &str) -> RecallHit {
        RecallHit {
            id: id.into(),
            category: category.into(),
            action: "先 read 重建 old_string 再重试".into(),
            status: "active".into(),
            source: "episode E-1 步 3".into(),
            policy_action: "fingerprint".into(),
        }
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
        assert!(content.contains("[记忆命中 M-009 | sop]"), "{content}");
        assert!(
            content.contains("行动: 先 read 重建 old_string 再重试"),
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
        assert!(!c2.contains("记忆命中 M-009"));
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
        // 到轮末没再失败 → true。Drop 统一回收,所有退出路径都覆盖。
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
        assert!(!got[0].changed, "注入后同类又失败,行为未改变: {got:?}");

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
        assert!(got2[0].changed, "注入后未再失败,行为改变成立: {got2:?}");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 失败计数累加_供ReRetrieve判重() {
        // 同 (tool, kind) 第三次失败时 failure_count 应到 3——policy 据此决定
        // ReRetrieve 换 query。用 Arc 共享计数验证 retrieve 收到的 failure_count 递增。
        struct CountingPolicy {
            seen: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
        }
        impl RecallPolicy for CountingPolicy {
            fn retrieve(&self, trigger: &RecallTrigger) -> Vec<RecallHit> {
                self.seen.lock().unwrap().push(trigger.failure_count);
                vec![]
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
        let counts = seen.lock().unwrap().clone();
        assert_eq!(counts, vec![1, 2, 3], "失败计数必须随同类失败递增");
    }
}
