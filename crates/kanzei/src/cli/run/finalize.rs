use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use kanzei_harness::config::KanzeiConfig;
use kanzei_harness::{ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, Message, ProxyConfig};

use super::super::memory::consolidate_memory_inbox;

pub(crate) struct FinalizeState<'a> {
    pub(crate) state_path: &'a Path,
    pub(crate) session_id: &'a str,
    pub(crate) typed_writer: Arc<Mutex<kanzei_core::TypedSessionWriter>>,
    pub(crate) prior: &'a [Message],
    pub(crate) prompt: &'a str,
    pub(crate) ctx: &'a ToolCtx,
    pub(crate) config: &'a KanzeiConfig,
    pub(crate) proxy: &'a ProxyConfig,
    pub(crate) client: &'a LlmClient,
    pub(crate) rctx: &'a ResolveCtx,
    pub(crate) provider: &'a str,
    pub(crate) model: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) input_id: &'a str,
    pub(crate) run_started: Instant,
    pub(crate) run_epoch_ms: i64,
}

pub(crate) async fn finish_run(
    run_result: anyhow::Result<kanzei_core::RunSummary>,
    typed_flush_task: tokio::task::JoinHandle<()>,
    state: FinalizeState<'_>,
) -> anyhow::Result<()> {
    let store = kanzei_core::SessionStore::open(state.state_path)?;
    match &run_result {
        Ok(summary) => {
            state
                .typed_writer
                .lock()
                .unwrap()
                .finish(if summary.halted_by_user {
                    kanzei_core::SessionTurnTerminal::Stopped
                } else {
                    kanzei_core::SessionTurnTerminal::Completed
                });
            store.set_status(state.session_id, "idle")?;
            store.append_event(
                state.session_id,
                "session.status_changed",
                &serde_json::json!({ "status": "idle" }),
            )?;
            // 轮末快照继续写入(验收⑦顺延:压缩摘要仍经它持久化,见 R-242 进展)。
            store.append_event(
                state.session_id,
                "conversation.updated",
                &serde_json::json!({ "messages": summary.messages }),
            )?;
            state
                .typed_writer
                .lock()
                .unwrap()
                .write_shadow_report(&summary.messages);
        }
        Err(error) => {
            state
                .typed_writer
                .lock()
                .unwrap()
                .finish(kanzei_core::SessionTurnTerminal::Failed(error.to_string()));
            state
                .typed_writer
                .lock()
                .unwrap()
                .write_shadow_report(state.prior);
            store.set_status(state.session_id, "failed")?;
            store.append_event(
                state.session_id,
                "session.status_changed",
                &serde_json::json!({ "status": "failed" }),
            )?;
            store.append_event(
                state.session_id,
                "run.failed",
                &serde_json::json!({ "error": error.to_string() }),
            )?;
        }
    }
    typed_flush_task.abort();
    let summary = run_result?;

    if summary.halted_by_user {
        eprintln!("\n\x1b[33m(stopped: permission declined)\x1b[0m");
    }
    println!(
        "\n\x1b[90m— steps {} · in {} (cache r{} w{}) · out {}\x1b[0m",
        summary.steps,
        summary.usage.input,
        summary.usage.cache_read,
        summary.usage.cache_write,
        summary.usage.output
    );
    let context_total: usize = summary.context_report.iter().map(|(_, n)| n).sum();
    println!(
        "\x1b[90m— context {} 源 {} 字符\x1b[0m",
        summary.context_report.len(),
        context_total
    );
    // 本轮切片:summary.messages = prior + 本轮。统计与失败提炼都只看本轮,
    // 否则历史失败会被反复上报、工具计数也会累计全历史(R-099 基线失真)。
    let this_run = &summary.messages[state.prior.len().min(summary.messages.len())..];
    // 轮末采集(D-229/D-214):CLI 与桌面端共用 harvest_end_of_run——失败提炼 →
    // 条目收口判定 → SOP 候选(项目 inbox,落库目标 global)→ 根因 fact 候选(项目
    // inbox)。SOP 通道 D-229 起双端一致,D-214 起候选投项目 inbox 进消化通道。
    {
        let (delivered, sop, fact) = kanzei_tools::memory::harvest_end_of_run(
            &state.ctx.project_root,
            state.prompt,
            this_run,
        );
        if delivered > 0 {
            eprintln!("\x1b[90m(memory: 投递 {delivered} 条失败观察待整理)\x1b[0m");
        }
        if sop {
            eprintln!("\x1b[90m(memory: 已投递候选 SOP 待用户采纳)\x1b[0m");
        }
        if fact {
            eprintln!("\x1b[90m(memory: 已投递根因候选待整理)\x1b[0m");
        }
    }
    // episode 落库(R-106):机械轨迹画像,R-099 度量与记忆系统共用。失败不影响本轮。
    // R-213:当轮 episode_id 留到轮末代填给 memory manager——episode 轮末才落库,
    // manager 在轮内自报不出真实 id,不代填则 provenance 校验会拦下一切晋升。
    let mut current_episode_id: Option<i64> = None;
    {
        let outcome = if summary.halted_by_user {
            "halted"
        } else {
            "completed"
        };
        let tools = kanzei_core::summarize_tools(this_run);
        let store = kanzei_core::SessionStore::open(state.state_path)?;
        if let Ok(episode_id) = store.append_episode(&kanzei_core::EpisodeRecord {
            session_id: state.session_id,
            prompt_head: state.prompt,
            outcome,
            steps: summary.steps,
            input_tokens: summary.usage.input,
            output_tokens: summary.usage.output,
            tools_json: &serde_json::to_string(&tools).unwrap_or_default(),
            context_json: &serde_json::to_string(&summary.context_report).unwrap_or_default(),
            // R-099 调用画像:CLI 与桌面端落同一份口径,基线才可比。
            metrics_json: &serde_json::to_string(&kanzei_core::summarize_metrics(this_run))
                .unwrap_or_default(),
            // D-173:轮次归属。没有这几列时,"这一轮跑的哪个模型"只能靠当前配置反推。
            provider: state.provider,
            model: state.model,
            run_id: state.run_id,
            input_id: state.input_id,
            duration_ms: state.run_started.elapsed().as_millis() as u64,
            // R-106:上下文溢出压缩丢弃的轨迹段沉淀为 episode 的一部分,
            // 让溢出路径不再无声丢弃轨迹,复盘时可通过 episodes.overflow_json 查回。
            overflow_json: &serde_json::to_string(&summary.overflow_traces).unwrap_or_default(),
        }) {
            // R-161:本轮开跑预检索的 recall_events 归因到该 episode,可 join 查询。
            let _ = store.link_recall_events_to_episode(episode_id, state.run_epoch_ms);
            current_episode_id = Some(episode_id);
        }
        // 给这次输入一个结局:此后任何停止都不再把它追认为 cancelled。
        let _ = store.finish_input(state.input_id, true);
    }
    // 轮末记忆整理(R-105):inbox 有草稿才起 manager 迷你 run,尽力而为。
    // R-213:把当轮 episode_id 代填给 manager,晋升证据才能指向真实轮次。
    let consolidation = consolidate_memory_inbox(
        state.config,
        state.proxy,
        state.client,
        state.rctx,
        state.ctx,
        current_episode_id,
    )
    .await;
    if consolidation.has_failures() {
        eprintln!("\x1b[33m{}\x1b[0m", consolidation.summary());
    } else if consolidation.pending_before > 0 {
        eprintln!("\x1b[90m{}\x1b[0m", consolidation.summary());
    }
    // D-341/R-195/R-295:轮末自动处置 candidate——有真实当轮 episode 且复发≥3 的
    // 自动 promote,超期未处置或超过健康水位的低价值 candidate 自动 deprecated 归档,
    // 其余保持 candidate。
    // 与 inbox 消化解耦:没有草稿也要跑,否则 candidate 永远躺着无人验收。
    // 机械判定不依赖 LLM,失败不阻塞收尾(报告仅用于打日志留证据)。
    if let Ok(report) = kanzei_tools::memory::reconcile_candidates(
        &state.ctx.project_root,
        current_episode_id,
        kanzei_tools::memory::CANDIDATE_MAX_AGE_DAYS,
    ) {
        if !report.promoted.is_empty() || !report.deprecated.is_empty() {
            eprintln!(
                "\x1b[90m(memory: candidate 自动处置: promote {} / deprecated {} / 未动 {} \
                 (文件 {}→{}, 索引 {}→{})\x1b[0m",
                report.promoted.len(),
                report.deprecated.len(),
                report.untouched.len(),
                report.candidate_files_before,
                report.candidate_files_after,
                report.candidate_index_before,
                report.candidate_index_after,
            );
        }
    }
    // R-169:CLI 轮末消费自主推进状态机的 backlog 单源(与桌面端同一实现,
    // kanzei_tools::tracker::backlog_status;D-229 类桌面端独占能力架构债消除)。
    // CLI 无交互循环不做自动续跑,只在无可推进条目时提示,与桌面端刹车一致。
    match kanzei_tools::tracker::backlog_status(&state.ctx.project_root) {
        kanzei_harness::auto_run::BacklogStatus::AllBlocked => {
            eprintln!("\x1b[33m(auto: 需求与缺陷全部被阻塞,自主推进无可用目标)\x1b[0m");
        }
        kanzei_harness::auto_run::BacklogStatus::Empty => {
            eprintln!("\x1b[33m(auto: 需求与缺陷已清空,自主推进无可用目标)\x1b[0m");
        }
        _ => {}
    }
    let exit_code = super::super::cli_exit_code(summary.halted_by_user);
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
