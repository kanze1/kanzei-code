//! `run_once_with_parts` 的装配域。
//!
//! 本模块只负责开跑时的工具物化、system 分块、消息初始化和轮级运行态初值；
//! 主循环仍由 `drive.rs` 持有，保持初始化后变量名、生命周期和执行顺序不变。

use super::*;

/// Construct the provider request alongside the existing context assembly owner.
pub(super) fn build_request(
    config: &RunnerConfig,
    system: &[String],
    messages: Vec<Message>,
    specs: &[ToolSpec],
) -> LlmRequest {
    LlmRequest {
        model: config.model.clone(),
        system: system.to_vec(),
        messages,
        tools: specs.to_vec(),
        hosted_tools: vec![],
        max_tokens: config.max_tokens,
        temperature: None,
        reasoning: config.reasoning,
        service_tier: config.service_tier.clone(),
    }
}

/// Publish the same report on both success and required-source failure.
pub(super) fn observe_context_sources(
    baseline: kanzei_harness::ContextBaseline,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> anyhow::Result<(String, Vec<kanzei_harness::ContextSourceReport>)> {
    let sources = match &baseline {
        Ok((_, sources)) => sources,
        Err(error) => &error.sources,
    };
    on_event(RunEvent::ContextSources {
        sources: sources.clone(),
    });
    Ok(baseline?)
}

/// Preserve the existing character ledger, replacing refreshing rows rather than
/// accumulating historical sizes or leaving stale rows after an empty/failed read.
pub(super) fn update_source_ledger(
    ledger: &mut Vec<(String, usize)>,
    sources: &[kanzei_harness::ContextSourceReport],
) {
    for source in sources {
        ledger.retain(|(key, _)| key != &source.key);
        if source.status == kanzei_harness::ContextSourceStatus::Loaded {
            ledger.push((source.key.clone(), source.chars));
        }
    }
}

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
    pub(super) stable_system_sources: Vec<Vec<(String, usize)>>,
    pub(super) refreshable_baseline: String,
    pub(super) refreshable_sources: Vec<(String, usize)>,
    pub(super) context_source_reads: Vec<kanzei_harness::ContextSourceReport>,
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
    pub(super) recall: RecallWatch<'a>,
    pub(super) step: u32,
}

/// R-173/R-280:task 只在子代理运行时存在时加入工具面。把判定抽成纯函数，
/// 让「关闭即不注册」成为可直接断言的构造层契约，而不是注册后再拒绝。
fn append_subagent_spec(
    specs: &mut Vec<ToolSpec>,
    subagents_enabled: bool,
    subagent: Option<&SubagentRuntime>,
) {
    if !subagents_enabled || subagent.is_some_and(|rt| !rt.options.mode.enabled()) {
        return;
    }
    // R-327:schema 的人格枚举取自**运行时实际持有的名册**,不硬编码。
    match subagent {
        Some(rt) => specs.push(
            rt.options
                .host
                .as_ref()
                .map(|host| host.spec())
                .unwrap_or_else(|| task_spec_for(&rt.agent_names())),
        ),
        None => specs.push(task_spec()),
    }
}

/// Assemble tool/system sources, clean paired history and project the current input.
/// Live execution and durable session facts use the same source-aware input projection.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_run_once<'a>(
    snapshot: &HarnessSnapshot,
    agent: &AgentDef,
    config: &'a RunnerConfig,
    prompt: &str,
    ctx: &kanzei_harness::ToolCtx,
    memory_hints: Option<&str>,
    invoked_skills: Option<&kanzei_harness::skills::InvokedSkills>,
    prior: &[Message],
    initial_parts: Option<&[Part]>,
    subagent: Option<&SubagentRuntime>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> anyhow::Result<RunOnceAssembly<'a>> {
    let tools: Vec<Arc<dyn Tool>> = snapshot.materialize_tools();
    let mut specs: Vec<ToolSpec> = snapshot
        .resident_tools()
        .iter()
        .map(|tool| super::tool_spec(tool.as_ref()))
        .collect();
    append_subagent_spec(&mut specs, subagent.is_some(), subagent);

    // system 分块:agent 提示词 + harness baseline(M2 起 baseline 进 Context Epoch)。
    let (baseline, stable_report) =
        observe_context_sources(snapshot.stable_system_baseline_with_report(), on_event)?;
    let (refreshable_baseline, refreshable_report) =
        observe_context_sources(snapshot.refreshable_system_baseline_with_report(), on_event)?;
    let mut context_report = Vec::new();
    update_source_ledger(&mut context_report, &stable_report);
    update_source_ledger(&mut context_report, &refreshable_report);
    let deferred_catalog = snapshot.deferred_catalog();
    if let Some(catalog) = deferred_catalog.as_ref() {
        context_report.push(("tools/catalog".into(), catalog.chars().count()));
    }
    if !agent.system.trim().is_empty() {
        context_report.insert(0, ("agent/system".into(), agent.system.chars().count()));
    }
    // 初始 tools/schema 只统计 resident 与 task;deferred catalog 单列 tools/catalog,
    // 后续动态加载的 schema 各自记入 tools/loaded:<name>。预算估算始终使用当前 specs。
    let spec_chars: usize = specs.iter().map(ToolSpec::char_len).sum();
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
    let mut stable_system: Vec<String> = [agent.system.clone(), baseline]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    let mut stable_system_sources = Vec::new();
    if !agent.system.trim().is_empty() {
        stable_system_sources.push(vec![("agent/system".into(), agent.system.len())]);
    }
    let stable_sources = source_spans(&stable_report);
    if !stable_sources.is_empty() {
        stable_system_sources.push(stable_sources);
    }
    let refreshable_sources = source_spans(&refreshable_report);
    let context_source_reads = stable_report
        .into_iter()
        .chain(refreshable_report)
        .collect();
    if let Some(runtime) = subagent {
        let mut guidance = runtime.options.mode.guidance().to_string();
        if runtime.options.mode.enabled() {
            guidance.push('\n');
            guidance.push_str(kanzei_harness::subagent_policy::DELEGATION_GUIDANCE);
        }
        context_report.push(("runtime/delegation".into(), guidance.chars().count()));
        stable_system.push(guidance);
        stable_system_sources.push(vec![(
            "runtime/delegation".into(),
            stable_system.last().unwrap().len(),
        )]);
    }
    if config.ask_policy.records_questions() {
        let instructions = super::question::AUTONOMOUS_DECISIONS;
        context_report.push(("runtime/decisions".into(), instructions.chars().count()));
        stable_system.push(instructions.into());
        stable_system_sources.push(vec![("runtime/decisions".into(), instructions.len())]);
    }
    if let Some(catalog) = deferred_catalog {
        stable_system.push(catalog);
        stable_system_sources.push(vec![(
            "tools/catalog".into(),
            stable_system.last().unwrap().len(),
        )]);
    }
    if let Some(hints) = memory_hints {
        if !hints.trim().is_empty() {
            stable_system.push(hints.to_string());
            stable_system_sources.push(vec![("memory/hints".into(), hints.len())]);
        }
    }

    if let Some(skills) = invoked_skills {
        let text = skills.as_str();
        context_report.push(("skills/invoked".into(), text.chars().count()));
        stable_system.push(text.into());
        stable_system_sources.push(vec![("skills/invoked".into(), text.len())]);
    }

    // prior 可能来自旧快照或跨进程恢复，先统一清洗孤儿工具 part，避免首次请求
    // 在尚未触发上下文压缩时就把非法消息交给 provider。
    let mut messages: Vec<Message> = crate::history::filter_message_history(prior);
    super::tool_catalog::load_deferred_tools_from_history(
        snapshot,
        &messages,
        &mut specs,
        &mut context_report,
    );
    messages.push(crate::input_message(
        ctx.input_origin,
        prompt,
        &ctx.input_attachments,
        initial_parts.unwrap_or_default(),
    ));
    let total_usage = Usage::default();
    // The anchor is complete provider input, including cache reads and writes.
    // None means this run has not received valid usage yet.
    let last_input_tokens: Option<u64> = None;
    let last_estimated_tokens: Option<u64> = None;
    // D-342:停止检查点用。提前初始化——halted 提前返回时它是「最近一步的文本」。
    let final_text = String::new();
    // 子任务不设步数上限；主执行流保留其显式设置。
    let max_steps = kanzei_harness::effective_agent_steps(agent.steps, agent.mode);
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
    // R-162 事件触发召回:工具失败瞬间把相关记忆 Packet 注入下一请求前。
    // 策略从 config 借用(不拥有);None = 关闭召回,零行为变化。
    let recall = RecallWatch::new(config.recall.as_deref());

    let step = 0u32;
    Ok(RunOnceAssembly {
        tools,
        specs,
        context_report,
        stable_system,
        stable_system_sources,
        refreshable_baseline,
        refreshable_sources,
        context_source_reads,
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
        recall,
        step,
    })
}

pub(super) fn source_spans(
    reports: &[kanzei_harness::ContextSourceReport],
) -> Vec<(String, usize)> {
    reports
        .iter()
        .filter(|report| report.status == kanzei_harness::ContextSourceStatus::Loaded)
        .map(|report| (report.key.clone(), report.bytes))
        .collect()
}

#[cfg(test)]
mod subagent_tool_surface_tests {
    use super::*;

    #[test]
    fn disabled_subagents_do_not_add_task_to_tool_specs() {
        let mut specs = Vec::new();
        append_subagent_spec(&mut specs, false, None);
        assert!(specs.iter().all(|spec| spec.name != "task"));
    }

    #[test]
    fn enabled_subagents_add_task_to_tool_specs() {
        let mut specs = Vec::new();
        append_subagent_spec(&mut specs, true, None);
        assert!(specs.iter().any(|spec| spec.name == "task"));
    }
}
