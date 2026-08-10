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

use crate::memory::FailureRecallPolicy;

/// 把六臂记忆注入接到真实检索策略上。
pub struct ReplayMemoryProvider {
    /// Current/Candidate/LeaveOneOut 共用的检索策略。
    policy: FailureRecallPolicy,
}

impl ReplayMemoryProvider {
    pub fn new(project_root: &std::path::Path) -> Self {
        Self {
            policy: FailureRecallPolicy::new(project_root),
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
}

impl MemoryContextProvider for ReplayMemoryProvider {
    fn context_for(&self, arm: &Arm, case: &ReplayCase) -> String {
        match arm {
            Arm::NoMemory => String::new(),
            Arm::Current => self.current_text(case),
            // 新策略与现状同源:检索差异由后续条目引入,本批保持对照可比。
            Arm::Candidate => self.current_text(case),
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
            let mut system = vec![
                "你是回放评估台:面对一个历史失败场景,给出**下一步行动**。\
                 只输出要执行的工具与理由,不要复述背景。"
                    .to_string(),
            ];
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
        assert_eq!(normalize_kind("old_string not found at line 42"), "old string not found at line");
        assert_eq!(normalize_kind("路径 /a/b/c 不存在"), "a b c");
    }

    #[test]
    fn oracle臂注入自动事后正确做法_NoMemory臂恒空() {
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
        // Candidate/CompressionCF 与 Current 同源(本批装配约束)。
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
    fn leaveOneOut_去掉第一条命中_无命中时不变() {
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
