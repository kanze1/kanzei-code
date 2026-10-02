//! 上下文预算域(R-155 B4):预算估算与校准(budgeted_tokens/estimate_prompt_tokens/
//! update_calibration)、trim_tail、纪要渲染与质量校验(render_for_digest/
//! digest_plausible/extract_file_names/clip)、周期盘点(is_budget_checkpoint)。
//! is_text_user_message 三归属(compact_with_digest/trim_tail/drive),提 pub(super);
//! MAX_FUTILE_COMPACTIONS 留 mod.rs(pub(super))。

use crate::runner::compaction::dropped_trace;
use kanzei_llm::protocol::ProtocolKind;
use kanzei_llm::{Message, Part, Role, ToolSpec};
use std::collections::HashSet;

/// 轮内主动压缩的触发线(占 context_limit 的比例)。测试锚点:生产调用方以
/// 字面量传参(0.7),本常量被测试断言引用(lib 构建显 unused,故 allow)。
#[allow(dead_code)]
pub const CONTEXT_BUDGET_RATIO: f64 = 0.7;

/// D-592:冷启动校准必须保守。首个 provider usage 到达前,中文/代码/工具 schema
/// 的全量 bytes/4 估算可能系统性低估;取上限 2.0 先保护小窗口,后续由真实 usage EMA 下调。
pub(crate) const COLD_START_CALIBRATION: f64 = 2.0;

pub(crate) fn conservative_calibration() -> f64 {
    COLD_START_CALIBRATION
}
/// 盘点检查点:第 20/40 步,之后每 40 步一次(80/120/160…)。
///
/// 旧实现 `matches!(step, 20 | 40 | 80)` 是有限清单——第 80 步之后的长 run 永不再
/// 盘点,而无步数上限的自举 run 恰恰是最需要盘点的(D-206 顺带修:与压缩总量配额
/// 同型,把"周期性动作"写成了"有限次动作")。
pub(crate) fn is_budget_checkpoint(step: u32) -> bool {
    step == 20 || (step >= 40 && step.is_multiple_of(40))
}

/// 主动压缩后,最近这部分历史逐字保留的预算占比(相对 context_limit)。
/// 主动压缩发生在还有余量的时候,没理由像应急路径那样推倒重来——保住近期
/// 工作区,模型才知道自己刚做了什么,不会压完就原地重做。
#[allow(dead_code)] // 测试锚点:生产调用方传字面量 0.35
pub(crate) const RECENT_VERBATIM_RATIO: f64 = 0.35;
/// 交给 fast 模型做纪要的原文上限(字符)。超出时取**最近**的部分:
/// 中段越靠后越相关。
pub(crate) const DIGEST_SOURCE_CHARS: usize = 24_000;

/// 把消息渲染成给"纪要模型"看的文本。工具调用必须带上工具名与关键入参——
/// 只留工具输出而不知道是谁产生的,压缩完等于一堆无主的结果(D-181)。
pub(crate) fn render_for_digest(messages: &[Message]) -> String {
    let mut out = String::new();
    for message in messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        for part in &message.parts {
            match part {
                Part::Text { text } => {
                    out.push_str(&format!("[{role}] {}\n", clip(text, 2_000)));
                }
                Part::ToolCall { name, input, .. } => {
                    out.push_str(&format!(
                        "[tool-call] {name} {}\n",
                        clip(&input.to_string(), 400)
                    ));
                }
                Part::ToolResult {
                    content, is_error, ..
                } => {
                    out.push_str(&format!(
                        "[tool-result{}] {}\n",
                        if *is_error { " ERROR" } else { "" },
                        clip(content, 1_200)
                    ));
                }
                _ => {}
            }
        }
    }
    // 超长时保留末尾(最近的)。
    let chars: Vec<char> = out.chars().collect();
    if chars.len() > DIGEST_SOURCE_CHARS {
        return chars[chars.len() - DIGEST_SOURCE_CHARS..].iter().collect();
    }
    out
}
/// 按**字符**截断。原实现用字节余额去 take 字符数,中文场景下实际长度会超出
/// 三倍,那个上限名不副实(D-181)。
pub(crate) fn clip(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push_str("…[截断]");
    }
    out
}
/// R-236 B1:附件(Image/Document)的固定 token 成本。它们的 data 是 base64,
/// 按字节数/4 估算会把一张截图算成几万 token——带附件的会话每轮"必超线",
/// 压缩被虚高误触发。provider 侧图片实际计费在千 token 量级,取保守固定值。
pub(crate) const ATTACHMENT_TOKEN_COST: u64 = 1_500;

/// R-236 B1:压缩触发线/预算线的统一公式,轮内与轮末同一把尺:
/// `budget = limit − max(min(单步输出上限, limit/3), min(headroom buffer, limit/3))`,封底 `limit/4`。
/// 输出和 buffer 都按窗口比例封顶,避免小窗口被单个 `max_tokens` 固定吞掉一半。
/// `context_budget_ratio` 配置键保留但不再被触发路径消费。
pub fn compaction_budget(context_limit: u64, max_output_tokens: u32, buffer_tokens: u64) -> u64 {
    // D-592:输出上限和 buffer 都不能单独吞掉小窗口的一半。保留最多三分之一
    // 窗口给单步输出/headroom,窗口越小预算越自适应；大窗口仍保留配置的真实值。
    let adaptive_reserve = context_limit / 3;
    let output_reserve = (max_output_tokens as u64).min(adaptive_reserve);
    let buffer_reserve = buffer_tokens.min(adaptive_reserve);
    context_limit
        .saturating_sub(output_reserve.max(buffer_reserve))
        .max(context_limit / 4)
}

/// 本步请求的 token 粗估:system + 历史 + **工具 schema**。
///
/// 工具 schema 必须计入——它每一步都整份重发,在工具多的 profile 下是常驻大头,
/// 漏算就会让预算长期偏低、该压的时候不压。粒度沿用 len/4,与既有压缩预检同源;
/// 附件不按 base64 字节算,按固定成本(R-236 B1,见 ATTACHMENT_TOKEN_COST)。
pub(crate) fn estimate_prompt_tokens(
    system: &[String],
    messages: &[Message],
    specs: &[ToolSpec],
) -> u64 {
    estimate_prompt_tokens_for_protocol(system, messages, specs, None)
}

/// 按 wire 协议估算本步请求。OpenAI Chat 的 builder 会丢弃 Reasoning part,
/// 因此不能把它计入实际 prompt；Responses/Anthropic/DeepSeek 则保留该历史块。
pub(crate) fn estimate_prompt_tokens_for_protocol(
    system: &[String],
    messages: &[Message],
    specs: &[ToolSpec],
    protocol: Option<ProtocolKind>,
) -> u64 {
    let system_bytes: usize = system.iter().map(String::len).sum();
    let message_bytes: usize = messages
        .iter()
        .map(|message| {
            if protocol == Some(ProtocolKind::OpenAiChat)
                && message
                    .parts
                    .iter()
                    .all(|part| matches!(part, Part::Reasoning { .. }))
            {
                return 0;
            }
            message
                .parts
                .iter()
                .map(|part| match part {
                    Part::Reasoning { .. } if protocol == Some(ProtocolKind::OpenAiChat) => 0,
                    Part::Image { .. } | Part::Document { .. } => {
                        (ATTACHMENT_TOKEN_COST * 4) as usize
                    }
                    other => json_bytes(other),
                })
                .sum::<usize>()
                // 消息外壳(role 字段与括号)的近似,对齐旧的整段序列化口径。
                + 24
        })
        .sum();
    let spec_bytes: usize = specs
        .iter()
        .map(|spec| spec.name.len() + spec.description.len() + json_bytes(&spec.input_schema))
        .sum();
    ((system_bytes + message_bytes + spec_bytes) / 4) as u64
}

/// 序列化后的字节数,**不产出字符串**。
///
/// 这个函数每步至少调一次、每个 part 一次,而它要的只是长度:原实现
/// `to_string().len()` 为了量一下就把整段 JSON 物化再丢掉——实测本仓主会话
/// (993 条消息 / 1665 个 part / 189 万字符)每次调用白分配约 2MB。
/// `to_writer` 往只计数的 Writer 里写,字节数与 `to_string().len()` **逐字节相同**
/// (同一个序列化器、同一份输出),口径不变,分配为零。
fn json_bytes<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    struct ByteCounter(usize);
    impl std::io::Write for ByteCounter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = ByteCounter(0);
    // 序列化失败与旧实现的 map_or(0, ..) 同样计 0。
    serde_json::to_writer(&mut counter, value)
        .map(|()| counter.0)
        .unwrap_or(0)
}

/// 用 provider 返回的真实 input token 数做滑动校准,修正 len/4 估算的
/// 系统性偏差(中文 \uXXXX 转义、工具输出密集等)。EMA 收敛,单步比值限幅
/// 在 [0.5, 2.0],防止异常记账值(如缓存命中的特殊计费)把校准一次带飞。
pub(crate) fn update_calibration(current: f64, estimated: u64, actual: u64) -> f64 {
    if estimated == 0 || actual == 0 {
        return current;
    }
    let ratio = (actual as f64 / estimated as f64).clamp(0.5, 2.0);
    current * 0.7 + ratio * 0.3
}

/// 预算比较专用:原始估算 × 校准因子(D-203)。
///
/// **所有跟 budget 比大小的地方只准走这个**,散落的 `raw × calibration` 乘法会漏——
/// a119eeb 就漏了 trim_tail:调用方三处都乘了校准,trim_tail 内部却用未校准的原始值
/// 去够同一条 budget。calibration 为修正"真实 token 高于估算"(中文 \uXXXX 转义)
/// 而生,典型值 >1,于是 trim_tail 提前收手、调用方视角仍超线,下一步预算检查
/// 立刻再压——而"避免连续两次压缩(缓存前缀双倍重算)"恰恰是 trim_tail 存在的
/// 理由。校准越准,这个洞越大。
///
/// 注意 update_calibration 的输入必须是 estimate_prompt_tokens 的**原始**值
/// (last_estimated),不能走这里——乘了校准就是拿自己的输出当输入,EMA 会发散。
/// 两个函数分开命名就是为了让这两种用途一眼分得开。
#[allow(dead_code)] // 保留通用测试/非协议调用口径。
pub(crate) fn budgeted_tokens(
    system: &[String],
    messages: &[Message],
    specs: &[ToolSpec],
    calibration: f64,
) -> u64 {
    (estimate_prompt_tokens(system, messages, specs) as f64 * calibration).round() as u64
}

/// 与实际 wire 协议一致的校准预算口径。
pub(crate) fn budgeted_tokens_for_protocol(
    system: &[String],
    messages: &[Message],
    specs: &[ToolSpec],
    calibration: f64,
    protocol: Option<ProtocolKind>,
) -> u64 {
    (estimate_prompt_tokens_for_protocol(system, messages, specs, protocol) as f64 * calibration)
        .round() as u64
}

/// 预算检查的增量口径(D-592):上一请求已有真实 `prompt_tokens` 时,只把当前
/// 估算相对上一请求估算的新增部分加回;完整 `bytes/4 × calibration` 仅用于冷启动。
/// 这样中文、代码和工具 schema 的系统性偏差不会在每一步重新把整段历史压低。
pub(crate) fn budgeted_tokens_from_last_usage(
    last_input_tokens: Option<u64>,
    last_estimated_tokens: Option<u64>,
    current_estimated_tokens: u64,
    calibration: f64,
) -> u64 {
    match (last_input_tokens, last_estimated_tokens) {
        (Some(actual), Some(previous_estimate)) => {
            actual.saturating_add(current_estimated_tokens.saturating_sub(previous_estimate))
        }
        _ => (current_estimated_tokens as f64 * calibration).round() as u64,
    }
}

/// 主动压缩后仍超预算线:tail 太大或 head 太大。从 tail 最旧端往回收,删到
/// 不超线为止;任务定义、纪要与当前用户消息一律不动。否则下一步预算检查
/// 立刻再压——连续两次压缩 = 缓存前缀两次全量重算(cache_write 双倍),
/// 省下的 token 不够补缓存成本。
#[allow(dead_code)] // 保留通用测试/非协议调用口径。
pub(crate) fn trim_tail(
    messages: &mut Vec<Message>,
    system: &[String],
    specs: &[ToolSpec],
    budget: u64,
    calibration: f64,
    overflow_traces: &mut Vec<String>,
) {
    trim_tail_for_protocol(
        messages,
        system,
        specs,
        budget,
        calibration,
        overflow_traces,
        None,
    );
}

pub(crate) fn trim_tail_for_protocol(
    messages: &mut Vec<Message>,
    system: &[String],
    specs: &[ToolSpec],
    budget: u64,
    calibration: f64,
    overflow_traces: &mut Vec<String>,
    protocol: Option<ProtocolKind>,
) {
    let mut dropped_any = false;
    loop {
        // 校准口径,与调用方判"是否仍超线"同源(D-203):这里用原始估算的话,
        // calibration>1 时会提前收手,调用方视角仍超线,下一步立刻再压。
        if budgeted_tokens_for_protocol(system, messages, specs, calibration, protocol) <= budget {
            break;
        }
        let Some(head_index) = messages.iter().position(is_text_user_message) else {
            break;
        };
        let Some(current_index) = messages.iter().rposition(is_text_user_message) else {
            break;
        };
        // head 之后、当前用户消息之前最靠前的一条 = tail 最旧端。纪要消息
        // (以 "(系统:" 开头)是中段的替代品,不是可回收物,跳过。
        let target = (head_index + 1..current_index).find(|&i| {
            !messages[i]
                .parts
                .iter()
                .any(|p| matches!(p, Part::Text { text } if text.starts_with("(系统:")))
        });
        let Some(i) = target else {
            break;
        };
        let dropped_msg = messages.remove(i);
        dropped_any = true;
        overflow_traces.push(dropped_trace(std::slice::from_ref(&dropped_msg)));
    }
    // D-723:按下标删整条消息会把 ToolCall 和它的 ToolResult 拆开。循环一旦恰好
    // 在删掉 assistant 那条后够线收手,留下的就是孤儿结果;Responses 协议下
    // 直接 400:No tool call found for function call output with call_id ...。
    // 装载时的 filter_message_history 清的是**入参历史**,清不到本函数刚制造的
    // 孤儿,所以这里补一次。它只会再删,不会把 token 涨回线上。
    if dropped_any {
        *messages = crate::history::filter_message_history(messages);
    }
}

/// 从文本中提取常见源码/文档文件 basename(如 runner.rs、style.css)。
/// 用于纪要质量校验:纪要必须保留中段出现过的关键文件,否则判定不可信。
pub(crate) fn extract_file_names(text: &str) -> HashSet<String> {
    const EXTS: &[&str] = &[
        "rs", "md", "toml", "json", "js", "ts", "css", "html", "mjs", "ps1", "sql", "db", "yml",
        "yaml", "lock", "txt", "tsx", "vue", "cjs", "snap",
    ];
    let mut out = HashSet::new();
    for token in text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | ',' | '(' | ')' | '[' | ']' | ':' | ';'
            )
    }) {
        let t = token
            .trim_start_matches(['.', '/', '\\'])
            .trim_end_matches(['.', ',', ';', ':', '、', '，', ')']);
        let Some((_, ext)) = t.rsplit_once('.') else {
            continue;
        };
        if !EXTS.contains(&ext) {
            continue;
        }
        let base = t.rsplit(['/', '\\']).next().unwrap_or(t);
        if base.len() >= 4 && !base.starts_with('.') {
            out.insert(base.to_string());
        }
    }
    out
}

/// 把消息段拼成质量校验用的语料(文本 + 工具名/入参 + 工具结果)。
pub(crate) fn message_corpus(messages: &[Message]) -> String {
    let mut source = String::new();
    for message in messages {
        for part in &message.parts {
            match part {
                Part::Text { text } => source.push_str(text),
                Part::ToolCall { name, input, .. } => {
                    source.push_str(name);
                    source.push_str(&input.to_string());
                }
                Part::ToolResult { content, .. } => source.push_str(content),
                _ => {}
            }
        }
        source.push('\n');
    }
    source
}

/// 纪要质量门槛(R-236 B2 升级为双向):长度下限 + recall + precision。
///
/// - **recall**(D-181 原有):fast 模型纪要最常见的失败是泛化成"进行了一些修改",
///   一个文件都不提——语料里出现过 ≥2 个文件而纪要一个都没提 → 不可信。
/// - **precision**(B2 新增):反向防编造——纪要提到的文件**过半**在语料里不存在,
///   说明文件清单是幻觉出来的(实体级忠实度校验,先例见 arXiv:2102.09130),同样
///   不可信。不达标由调用方重试一次或回落原文节选(节选是原文,不可能编)。
pub(crate) fn digest_acceptable(corpus: &str, digest: &str) -> bool {
    if digest.chars().count() < 60 {
        return false;
    }
    let source_files = extract_file_names(corpus);
    let digest_files = extract_file_names(digest);
    if source_files.len() >= 2 {
        // recall:语料里有文件可校验,纪要至少要保留其一。
        if !digest_files
            .iter()
            .any(|file| source_files.contains(file.as_str()))
        {
            return false;
        }
    }
    if !digest_files.is_empty() {
        // precision:纪要文件清单过半不在语料 → 编造,拒。
        let unknown = digest_files
            .iter()
            .filter(|file| !source_files.contains(file.as_str()))
            .count();
        if unknown * 2 > digest_files.len() {
            return false;
        }
    }
    true
}
pub(super) fn is_text_user_message(message: &Message) -> bool {
    message.role == Role::User
        && message
            .parts
            .iter()
            .any(|part| matches!(part, Part::Text { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-176:轮内预算必须把**工具 schema** 算进去。它每步整份重发,在工具多的
    /// profile 下是常驻大头;漏算会让预算长期偏低,该压的时候不压。
    #[test]
    fn 上下文估算把工具schema计入并按预算线判定() {
        let system = vec!["s".repeat(400)];
        let messages = vec![Message::user_text("m".repeat(4_000))];
        let specs = vec![kanzei_llm::ToolSpec {
            name: "bash".into(),
            description: "d".repeat(2_000),
            input_schema: serde_json::json!({ "blob": "x".repeat(4_000) }),
        }];

        let without_tools = estimate_prompt_tokens(&system, &messages, &[]);
        let with_tools = estimate_prompt_tokens(&system, &messages, &specs);
        assert!(
            with_tools >= without_tools + 1_400,
            "工具 schema 必须计入预算: {without_tools} -> {with_tools}"
        );

        // 预算线的语义:超过 limit*ratio 才动手,没超不动。
        let limit = 3_000u64;
        let budget = (limit as f64 * CONTEXT_BUDGET_RATIO) as u64;
        assert_eq!(budget, 2_100);
        assert!(with_tools > budget, "构造的样本应当越线: {with_tools}");
        // 同一个样本在不计工具 schema 时并不越线——这正是漏算会漏压的场景。
        assert!(without_tools < budget, "{without_tools}");
        assert!(
            estimate_prompt_tokens(&system, &[Message::user_text("短")], &[]) < budget,
            "小请求不该越线"
        );
    }
    /// R-236 验收④：固定同一组负载，对照遗留 0.7 比例线与 headroom 新线。
    /// 这是可复现的机制测量，不冒充真实 provider 生产样本；生产轨迹仍由
    /// ContextPruned/context.compacted 事件记录。
    #[test]
    fn 同一固定负载的压缩触发频率_旧线对照新线() {
        let limit = 128_000u64;
        let output = 8_192u32;
        let buffer = 20_000u64;
        let samples = [80_000, 90_000, 100_000, 105_000, 110_000, 115_000, 120_000];
        let legacy_budget = (limit as f64 * CONTEXT_BUDGET_RATIO) as u64;
        let current_budget = compaction_budget(limit, output, buffer);
        let legacy_triggers = samples
            .iter()
            .filter(|tokens| **tokens > legacy_budget)
            .count();
        let current_triggers = samples
            .iter()
            .filter(|tokens| **tokens > current_budget)
            .count();

        assert_eq!(legacy_budget, 89_600);
        assert_eq!(current_budget, 108_000);
        assert_eq!(legacy_triggers, 6, "旧线应在 7 个样本中触发 6 次");
        assert_eq!(current_triggers, 3, "新线应在 7 个样本中触发 3 次");
        assert_eq!((legacy_triggers, samples.len()), (6, 7));
        assert_eq!((current_triggers, samples.len()), (3, 7));
    }

    /// R-236 B1:预算公式统一为 headroom 形态——limit − max(output, buffer),
    /// 封底 limit/4;轮内与轮末同一把尺,谁把比例线加回来先删这条。
    #[test]
    fn 压缩预算_headroom公式_封底四分之一() {
        // 常规:128k 窗口、8k 输出、20k buffer → 108k。
        assert_eq!(compaction_budget(128_000, 8_192, 20_000), 108_000);
        // 输出上限大于 buffer 时取输出上限,但最多预留窗口三分之一。
        assert_eq!(compaction_budget(128_000, 32_000, 20_000), 96_000);
        // 小窗口:输出与 buffer 都按窗口三分之一封顶,不再让 32k 输出上限吃掉一半。
        assert_eq!(compaction_budget(32_000, 8_192, 20_000), 21_334);
        assert_eq!(compaction_budget(65_536, 32_768, 20_000), 43_691);
        // 更小窗口仍保留至少四分之一封底,且预算随窗口自适应。
        assert_eq!(compaction_budget(16_000, 8_192, 20_000), 10_667);
    }

    /// R-236 B1:附件按固定成本估算,不按 base64 字节——带一张截图的会话不再
    /// 被虚高几万 token、每轮误触发压缩。
    #[test]
    fn 附件估算按固定成本_不按base64字节() {
        // 约 1MB base64:旧口径(整段序列化 len/4)会估出 ~26 万 token。
        let big_image = Message {
            role: kanzei_llm::Role::User,
            parts: vec![Part::Image {
                media_type: "image/png".into(),
                data: "A".repeat(1_000_000),
            }],
        };
        let with_image = estimate_prompt_tokens(&[], std::slice::from_ref(&big_image), &[]);
        assert!(
            with_image <= ATTACHMENT_TOKEN_COST + 100,
            "附件必须按固定成本(≈{ATTACHMENT_TOKEN_COST})计,实得 {with_image}"
        );
        // 文本消息口径不变:len/4 量级。
        let text = Message::user_text("x".repeat(4_000));
        let text_estimate = estimate_prompt_tokens(&[], std::slice::from_ref(&text), &[]);
        assert!(
            (900..=1_300).contains(&text_estimate),
            "纯文本估算应保持 len/4 量级,实得 {text_estimate}"
        );
    }

    /// 中文场景下 clip 的上限必须是字符数,而不是被字节余额放大三倍。
    #[test]
    fn clip按字符截断且中文不超额() {
        let chinese = "上下文压缩".repeat(100); // 500 字,1500 字节
        let clipped = clip(&chinese, 50);
        assert_eq!(clipped.chars().count(), 50 + "…[截断]".chars().count());
        assert_eq!(clip("短", 50), "短", "未超长不加省略标记");
    }
    /// D-592 B2:首步没有 usage 时必须采用保守校准,避免新会话首步裸奔。
    #[test]
    fn 冷启动校准使用保守上限() {
        assert_eq!(conservative_calibration(), COLD_START_CALIBRATION);
        assert_eq!(COLD_START_CALIBRATION, 2.0);
    }

    /// D-592 B3:OpenAI Chat 不回放 reasoning,预算不能把它当成实际 prompt；
    /// 其他协议仍保留 reasoning 历史，避免把协议范围错误收窄。
    #[test]
    fn 协议估算与_reasoning_回放口径一致() {
        let reasoning = Message::assistant(vec![Part::Reasoning {
            text: "内部推理 ".repeat(40),
            signature: None,
        }]);
        let without_reasoning = estimate_prompt_tokens(&[], &[], &[]);
        let chat = estimate_prompt_tokens_for_protocol(
            &[],
            std::slice::from_ref(&reasoning),
            &[],
            Some(ProtocolKind::OpenAiChat),
        );
        let responses = estimate_prompt_tokens_for_protocol(
            &[],
            std::slice::from_ref(&reasoning),
            &[],
            Some(ProtocolKind::OpenAiResponses),
        );
        assert_eq!(chat, without_reasoning);
        assert!(responses > chat);
    }
    /// 校准因子:中文 \uXXXX 转义/工具输出密集场景下 len/4 估算偏高,真实 usage
    /// 应把它拉回;单步异常比值被限幅,不会一次带飞;拿不到 usage 时不更新。
    #[test]
    fn 校准因子按真实usage收敛且单步限幅() {
        // 估算偏高 1 倍(实际只有一半),EMA 向下收敛。
        let mut c = update_calibration(1.0, 10_000, 5_000);
        assert!(
            (c - 0.85).abs() < 1e-9,
            "第一次应得 1.0*0.7+0.5*0.3=0.85,实得 {c}"
        );
        c = update_calibration(c, 10_000, 5_000);
        assert!(
            (c - 0.745).abs() < 1e-9,
            "第二次应得 0.85*0.7+0.5*0.3=0.745,实得 {c}"
        );
        // 实际远超估算(3 倍)被限幅在 2 倍,单步最多 +0.3。
        let c3 = update_calibration(1.0, 10_000, 30_000);
        assert!(
            (c3 - 1.3).abs() < 1e-9,
            "限幅后应得 1.0*0.7+2.0*0.3=1.3,实得 {c3}"
        );
        // 零值保护:估算为 0 或拿不到真实值时不更新。
        assert_eq!(update_calibration(0.9, 0, 100), 0.9);
        assert_eq!(update_calibration(0.9, 100, 0), 0.9);
        // 估算恰好命中时不漂移。
        assert!((update_calibration(1.0, 8_000, 8_000) - 1.0).abs() < 1e-9);
    }
    /// D-592:有真实 usage 时,预算只用上一步实际值加本步新增估算;
    /// 没有 usage 才回退到完整估算×校准。
    #[test]
    fn 预算估算优先锚定上一步真实usage() {
        assert_eq!(
            budgeted_tokens_from_last_usage(Some(10_000), Some(12_000), 16_000, 2.0),
            14_000,
            "真实上一请求 + 新增 4k,不应重新按整段 16k×校准"
        );
        assert_eq!(
            budgeted_tokens_from_last_usage(Some(10_000), Some(12_000), 8_000, 2.0),
            10_000,
            "当前估算因压缩变小不能把真实历史倒扣"
        );
        assert_eq!(
            budgeted_tokens_from_last_usage(None, None, 8_000, 1.5),
            12_000,
            "冷启动仍使用完整估算×校准"
        );
    }
    /// trim_tail:压缩后仍超线时只收 tail 最旧端,任务定义、纪要、当前用户
    /// 消息一律不动——否则下一步预算检查立刻再压,缓存前缀两次全量重算。
    #[test]
    fn trim_tail只收最旧端且不动首尾与纪要() {
        let system = vec![String::new()];
        let mut messages = vec![Message::user_text("任务定义:修复 D-123 的空指针")];
        messages.push(Message::user_text(
            "(系统:此前 40 条消息已压缩为纪要,基于它继续)\n改动了 runner.rs 的压缩逻辑,store.rs 待处理",
        ));
        for i in 0..30 {
            messages.push(Message::user_text(format!(
                "tail 第 {i} 条 {}",
                "x".repeat(300)
            )));
        }
        messages.push(Message::user_text("当前指令:继续修"));
        let mut traces = Vec::new();
        trim_tail(&mut messages, &system, &[], 1_200, 1.0, &mut traces);

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
            estimate_prompt_tokens(&system, &messages, &[]) <= 1_200,
            "必须收到预算线内"
        );
        assert!(text.contains("任务定义:修复 D-123"), "任务定义不能动");
        assert!(text.contains("runner.rs 的压缩逻辑"), "纪要不能动");
        assert!(text.contains("当前指令:继续修"), "当前用户消息不能动");
        assert!(text.contains("tail 第 29 条"), "最近的 tail 要优先保住");
        assert!(!text.contains("tail 第 0 条"), "最旧的 tail 先被回收");
        assert!(!traces.is_empty(), "回收的 tail 要留轨迹");
    }
    /// D-723:trim_tail 删的是**整条消息**,而一次工具轮是 assistant(ToolCall) +
    /// user(ToolResult) 两条。只删前一条就留下孤儿 function_call_output,
    /// Responses 协议直接 400。预算线落在哪里是连续的,所以这里扫一排预算
    /// 逐个验证不变式:trim 完的历史经 filter_message_history 应逐字节不变。
    #[test]
    fn trim_tail不得把工具调用与结果拆成孤儿() {
        let system = vec![String::new()];
        let big = "y".repeat(600);
        let build = || {
            let mut messages = vec![Message::user_text("任务定义:修复空指针")];
            for i in 0..12 {
                // 真实历史里 assistant 那条是**正文 + 工具调用**一起的。只放一个光秃
                // ToolCall 的话删它几乎不降 token,循环几乎总在删完结果那步才收手,
                // 孤儿碍于样本形状被掩盖。带正文才能让断点落在两者之间。
                messages.push(Message::assistant(vec![
                    Part::Text {
                        text: format!("先看一下第 {i} 个文件 {}", big.clone()),
                    },
                    Part::ToolCall {
                        id: format!("call_{i}"),
                        name: "read".into(),
                        input: serde_json::json!({"path": format!("src/f{i}.rs")}),
                    },
                ]));
                messages.push(Message::tool_results(vec![Part::ToolResult {
                    call_id: format!("call_{i}"),
                    content: big.clone(),
                    is_error: false,
                }]));
            }
            messages.push(Message::user_text("当前指令:继续修"));
            messages
        };
        for budget in (200..6_000).step_by(10) {
            let mut messages = build();
            let mut traces = Vec::new();
            trim_tail(&mut messages, &system, &[], budget, 1.0, &mut traces);
            let filtered = crate::history::filter_message_history(&messages);
            assert_eq!(
                filtered.len(),
                messages.len(),
                "budget={budget} 时 trim_tail 留下了孤儿工具 part（Responses 会 400）"
            );
            for (a, b) in filtered.iter().zip(messages.iter()) {
                assert_eq!(
                    a.parts.len(),
                    b.parts.len(),
                    "budget={budget} 时有孤儿 part"
                );
            }
        }
    }
    /// D-206:检查点是周期性动作,不是有限清单。旧实现 20|40|80 之后永不盘点,
    /// 无步数上限的自举 run 后半程恰恰最需要。
    #[test]
    fn 盘点检查点长跑不熄火() {
        use super::is_budget_checkpoint;
        for step in [20, 40, 80, 120, 160, 400] {
            assert!(is_budget_checkpoint(step), "第 {step} 步应盘点");
        }
        for step in [1, 19, 21, 39, 41, 79, 81, 119, 399] {
            assert!(!is_budget_checkpoint(step), "第 {step} 步不该盘点");
        }
    }
    /// D-203:trim_tail 必须用**校准口径**够预算线,与调用方同一把尺子。
    /// a119eeb 把校准乘在了三个调用点上,trim_tail 内部却用原始估算——
    /// calibration>1 时它提前收手,调用方视角仍超线,下一步立刻再压,
    /// 缓存前缀两次全量重算,恰是它要防的事。谁把 budgeted_tokens 改回
    /// 原始估算,这条立刻红。
    #[test]
    fn trim_tail按校准口径收线_调用方视角不再超预算() {
        let calibration = 1.6; // 中文密集轨迹的真实形态:估算系统性偏低
        let system = vec![String::new()];
        let build = || {
            let mut messages = vec![Message::user_text("任务定义:修复 D-123")];
            for i in 0..40 {
                messages.push(Message::user_text(format!(
                    "tail 第 {i} 条 {}",
                    "x".repeat(300)
                )));
            }
            messages.push(Message::user_text("当前指令:继续修"));
            messages
        };
        // 预算选在"原始口径已达标、校准口径仍超线"的区间,专门打那个提前收手。
        let mut messages = build();
        let budget = estimate_prompt_tokens(&system, &messages, &[]) - 500;
        assert!(
            budgeted_tokens(&system, &messages, &[], calibration) > budget,
            "夹具无效:校准口径必须超线"
        );

        let mut traces = Vec::new();
        trim_tail(
            &mut messages,
            &system,
            &[],
            budget,
            calibration,
            &mut traces,
        );
        assert!(
            budgeted_tokens(&system, &messages, &[], calibration) <= budget,
            "trim_tail 收完后调用方的校准视角必须在预算内,否则下一步立刻再压"
        );
        // calibration = 1.0 时退化为原始口径,老行为不变。
        let mut plain = build();
        let mut plain_traces = Vec::new();
        trim_tail(&mut plain, &system, &[], budget, 1.0, &mut plain_traces);
        assert!(estimate_prompt_tokens(&system, &plain, &[]) <= budget);
    }
    /// 纪要质量(R-236 B2 双向):泛化纪要(recall 失守)与编造文件清单
    /// (precision 失守)都判不可信、回落原文节选;保留关键文件且不编造的放行。
    #[test]
    fn 纪要质量校验_泛化与编造双向拒绝() {
        let middle = vec![
            Message::user_text("改了 crates/kanzei-core/src/runner.rs 的压缩逻辑"),
            Message::assistant(vec![Part::ToolCall {
                id: "t1".into(),
                name: "grep".into(),
                input: serde_json::json!({ "pattern": "trim_tail", "path": "src/store.rs" }),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "t1".into(),
                content: "src/store.rs:12: fn trim_tail".into(),
                is_error: false,
            }]),
        ];
        let corpus = message_corpus(&middle);
        // 泛化纪要:一个文件都不提 → recall 失守,不可信。
        assert!(!digest_acceptable(
            &corpus,
            "对项目进行了若干修改,修复了多个问题,下一步继续推进。"
        ));
        // 提到关键文件且没编造 → 可信。
        assert!(digest_acceptable(
            &corpus,
            "本次改动集中在 runner.rs 的上下文压缩路径:新增 trim_tail 与纪要质量校验,\
             store.rs 的迁移逻辑尚未处理,下一步补上对应测试并跑全量回归。"
        ));
        // precision:文件清单过半是语料里不存在的 → 编造,拒。
        assert!(!digest_acceptable(
            &corpus,
            "本次修改了 runner.rs、helper.rs、pipeline.rs 与 scheduler.rs 四个文件,\
             重构了调度与压缩路径,下一步跑回归验证整体行为。"
        ));
        // 太短 → 不可信。
        assert!(!digest_acceptable(&corpus, "修了 bug。"));
        // 语料几乎没文件 → 长度门槛兜底,不误杀(纪要足够具体)。
        let bare_corpus = message_corpus(&[Message::user_text("纯对话轮次,没有文件操作")]);
        assert!(digest_acceptable(
            &bare_corpus,
            "讨论了方案 A 与方案 B 的取舍,结论是 A 优于 B:改动面最小、风险可控,且与既有抽象保持一致;\
             下一步与用户确认接口约定后再动手实现。"
        ));

        // 路径提取边界:扩展名白名单外的不算,没有点号的普通词不算。
        let files = extract_file_names(
            "crates/kanzei-core/src/runner.rs 与 style.css 与 foo.exe 与 一个普通词",
        );
        assert!(files.contains("runner.rs"));
        assert!(files.contains("style.css"));
        assert!(!files.contains("foo.exe"), "exe 不在白名单");
        assert!(!files.contains("普通词"), "没有点号的不算文件");
    }
}
