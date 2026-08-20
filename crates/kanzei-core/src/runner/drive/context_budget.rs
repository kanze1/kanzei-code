//! 每步建流前的上下文预算维护。
//!
//! 该模块只负责主动 prune、压缩和 trim；调用方继续持有 system/messages 与轮级
//! 运行态，保持预算检查在每步请求前执行以及事件顺序不变。

use super::*;
use kanzei_llm::protocol::ProtocolKind;

/// R-202 批6:轮内上下文预算——每步开跑前的主动 prune / 压缩 / trim。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - R-219:context_limit 未知时按保守默认 32k 启用主动预算,首次 tracing::warn
///   点名一次(可见不阻塞);
/// - R-236 B1:触发线 headroom 公式;R-236 B4:L0 prune 先行,凑不满最小收益自弃;
/// - D-206:只按"有没有用"记账——压回线内清零,压不动(连 trim_tail 都上了)仍
///   超线则递增,连续 MAX_FUTILE_COMPACTIONS 次后交给撞墙的被动恢复;
/// - D-203:trim_tail 与预算检查用同一个 calibration 口径。
#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202)。
pub(super) async fn enforce_context_budget(
    client: &LlmClient,
    subagent: Option<&SubagentRuntime>,
    config: &RunnerConfig,
    system: &[String],
    specs: &[ToolSpec],
    protocol: ProtocolKind,
    messages: &mut Vec<Message>,
    last_input_tokens: Option<u64>,
    last_estimated_tokens: Option<u64>,
    calibration: f64,
    futile_compactions: &mut u32,
    overflow_traces: &mut Vec<String>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) {
    // 轮内上下文预算(D-176)。压缩检查原先只写在**一轮结束之后**,而长轮与
    // 自动续跑恰恰是最需要它的场景:一轮不结束就一次也轮不到。实测一次 41
    // 分钟的运行里检查点执行了 0 次,用户按停止后更是直接跳过收尾,全程只能
    // 等 provider 报 overflow 再被动裁剪。这里在每步开跑前主动估一次。
    // R-219:context_limit 未知(白名单外 provider)时按保守默认 32k 启用主动
    // 预算——未知不等于没有上限,放任涨到撞墙会把整个 run 拖进被动恢复;
    // 启动时 tracing::warn 点名一次(可见不阻塞),不打断运行。
    let effective_limit = config.context_limit.unwrap_or_else(|| {
        tracing::warn!(
            "provider 无已知 context_limit,按保守默认 32k 做轮内预算; \
             撞墙前的主动压缩只降级不终止(第三次 overflow 仍会被动终止)"
        );
        32_000
    });
    {
        // R-236 B1:触发线换 headroom 公式(与轮末同一把尺,见 compaction_budget)。
        let budget = compaction_budget(
            effective_limit,
            config.max_tokens,
            config.limits.compact_buffer_tokens(),
        );
        let anchored_tokens = |messages: &[Message]| {
            let current_estimated_tokens =
                estimate_prompt_tokens_for_protocol(system, messages, specs, Some(protocol));
            budgeted_tokens_from_last_usage(
                last_input_tokens,
                last_estimated_tokens,
                current_estimated_tokens,
                calibration,
            )
        };
        let mut before = anchored_tokens(messages);
        // R-236 B4:L0 prune 先行——超线时先机械清旧工具结果(零幻觉零
        // LLM),清完够线就不必动纪要;凑不满最小收益 prune 自己会放弃。
        if before > budget && messages.len() > 1 {
            let cleared = prune_old_tool_results(
                messages,
                config.limits.prune_protect_tokens(),
                config.limits.prune_min_gain_tokens(),
            );
            if cleared > 0 {
                let after_prune = anchored_tokens(messages);
                on_event(RunEvent::ContextPruned {
                    cleared_results: cleared,
                    before_tokens: before,
                    after_tokens: after_prune,
                });
                before = after_prune;
            }
        }
        if before > budget && *futile_compactions < MAX_FUTILE_COMPACTIONS && messages.len() > 1 {
            let dropped_messages = compact_with_digest(
                client,
                subagent,
                messages,
                budget,
                overflow_traces,
                config.limits.recent_verbatim_ratio(),
            )
            .await;
            if dropped_messages > 0 {
                // 压了还超线:tail 太大或 head 太大。再砍 tail 到预算内,否则
                // 下一步预算检查立刻再压——连续两次压缩 = 缓存前缀两次全量
                // 重算(cache_write 双倍),省下的 token 不够补缓存成本。
                // trim_tail 拿同一个 calibration:两边必须用同一把尺子量同一条
                // 预算线,否则它按原始口径够线就收手,这里看还超线(D-203)。
                if anchored_tokens(messages) > budget {
                    trim_tail_for_protocol(
                        messages,
                        system,
                        specs,
                        budget,
                        calibration,
                        overflow_traces,
                        Some(protocol),
                    );
                }
                let after = anchored_tokens(messages);
                // D-206:只按"有没有用"记账。压回线内 = 压缩在正常工作,清零、
                // 下次照压;压完(连 trim_tail 都上了)仍超线 = head+当前消息
                // 本身超线,连续两次就停,交给撞墙后的被动恢复,别空转。
                if after <= budget {
                    *futile_compactions = 0;
                } else {
                    *futile_compactions += 1;
                }
                on_event(RunEvent::ContextCompacted {
                    before_tokens: before,
                    after_tokens: after,
                    budget_tokens: budget,
                    limit_tokens: effective_limit,
                    dropped_messages,
                });
            } else {
                // 中段为空压不动:不发事件(没骗 UI),但要计无效——否则每步
                // 白跑一次 compact,同样是注释里说的空转。
                *futile_compactions += 1;
            }
        }
    }
}
