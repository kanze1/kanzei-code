//! 上下文压缩域(R-155 B5):主动压缩(compact_with_digest/digest_segment)与
//! 应急路径(recover_context_overflow/compact_messages_for_retry/
//! compact_messages_aggressively),dropped_trace/add_usage/summarize_input 随域。
//! 依赖 B2(metrics:summarize_tools/failures)+ B4(context:clip/digest_plausible/
//! estimate_prompt_tokens/is_text_user_message/render_for_digest)。

use super::SubagentRuntime;
use super::MAX_CONTEXT_OVERFLOW_RECOVERIES;
use crate::runner::context::{clip, digest_plausible, is_text_user_message, render_for_digest};
use crate::runner::metrics::{summarize_failures, summarize_tools};
use futures::StreamExt;
use kanzei_llm::{LlmClient, LlmEvent, LlmRequest, Message, Part, ReasoningEffort, Usage};

/// 主动压缩:保住任务定义与近期工作区,只把中段交给 fast 模型出纪要。
///
/// 与应急路径 `compact_messages_for_retry` 的分工是刻意的:那条路发生在
/// provider 已经拒绝请求之后,粗暴但必须成功;这条路发生在还有三成余量时,
/// 有时间也有理由做得体面。实测旧实现把主动路径直接接到应急函数上,一次从
/// 89.6k 预算砍到约 2k(97%),而且保留的是**最旧**的 8000 字节、丢掉刚做完的
/// 工作——压完模型不知道自己在干什么,大概率原地重做。
pub(crate) async fn compact_with_digest(
    client: &LlmClient,
    subagent: Option<&SubagentRuntime>,
    messages: &mut Vec<Message>,
    budget: u64,
    overflow_traces: &mut Vec<String>,
    recent_verbatim_ratio: f64,
) -> usize {
    // 任务定义:第一条纯文本用户消息。丢了它模型会跑偏。
    let head_index = messages.iter().position(is_text_user_message);
    // 近期工作区:从末尾往前收,收到占满 RECENT_VERBATIM_RATIO 为止。
    let recent_budget = (budget as f64 * recent_verbatim_ratio) as u64;
    let mut tail_start = messages.len();
    let mut tail_tokens = 0u64;
    while tail_start > 0 {
        let candidate = &messages[tail_start - 1];
        let cost = serde_json::to_string(candidate).map_or(0, |t| t.len()) as u64 / 4;
        if tail_tokens + cost > recent_budget && tail_start < messages.len() {
            break;
        }
        tail_tokens += cost;
        tail_start -= 1;
    }
    let middle_end = tail_start;
    let middle_start = head_index.map_or(0, |index| index + 1);
    if middle_end <= middle_start {
        // 中段是空的:说明超线来自 head/tail 本身,主动压缩无从下手,
        // 交给应急路径去做取舍,别在这里假装压过。
        return 0;
    }

    let middle: Vec<Message> = messages[middle_start..middle_end].to_vec();
    overflow_traces.push(dropped_trace(&middle));
    let digest = match subagent {
        Some(rt) => {
            let raw = digest_segment(client, rt, &render_for_digest(&middle)).await;
            // 纪要质量门槛:fast 模型纪要可能泛化成"进行了一些修改"、丢光关键
            // 文件,压完模型不知道自己在干什么,原地重做。机械校验文件保留率,
            // 不合格回落到原文节选——节选是原文,不可能丢(D-181 遗留)。
            raw.filter(|d| digest_plausible(&middle, d))
        }
        None => None,
    };
    let replacement = match digest {
        Some(text) => format!(
            "(系统:此前 {} 条消息已压缩为纪要,基于它继续)\n{text}",
            middle.len()
        ),
        // 纪要拿不到(未启用子代理/模型失败)时回落到截断,但**只截中段**,
        // head 与近期工作区照样保住——比旧实现整段推倒仍然好得多。
        None => format!(
            "(系统:此前 {} 条消息已压缩为节选,纪要模型不可用)\n{}",
            middle.len(),
            clip(&render_for_digest(&middle), 3_000)
        ),
    };

    let mut rebuilt: Vec<Message> = Vec::with_capacity(messages.len() - middle.len() + 1);
    if let Some(index) = head_index {
        rebuilt.push(messages[index].clone());
    }
    rebuilt.push(Message::user_text(replacement));
    rebuilt.extend_from_slice(&messages[middle_end..]);
    // 中段被抽走后,尾段里可能出现指向已消失调用的孤儿工具结果。
    *messages = crate::history::filter_message_history(&rebuilt);
    middle.len()
}

/// 用 fast 模型把一段协作记录压成纪要。失败返回 None,调用方回落到截断。
async fn digest_segment(
    client: &LlmClient,
    rt: &SubagentRuntime,
    transcript: &str,
) -> Option<String> {
    if transcript.trim().is_empty() {
        return None;
    }
    let request = LlmRequest {
        model: rt.fast.1.clone(),
        system: vec![
            "把下面的人机协作记录压成中文纪要,供同一个 agent 继续这项工作。必须保留:\
             目标、已完成的改动(具体文件/函数/标识符原样写出)、失败与其根因、\
             已确认不可行的方向、下一步。不要泛化成'进行了一些修改'。\
             markdown 列表,600 字以内。"
                .into(),
        ],
        messages: vec![Message::user_text(transcript.to_string())],
        tools: vec![],
        max_tokens: 1024,
        temperature: None,
        reasoning: ReasoningEffort::Off,
        service_tier: rt.fast_service_tier.clone(),
    };
    let mut stream = client.stream(&rt.fast.0, &request).await.ok()?;
    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let Ok(LlmEvent::TextDelta { text, .. }) = event {
            summary.push_str(&text);
        }
    }
    (!summary.trim().is_empty()).then(|| summary.trim().to_string())
}

pub(crate) fn recover_context_overflow(
    messages: &mut Vec<Message>,
    recoveries: &mut u32,
    overflow_traces: &mut Vec<String>,
) -> bool {
    match *recoveries {
        0 => compact_messages_for_retry(messages, overflow_traces),
        1 => compact_messages_aggressively(messages, overflow_traces),
        _ => return false,
    }
    *recoveries += 1;
    tracing::warn!(
        attempt = *recoveries,
        max = MAX_CONTEXT_OVERFLOW_RECOVERIES,
        "provider context overflow, retrying with compacted history"
    );
    true
}

/// 被压缩丢弃的消息段的轨迹摘要(R-106):工具画像 + 失败信号 + 文本预览,
/// 随 episode 沉淀,让激进压缩不再无声丢弃轨迹。
pub(crate) fn dropped_trace(messages: &[Message]) -> String {
    let preview: String = messages
        .iter()
        .flat_map(|message| &message.parts)
        .find_map(|part| match part {
            Part::Text { text } => Some(text.chars().take(120).collect()),
            _ => None,
        })
        .unwrap_or_default();
    serde_json::json!({
        "dropped_messages": messages.len(),
        "tools": summarize_tools(messages),
        "failures": summarize_failures(messages),
        "preview": preview,
    })
    .to_string()
}

pub(crate) fn compact_messages_for_retry(
    messages: &mut Vec<Message>,
    overflow_traces: &mut Vec<String>,
) {
    let Some(current_index) = messages.iter().rposition(is_text_user_message) else {
        return;
    };
    let current = messages[current_index].clone();
    let dropped: Vec<Message> = messages
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != current_index)
        .map(|(_, message)| message.clone())
        .collect();
    if !dropped.is_empty() {
        overflow_traces.push(dropped_trace(&dropped));
    }
    // 应急路径仍然粗暴(此刻 provider 已经拒过一次请求,必须一次成功),但两处
    // 缺陷该修:①原实现从**最旧**的消息开始收,攒够就停,于是留下开场白、丢掉
    // 刚做完的工作;近期内容才是继续任务所必需的,改为从最近往回收。②预算按
    // 字节算却用 chars().take() 取,中文场景实际长度超出三倍(D-181)。
    const EMERGENCY_KEEP_CHARS: usize = 4_000;
    let mut recent: Vec<String> = Vec::new();
    let mut kept = 0usize;
    'outer: for (index, message) in messages.iter().enumerate().rev() {
        if index == current_index {
            continue;
        }
        for part in message.parts.iter().rev() {
            let line = match part {
                Part::Text { text } => text.clone(),
                Part::ToolCall { name, input, .. } => {
                    format!("[tool-call] {name} {}", clip(&input.to_string(), 200))
                }
                Part::ToolResult { content, .. } => content.clone(),
                _ => continue,
            };
            if kept >= EMERGENCY_KEEP_CHARS {
                break 'outer;
            }
            let snippet = clip(&line, EMERGENCY_KEEP_CHARS - kept);
            kept += snippet.chars().count();
            recent.push(snippet);
        }
    }
    recent.reverse();
    let history = recent.join("\n");
    messages.clear();
    if !history.trim().is_empty() {
        messages.push(Message::user_text(format!(
            "以下是此前工具执行结果的压缩记录，仅供继续当前任务参考：\n{}",
            history.trim_end()
        )));
    }
    messages.push(current);
}

pub(crate) fn compact_messages_aggressively(
    messages: &mut Vec<Message>,
    overflow_traces: &mut Vec<String>,
) {
    let Some(current_index) = messages.iter().rposition(is_text_user_message) else {
        return;
    };
    let current = messages[current_index].clone();
    let dropped: Vec<Message> = messages
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != current_index)
        .map(|(_, message)| message.clone())
        .collect();
    if !dropped.is_empty() {
        overflow_traces.push(dropped_trace(&dropped));
    }
    messages.clear();
    messages.push(current);
}

pub(crate) fn add_usage(a: Usage, b: Usage) -> Usage {
    Usage {
        input: a.input + b.input,
        output: a.output + b.output,
        reasoning: a.reasoning + b.reasoning,
        cache_read: a.cache_read + b.cache_read,
        cache_write: a.cache_write + b.cache_write,
    }
}

pub(crate) fn summarize_input(input: &serde_json::Value, raw: &str) -> String {
    let rendered = if input.is_null() {
        raw.to_string()
    } else {
        input.to_string()
    };
    match rendered.char_indices().nth(160) {
        Some((idx, _)) => format!("{}…", &rendered[..idx]),
        None => rendered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::context::{clip, estimate_prompt_tokens};
    use kanzei_llm::{Message, Part};

    /// D-181:主动压缩必须保住**任务定义**与**近期工作区**,只压中段。
    /// 旧实现直接复用应急函数,一次从预算线砍到约 2k(97%),而且留下的是最旧的
    /// 内容——压完模型不知道自己刚做了什么。
    #[tokio::test]
    async fn 主动压缩保住任务定义与近期工作并只压中段() {
        let mut messages = vec![Message::user_text("任务定义:修复 D-123 的空指针")];
        for i in 0..60 {
            messages.push(Message::user_text(format!(
                "中段第 {i} 条 {}",
                "x".repeat(400)
            )));
        }
        messages.push(Message::user_text("最近工作:正在改 store.rs 的 migrate"));
        messages.push(Message::user_text("最近工作:刚跑完 cargo test"));

        let before = estimate_prompt_tokens(&[], &messages, &[]);
        let mut traces = Vec::new();
        // subagent=None → 纪要模型不可用,走截断回落;即便如此也必须保住首尾。
        let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
        let dropped =
            super::compact_with_digest(&client, None, &mut messages, 2_000, &mut traces, 0.35)
                .await;

        assert!(dropped > 0, "中段应当被压掉");
        let text: String = messages
            .iter()
            .flat_map(|m| &m.parts)
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("任务定义:修复 D-123"),
            "任务定义必须逐字保留:\n{text}"
        );
        assert!(text.contains("刚跑完 cargo test"), "最近一条必须逐字保留");
        assert!(text.contains("正在改 store.rs"), "近期工作区必须逐字保留");
        assert!(!traces.is_empty(), "被压掉的中段要留轨迹");

        let after = estimate_prompt_tokens(&[], &messages, &[]);
        assert!(after < before, "压缩要见效: {before} -> {after}");
        // 不该像旧实现那样推倒重来:保住的内容必须显著多于应急路径的残留。
        assert!(
            messages.len() >= 3,
            "首条任务定义 + 纪要 + 近期若干条都要在,实得 {}",
            messages.len()
        );
    }

    /// 应急路径保留的应当是**最近**的内容,而不是开场白。
    #[test]
    fn 应急压缩保留最近内容而非最旧内容() {
        let mut messages: Vec<Message> = (0..30)
            .map(|i| Message::user_text(format!("第 {i} 条 {}", "y".repeat(600))))
            .collect();
        messages.push(Message::user_text("当前指令"));
        let mut traces = Vec::new();
        compact_messages_for_retry(&mut messages, &mut traces);

        let text: String = messages
            .iter()
            .flat_map(|m| &m.parts)
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("当前指令"), "当前用户消息必须留下");
        assert!(
            text.contains("第 29 条"),
            "最近的历史要留下:\n{}",
            clip(&text, 300)
        );
        assert!(!text.contains("第 0 条"), "最旧的历史不该占满配额");
    }

    /// D-206:主动压缩不设总量配额。等价类断言写在常量语义上:
    /// 刹车常量只允许"连续无效"语义存在——谁把总量配额加回来,先得删这条测试。
    #[test]
    fn 压缩刹车只认连续无效不设总量配额() {
        // 常量本身:连续无效两次即停(压不动了),而不是"一共只能压 N 次"。
        assert_eq!(crate::runner::MAX_FUTILE_COMPACTIONS, 2);
        // 语义锚点:成功压缩必须能无限次发生。模拟 58 步长 run 的记账序列——
        // 三次成功压缩(after<=budget → 清零)后计数仍为 0,第四次照样允许;
        // 旧实现(每次成功 +1、上限 3)在同一序列后是 3,第四次被拒。
        let budget = 100u64;
        let mut futile = 0u32;
        for _ in 0..3 {
            let after = 90u64; // 压回线内
            if after <= budget {
                futile = 0
            } else {
                futile += 1
            }
        }
        assert_eq!(futile, 0, "成功的压缩不得累计任何配额");
        assert!(
            futile < crate::runner::MAX_FUTILE_COMPACTIONS,
            "第四次压缩必须仍被允许"
        );
        // 连续压不动(after>budget)两次后停——这才是注释里"再压无益"的原意。
        for _ in 0..2 {
            let after = 120u64;
            if after <= budget {
                futile = 0
            } else {
                futile += 1
            }
        }
        assert!(
            futile >= crate::runner::MAX_FUTILE_COMPACTIONS,
            "连续无效两次后必须刹车"
        );
        // 中间只要成功一次就复位,不是一杆子打死。
        let after = 90u64;
        if after <= budget {
            futile = 0
        } else {
            futile += 1
        }
        assert_eq!(futile, 0);
    }

    /// 压缩后必须真的变小,而且当前用户消息要留下——否则模型会丢掉正在做的事。
    #[test]
    fn 主动压缩显著缩小上下文且保留当前用户消息() {
        let system = vec![String::new()];
        let mut messages: Vec<Message> = (0..40)
            .map(|i| Message::user_text(format!("历史第 {i} 条 {}", "x".repeat(500))))
            .collect();
        messages.push(Message::user_text("当前这条必须留下"));
        let before = estimate_prompt_tokens(&system, &messages, &[]);

        let mut traces = Vec::new();
        compact_messages_for_retry(&mut messages, &mut traces);
        let after = estimate_prompt_tokens(&system, &messages, &[]);

        assert!(after * 2 < before, "压缩要真的见效: {before} -> {after}");
        assert!(!traces.is_empty(), "被裁掉的段必须留下轨迹");
        let text: String = messages
            .iter()
            .flat_map(|m| &m.parts)
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(text.contains("当前这条必须留下"), "当前用户消息不能被裁掉");
    }
}
