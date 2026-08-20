//! `run_once_with_parts` 的装配域。
//!
//! 本模块只负责开跑时的工具物化、system 分块、消息初始化和轮级运行态初值；
//! 主循环仍由 `drive.rs` 持有，保持初始化后变量名、生命周期和执行顺序不变。

use super::*;

/// R-202 批6:run_once_with_parts 装配段产物。
///
/// 装配(工具/specs/system 分块/消息初始化/各类运行态)一次性完成,调用方解构后
/// 变量名与内联时代逐字节一致,后续主循环零改动。halt/halted 借用 config 不进入
/// struct(生命周期留在调用方)。
pub(super) struct RunOnceAssembly<'a> {
    pub(super) tools: Vec<Arc<dyn Tool>>,
    pub(super) specs: Vec<ToolSpec>,
    pub(super) context_report: Vec<(String, usize)>,
    pub(super) stable_system: Vec<String>,
    pub(super) refreshable_baseline: String,
    pub(super) messages: Vec<Message>,
    pub(super) total_usage: Usage,
    pub(super) last_input_tokens: Option<u64>,
    pub(super) last_estimated_tokens: Option<u64>,
    pub(super) final_text: String,
    pub(super) max_steps: u32,
    pub(super) session_approved: std::collections::HashSet<(String, String)>,
    pub(super) session_rules: Vec<(String, String)>,
    pub(super) overflow_recoveries: u32,
    pub(super) futile_compactions: u32,
    pub(super) overflow_traces: Vec<String>,
    pub(super) calibration: f64,
    pub(super) redundancy: RedundancyWatch,
    pub(super) recall: RecallWatch<'a>,
    pub(super) step: u32,
}

/// R-173/R-280:task 只在子代理运行时存在时加入工具面。把判定抽成纯函数，
/// 让「关闭即不注册」成为可直接断言的构造层契约，而不是注册后再拒绝。
fn append_subagent_spec(specs: &mut Vec<ToolSpec>, subagents_enabled: bool) {
    if subagents_enabled {
        specs.push(task_spec());
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod subagent_tool_surface_tests {
    use super::*;

    #[test]
    fn disabled_subagents_do_not_add_task_to_tool_specs() {
        let mut specs = Vec::new();
        append_subagent_spec(&mut specs, false);
        assert!(specs.iter().all(|spec| spec.name != "task"));
    }

    #[test]
    fn enabled_subagents_add_task_to_tool_specs() {
        let mut specs = Vec::new();
        append_subagent_spec(&mut specs, true);
        assert!(specs.iter().any(|spec| spec.name == "task"));
    }
}

/// R-202 批6:run_once_with_parts 装配段——工具物化、task 注册、system 分块
/// (agent/baseline/记忆提示)、context_report 账单、prior 清洗与用户消息装载、
/// 以及全部轮级运行态(usage/停止文本/会话批准/压缩计数/校准/冗余门禁/召回)
/// 的初始化。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - task 注册仍只发生在 subagent.is_some()(R-173 批4.5 口径,见内联注释);
/// - baseline 进 Context Epoch,refreshable_baseline 每步在调用方刷新;
/// - messages 先经 filter_message_history 清洗孤儿工具 part,再装载用户消息;
/// - halted 闭包与 config.halt 生命周期耦合,留在调用方构造。
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_run_once<'a>(
    snapshot: &HarnessSnapshot,
    agent: &AgentDef,
    config: &'a RunnerConfig,
    prompt: &str,
    memory_hints: Option<&str>,
    scout_brief: Option<&str>,
    prior: &[Message],
    initial_parts: Option<&[Part]>,
    subagent: Option<&SubagentRuntime>,
) -> RunOnceAssembly<'a> {
    let tools: Vec<Arc<dyn Tool>> = snapshot.materialize_tools();
    let mut specs: Vec<ToolSpec> = tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description(),
            input_schema: t.input_schema(),
        })
        .collect();
    append_subagent_spec(&mut specs, subagent.is_some());

    // system 分块:agent 提示词 + harness baseline(M2 起 baseline 进 Context Epoch)。
    let (baseline, mut context_report) = snapshot.stable_system_baseline_with_report();
    let (refreshable_baseline, refreshable_report) =
        snapshot.refreshable_system_baseline_with_report();
    context_report.extend(refreshable_report);
    if !agent.system.trim().is_empty() {
        context_report.insert(0, ("agent/system".into(), agent.system.chars().count()));
    }
    // 工具 schema 是每轮上下文里最大的一块之一(桌面 dev 档 26 个工具的完整 JSON
    // Schema),estimate_prompt_tokens 也把它算进 prompt。账单要回答"本轮上下文里
    // 有什么、各占多少",漏掉它等于漏掉最大的那一项(R-106)。
    let spec_chars: usize = specs
        .iter()
        .map(|spec| {
            spec.name.chars().count()
                + spec.description.chars().count()
                + spec.input_schema.to_string().chars().count()
        })
        .sum();
    if spec_chars > 0 {
        context_report.push(("tools/schema".into(), spec_chars));
    }
    // D-185:记忆提示块是稳定 system 段(每步都在,同 agent/system),但**不进
    // messages**——messages 是持久化与回灌的载体,进去就会被逐轮累积回灌。
    // context_report 单独记 memory/hints,让注入 token 账单能看到 hint 段的占比。
    if let Some(hints) = memory_hints {
        if !hints.trim().is_empty() {
            context_report.push(("memory/hints".into(), hints.chars().count()));
        }
    }
    // 勘察简报同 D-185 待遇:稳定 system 段,不进 messages。它原先被拼进 prompt,
    // 于是随 User message 落进 conversations,下一轮作为 prior 回灌——而流水线每轮
    // 都会重新勘察,回灌的那份旧简报永远不是最新可用信息,只是让 agent 多花一次
    // 「这是不是上轮残留」的分辨成本。单独记账,让它的 token 占比可见。
    if let Some(brief) = scout_brief {
        if !brief.trim().is_empty() {
            context_report.push(("scout/brief".into(), brief.chars().count()));
        }
    }
    let mut stable_system: Vec<String> = [agent.system.clone(), baseline]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if let Some(hints) = memory_hints {
        if !hints.trim().is_empty() {
            stable_system.push(hints.to_string());
        }
    }
    if let Some(brief) = scout_brief {
        if !brief.trim().is_empty() {
            stable_system.push(brief.to_string());
        }
    }

    // prior 可能来自旧快照或跨进程恢复，先统一清洗孤儿工具 part，避免首次请求
    // 在尚未触发上下文压缩时就把非法消息交给 provider。
    let mut messages: Vec<Message> = crate::history::filter_message_history(prior);
    let user_parts = match initial_parts {
        Some(parts) => {
            let mut parts = parts.to_vec();
            if !prompt.is_empty() {
                parts.insert(
                    0,
                    Part::Text {
                        text: prompt.to_string(),
                    },
                );
            }
            parts
        }
        None => vec![Part::Text {
            text: prompt.to_string(),
        }],
    };
    messages.push(Message {
        role: Role::User,
        parts: user_parts,
    });
    let total_usage = Usage::default();
    // R-236 B1:轮末优先使用最近一次 provider usage.input；None 仅表示本轮没有有效 usage。
    let last_input_tokens: Option<u64> = None;
    let last_estimated_tokens: Option<u64> = None;
    // D-342:停止检查点用。提前初始化——halted 提前返回时它是「最近一步的文本」。
    let final_text = String::new();
    // 存量 agent 可能仍带 steps=0；运行边界统一转换，不能让旧配置绕过有限上限。
    let max_steps = kanzei_harness::effective_agent_steps(agent.steps);
    // 本次运行内已放行的 (action, resource):同一资源不重复问(用户反馈:别烦我)。
    let session_approved: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // "总是允许"的会话内即时生效层(D-006):快照是开跑时定死的,新写入的规则
    // 本次运行读不到——泛化 pattern 记在这里,同类资源当场不再询问。
    let session_rules: Vec<(String, String)> = Vec::new();

    let overflow_recoveries = 0;
    // 主动压缩的连续无效计数(D-206),与被动恢复各记各的。只数"压了没用",
    // 成功的压缩清零——压缩是常规运营动作,不设总量配额。
    let futile_compactions = 0u32;
    let overflow_traces: Vec<String> = Vec::new();
    // D-592:B2 冷启动先用保守上限,避免首个真实 usage 到达前中文/代码/工具 schema
    // 的 bytes/4 系统性低估放飞;收到 usage 后再由 EMA 持续下调。
    let calibration = conservative_calibration();
    // R-100 冗余机械门禁:按单次运行持有(跨轮清零),提醒追加进工具结果不阻断。
    let redundancy = RedundancyWatch::default();
    // R-162 事件触发召回:工具失败瞬间把相关记忆 Packet 注入下一请求前。
    // 策略从 config 借用(不拥有);None = 关闭召回,零行为变化。
    let recall = RecallWatch::new(config.recall.as_deref());

    let step = 0u32;
    RunOnceAssembly {
        tools,
        specs,
        context_report,
        stable_system,
        refreshable_baseline,
        messages,
        total_usage,
        last_input_tokens,
        last_estimated_tokens,
        final_text,
        max_steps,
        session_approved,
        session_rules,
        overflow_recoveries,
        futile_compactions,
        overflow_traces,
        calibration,
        redundancy,
        recall,
        step,
    }
}
