//! 上下文预算域(R-155 B4):预算估算与校准(budgeted_tokens/estimate_prompt_tokens/
//! update_calibration)、trim_tail、纪要渲染与质量校验(render_for_digest/
//! digest_plausible/extract_file_names/clip)、周期盘点(is_budget_checkpoint)。
//! is_text_user_message 三归属(compact_with_digest/trim_tail/drive),提 pub(super);
//! MAX_FUTILE_COMPACTIONS 留 mod.rs(pub(super))。

use crate::runner::compaction::dropped_trace;
use kanzei_llm::{Message, Part, Role, ToolSpec};
use std::collections::HashSet;

/// 轮内主动压缩的触发线(占 context_limit 的比例)。测试锚点:生产调用方以
/// 字面量传参(0.7),本常量被测试断言引用(lib 构建显 unused,故 allow)。
#[allow(dead_code)]
pub const CONTEXT_BUDGET_RATIO: f64 = 0.7;
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
/// 本步请求的 token 粗估:system + 历史 + **工具 schema**。
///
/// 工具 schema 必须计入——它每一步都整份重发,在工具多的 profile 下是常驻大头,
/// 漏算就会让预算长期偏低、该压的时候不压。粒度沿用 len/4,与既有压缩预检同源。
pub(crate) fn estimate_prompt_tokens(
    system: &[String],
    messages: &[Message],
    specs: &[ToolSpec],
) -> u64 {
    let system_bytes: usize = system.iter().map(String::len).sum();
    let message_bytes = serde_json::to_string(messages).map_or(0, |text| text.len());
    let spec_bytes: usize = specs
        .iter()
        .map(|spec| spec.name.len() + spec.description.len() + spec.input_schema.to_string().len())
        .sum();
    ((system_bytes + message_bytes + spec_bytes) / 4) as u64
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
pub(crate) fn budgeted_tokens(
    system: &[String],
    messages: &[Message],
    specs: &[ToolSpec],
    calibration: f64,
) -> u64 {
    (estimate_prompt_tokens(system, messages, specs) as f64 * calibration).round() as u64
}

/// 主动压缩后仍超预算线:tail 太大或 head 太大。从 tail 最旧端往回收,删到
/// 不超线为止;任务定义、纪要与当前用户消息一律不动。否则下一步预算检查
/// 立刻再压——连续两次压缩 = 缓存前缀两次全量重算(cache_write 双倍),
/// 省下的 token 不够补缓存成本。
pub(crate) fn trim_tail(
    messages: &mut Vec<Message>,
    system: &[String],
    specs: &[ToolSpec],
    budget: u64,
    calibration: f64,
    overflow_traces: &mut Vec<String>,
) {
    loop {
        // 校准口径,与调用方判"是否仍超线"同源(D-203):这里用原始估算的话,
        // calibration>1 时会提前收手,调用方视角仍超线,下一步立刻再压。
        if budgeted_tokens(system, messages, specs, calibration) <= budget {
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
        overflow_traces.push(dropped_trace(std::slice::from_ref(&dropped_msg)));
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

/// 纪要质量门槛:长度下限 + 关键文件保留率。
///
/// fast 模型纪要最常见的失败是泛化成"进行了一些修改",一个文件都不提——
/// 压完模型不知道自己在干什么,原地重做。机械校验:中段里出现过 ≥2 个文件
/// 而纪要一个都没提 → 不可信,调用方回落到原文节选(节选是原文,不可能丢)。
pub(crate) fn digest_plausible(middle: &[Message], digest: &str) -> bool {
    if digest.chars().count() < 60 {
        return false;
    }
    let mut source = String::new();
    for message in middle {
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
    let files = extract_file_names(&source);
    if files.len() < 2 {
        return true; // 没几个文件可校验,长度门槛兜底。
    }
    files.iter().any(|f| digest.contains(f.as_str()))
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
    /// 中文场景下 clip 的上限必须是字符数,而不是被字节余额放大三倍。
    #[test]
    fn clip按字符截断且中文不超额() {
        let chinese = "上下文压缩".repeat(100); // 500 字,1500 字节
        let clipped = clip(&chinese, 50);
        assert_eq!(clipped.chars().count(), 50 + "…[截断]".chars().count());
        assert_eq!(clip("短", 50), "短", "未超长不加省略标记");
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
    /// 纪要质量:泛化纪要(一个文件都不提)判定不可信、回落到原文节选;
    /// 保留了关键文件的纪要放行;太短同样不可信。
    #[test]
    fn 纪要质量校验拒绝泛化纪要() {
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
        // 泛化纪要:一个文件都不提 → 不可信。
        assert!(!digest_plausible(
            &middle,
            "对项目进行了若干修改,修复了多个问题,下一步继续推进。"
        ));
        // 提到任一关键文件 → 可信(长度也要过门槛,真实纪要约数百字)。
        assert!(digest_plausible(
            &middle,
            "本次改动集中在 runner.rs 的上下文压缩路径:新增 trim_tail 与纪要质量校验,\
             store.rs 的迁移逻辑尚未处理,下一步补上对应测试并跑全量回归。"
        ));
        // 太短 → 不可信。
        assert!(!digest_plausible(&middle, "修了 bug。"));
        // 中段几乎没文件 → 长度门槛兜底,不误杀(纪要足够具体)。
        let bare = vec![Message::user_text("纯对话轮次,没有文件操作")];
        assert!(digest_plausible(
            &bare,
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
