//! `kz run` 轮末记忆整理与直通放行(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:轮末记忆整理(`consolidate_memory_inbox` 消化 inbox 草稿)是写读分离
//! 的写端,`persist_always_allow` 是交互式「总是允许」规则落盘——两者都是 run 的
//! 轮末/应答辅助,与命令分发正交(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):manager 每轮至多跑一次、prompt 仅数 KB,用主模型换蒸馏质量
//! (M-003 教训);R-213 当轮 episode_id 代填给 manager,否则 provenance 校验拦下
//! 一切晋升。

use kanzei_harness::config::KanzeiConfig;
use kanzei_harness::{Harness, ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};

/// memory-manager 迷你 run:消化 inbox 草稿(写读分离的写端)。
/// fast 失败升级 primary;成功判据只看箱——清空才算消化完成。
///
/// D-409:分批消化(机制与 kanzei-app/src/memory.rs 同构)——每轮取
/// `read_inbox_batch(10)` 条喂 manager,逐条 memory_inbox_discard 销账,
/// 不再整箱(曾 251KB/201 条)塞单轮 4096 token prompt;run 失败 eprintln
/// 诊断(不静默),连续 3 批 pending 未降停止本轮防死循环,pending>100 轮末告警。
pub(crate) async fn consolidate_memory_inbox(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    client: &LlmClient,
    rctx: &ResolveCtx,
    ctx: &ToolCtx,
    current_episode_id: Option<i64>,
) {
    let store = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
    if store.pending_notes() == 0 {
        return;
    }
    let mut harness = Harness::default();
    harness.add(kanzei_tools::memory::MemoryManagerComponent);
    let Ok(snapshot) = harness.resolve(rctx) else {
        return;
    };
    let agent = kanzei_tools::memory::manager_agent();
    // D-409:分批消化——每轮固定条数,未销账留箱下轮重试(幂等)。
    const BATCH_SIZE: usize = 10;
    let mut consecutive_no_progress = 0;
    loop {
        let pending = store.pending_notes();
        if pending == 0 {
            break;
        }
        let (batch_text, batch_count) = store.read_inbox_batch(BATCH_SIZE);
        if batch_count == 0 {
            break;
        }
        // R-213:引擎轮末代填当轮 episode_id——manager 在轮内自报不出真实 id(episode 轮末
        // 才落库、list_episodes 不含 id),不注入的话 memory_promote 的 provenance 校验会
        // 拦下一切晋升,候选记忆永远升不了 active。
        let prompt = kanzei_tools::memory::consolidation_prompt(&batch_text, current_episode_id);
        // primary 优先(fast 兜底):记忆会注入之后每一轮的上下文,写错一条就长期误导。
        // 实测 fast(qwen3.5:4b)把失败**次数**误读成事实内容,生成了"需要约 7 次重试才能成功"
        // 这种编造结论(M-003 已人工校正)。manager 每轮至多跑一次、prompt 仅数 KB,
        // 用主模型换蒸馏质量是划算的。
        let before = pending;
        let mut consumed = false;
        for role in ["primary", "fast"] {
            let Ok(resolved) = config.resolve_model(role) else {
                continue;
            };
            let Ok(route) = kanzei_core::build_route(&resolved, proxy).await else {
                continue;
            };
            let runner_config = kanzei_core::RunnerConfig {
                model: resolved.model.clone(),
                max_tokens: 4096,
                reasoning: kanzei_llm::ReasoningEffort::Off,
                service_tier: config.service_tier_for(&resolved),
                context_limit: resolved.provider.context_limit,
                limits: config.limits.clone(),
                recall: None,
                execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
                ask_policy: kanzei_core::AskPolicy::NonInteractive,
                halt: None,
            };
            let mut on_event = |_event: kanzei_core::RunEvent| {};
            let mut ask = |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
                Box::pin(async move {
                    match request {
                        // 快照里只有 memory_* 工具,放行安全;问题一律取消(无人应答)。
                        kanzei_core::AskRequest::Permission { .. } => {
                            kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                        }
                        kanzei_core::AskRequest::Question { .. } => {
                            kanzei_core::AskResponse::Cancelled
                        }
                    }
                })
            };
            // D-409:失败可见——run 失败 eprintln 诊断,不再 `let _` 静默丢弃。
            match kanzei_core::run_once(
                client,
                &route,
                &snapshot,
                &agent,
                &runner_config,
                ctx,
                &prompt,
                None,
                &[],
                None,
                // R-246:记忆整理 run 不持有 LineRuntime。
                None,
                &mut on_event,
                &mut ask,
            )
            .await
            {
                Ok(_) => {
                    consumed = true;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[memory-consolidate] {role} 档消化本批 {batch_count} 条失败: {e}(未销账,下轮重试)"
                    );
                }
            }
        }
        if !consumed {
            // 两档都失败:诊断 + 停止本轮(避免无进展死循环)。
            eprintln!(
                "[memory-consolidate] 本批 {batch_count} 条消化失败(primary/fast 均不可用);未销账,下轮重试"
            );
            break;
        }
        // 防死循环:批次跑完 pending 未降(manager 未销账),连续无进展停止。
        if store.pending_notes() >= before {
            consecutive_no_progress += 1;
            if consecutive_no_progress >= 3 {
                eprintln!(
                    "[memory-consolidate] 连续 {consecutive_no_progress} 批无进展(pending 未降),停止本轮消化"
                );
                break;
            }
        } else {
            consecutive_no_progress = 0;
        }
    }
    // D-409:积压护栏——pending 仍超阈值时轮末明确告警(不再静默堆积)。
    let pending_left = store.pending_notes();
    if pending_left > 100 {
        eprintln!(
            "[memory-consolidate] 积压告警:inbox 仍有 {pending_left} 条待消化(>100);请在桌面端 Memory 页确认候选或触发一键整理"
        );
    } else if pending_left > 0 {
        eprintln!("\x1b[90m(memory: inbox 未消化完 {pending_left} 条,留待下轮)\x1b[0m");
    } else {
        eprintln!("\x1b[90m(memory: inbox 已整理入库)\x1b[0m");
    }
}

pub(crate) fn persist_always_allow(
    project_root: &std::path::Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<kanzei_core::AskReply> {
    let pattern = kanzei_harness::config::generalize_resource(action, resource);
    kanzei_harness::config::append_allow_rule(project_root, action, &pattern)?;
    Ok(kanzei_core::AskReply::AlwaysAllow)
}
