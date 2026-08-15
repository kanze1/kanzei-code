//! 流请求域(R-257 B2):单步请求段——从当前 messages 重建请求、建流、消费 SSE
//! 事件,并在 context overflow / transport 中断时按既有规则恢复重放。自 drive.rs
//! 原样迁出,零行为变更。

use std::collections::BTreeMap;

use futures::StreamExt;
use kanzei_harness::tolerant_parse;
use kanzei_llm::{
    FinishReason, LlmClient, LlmEvent, LlmRequest, Message, Part, Route, ToolSpec, Usage,
};

use crate::runner::compaction::{add_usage, decay_overflow_recoveries, recover_context_overflow};
use crate::runner::context::{estimate_prompt_tokens, update_calibration};
use crate::runner::drive::halt_signalled;
use crate::runner::{RunEvent, RunnerConfig};

/// R-202 批3:单步请求段的产物。
pub(crate) enum StepOutcome {
    /// 本步正常结束:消息已提交前的 (parts, calls, finish) 三元组,由调用方决定
    /// 是提交 assistant 消息还是进入工具执行。
    Completed {
        parts: Vec<Part>,
        calls: Vec<(String, String, serde_json::Value, String)>,
        finish: FinishReason,
    },
    /// D-342:流内收到停止信号,本步半成品丢弃。调用方构造 halted RunSummary。
    Stopped,
}

/// R-202 批3:单步请求段——从当前 messages 重建请求、建流、消费 SSE 事件,
/// 并在 context overflow / transport 中断时按既有规则恢复重放。
///
/// 职责边界与既有行为逐字节对齐(行为零变更):
/// - 重试循环内每次恢复都重克隆 messages 重建请求(压缩必须落进发出内容);
/// - StepFinish 更新 calibration / last_input_tokens / total_usage / overflow 恢复衰减;
/// - halted_mid_stream(流内停止)与恢复失败、协议错误都是提前退出,不再走工具执行。
///
/// 可变运行态全部经 &mut 传入:即使以 Stopped 提前退出,步内已累积的
/// calibration/usage/恢复计数也如实保留,调用方照常构造 RunSummary。
#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202)。
pub(crate) async fn stream_request_step(
    client: &LlmClient,
    route: &Route,
    config: &RunnerConfig,
    system: &[String],
    specs: &[ToolSpec],
    messages: &mut Vec<Message>,
    step: u32,
    last_step: bool,
    budget_checkpoint: bool,
    halt: Option<&crate::runner::CancellationToken>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    calibration: &mut f64,
    last_input_tokens: &mut Option<u64>,
    total_usage: &mut Usage,
    overflow_recoveries: &mut u32,
    overflow_traces: &mut Vec<String>,
) -> anyhow::Result<StepOutcome> {
    let mut stream_restarts: u32 = 0;
    loop {
        // 每次恢复都会改写 messages，请求必须在重试循环内重建；否则即使压缩了
        // 内存历史，发给 provider 的仍会是第一次克隆出的超长请求。
        let mut request_messages = messages.clone();
        // 最后一步收走工具强制收敛;必须同时明确告知(D-027:只收走不告知,
        // codex 仍试图调用工具,把调用 JSON 当纯文本狂喷并在思考里反复自我纠正)。
        if last_step {
            request_messages.push(Message::user_text(
                "(system) Final step of this run: tools are no longer available. Do NOT \
                 attempt any tool call and do NOT emit JSON — reply in plain text only, \
                 summarizing what was completed and what remains.",
            ));
        } else if budget_checkpoint {
            request_messages.push(Message::user_text(format!(
                "(system) Budget checkpoint — you are {step} steps into this run. This is not a \
                 stop signal and not a nudge to hurry: keep going. Before your next tool call, \
                 state in one or two lines what is DONE, what REMAINS, and whether the remaining \
                 work still belongs to the task you were given. If it has drifted into unrelated \
                 work, finish the original task first. If what remains needs a decision only the \
                 user can make, say so now in plain text instead of exploring further."
            )));
        }
        // 请求构造前记录本次估算:StepFinish 用真实 input tokens 反推校准因子,
        // 下一次预算判断就按校准后的口径来。tools 随 last_step 变化,估算必须
        // 与实际发出的请求同口径,否则校准因子被系统性偏差污染。
        let req_tools: &[ToolSpec] = if last_step { &[] } else { specs };
        let last_estimated = estimate_prompt_tokens(system, &request_messages, req_tools);
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.to_vec(),
            messages: request_messages,
            tools: req_tools.to_vec(),
            max_tokens: config.max_tokens,
            temperature: None,
            reasoning: config.reasoning,
            service_tier: config.service_tier.clone(),
        };
        let mut stream = match client
            .stream_with_retry_notice(route, &request, |attempt, delay| {
                on_event(RunEvent::Retry {
                    attempt,
                    max: kanzei_llm::client::MAX_TRANSPORT_RETRIES,
                    delay_ms: delay.as_millis(),
                });
            })
            .await
        {
            Err(error) if error.is_context_overflow() => {
                if recover_context_overflow(messages, overflow_recoveries, overflow_traces) {
                    continue;
                }
                return Err(error.into());
            }
            result => result?,
        };
        let mut text_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut reasoning_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut parts: Vec<Part> = Vec::new();
        let mut calls: Vec<(String, String, serde_json::Value, String)> = Vec::new();
        let mut finish = FinishReason::EndTurn;
        let mut stream_error: Option<kanzei_llm::LlmError> = None;
        let mut halted_mid_stream = false;

        loop {
            // D-342 流内检查点:停止时丢弃本步半成品(messages 尚未被本步
            // 触碰),立即收尾——不等模型把整步吐完。
            let event = tokio::select! {
                event = stream.next() => match event {
                    Some(event) => event,
                    None => break,
                },
                _ = halt_signalled(halt) => {
                    halted_mid_stream = true;
                    break;
                }
            };
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    stream_error = Some(error);
                    break;
                }
            };
            match event {
                LlmEvent::TextDelta { index, text } => {
                    on_event(RunEvent::Text(text.clone()));
                    text_buffers.entry(index).or_default().push_str(&text);
                }
                LlmEvent::TextEnd { index } => {
                    if let Some(text) = text_buffers.remove(&index) {
                        parts.push(Part::Text { text });
                    }
                }
                LlmEvent::ReasoningDelta { index, text } => {
                    on_event(RunEvent::Reasoning(text.clone()));
                    reasoning_buffers.entry(index).or_default().push_str(&text);
                }
                // reasoning 连同 signature(codex 的 encrypted_content)入历史,
                // Responses 协议多轮工具循环必须回放;其他协议的 builder 自行忽略。
                LlmEvent::ReasoningEnd { index, signature } => {
                    let text = reasoning_buffers.remove(&index).unwrap_or_default();
                    if !text.is_empty() || signature.is_some() {
                        parts.push(Part::Reasoning { text, signature });
                    }
                }
                LlmEvent::ToolCall {
                    id,
                    name,
                    input,
                    raw_input,
                } => {
                    // 协议层解析失败 → 宽容修复(尾逗号/单引号/裸键/围栏)。
                    let input = if input.is_null() {
                        tolerant_parse(&raw_input).unwrap_or(serde_json::Value::Null)
                    } else {
                        input
                    };
                    parts.push(Part::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: if input.is_null() {
                            serde_json::json!({})
                        } else {
                            input.clone()
                        },
                    });
                    calls.push((id, name, input, raw_input));
                }
                LlmEvent::StepFinish { usage, reason } => {
                    *calibration = update_calibration(*calibration, last_estimated, usage.input);
                    if usage.input > 0 {
                        *last_input_tokens = Some(usage.input);
                    }
                    *total_usage = add_usage(*total_usage, usage);
                    finish = reason.clone();
                    // R-219:恢复计数随成功衰减——被动恢复成功且后续步正常结束,
                    // 计数逐步回落,长跑不会因早先一次 overflow 就永久锁定在
                    // 「已恢复 2 次,下一次直接终止」。每成功一步减 1(封底 0),
                    // 让恢复额度在长时间稳定运行后重新充满。
                    *overflow_recoveries = decay_overflow_recoveries(*overflow_recoveries);
                    on_event(RunEvent::StepEnd { usage, reason });
                }
                _ => {}
            }
        }

        if halted_mid_stream {
            return Ok(StepOutcome::Stopped);
        }
        match stream_error {
            None => {
                for (_, text) in std::mem::take(&mut text_buffers) {
                    parts.push(Part::Text { text });
                }
                return Ok(StepOutcome::Completed {
                    parts,
                    calls,
                    finish,
                });
            }
            // Provider 也可能在 HTTP 200 的 SSE error 事件里报告上下文超限。
            // 此时本步工具尚未执行，压缩 messages 后安全地从头重放请求。
            Some(error) if error.is_context_overflow() => {
                if recover_context_overflow(messages, overflow_recoveries, overflow_traces) {
                    continue;
                }
                return Err(error.into());
            }
            // 只重放传输层中断:协议错误重放只会原样复现,白烧钱。
            Some(error)
                if matches!(error, kanzei_llm::LlmError::Transport(_))
                    && stream_restarts < config.limits.stream_restarts() =>
            {
                stream_restarts += 1;
                let delay = std::time::Duration::from_millis(500 * stream_restarts as u64);
                tracing::warn!(
                    attempt = stream_restarts,
                    delay_ms = delay.as_millis(),
                    error = %error,
                    "stream broke mid-flight, re-requesting step"
                );
                on_event(RunEvent::StreamRestart {
                    attempt: stream_restarts,
                    max: config.limits.stream_restarts(),
                    delay_ms: delay.as_millis(),
                });
                tokio::time::sleep(delay).await;
            }
            Some(error) => return Err(error.into()),
        }
    }
}
