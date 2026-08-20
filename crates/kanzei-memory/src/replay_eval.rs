//! R-163 批4:真实评估装配——把记忆检索策略与 LLM 真调接到六臂回放。
//!
//! core 侧的 [`kanzei_core::replay`] 定义了六臂机制与判据,但检索与 LLM
//! 属于 tools 域(依赖 kanzei-tools 的记忆栈与 kanzei-llm 的 client)。
//! 本模块提供两个装配件:
//!
//! 1. [`ReplayMemoryProvider`]:实现 [`MemoryContextProvider`],六臂各自的
//!    记忆注入:
//!    - NoMemory:空(下界);
//!    - Current:现有 [`FailureRecallPolicy`] 召回(指纹 → BM25);
//!    - Candidate:新策略(本批先与 Current 同源,策略差异留给后续条目);
//!    - Oracle:优先取 case 自动事后正确做法([`oracle_text_from_case`]),
//!      人工标定条目可后续覆盖;
//!    - LeaveOneOut:Current 召回中去掉第一条(消融);
//!    - CompressionCF:合并前原始文本(本批先用召回原文,合并器未落地)。
//! 2. [`LlmDecider`]:实现 [`ReplayDecider`],用 `LlmClient` 真调(fast 档),
//!    从流里收 TextDelta 拼决策文本,token 取 Usage.output。
//!
//! 两者组合即可在 CLI/桌面端跑真实批(验收②:≥30 case 可重复执行)。

use kanzei_core::replay::{
    oracle_text_from_case, Arm, MemoryContextProvider, ReplayCase, ReplayDecider,
};
use kanzei_core::RecallPolicy;
use kanzei_llm::{LlmClient, LlmEvent, LlmRequest, Message, Route, Usage};
use std::sync::Arc;

use crate::memory::{FailureRecallPolicy, IndexQuery, SqliteMemoryIndex};
use kanzei_harness::config::KanzeiConfig;

/// 把六臂记忆注入接到真实检索策略上。
pub struct ReplayMemoryProvider {
    /// Current/LeaveOneOut/CompressionCF 共用现状策略(fingerprint → BM25)。
    policy: FailureRecallPolicy,
    /// Candidate 臂:三通道混合检索(fingerprint → BM25 + dense → RRF 融合,
    /// R-164 批4 装配)。无 [embeddings] 配置时退化为 lexical,与 Current 同源。
    hybrid: SqliteMemoryIndex,
    /// 供 RecallEvent 落库(state.db)定位。
    project_root: std::path::PathBuf,
}

impl ReplayMemoryProvider {
    pub fn new(project_root: &std::path::Path) -> Self {
        let policy = FailureRecallPolicy::new(project_root);
        // 尝试从 [embeddings] 启用向量通道;未配置/provider 不可用 → None,
        // hybrid 自动退化为 lexical(设计 §5 验收①)。
        let embedder: Option<Arc<dyn crate::embed::Embedder>> = KanzeiConfig::load(project_root)
            .ok()
            .and_then(|cfg| crate::embed::OpenAiEmbedder::from_config(&cfg).ok())
            .map(|e| Arc::new(e) as Arc<dyn crate::embed::Embedder>);
        let hybrid = SqliteMemoryIndex::with_embedder(project_root, embedder);
        Self {
            policy,
            hybrid,
            project_root: project_root.to_path_buf(),
        }
    }

    /// 从 case 构造 RecallTrigger(第一个失败步骤的 tool/kind/sample)。
    /// kind 用归一化错误文本首段(抹掉路径与数字),与运行时指纹语义对齐。
    fn trigger_for(case: &ReplayCase) -> Option<kanzei_core::RecallTrigger> {
        let failed = case.steps.iter().find(|s| !s.ok)?;
        let sample = failed
            .error
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect::<String>();
        let kind = normalize_kind(&sample);
        Some(kanzei_core::RecallTrigger {
            tool: failed.tool.clone(),
            kind,
            sample,
            target: String::new(),
            failure_count: 1,
        })
    }

    /// Current 臂:policy 召回 → 拼 Packet 文本;无触发时返回空。
    fn current_text(&self, case: &ReplayCase) -> String {
        let Some(trigger) = Self::trigger_for(case) else {
            return String::new();
        };
        let hits = self.policy.retrieve(&trigger);
        if hits.is_empty() {
            return String::new();
        }
        let mut out = String::from("[recalled] 失败时相关的记忆条目:\n");
        for hit in hits {
            out.push_str(&format!("- [{}] {}\n", hit.category, hit.action));
        }
        out
    }

    /// Candidate 臂:三通道混合检索(fingerprint → RRF 融合),并落 RecallEvent
    /// (policy_action=hybrid,分段延迟填 lexical_ms/embed_ms/vector_ms——验收②)。
    /// 无 [embeddings] 时 hybrid 自动退化为 lexical,与 Current 同源(降级路径)。
    fn candidate_text(&self, case: &ReplayCase) -> String {
        let Some(trigger) = Self::trigger_for(case) else {
            return String::new();
        };
        let mut query_text = trigger.sample.chars().take(120).collect::<String>();
        if !trigger.target.is_empty() {
            query_text.push(' ');
            query_text.push_str(&trigger.target);
        }
        let query = IndexQuery::both(&trigger.tool, &trigger.kind, &query_text);
        let (hits, timing) = self.hybrid.search_hybrid_with_timing(&query, 5);
        if hits.is_empty() {
            return String::new();
        }
        let recall_hits: Vec<kanzei_core::RecallHit> = hits
            .iter()
            .map(|h| kanzei_core::RecallHit {
                id: h.id.clone(),
                category: h.category.clone(),
                action: h.action.clone(),
                status: h.status.clone(),
                source: format!("memory_hybrid:{}", h.id),
                policy_action: "hybrid".into(),
            })
            .collect();
        // 落 recall_events(与运行时 event_recall 同表,trigger_type 区分来源)。
        let ids: Vec<&str> = recall_hits.iter().map(|h| h.id.as_str()).collect();
        if let Ok(ids_json) = serde_json::to_string(&ids) {
            let path = self.project_root.join(".kanzei").join("state.db");
            if let Ok(store) = kanzei_core::SessionStore::open(&path) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default();
                let event = kanzei_core::RecallEvent {
                    recall_id: &format!("replay-candidate-{now}"),
                    episode_id: None,
                    step_id: None,
                    trigger_type: "replay_eval",
                    // serde_json 序列化:kind 是错误原文前缀,含引号/换行时手拼
                    // JSON 会产出坏行(实测 recall_events 12% payload 解析不了)。
                    trigger_payload: &serde_json::json!({
                        "tool": trigger.tool,
                        "kind": trigger.kind,
                    })
                    .to_string(),
                    policy_action: "hybrid",
                    query: &query_text.chars().take(120).collect::<String>(),
                    candidate_ids: &ids_json,
                    retrieved_ids: &ids_json,
                    injected_ids: &ids_json,
                    lexical_ms: timing.lexical_ms,
                    embed_ms: timing.embed_ms,
                    vector_ms: timing.vector_ms,
                    total_ms: timing.total(),
                };
                let _ = store.record_recall_event(&event);
            }
        }
        let mut out = String::from("[recalled] 失败时相关的记忆条目:\n");
        for hit in recall_hits {
            out.push_str(&format!("- [{}] {}\n", hit.category, hit.action));
        }
        out
    }
}

impl MemoryContextProvider for ReplayMemoryProvider {
    fn context_for(&self, arm: &Arm, case: &ReplayCase) -> String {
        match arm {
            Arm::NoMemory => String::new(),
            Arm::Current => self.current_text(case),
            // R-164 批4:新策略 = 三通道混合检索(有 [embeddings] 配置时真正融合;
            // 未配置时退化为 lexical,与 Current 同源,对照可比)。
            Arm::Candidate => self.candidate_text(case),
            Arm::Oracle => oracle_text_from_case(case),
            // 消融:去掉第一条命中(最容易系统性折叠的 id——D-230)。
            Arm::LeaveOneOut => {
                let mut text = self.current_text(case);
                if let Some(pos) = text.find("\n- ") {
                    // 删掉第一条命中的整行(含其后的换行)。
                    if let Some(end) = text[pos + 1..].find("\n- ") {
                        text.replace_range(pos..pos + 1 + end, "");
                    } else {
                        text.truncate(pos);
                    }
                }
                text
            }
            // 合并前后对照:合并器未落地,先与 Current 同源(报告会显示相同)。
            Arm::CompressionCF => self.current_text(case),
        }
    }

    fn evaluation_memory_ids(&self, case: &ReplayCase) -> Vec<String> {
        // Leave-One-Out 当前只消融第一条 Current 命中,因此只把这条真实
        // memory_id 作为本 case 的 F(m) 目标;不把 case_id 当作记忆主键。
        let Some(trigger) = Self::trigger_for(case) else {
            return Vec::new();
        };
        self.policy
            .retrieve(&trigger)
            .into_iter()
            .next()
            .map(|hit| vec![hit.id])
            .unwrap_or_default()
    }
}

/// 归一化错误 kind:小写、去引号、截断数字与路径噪音,取首 40 字符。
fn normalize_kind(sample: &str) -> String {
    sample
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(40)
        .collect()
}

/// 用真实 LLM 做回放决策(fast 档跑批)。
pub struct LlmDecider {
    client: Arc<LlmClient>,
    route: Arc<Route>,
    model: String,
    /// 决策 max_tokens(快速决策,不需要长输出)。
    max_tokens: u32,
}

impl LlmDecider {
    pub fn new(client: Arc<LlmClient>, route: Arc<Route>, model: String) -> Self {
        Self {
            client,
            route,
            model,
            max_tokens: 256,
        }
    }
}

impl ReplayDecider for LlmDecider {
    fn decide<'a>(
        &'a self,
        question: &'a str,
        memory_context: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<(String, u64)>> + Send + 'a>,
    > {
        Box::pin(async move {
            let mut system = vec!["你是回放评估台:面对一个历史失败场景,给出**下一步行动**。\
                 只输出要执行的工具与理由,不要复述背景。"
                .to_string()];
            if !memory_context.trim().is_empty() {
                system.push(format!("以下记忆供参考:\n{memory_context}"));
            }
            let request = LlmRequest {
                model: self.model.clone(),
                system,
                messages: vec![Message::user_text(question.to_string())],
                tools: vec![],
                max_tokens: self.max_tokens,
                temperature: None,
                reasoning: kanzei_llm::ReasoningEffort::Off,
                service_tier: None,
            };
            let mut stream = self.client.stream(&self.route, &request).await?;
            let mut text = String::new();
            let mut usage = Usage::default();
            use futures::StreamExt as _;
            while let Some(event) = stream.next().await {
                match event? {
                    LlmEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
                    LlmEvent::StepFinish { usage: u, .. } => usage = u,
                    _ => {}
                }
            }
            Ok((text.trim().to_string(), usage.output))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::replay::parse_trace_payload;

    const SAMPLE: &str = r#"{
      "events": [
        {"at": 1, "id": "a", "kind": "tool.started", "name": "edit", "summary": "{}"},
        {"at": 2, "durationMs": 5, "error": "old_string not found", "id": "a", "kind": "tool.completed", "name": "edit", "ok": false},
        {"at": 3, "id": "b", "kind": "tool.started", "name": "read", "summary": "{\"path\":\"src/main.rs\"}"},
        {"at": 4, "durationMs": 5, "id": "b", "kind": "tool.completed", "name": "read", "ok": true}
      ],
      "outcome": "completed"
    }"#;

    #[test]
    fn normalize_kind_抹掉数字与符号只留词干() {
        assert_eq!(
            normalize_kind("old_string not found at line 42"),
            "old string not found at line"
        );
        assert_eq!(normalize_kind("路径 /a/b/c 不存在"), "a b c");
    }

    #[test]
    fn oracle臂注入自动事后正确做法_no_memory臂恒空() {
        // 临时目录,避免在源码树里留下 memory 索引(D-174 文件隔离精神)。
        let root = std::env::temp_dir().join(format!("kz-replay-eval-{}", std::process::id()));
        let provider = ReplayMemoryProvider::new(&root);
        let case = parse_trace_payload(SAMPLE, "t").unwrap();
        // Oracle:失败后成功步骤(read)合成事后正确做法。
        let oracle = provider.context_for(&Arm::Oracle, &case);
        assert!(oracle.contains("[oracle]"), "{oracle}");
        assert!(oracle.contains("read"), "{oracle}");
        // NoMemory 恒空。
        assert!(provider.context_for(&Arm::NoMemory, &case).is_empty());
        // 空记忆目录 + 无 [embeddings] 配置:Candidate 退化为 lexical,与 Current
        // 同源(都无命中 → 空);CompressionCF 与 Current 同源(合并器未落地)。
        assert_eq!(
            provider.context_for(&Arm::Candidate, &case),
            provider.context_for(&Arm::Current, &case)
        );
        assert_eq!(
            provider.context_for(&Arm::CompressionCF, &case),
            provider.context_for(&Arm::Current, &case)
        );
    }

    #[test]
    fn candidate臂_有记忆条目时用hybrid检索并落recall_events() {
        // 验收②装配:Candidate 臂走三通道混合检索,命中后分段延迟落 recall_events。
        let root = std::env::temp_dir().join(format!("kz-replay-eval-cand-{}", std::process::id()));
        // 清理历史残留,保证断言独立。
        let _ = std::fs::remove_dir_all(&root);
        let store = crate::memory::MemoryStore::project(&root);
        // 种子:一条与 SAMPLE 失败指纹(tool=edit,kind=old string not found)精确匹配的
        // 记忆条目——fingerprint 触发应命中。
        store
            .append_note(
                "fixture source",
                "[fp:edit|old string not found]",
                "fact",
                &[],
            )
            .unwrap();
        store
            .add(
                "fact",
                "edit 失败处理",
                "old_string not found 时先 read",
                "[fp:edit|old string not found]",
                "replay-test",
                &[],
                None,
                true,
            )
            .unwrap();
        // R-165:编译产物须带证据晋升 active 才可检索(provenance 硬约束)。
        let (cand_id, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.title == "edit 失败处理")
            .map(|(p, e)| (e.id, p))
            .unwrap();
        let eid = crate::memory::seed_episode(&root, "ses");
        store
            .promote(&cand_id, &[(eid, Some(0), Some(5))], Some("replay-test"))
            .unwrap();
        // 构造带 embedder 的 provider(FakeEmbedder:同文本同向量,query 用相同文本必命中)。
        let mut provider = ReplayMemoryProvider::new(&root);
        provider.hybrid = SqliteMemoryIndex::with_embedder(
            &root,
            Some(Arc::new(crate::embed::FakeEmbedder::new(16))),
        );
        // rebuild 生成向量(FakeEmbedder 可用),让 dense 通道真正参与融合。
        use crate::memory::MemoryIndex as _;
        provider.hybrid.rebuild().unwrap();
        let case = parse_trace_payload(SAMPLE, "t5").unwrap();
        assert_eq!(
            provider.evaluation_memory_ids(&case),
            vec![cand_id.clone()],
            "回放评估目标必须是检索命中的真实 memory_id"
        );
        let candidate = provider.context_for(&Arm::Candidate, &case);
        assert!(
            candidate.contains("[recalled]"),
            "Candidate 必须命中 fingerprint 记忆: {candidate:?}"
        );
        assert!(
            candidate.contains("old_string not found 时先 read"),
            "{candidate:?}"
        );
        // recall_events 落库:policy_action=hybrid 且分段延迟可查(直连 state.db,
        // core 无通用读 API;recall_id 主键为 replay-candidate- 前缀)。
        let conn = rusqlite::Connection::open(root.join(".kanzei").join("state.db")).unwrap();
        let rows: Vec<(String, i64, i64, i64)> = conn
            .prepare(
                "SELECT policy_action, lexical_ms, embed_ms, vector_ms
                 FROM recall_events WHERE trigger_type='replay_eval'
                 ORDER BY created_at DESC LIMIT 1",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows.len(), 1, "必须恰好落一条 replay_eval 事件");
        let (action, _lex, embed_ms, _vec) = &rows[0];
        assert_eq!(action, "hybrid", "policy_action 必须是 hybrid");
        // FakeEmbedder 极快,embed_ms 可能为 0;只验证列存在且已填。
        let _ = embed_ms;
    }

    #[test]
    fn leave_one_out_去掉第一条命中_无命中时不变() {
        let root = std::env::temp_dir().join(format!("kz-replay-eval-loo-{}", std::process::id()));
        let provider = ReplayMemoryProvider::new(&root);
        let case = parse_trace_payload(SAMPLE, "t2").unwrap();
        let current = provider.context_for(&Arm::Current, &case);
        let loo = provider.context_for(&Arm::LeaveOneOut, &case);
        if current.is_empty() {
            // 无命中:消融无物可消,与 Current 相同。
            assert_eq!(loo, current);
        } else {
            assert_ne!(loo, current, "有命中时消融必须改变注入");
            assert!(loo.len() < current.len());
            assert!(!loo.contains("old_string not found"), "第一条命中被移除");
        }
    }

    #[test]
    fn trigger构造_取第一个失败步骤() {
        let case = parse_trace_payload(SAMPLE, "t3").unwrap();
        let trigger = ReplayMemoryProvider::trigger_for(&case).unwrap();
        assert_eq!(trigger.tool, "edit");
        assert!(trigger.sample.contains("old_string not found"));
        assert!(trigger.kind.contains("old"));
        // 全成功 case 无触发。
        let ok_case = parse_trace_payload(
            r#"{"events":[{"id":"a","kind":"tool.started","name":"git","summary":"{}"},
                {"id":"a","kind":"tool.completed","name":"git","ok":true}],"outcome":"completed"}"#,
            "t4",
        )
        .unwrap();
        assert!(ReplayMemoryProvider::trigger_for(&ok_case).is_none());
    }
}
