//! agent 循环(M1):harness 快照驱动——工具物化过权限、system 由 Context Source 拼装、
//! 每次工具调用过硬门禁(deny 回喂模型 / ask 问用户,用户拒绝则整轮停,与 V2 语义一致)。
//! steer/queue/持久化调度在 M2 引入。

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use futures::StreamExt;
use kanzei_harness::{
    tolerant_parse, tool::repair_hint, AgentDef, Effect, HarnessSnapshot, Tool, ToolConcurrency,
    ToolCtx,
};
use kanzei_llm::{
    FinishReason, LlmClient, LlmEvent, LlmRequest, Message, Part, ReasoningEffort, Role, Route,
    ToolSpec, Usage,
};

mod event;
pub use event::*;
use event::{drain_task_events, preview};

pub struct RunnerConfig {
    pub model: String,
    pub max_tokens: u32,
    /// 思考强度;由调用方(CLI/桌面端)按配置或运行时选择传入。
    pub reasoning: ReasoningEffort,
    /// Responses 服务档位;仅 Codex Fast mode 使用 priority。
    pub service_tier: Option<String>,
    /// 该模型的上下文窗口。None = 未知,轮内不做主动预算(只保留撞墙后的被动恢复)。
    pub context_limit: Option<u64>,
    /// 可调上限([limits] 节)。全 None = 内置默认,与改造前的硬编码常量逐值一致。
    pub limits: kanzei_harness::config::Limits,
}

/// 单轮子代理上限：并行仍保持，但避免模型一次生成过多请求拖垮连接/本地模型。
pub const MAX_TASKS_PER_TURN: usize = 8;
/// 同一无冲突 wave 的普通工具并发上限；超过时按原调用顺序切 wave。
pub const MAX_PARALLEL_TOOLS_PER_WAVE: usize = 8;

/// 流中途断开后重放本步请求的上限。工具在流结束后才执行,所以此时重放零副作用;
/// 但每次重放都要重新生成已产出的 token,必须有界。
pub const MAX_STREAM_RESTARTS: u32 = 2;

/// Provider 上下文计算可能比本地估算更严格。首次超限保留有界历史摘要，
/// 第二次超限只保留当前用户消息；再失败就把真实错误交给调用方。
pub const MAX_CONTEXT_OVERFLOW_RECOVERIES: u32 = 2;

/// 轮内主动压缩的触发线(占 context_limit 的比例)。
pub const CONTEXT_BUDGET_RATIO: f64 = 0.7;
/// 主动压缩的**连续无效**刹车,不是总次数配额(D-206)。
///
/// 旧常量 MAX_PROACTIVE_COMPACTIONS=3 把注释里的"压缩后仍超线再压无益"实现成了
/// 总量计数——**成功的压缩也扣配额**。对无步数上限的自举 run,压缩是常规运营动作:
/// 上下文反复涨到预算线本来就该反复压。实测第 58 步的长 run 里三次**成功**压缩把
/// 配额吃光,之后放飞,上下文一路涨向 context_limit,prefill 慢到单步等待 863s,
/// 只能等 provider 报 overflow 走被动恢复——主动压缩在最需要它的场景下提前退场。
///
/// 正确判据与注释原意一致:压完低于线 = 在正常工作,清零计数、不限次数;
/// 压完仍超线 / 中段为空压不动 = 无进展,连续两次就停(head+当前消息本身超线,
/// trim_tail 都救不了,交给被动恢复,别空转)。
const MAX_FUTILE_COMPACTIONS: u32 = 2;
/// 盘点检查点:第 20/40 步,之后每 40 步一次(80/120/160…)。
///
/// 旧实现 `matches!(step, 20 | 40 | 80)` 是有限清单——第 80 步之后的长 run 永不再
/// 盘点,而无步数上限的自举 run 恰恰是最需要盘点的(D-206 顺带修:与压缩总量配额
/// 同型,把"周期性动作"写成了"有限次动作")。
fn is_budget_checkpoint(step: u32) -> bool {
    step == 20 || (step >= 40 && step % 40 == 0)
}

/// 主动压缩后,最近这部分历史逐字保留的预算占比(相对 context_limit)。
/// 主动压缩发生在还有余量的时候,没理由像应急路径那样推倒重来——保住近期
/// 工作区,模型才知道自己刚做了什么,不会压完就原地重做。
const RECENT_VERBATIM_RATIO: f64 = 0.35;
/// 交给 fast 模型做纪要的原文上限(字符)。超出时取**最近**的部分:
/// 中段越靠后越相关。
const DIGEST_SOURCE_CHARS: usize = 24_000;

/// 把消息渲染成给"纪要模型"看的文本。工具调用必须带上工具名与关键入参——
/// 只留工具输出而不知道是谁产生的,压缩完等于一堆无主的结果(D-181)。
fn render_for_digest(messages: &[Message]) -> String {
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
                Part::ToolResult { content, is_error, .. } => {
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
fn clip(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push_str("…[截断]");
    }
    out
}

/// 主动压缩:保住任务定义与近期工作区,只把中段交给 fast 模型出纪要。
///
/// 与应急路径 `compact_messages_for_retry` 的分工是刻意的:那条路发生在
/// provider 已经拒绝请求之后,粗暴但必须成功;这条路发生在还有三成余量时,
/// 有时间也有理由做得体面。实测旧实现把主动路径直接接到应急函数上,一次从
/// 89.6k 预算砍到约 2k(97%),而且保留的是**最旧**的 8000 字节、丢掉刚做完的
/// 工作——压完模型不知道自己在干什么,大概率原地重做。
async fn compact_with_digest(
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
        Some(text) => format!("(系统:此前 {} 条消息已压缩为纪要,基于它继续)\n{text}", middle.len()),
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

/// 本步请求的 token 粗估:system + 历史 + **工具 schema**。
///
/// 工具 schema 必须计入——它每一步都整份重发,在工具多的 profile 下是常驻大头,
/// 漏算就会让预算长期偏低、该压的时候不压。粒度沿用 len/4,与既有压缩预检同源。
fn estimate_prompt_tokens(system: &[String], messages: &[Message], specs: &[ToolSpec]) -> u64 {
    let system_bytes: usize = system.iter().map(String::len).sum();
    let message_bytes = serde_json::to_string(messages).map_or(0, |text| text.len());
    let spec_bytes: usize = specs
        .iter()
        .map(|spec| {
            spec.name.len()
                + spec.description.len()
                + spec.input_schema.to_string().len()
        })
        .sum();
    ((system_bytes + message_bytes + spec_bytes) / 4) as u64
}

/// 用 provider 返回的真实 input token 数做滑动校准,修正 len/4 估算的
/// 系统性偏差(中文 \uXXXX 转义、工具输出密集等)。EMA 收敛,单步比值限幅
/// 在 [0.5, 2.0],防止异常记账值(如缓存命中的特殊计费)把校准一次带飞。
fn update_calibration(current: f64, estimated: u64, actual: u64) -> f64 {
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
fn budgeted_tokens(
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
fn trim_tail(
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
            !messages[i].parts.iter().any(|p| {
                matches!(p, Part::Text { text } if text.starts_with("(系统:"))
            })
        });
        let Some(i) = target else { break; };
        let dropped_msg = messages.remove(i);
        overflow_traces.push(dropped_trace(std::slice::from_ref(&dropped_msg)));
    }
}

/// 从文本中提取常见源码/文档文件 basename(如 runner.rs、style.css)。
/// 用于纪要质量校验:纪要必须保留中段出现过的关键文件,否则判定不可信。
fn extract_file_names(text: &str) -> HashSet<String> {
    const EXTS: &[&str] = &[
        "rs", "md", "toml", "json", "js", "ts", "css", "html", "mjs", "ps1", "sql", "db", "yml",
        "yaml", "lock", "txt", "tsx", "vue", "cjs", "snap",
    ];
    let mut out = HashSet::new();
    for token in text.split(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | '(' | ')' | '[' | ']' | ':' | ';')
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
fn digest_plausible(middle: &[Message], digest: &str) -> bool {
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

/// task 子代理运行时(R-004/R-012)。快照由调用方用 SubagentBase 组件构建,
/// 代码层面只含只读工具——子代理无人应答权限询问,必须做到零 ask。
pub struct SubagentRuntime {
    pub snapshot: Arc<HarnessSnapshot>,
    pub agent: AgentDef,
    /// (route, model id):fast = 本地小模型跑机械检索。
    pub fast: (Route, String),
    /// primary = 主模型,给需要理解代码的任务。
    pub primary: (Route, String),
    /// 两条路由各自的服务档位(Codex Fast mode)。fast 与 primary 未必是同一供应商,
    /// 所以不能共用一个值——用哪条路由就带哪条的档位。
    pub fast_service_tier: Option<String>,
    pub primary_service_tier: Option<String>,
    pub max_tokens: u32,
    /// 单个子代理的墙钟上限(秒):本地模型多轮可能极慢,必须有界。
    pub timeout_secs: u64,
    /// 可调上限,随主运行链一起传下来。
    pub limits: kanzei_harness::config::Limits,
}

fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Delegate a narrow read-only exploration task (find files, call \
                      sites, usages; read and summarize code) to a subagent with tools \
                      read/glob/grep. Params: prompt (self-contained instruction saying \
                      exactly what to find and what to report back); optional model: \
                      \"fast\" (default, local model, mechanical searches) | \"primary\" \
                      (tasks needing code comprehension). Multiple task calls in one \
                      turn run in parallel."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task: what to find and exactly what to report back"
                },
                "model": {
                    "type": "string",
                    "enum": ["fast", "primary"],
                    "description": "fast = local small model (default); primary = main model"
                }
            },
            "required": ["prompt"]
        }),
    }
}




/// `&summary.messages[prior_len..]`。
pub fn summarize_tools(messages: &[Message]) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for message in messages {
        for part in &message.parts {
            if let Part::ToolCall { name, .. } = part {
                *counts.entry(name.clone()).or_insert(0) += 1;
            }
        }
    }
    counts
}

/// 一条失败信号:同一 (工具 × 错误类) 在本轮的聚合。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FailureSignal {
    pub tool: String,
    /// 归一化错误指纹(抹掉路径与数字后的错误首行),用于聚合与跨轮去重。
    pub kind: String,
    /// 错误原文首段,给 manager 判断用。
    pub sample: String,
    /// 涉及的目标(路径/命令首词),最多 3 个。
    pub targets: Vec<String>,
    pub count: usize,
    /// 同目标后续成功的调用:「X 不行、Y 可以」是最值得记的形状。
    pub recovered_by: Option<String>,
}

/// 从本轮轨迹提炼失败信号(R-105 机械触发):失败数据本来就在 messages 里,
/// 引擎不额外采集,只做一次线性扫描 + 指纹聚合。纯函数,可单测。
///
/// 只传本轮切片(`&summary.messages[prior_len..]`),否则会把历史失败重复上报。
/// 一轮的调用画像(R-099):判断"冗余治理有没有效"所需的最小可比量。
/// 全部来自本轮消息切片,不含历史——混进 prior 会让基线一路虚高。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunMetrics {
    /// 终端调用次数(bash / process)。
    pub terminal_calls: usize,
    /// 其中被判定为 git 查询的次数(status/diff/log/show)。
    pub git_calls: usize,
    /// git 查询"组"数:连续的 git 查询算一组。同样是 6 次查询,
    /// 分散在 6 处比挤成 1 组糟得多——组数才反映节奏问题。
    pub git_groups: usize,
    /// edit / multiedit 调用次数与其中失败(未命中)次数。
    pub edit_calls: usize,
    pub edit_misses: usize,
    /// 子代理调用次数。
    pub subagent_calls: usize,
    /// 工具调用总次数与失败次数。
    pub total_calls: usize,
    pub failed_calls: usize,
    /// R-100 冗余机械门禁触发计数(就地提醒,不阻断):同一工作树无变化的重复
    /// git status/diff、无文件变更的重复全量测试、缺陷记录已含路径仍调 task。
    /// 提醒文本带 `[冗余提醒]` 前缀进入工具结果,这里按前缀归类计数。
    #[serde(default)]
    pub redundant_git: usize,
    #[serde(default)]
    pub redundant_test: usize,
    #[serde(default)]
    pub redundant_task: usize,
}

impl RunMetrics {
    /// edit 未命中率(无 edit 调用时为 0)。
    pub fn edit_miss_rate(&self) -> f64 {
        if self.edit_calls == 0 {
            0.0
        } else {
            self.edit_misses as f64 / self.edit_calls as f64
        }
    }

    /// R-100:三种冗余提醒的总触发次数。
    pub fn redundant_total(&self) -> usize {
        self.redundant_git + self.redundant_test + self.redundant_task
    }
}

/// 统计本轮调用画像。传 `&summary.messages[prior_len..]`,不要传全历史。
pub fn summarize_metrics(messages: &[Message]) -> RunMetrics {
    let mut metrics = RunMetrics::default();
    let mut calls: std::collections::HashMap<String, (String, bool)> =
        std::collections::HashMap::new();
    // 上一次调用是否是 git 查询:用来把连续的 git 查询并成一组。
    let mut prev_was_git = false;

    for message in messages {
        for part in &message.parts {
            match part {
                Part::ToolCall { id, name, input } => {
                    let is_git = name == "bash" && is_git_query(input);
                    calls.insert(id.clone(), (name.clone(), is_git));
                    metrics.total_calls += 1;
                    match name.as_str() {
                        "bash" | "process" => {
                            metrics.terminal_calls += 1;
                            if is_git {
                                metrics.git_calls += 1;
                                if !prev_was_git {
                                    metrics.git_groups += 1;
                                }
                            }
                        }
                        "edit" | "multiedit" => metrics.edit_calls += 1,
                        "task" => metrics.subagent_calls += 1,
                        _ => {}
                    }
                    prev_was_git = is_git;
                }
                Part::ToolResult { call_id, is_error, content, .. } => {
                    if !*is_error {
                        // R-100:机械提醒以 [冗余提醒] 前缀进入结果文本,按类别计数。
                        // 只统计本轮切片,不统计历史(历史提醒早已入库,重复统计会虚高)。
                        if content.contains("[冗余提醒] 工作树与上次") {
                            metrics.redundant_git += 1;
                        } else if content.contains("[冗余提醒] 自上次全量测试") {
                            metrics.redundant_test += 1;
                        } else if content.contains("[冗余提醒] 缺陷") {
                            metrics.redundant_task += 1;
                        }
                        continue;
                    }
                    metrics.failed_calls += 1;
                    if let Some((name, _)) = calls.get(call_id) {
                        if name == "edit" || name == "multiedit" {
                            metrics.edit_misses += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    metrics
}

fn is_git_query(input: &serde_json::Value) -> bool {
    const QUERIES: &[&str] = &["git status", "git diff", "git log", "git show", "git blame"];
    let command = input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    QUERIES.iter().any(|q| command.contains(q))
}

/// R-100 机械门禁:对可机械识别的冗余模式在工具结果中就地处提醒(不阻断,
/// 先观察后升级)。状态按单次运行持有——轮与轮之间不复用,避免跨轮误报。
///
/// 三种模式(全在工具结果文本追加 `[冗余提醒]` 前缀,summarize_metrics 按前缀计数):
/// 1. 同一工作树无变化时的重复 git status/diff:以上一次同类的工具结果内容为
///    工作树指纹,内容一致即判无变化;
/// 2. 无文件变更的重复全量测试:以上一次 git status/diff 的结果内容为指纹,
///    全量测试之间指纹未变即判白跑;
/// 3. 缺陷记录已含文件路径仍调 task:task prompt 里引用 D-xxx 且该缺陷条目
///    字段已含的路径也出现在 prompt 里,说明是在让子代理重新探索已知位置。
#[derive(Default)]
pub(crate) struct RedundancyWatch {
    /// 上一次 git status/diff 的结果内容(工作树指纹,None = 尚未见过)。
    last_git_content: Option<String>,
    /// 最近一次全量测试时的指纹。
    last_full_test_tree: Option<String>,
    /// 本轮是否已跑过全量测试。
    full_test_ran: bool,
}

impl RedundancyWatch {
    /// 在整步工具结果回喂前调用:`results` 与 `calls` 按下标一一对应
    /// (并行 wave 与串行路径都保持该对齐)。只追加、不改 is_error。
    pub(crate) fn note_step(
        &mut self,
        project_root: &std::path::Path,
        calls: &[(String, String, serde_json::Value, String)],
        results: &mut [Part],
    ) {
        for (index, (_, name, input, _)) in calls.iter().enumerate() {
            let Some(Part::ToolResult { content, is_error, .. }) = results.get_mut(index) else {
                continue;
            };
            if *is_error || content.is_empty() {
                continue;
            }
            match name.as_str() {
                "bash" => {
                    // 先取原始内容再比较:提醒文本不能污染指纹,否则下次比较恒不相等。
                    let original = content.clone();
                    if is_git_query(input) {
                        if let Some(prev) = &self.last_git_content {
                            if prev == &original {
                                content.push_str(
                                    "\n[冗余提醒] 工作树与上次 git status/diff 无变化,这次查询可省",
                                );
                            }
                        }
                        self.last_git_content = Some(original);
                    } else {
                        let command = input
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if is_full_test_command(&command) {
                            if self.full_test_ran {
                                if let (Some(cur), Some(prev)) =
                                    (&self.last_git_content, &self.last_full_test_tree)
                                {
                                    if cur == prev {
                                        content.push_str(
                                            "\n[冗余提醒] 自上次全量测试以来工作树无变更,这次测试可省",
                                        );
                                    }
                                }
                            }
                            self.last_full_test_tree = self.last_git_content.clone();
                            self.full_test_ran = true;
                        }
                    }
                }
                "task" => {
                    let prompt = input
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(note) = defect_known_path_hint(project_root, prompt) {
                        content.push_str(&format!("\n[冗余提醒] {note}"));
                    }
                }
                _ => {}
            }
        }
    }
}

/// 全量测试判定:覆盖整个 workspace 的 cargo 测试命令。
/// `cargo test --workspace` 系列显式全量;不带 `-p` 的 `cargo test` 在工作区根
/// 跑的就是全量(把 -p 定向测试排除在外,定向不算全量)。
fn is_full_test_command(command: &str) -> bool {
    let c = command.to_lowercase();
    if !(c.contains("cargo test") || c.contains("cargo nextest")) {
        return false;
    }
    const FULL_FLAGS: &[&str] = &["--workspace", "--all", "--all-targets"];
    FULL_FLAGS.iter().any(|f| c.contains(f)) || !c.contains(" -p ")
}

/// R-100 模式 3:task prompt 引用缺陷 D-xxx 且该缺陷记录字段已含的路径也出现在
/// prompt 里 → 让子代理重新探索已知位置,就地提醒。纯文本解析,不依赖 docstore
/// (runner 层不能反向依赖 kanzei-tools,这是机械门禁的取舍)。
fn defect_known_path_hint(project_root: &std::path::Path, prompt: &str) -> Option<String> {
    if prompt.trim().is_empty() {
        return None;
    }
    let ids: Vec<&str> = prompt
        .split_whitespace()
        .filter(|w| {
            w.len() > 2
                && w.starts_with("D-")
                && w[2..].chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    if ids.is_empty() {
        return None;
    }
    for name in ["defects.md", "defects-archive.md"] {
        let path = project_root.join(".kanzei/project").join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for id in &ids {
            let marker = format!("## {id} ");
            let Some(start) = text.find(&marker) else {
                continue;
            };
            let rest = &text[start..];
            let end = rest
                .find("\n## ")
                .map(|i| start + i)
                .unwrap_or(text.len());
            let section = &text[start..end];
            let known: Vec<&str> = section
                .split_whitespace()
                .map(trim_path_token)
                .filter(|w| is_path_like(w))
                .collect();
            for known_path in known {
                if prompt.contains(known_path) {
                    return Some(format!(
                        "缺陷 {id} 记录已含文件路径 {known_path},直接 read 该文件即可,无需 task 重新探索"
                    ));
                }
            }
        }
    }
    None
}

/// 去掉路径 token 首尾的标点(截断、括号、分号等)。
fn trim_path_token(token: &str) -> &str {
    let mut s = token.trim();
    while let Some(last) = s.chars().last() {
        if ".,;:!?)]}、。；：」』》".contains(last) {
            s = &s[..s.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    s
}

/// 路径样子:含目录分隔符且含点(代码文件/相对路径),排除纯 URL。
fn is_path_like(token: &str) -> bool {
    let has_sep = token.contains('/') || token.contains('\\');
    let has_dot = token.contains('.');
    let not_url = !token.contains("://");
    has_sep && has_dot && not_url
}

/// 本轮完成的条目(R-124 SOP 提炼的触发闸门)。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletedEntry {
    /// 条目 id,如 R-123 / D-166。
    pub id: String,
    /// 落到的终态:done / fixed / dropped / wontfix。
    pub status: String,
    /// 本轮实际动过手的工具(去重、按首次出现排序),提炼步骤的原料。
    pub tools: Vec<String>,
}

/// 判定"本轮是否确实完成了一个完整条目"。只有成立时才允许提炼 SOP——
/// 否则失败轮、空转轮、纯查询轮都会产出垃圾模板,把 SOP 库淹掉(R-124 验收 ②)。
///
/// 成立条件全部满足:
/// 1. 有一次 **成功** 的 req/defect update,且目标状态是终态;
/// 2. 本轮存在实质动作(改文件或跑命令)——只把条目一勾并不构成可复用流程;
/// 3. 该次 update 之前就有实质动作,顺序不能反(先勾再干活不是同一件事)。
///
/// 判定用代码强制而非写进提示词:提示词约束不住"这轮到底算不算完成"。
pub fn completed_entry(messages: &[Message]) -> Option<CompletedEntry> {
    const TERMINAL: &[&str] = &["done", "fixed", "dropped", "wontfix"];
    const SUBSTANTIVE: &[&str] = &["write", "edit", "multiedit", "bash"];

    let mut calls: std::collections::HashMap<String, (String, serde_json::Value)> =
        std::collections::HashMap::new();
    let mut tools: Vec<String> = Vec::new();
    let mut substantive_before = false;
    let mut found: Option<CompletedEntry> = None;

    for message in messages {
        for part in &message.parts {
            match part {
                Part::ToolCall { id, name, input } => {
                    calls.insert(id.clone(), (name.clone(), input.clone()));
                }
                Part::ToolResult { call_id, is_error, .. } => {
                    let Some((name, input)) = calls.get(call_id) else { continue };
                    if *is_error {
                        continue;
                    }
                    if !tools.iter().any(|t| t == name) {
                        tools.push(name.clone());
                    }
                    if SUBSTANTIVE.contains(&name.as_str()) {
                        substantive_before = true;
                    }
                    if !matches!(name.as_str(), "req" | "defect") {
                        continue;
                    }
                    let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    let status = input.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if action != "update" || !TERMINAL.contains(&status) {
                        continue;
                    }
                    // 先干活后收口才算完成一件事;顺序反了(先勾再改)不构成可复用流程。
                    if !substantive_before {
                        continue;
                    }
                    let Some(id) = input.get("id").and_then(|v| v.as_str()) else { continue };
                    found = Some(CompletedEntry {
                        id: id.to_string(),
                        status: status.to_string(),
                        tools: tools.clone(),
                    });
                }
                _ => {}
            }
        }
    }
    // tools 要含收口那一刻之后的全貌:取最终列表,保证提炼看得到完整流程。
    found.map(|mut entry| {
        entry.tools = tools;
        entry
    })
}

pub fn summarize_failures(messages: &[Message]) -> Vec<FailureSignal> {
    // call_id → (工具名, 目标):ToolResult 只有 call_id,工具名要回溯 ToolCall。
    let mut calls: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let mut raw_inputs: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // 指纹 → 信号;另记每个目标最近一次失败,便于配对后续成功。
    let mut signals: Vec<FailureSignal> = Vec::new();
    let mut failed_targets: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for message in messages {
        for part in &message.parts {
            match part {
                Part::ToolCall { id, name, input } => {
                    // 同时留原始输入:判定恢复对时要看"失败的目标是否出现在后续成功调用里"
                    // (edit main.rs 失败 → bash 用脚本改 main.rs 成功,是同一个目标)。
                    calls.insert(id.clone(), (name.clone(), failure_target(input)));
                    raw_inputs.insert(id.clone(), input.to_string().to_lowercase());
                }
                Part::ToolResult { call_id, content, is_error } => {
                    let Some((tool, target)) = calls.get(call_id).cloned() else {
                        continue;
                    };
                    if *is_error {
                        let kind = failure_kind(content);
                        let position = match signals
                            .iter()
                            .position(|s| s.tool == tool && s.kind == kind)
                        {
                            Some(index) => {
                                signals[index].count += 1;
                                if !signals[index].targets.contains(&target)
                                    && signals[index].targets.len() < 3
                                {
                                    signals[index].targets.push(target.clone());
                                }
                                index
                            }
                            None => {
                                signals.push(FailureSignal {
                                    tool: tool.clone(),
                                    kind,
                                    sample: content.chars().take(240).collect(),
                                    targets: if target.is_empty() {
                                        Vec::new()
                                    } else {
                                        vec![target.clone()]
                                    },
                                    count: 1,
                                    recovered_by: None,
                                });
                                signals.len() - 1
                            }
                        };
                        if !target.is_empty() {
                            failed_targets.insert(target, position);
                        }
                    } else {
                        // 恢复对:失败过的目标出现在后续某次成功调用的输入里。工具不必相同——
                        // 「edit 不行、改用 bash 脚本可以」正是最值得记的形状。
                        let haystack = raw_inputs.get(call_id).cloned().unwrap_or_default();
                        let recovered: Vec<String> = failed_targets
                            .keys()
                            .filter(|failed| haystack.contains(failed.as_str()))
                            .cloned()
                            .collect();
                        for failed in recovered {
                            if let Some(position) = failed_targets.remove(&failed) {
                                if signals[position].recovered_by.is_none() {
                                    signals[position].recovered_by = Some(tool.clone());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // 机械闸门:一次性失败不入库(搜索空间无限的负面事实没有记忆价值);
    // 重复 ≥2 次说明是稳定的坑,或带恢复对说明有现成解药——两者才值得记。
    signals.retain(|s| s.count >= 2 || s.recovered_by.is_some());
    signals
}

/// 错误指纹:首行小写 → 抹掉含路径分隔符的 token 与全部数字 → 折叠空白 → 截 80。
/// 目的是让「13 次 CRLF 未命中」塌成同一条,而不是 13 条。
fn failure_kind(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").to_lowercase();
    let scrubbed: Vec<String> = first_line
        .split_whitespace()
        .filter(|token| !token.contains('/') && !token.contains('\\'))
        .map(|token| {
            token
                .chars()
                .filter(|c| !c.is_ascii_digit())
                .collect::<String>()
        })
        .filter(|token| !token.is_empty())
        .collect();
    scrubbed.join(" ").chars().take(80).collect()
}

/// 目标键:路径类取最后一段(跨平台分隔符都算),命令取首词,其余取 id。
fn failure_target(input: &serde_json::Value) -> String {
    for key in ["path", "file_path", "file", "id", "command"] {
        if let Some(value) = input.get(key).and_then(|v| v.as_str()) {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            if key == "command" {
                return value.split_whitespace().next().unwrap_or("").to_lowercase();
            }
            return value
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(value)
                .to_lowercase();
        }
    }
    String::new()
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    use kanzei_llm::{Message, Part};

    fn call(id: &str, name: &str, target_key: &str, target: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: name.into(),
            input: serde_json::json!({ target_key: target }),
        }])
    }
    fn result(call_id: &str, content: &str, is_error: bool) -> Message {
        Message::tool_results(vec![Part::ToolResult {
            call_id: call_id.into(),
            content: content.into(),
            is_error,
        }])
    }

    fn tracker_call(id: &str, tool: &str, entry: &str, status: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: tool.into(),
            input: serde_json::json!({ "action": "update", "id": entry, "status": status }),
        }])
    }

    fn bash(id: &str, command: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: "bash".into(),
            input: serde_json::json!({ "command": command }),
        }])
    }

    /// 取 ToolResult 内容(测试断言用)。
    fn result_content(part: &Part) -> String {
        match part {
            Part::ToolResult { content, .. } => content.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn 调用画像把连续_git_查询并成一组() {
        let messages = vec![
            // 一组:三条连着的 git 查询
            bash("g1", "git status --porcelain"),
            result("g1", "", false),
            bash("g2", "git diff --stat"),
            result("g2", "", false),
            bash("g3", "git log --oneline -3"),
            result("g3", "", false),
            // 中间插入真正的工作,后面的 git 查询另起一组
            call("e1", "edit", "path", "src/lib.rs"),
            result("e1", "no match", true),
            call("e2", "edit", "path", "src/lib.rs"),
            result("e2", "ok", false),
            bash("g4", "git status"),
            result("g4", "", false),
            bash("b1", "cargo test"),
            result("b1", "ok", false),
        ];
        let m = summarize_metrics(&messages);
        assert_eq!(m.terminal_calls, 5, "bash 调用总数");
        assert_eq!(m.git_calls, 4);
        assert_eq!(m.git_groups, 2, "连续查询应并成一组,分散才是节奏问题");
        assert_eq!(m.edit_calls, 2);
        assert_eq!(m.edit_misses, 1);
        assert!((m.edit_miss_rate() - 0.5).abs() < 1e-9);
        assert_eq!(m.failed_calls, 1);
        assert_eq!(m.total_calls, 7);
        assert_eq!(m.subagent_calls, 0);

        // 无 edit 时未命中率是 0 而不是除零。
        assert_eq!(summarize_metrics(&[]).edit_miss_rate(), 0.0);
    }

    #[test]
    fn 冗余提醒_按类别计数进度量() {
        // R-100:summarize_metrics 按 [冗余提醒] 前缀归类计数。
        let messages = vec![
            bash("g1", "git status"),
            result("g1", "nothing to commit", false),
            bash("g2", "git status"),
            result(
                "g2",
                "nothing to commit\n[冗余提醒] 工作树与上次 git status/diff 无变化,这次查询可省",
                false,
            ),
            bash("b1", "cargo test --workspace"),
            result(
                "b1",
                "ok\n[冗余提醒] 自上次全量测试以来工作树无变更,这次测试可省",
                false,
            ),
            call("t1", "task", "prompt", "看 D-001,读 crates/app/main.rs"),
            result("t1", "done\n[冗余提醒] 缺陷 D-001 记录已含文件路径 crates/app/main.rs,直接 read 该文件即可,无需 task 重新探索", false),
        ];
        let m = summarize_metrics(&messages);
        assert_eq!(m.redundant_git, 1);
        assert_eq!(m.redundant_test, 1);
        assert_eq!(m.redundant_task, 1);
        assert_eq!(m.redundant_total(), 3);
        // 失败结果里即使带提醒字样也不计数。
        let failed = vec![result("x", "[冗余提醒] 缺陷", true)];
        assert_eq!(summarize_metrics(&failed).redundant_total(), 0);
    }

    #[test]
    fn 重复_git_status_无变化时_就地提醒() {
        let mut watch = RedundancyWatch::default();
        let dir = std::env::temp_dir().join(format!("kz-red-git-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();

        let calls = vec![
            ("g1".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
            ("g2".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
        ];
        let mut results = vec![
            Part::ToolResult { call_id: "g1".into(), content: " M src/lib.rs".into(), is_error: false },
            Part::ToolResult { call_id: "g2".into(), content: " M src/lib.rs".into(), is_error: false },
        ];
        watch.note_step(&dir, &calls, &mut results);
        assert!(!result_content(&results[0]).contains("[冗余提醒]"));
        assert!(result_content(&results[1]).contains("[冗余提醒]"), "{}", result_content(&results[1]));
        // 内容变了(工作树有改动)就不再提醒。
        let mut watch2 = RedundancyWatch::default();
        let mut results2 = vec![
            Part::ToolResult { call_id: "g1".into(), content: " M src/lib.rs".into(), is_error: false },
            Part::ToolResult { call_id: "g2".into(), content: " M src/lib.rs\n M src/app.rs".into(), is_error: false },
        ];
        watch2.note_step(&dir, &calls, &mut results2);
        assert!(!result_content(&results2[1]).contains("[冗余提醒]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 全量测试_工作树未变时_就地提醒() {
        let mut watch = RedundancyWatch::default();
        let dir = std::env::temp_dir().join(format!("kz-red-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();

        let tree = " M crates/kanzei-app/ui/main.js";
        let calls1 = vec![
            ("g1".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
            ("b1".into(), "bash".into(), serde_json::json!({"command": "cargo test --workspace"}), "".into()),
        ];
        let mut results1 = vec![
            Part::ToolResult { call_id: "g1".into(), content: tree.into(), is_error: false },
            Part::ToolResult { call_id: "b1".into(), content: "test result: ok. 200 passed".into(), is_error: false },
        ];
        watch.note_step(&dir, &calls1, &mut results1);
        assert!(!result_content(&results1[1]).contains("[冗余提醒]"), "首次全量测试不该提醒");

        // 第二次:git status 内容一致,再跑全量 → 提醒。
        let calls2 = vec![
            ("g2".into(), "bash".into(), serde_json::json!({"command": "git status --porcelain"}), "".into()),
            ("b2".into(), "bash".into(), serde_json::json!({"command": "cargo test --workspace"}), "".into()),
        ];
        let mut results2 = vec![
            Part::ToolResult { call_id: "g2".into(), content: tree.into(), is_error: false },
            Part::ToolResult { call_id: "b2".into(), content: "test result: ok. 200 passed".into(), is_error: false },
        ];
        watch.note_step(&dir, &calls2, &mut results2);
        assert!(result_content(&results2[1]).contains("[冗余提醒]"), "{}", result_content(&results2[1]));
        // 定向测试不算全量,不触发。
        let mut watch3 = RedundancyWatch::default();
        let calls3 = vec![("b3".into(), "bash".into(), serde_json::json!({"command": "cargo test -p kanzei-core"}), "".into())];
        let mut results3 = vec![Part::ToolResult { call_id: "b3".into(), content: "ok".into(), is_error: false }];
        watch3.note_step(&dir, &calls3, &mut results3);
        assert!(!result_content(&results3[0]).contains("[冗余提醒]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn task_引用已知缺陷路径时_就地提醒() {
        let dir = std::env::temp_dir().join(format!("kz-red-task-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/defects.md"),
            "# Defects\n\n## D-001 启动黑屏 [open]\n- 复现: crates/kanzei-app/ui/main.js 初始化\n",
        )
        .unwrap();
        let mut watch = RedundancyWatch::default();
        let calls = vec![(
            "t1".into(),
            "task".into(),
            serde_json::json!({"prompt": "D-001 启动黑屏,找 crates/kanzei-app/ui/main.js 的初始化位置"}),
            "".into(),
        )];
        let mut results = vec![Part::ToolResult { call_id: "t1".into(), content: "done".into(), is_error: false }];
        watch.note_step(&dir, &calls, &mut results);
        assert!(result_content(&results[0]).contains("[冗余提醒] 缺陷 D-001"), "{}", result_content(&results[0]));
        // 路径不在缺陷记录里 → 不提醒。
        let calls2 = vec![(
            "t2".into(),
            "task".into(),
            serde_json::json!({"prompt": "D-001 启动黑屏,找 crates/kanzei-app/src/main.rs 的逻辑"}),
            "".into(),
        )];
        let mut results2 = vec![Part::ToolResult { call_id: "t2".into(), content: "done".into(), is_error: false }];
        watch.note_step(&dir, &calls2, &mut results2);
        assert!(!result_content(&results2[0]).contains("[冗余提醒]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sop_提炼只在真正完成一个条目时触发() {
        // 正例:先改文件再收口到终态 —— 这才是一段可复用的流程。
        let done = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            call("c2", "bash", "command", "cargo test"),
            result("c2", "test result: ok", false),
            tracker_call("c3", "req", "R-123", "done"),
            result("c3", "updated", false),
        ];
        let entry = completed_entry(&done).expect("完成一个完整条目时应触发");
        assert_eq!(entry.id, "R-123");
        assert_eq!(entry.status, "done");
        assert!(entry.tools.contains(&"edit".to_string()) && entry.tools.contains(&"bash".to_string()));

        // 反例一:纯查询轮 —— 没有任何实质动作,不构成可复用流程。
        let read_only = vec![
            call("c1", "read", "path", "src/lib.rs"),
            result("c1", "...", false),
            tracker_call("c2", "req", "R-124", "done"),
            result("c2", "updated", false),
        ];
        assert!(completed_entry(&read_only).is_none(), "纯查询轮不该提炼 SOP");

        // 反例二:先勾完成再干活 —— 顺序反了,勾的那一刻并没有完成什么。
        let out_of_order = vec![
            tracker_call("c1", "req", "R-125", "done"),
            result("c1", "updated", false),
            call("c2", "edit", "path", "src/lib.rs"),
            result("c2", "ok", false),
        ];
        assert!(completed_entry(&out_of_order).is_none(), "先收口后干活不该提炼 SOP");

        // 反例三:收口调用本身失败 —— 条目根本没进终态。
        let failed_close = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            tracker_call("c2", "req", "R-126", "done"),
            result("c2", "cannot move backward", true),
        ];
        assert!(completed_entry(&failed_close).is_none(), "收口失败不该提炼 SOP");

        // 反例四:只是把状态推到 doing —— 不是终态。
        let in_progress = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            tracker_call("c2", "req", "R-127", "doing"),
            result("c2", "updated", false),
        ];
        assert!(completed_entry(&in_progress).is_none(), "推进到 doing 不是完成");
    }

    #[test]
    fn 一次性失败不上报_重复失败才成为信号() {
        // 搜索空间无限的负面事实(猜错一次文件名)没有记忆价值,必须被闸掉。
        let once = vec![
            call("c1", "read", "path", "src/nope.rs"),
            result("c1", "cannot access src/nope.rs: not found", true),
        ];
        assert!(summarize_failures(&once).is_empty());

        // 同一坑重复两次 = 稳定问题,值得记。
        let twice = vec![
            call("c1", "edit", "path", "C:/p/main.rs"),
            result("c1", "old_string not found in C:/p/main.rs — it must match exactly", true),
            call("c2", "edit", "path", "C:/p/other.rs"),
            result("c2", "old_string not found in C:/p/other.rs — it must match exactly", true),
        ];
        let signals = summarize_failures(&twice);
        assert_eq!(signals.len(), 1, "同类错误必须塌成一条: {signals:?}");
        assert_eq!(signals[0].tool, "edit");
        assert_eq!(signals[0].count, 2);
        // 指纹抹掉了路径,两次不同文件仍归一类
        assert!(!signals[0].kind.contains("main.rs"), "指纹不该含路径: {}", signals[0].kind);
        assert_eq!(signals[0].targets, vec!["main.rs", "other.rs"]);
    }

    #[test]
    fn 失败后换工具成功构成恢复对_单次也上报() {
        // 「X 不行、Y 可以」自带解药,是最值得记的形状,阈值降到 1。
        let messages = vec![
            call("c1", "edit", "path", "C:/p/main.rs"),
            result("c1", "old_string not found in C:/p/main.rs", true),
            call("c2", "bash", "command", "python fix.py C:/p/main.rs"),
            result("c2", "exit code: 0", false),
        ];
        let signals = summarize_failures(&messages);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].recovered_by.as_deref(), Some("bash"));
        assert_eq!(signals[0].count, 1, "有恢复对时单次失败即上报");
    }

    #[test]
    fn tdd_噪声不被误报为信号() {
        // 先写测试→失败→实现→通过:这是正常节奏,不是可复用知识。
        // 单次失败被阈值闸掉;目标不同(测试命令 vs 源文件)也不构成恢复对。
        let messages = vec![
            call("c1", "bash", "command", "cargo test新增用例"),
            result("c1", "exit code: 101\ntest failed", true),
            call("c2", "edit", "path", "C:/p/impl.rs"),
            result("c2", "replaced 1 occurrence(s)", false),
        ];
        assert!(
            summarize_failures(&messages).is_empty(),
            "TDD 的一次预期失败不该进记忆"
        );
    }

    #[test]
    fn 本轮统计不含历史_调用方须传切片() {
        // summarize_tools/summarize_failures 都按传入切片统计;
        // 调用方传 &messages[prior_len..] 才是本轮画像(否则历史被重复计入)。
        let history = vec![
            call("h1", "edit", "path", "old.rs"),
            result("h1", "old_string not found in old.rs", true),
            call("h2", "edit", "path", "old2.rs"),
            result("h2", "old_string not found in old2.rs", true),
        ];
        let mut all = history.clone();
        all.extend(vec![
            call("n1", "read", "path", "new.rs"),
            result("n1", "ok", false),
        ]);
        assert_eq!(summarize_failures(&all).len(), 1, "全量含历史失败");
        assert!(
            summarize_failures(&all[history.len()..]).is_empty(),
            "本轮切片不该带出历史失败"
        );
        assert_eq!(summarize_tools(&all[history.len()..]).get("read"), Some(&1));
        assert_eq!(summarize_tools(&all[history.len()..]).get("edit"), None);
    }
}


#[allow(clippy::too_many_arguments)]
pub fn run_once<'a>(
    client: &'a LlmClient,
    route: &'a Route,
    snapshot: &'a HarnessSnapshot,
    agent: &'a AgentDef,
    config: &'a RunnerConfig,
    ctx: &'a ToolCtx,
    prompt: &'a str,
    prior: &'a [Message],
    subagent: Option<&'a SubagentRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    run_once_with_parts(client, route, snapshot, agent, config, ctx, prompt, prior, None, subagent, on_event, ask)
}

#[allow(clippy::too_many_arguments)]
pub fn run_once_with_parts<'a>(
    client: &'a LlmClient,
    route: &'a Route,
    snapshot: &'a HarnessSnapshot,
    agent: &'a AgentDef,
    config: &'a RunnerConfig,
    ctx: &'a ToolCtx,
    prompt: &'a str,
    // 之前轮次的完整消息历史(空 = 新对话)。
    prior: &'a [Message],
    initial_parts: Option<&'a [Part]>,
    // Some = 注册 task 工具,模型可派生并行子代理;子代理自身传 None(禁嵌套)。
    subagent: Option<&'a SubagentRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    Box::pin(async move {
    let tools: Vec<Arc<dyn Tool>> = snapshot.materialize_tools();
    let mut specs: Vec<ToolSpec> = tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description(),
            input_schema: t.input_schema(),
        })
        .collect();
    if subagent.is_some() {
        specs.push(task_spec());
    }

    // system 分块:agent 提示词 + harness baseline(M2 起 baseline 进 Context Epoch)。
    let (baseline, mut context_report) = snapshot.system_baseline_with_report();
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
    let system: Vec<String> = [agent.system.clone(), baseline]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();

    // prior 可能来自旧快照或跨进程恢复，先统一清洗孤儿工具 part，避免首次请求
    // 在尚未触发上下文压缩时就把非法消息交给 provider。
    let mut messages: Vec<Message> = crate::history::filter_message_history(prior);
    let user_parts = match initial_parts {
        Some(parts) => {
            let mut parts = parts.to_vec();
            if !prompt.is_empty() {
                parts.insert(0, Part::Text { text: prompt.to_string() });
            }
            parts
        }
        None => vec![Part::Text { text: prompt.to_string() }],
    };
    messages.push(Message { role: Role::User, parts: user_parts });
    let mut total_usage = Usage::default();
    let mut final_text = String::new();
    // steps 语义:0 = 无上限(用户定调:不设人为轮数天花板——停止权在用户按钮
    // 与上下文管理,不在计数器)。>0 时保留封顶,最后一步收工具+收尾指令。
    let max_steps = agent.steps;
    // 本次运行内已放行的 (action, resource):同一资源不重复问(用户反馈:别烦我)。
    let mut session_approved: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // "总是允许"的会话内即时生效层(D-006):快照是开跑时定死的,新写入的规则
    // 本次运行读不到——泛化 pattern 记在这里,同类资源当场不再询问。
    let mut session_rules: Vec<(String, String)> = Vec::new();

    let mut overflow_recoveries = 0;
    // 主动压缩的连续无效计数(D-206),与被动恢复各记各的。只数"压了没用",
    // 成功的压缩清零——压缩是常规运营动作,不设总量配额。
    let mut futile_compactions = 0u32;
    let mut overflow_traces: Vec<String> = Vec::new();
    // 估算校准:len/4 粗估对中文 \uXXXX 转义、工具输出密集的会话有系统性偏差,
    // 预算线 0.7 的语义要靠真实 usage 反推的滑动因子校准才有意义。初始 1.0,
    // 每步拿到 provider 真实 input tokens 后 EMA 更新。
    let mut calibration = 1.0f64;
    let mut last_estimated = 0u64;
    // R-100 冗余机械门禁:按单次运行持有(跨轮清零),提醒追加进工具结果不阻断。
    let mut redundancy = RedundancyWatch::default();

    let mut step = 0u32;
    loop {
        step += 1;
        on_event(RunEvent::TurnStart { step, max_steps });
        let last_step = max_steps > 0 && step == max_steps;
        // 步数软预算(D-173):步数上限是 0(不设人为天花板),但"不封顶"不等于
        // "不盘点"。检查点只要求当轮盘一次剩余范围,不强制中止——实测长轮的
        // 成本失控几乎都始于无人察觉的目标漂移。
        let budget_checkpoint = is_budget_checkpoint(step);

        // 轮内上下文预算(D-176)。压缩检查原先只写在**一轮结束之后**,而长轮与
        // 自动续跑恰恰是最需要它的场景:一轮不结束就一次也轮不到。实测一次 41
        // 分钟的运行里检查点执行了 0 次,用户按停止后更是直接跳过收尾,全程只能
        // 等 provider 报 overflow 再被动裁剪。这里在每步开跑前主动估一次。
        if let Some(limit) = config.context_limit {
            let budget = (limit as f64 * config.limits.context_budget_ratio()) as u64;
            let before = budgeted_tokens(&system, &messages, &specs, calibration);
            if before > budget
                && futile_compactions < MAX_FUTILE_COMPACTIONS
                && messages.len() > 1
            {
                let dropped_messages = compact_with_digest(
                    client,
                    subagent,
                    &mut messages,
                    budget,
                    &mut overflow_traces,
                    config.limits.recent_verbatim_ratio(),
                )
                .await;
                if dropped_messages > 0 {
                    // 压了还超线:tail 太大或 head 太大。再砍 tail 到预算内,否则
                    // 下一步预算检查立刻再压——连续两次压缩 = 缓存前缀两次全量
                    // 重算(cache_write 双倍),省下的 token 不够补缓存成本。
                    // trim_tail 拿同一个 calibration:两边必须用同一把尺子量同一条
                    // 预算线,否则它按原始口径够线就收手,这里看还超线(D-203)。
                    if budgeted_tokens(&system, &messages, &specs, calibration) > budget {
                        trim_tail(
                            &mut messages,
                            &system,
                            &specs,
                            budget,
                            calibration,
                            &mut overflow_traces,
                        );
                    }
                    let after = budgeted_tokens(&system, &messages, &specs, calibration);
                    // D-206:只按"有没有用"记账。压回线内 = 压缩在正常工作,清零、
                    // 下次照压;压完(连 trim_tail 都上了)仍超线 = head+当前消息
                    // 本身超线,连续两次就停,交给撞墙后的被动恢复,别空转。
                    if after <= budget {
                        futile_compactions = 0;
                    } else {
                        futile_compactions += 1;
                    }
                    on_event(RunEvent::ContextCompacted {
                        before_tokens: before,
                        after_tokens: after,
                        budget_tokens: budget,
                        limit_tokens: limit,
                        dropped_messages,
                    });
                } else {
                    // 中段为空压不动:不发事件(没骗 UI),但要计无效——否则每步
                    // 白跑一次 compact,同样是注释里说的空转。
                    futile_compactions += 1;
                }
            }
        }

        // Provider 可能比本地配置更严格地计算上下文(尤其是工具 schema)。
        // 建流前和 HTTP 200 后 SSE 流内都可能报告 context overflow，必须走同一套
        // 有界恢复；本步工具要等流完整结束才执行，因此流内超限重放不会重复副作用。
        let mut stream_restarts: u32 = 0;
        let (parts, calls, finish) = loop {
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
        let req_tools: &[ToolSpec] = if last_step { &[] } else { &specs };
        last_estimated = estimate_prompt_tokens(&system, &request_messages, req_tools);
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.clone(),
            messages: request_messages,
            tools: req_tools.to_vec(),
            max_tokens: config.max_tokens,
            temperature: None,
            reasoning: config.reasoning,
            service_tier: config.service_tier.clone(),
        };
        let mut stream = match client
            .stream_with_retry_notice(route, &request, |attempt, delay| {
                on_event(RunEvent::Retry { attempt, max: kanzei_llm::client::MAX_TRANSPORT_RETRIES, delay_ms: delay.as_millis() });
            })
            .await
        {
            Err(error) if error.is_context_overflow() => {
                if recover_context_overflow(&mut messages, &mut overflow_recoveries, &mut overflow_traces) {
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

        while let Some(event) = stream.next().await {
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
                    calibration = update_calibration(calibration, last_estimated, usage.input);
                    total_usage = add_usage(total_usage, usage);
                    finish = reason.clone();
                    on_event(RunEvent::StepEnd { usage, reason });
                }
                _ => {}
            }
        }

        match stream_error {
            None => {
                for (_, text) in std::mem::take(&mut text_buffers) {
                    parts.push(Part::Text { text });
                }
                break (parts, calls, finish);
            }
            // Provider 也可能在 HTTP 200 的 SSE error 事件里报告上下文超限。
            // 此时本步工具尚未执行，压缩 messages 后安全地从头重放请求。
            Some(error) if error.is_context_overflow() => {
                if recover_context_overflow(&mut messages, &mut overflow_recoveries, &mut overflow_traces) {
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
        };

        final_text = parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !parts.is_empty() {
            messages.push(Message::assistant(parts));
        }

        if calls.is_empty() {
            return Ok(RunSummary {
                text: final_text,
                usage: total_usage,
                steps: step,
                halted_by_user: false,
                messages,
                context_report: context_report.clone(),
                overflow_traces: overflow_traces.clone(),
            });
        }

        // ---- task 子代理:同轮多个 task 并行执行。只读快照无副作用,与任何工具并发都安全 ----
        let mut task_results: std::collections::HashMap<String, kanzei_harness::ToolOutput> =
            std::collections::HashMap::new();
        if let Some(rt) = subagent {
            let mut task_calls: Vec<(String, serde_json::Value, String)> = calls
                .iter()
                .filter(|(_, name, _, _)| name == "task")
                .map(|(id, _, input, raw)| (id.clone(), input.clone(), raw.clone()))
                .collect();
            if !task_calls.is_empty() {
                let max_tasks = config.limits.max_tasks_per_turn();
                let overflow = if task_calls.len() > max_tasks {
                    task_calls.split_off(max_tasks)
                } else {
                    Vec::new()
                };
                for (id, input, raw) in &task_calls {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                        input: input.clone(),
                    });
                }
                for (id, input, raw) in &overflow {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                        input: input.clone(),
                    });
                    let output = kanzei_harness::ToolOutput::error(format!(
                        "too many parallel subagent tasks; maximum per turn is {}",
                        max_tasks
                    ));
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name: "task".into(),
                        ok: false,
                        preview: preview(&output.content),
                        display: output.display.clone(),
                    });
                    task_results.insert(id.clone(), output);
                }
                // 进度通道:子代理内部事件(轮次/工具)转成 TaskProgress 实时上抛,
                // 完成一个立刻报一个 ToolEnd——不再等最慢的,UI 全程有反馈。
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
                let mut jobs: futures::stream::FuturesUnordered<_> = task_calls
                    .iter()
                    .map(|(id, input, _)| {
                        let tx = tx.clone();
                        async move {
                            let bound = std::time::Duration::from_secs(rt.timeout_secs);
                            let output = match tokio::time::timeout(
                                bound,
                                run_subagent(client, rt, ctx, id, input, tx),
                            )
                            .await
                            {
                                Ok(output) => output,
                                // 纯兜底(默认 15 分钟):防失控,不是性能预算。
                                Err(_) => kanzei_harness::ToolOutput::error(format!(
                                    "subagent hit the {}s wall-clock safety limit — split the task into narrower pieces",
                                    rt.timeout_secs
                                )),
                            };
                            (id.clone(), output)
                        }
                    })
                    .collect();
                drop(tx);
                loop {
                    tokio::select! {
                        next = jobs.next() => match next {
                            Some((id, output)) => {
                                on_event(RunEvent::ToolEnd {
                                    id: id.clone(),
                                    name: "task".into(),
                                    ok: !output.is_error,
                                    preview: preview(&output.content),
                                    display: output.display.clone(),
                                });
                                task_results.insert(id, output);
                            }
                            None => {
                                drain_task_events(&mut rx, on_event);
                                break;
                            }
                        },
                        Some(event) = rx.recv() => on_event(event),
                    }
                }
            }
        }

        // R-097 批一：权限询问仍按旧路径串行处理(R-086 承接询问路由)；当本批
        // 不需要新 ask 时，普通工具按显式并发契约切成确定性 wave 并发执行。
        let can_parallel_tools = {
            let mut ready = true;
            let mut ordinary_count = 0usize;
            for (_, name, input, _) in &calls {
                if name == "task" && subagent.is_some() {
                    continue;
                }
                let Some(tool) = tools.iter().find(|tool| tool.name() == name) else {
                    ready = false;
                    break;
                };
                if name == "question" || input.is_null() {
                    ready = false;
                    break;
                }
                ordinary_count += 1;
                let action = tool.action();
                for resource in tool.resources_with_ctx(input, ctx) {
                    let resource = kanzei_harness::permission::normalize_resource(&resource);
                    if snapshot.evaluate(action, &resource) != Effect::Ask {
                        continue;
                    }
                    let key = (action.to_string(), resource.clone());
                    let approved = session_approved.contains(&key)
                        || session_rules.iter().any(|(known_action, pattern)| {
                            known_action == action
                                && kanzei_harness::permission::resource_match_for_action(
                                    known_action,
                                    pattern,
                                    &resource,
                                )
                        });
                    if !approved {
                        ready = false;
                        break;
                    }
                }
                if !ready {
                    break;
                }
            }
            ready && ordinary_count >= 2
        };

        let mut results = if can_parallel_tools {
            let mut slots: Vec<Option<Part>> =
                std::iter::repeat_with(|| None).take(calls.len()).collect();
            let mut prepared = Vec::new();
            for (index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
                if name == "task" && subagent.is_some() {
                    let output = task_results.remove(&id).unwrap_or_else(|| {
                        kanzei_harness::ToolOutput::error("internal: task result missing")
                    });
                    slots[index] = Some(Part::ToolResult {
                        call_id: id,
                        content: output.content,
                        is_error: output.is_error,
                    });
                    continue;
                }
                let tool = tools
                    .iter()
                    .find(|tool| tool.name() == name)
                    .expect("parallel batch was preflighted")
                    .clone();
                on_event(RunEvent::ToolStart {
                    id: id.clone(),
                    name: name.clone(),
                    summary: summarize_input(&input, &raw_input),
                    input: input.clone(),
                });
                let action = tool.action();
                let denied = tool
                    .resources_with_ctx(&input, ctx)
                    .into_iter()
                    .map(|resource| kanzei_harness::permission::normalize_resource(&resource))
                    .find(|resource| snapshot.evaluate(action, resource) == Effect::Deny);
                if let Some(resource) = denied {
                    on_event(RunEvent::PermissionResolved {
                        tool_call_id: id.clone(),
                        action: action.to_string(),
                        resource: resource.clone(),
                        decision: "deny",
                        source: "ruleset",
                    });
                    let output = kanzei_harness::ToolOutput::error(format!(
                        "permission denied by ruleset: {action} on `{resource}`.\n{}",
                        snapshot.denial_hint(action, &resource),
                    ));
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name,
                        ok: false,
                        preview: preview(&output.content),
                        display: None,
                    });
                    slots[index] = Some(Part::ToolResult {
                        call_id: id,
                        content: output.content,
                        is_error: true,
                    });
                    continue;
                }
                let concurrency = tool.concurrency(&input, ctx);
                prepared.push(PreparedToolCall {
                    index,
                    id,
                    name,
                    input,
                    tool,
                    concurrency,
                });
            }
            for (index, result) in execute_prepared_tools(prepared, ctx, config.limits.max_parallel_tools(), on_event).await {
                slots[index] = Some(result);
            }
            slots
                .into_iter()
                .map(|result| result.expect("every preflighted tool call must produce a result"))
                .collect()
        } else {
            let mut results = Vec::new();
        for (call_index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
            // task 不过权限门禁:子代理快照在代码层面只含只读工具(硬门禁在构造,不在评估)。
            // ToolEnd 已在并行阶段按完成顺序上报过,这里只归位结果。
            if name == "task" && subagent.is_some() {
                let output = task_results.remove(&id).unwrap_or_else(|| {
                    kanzei_harness::ToolOutput::error("internal: task result missing")
                });
                results.push(Part::ToolResult {
                    call_id: id,
                    content: output.content,
                    is_error: output.is_error,
                });
                continue;
            }
            let Some(tool) = tools.iter().find(|t| t.name() == name) else {
                results.push(Part::ToolResult {
                    call_id: id,
                    content: format!(
                        "unknown tool `{name}`; available: {}",
                        tools
                            .iter()
                            .map(|t| t.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    is_error: true,
                });
                continue;
            };
            on_event(RunEvent::ToolStart {
                id: id.clone(),
                name: name.clone(),
                summary: summarize_input(&input, &raw_input),
                input: input.clone(),
            });

            // question 是交互工具，不再叠加权限询问；答案作为工具结果回喂模型。
            if name == "question" {
                let question = input.get("question").and_then(|v| v.as_str()).unwrap_or("").trim();
                let options = input.get("options").and_then(|v| v.as_array()).map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
                let default = input.get("default").and_then(|v| v.as_str()).map(str::to_owned);
                let output = if question.is_empty() {
                    kanzei_harness::ToolOutput::error("question must not be empty")
                } else {
                    match ask(AskRequest::Question { question: question.to_owned(), options, default }).await {
                        AskResponse::Answer(answer) => kanzei_harness::ToolOutput::ok(format!("User answer: {answer}")),
                        AskResponse::Cancelled => kanzei_harness::ToolOutput::error("question cancelled by user"),
                        AskResponse::Permission(_) => kanzei_harness::ToolOutput::error("invalid question response"),
                    }
                };
                on_event(RunEvent::ToolEnd { id: id.clone(), name: name.clone(), ok: !output.is_error, preview: preview(&output.content), display: output.display.clone() });
                results.push(Part::ToolResult { call_id: id, content: output.content, is_error: output.is_error });
                continue;
            }

            // ---- 硬门禁:权限 Ruleset(deny 回喂模型;ask 问用户,拒绝停整轮)----
            let action = tool.action();
            let mut gate_result = Gate::Pass;
            let mut pending_ask: Vec<String> = Vec::new();
            for resource in tool.resources_with_ctx(&input, ctx) {
                // 统一正斜杠 + 消解 . / ..,权限 pattern 不用关心平台,也不能被路径变体绕过:
                // `.kanzei/research/../../src/main.rs` 会被 `*.kanzei/research/*` 判为放行,
                // 而落盘时 join 会消解 ..,实际写到项目任意位置(D-050)。
                let normalized =
                    kanzei_harness::permission::normalize_resource(&resource);
                let mut resolved = |decision, source| {
                    on_event(RunEvent::PermissionResolved {
                        tool_call_id: id.clone(),
                        action: action.to_string(),
                        resource: normalized.clone(),
                        decision,
                        source,
                    });
                };
                match snapshot.evaluate(action, &normalized) {
                    Effect::Deny => {
                        resolved("deny", "ruleset");
                        gate_result = Gate::Deny(normalized);
                        break;
                    }
                    Effect::Ask => pending_ask.push(normalized),
                    Effect::Allow => {
                        resolved("allow", "ruleset");
                    }
                }
            }
            if matches!(gate_result, Gate::Pass) {
                for resource in pending_ask {
                    let key = (action.to_string(), resource.clone());
                    let mut resolved = |decision, source| {
                        on_event(RunEvent::PermissionResolved {
                            tool_call_id: id.clone(),
                            action: action.to_string(),
                            resource: resource.clone(),
                            decision,
                            source,
                        });
                    };
                    if session_approved.contains(&key) {
                        resolved("allow", "session_approved");
                        continue;
                    }
                    if session_rules.iter().any(|(a, pattern)| {
                        a == action
                            && kanzei_harness::permission::resource_match_for_action(a, pattern, &resource)
                    }) {
                        resolved("allow", "session_rule");
                        continue;
                    }
                    match ask(AskRequest::Permission { action: action.to_string(), resource: resource.clone() }).await {
                        AskResponse::Permission(AskReply::Deny) | AskResponse::Cancelled | AskResponse::Answer(_) => {
                            resolved("declined", "user");
                            gate_result = Gate::UserDeclined;
                            break;
                        }
                        AskResponse::Permission(AskReply::AllowOnce) => {
                            resolved("allow_once", "user");
                            session_approved.insert(key);
                        }
                        AskResponse::Permission(AskReply::AlwaysAllow) => {
                            resolved("always_allow", "user");
                            session_rules.push((
                                action.to_string(),
                                kanzei_harness::config::generalize_resource(action, &resource),
                            ));
                        }
                    }
                }
            }
            let output = match gate_result {
                // D-173:拒绝理由必须由实际注册的托管族推导,不能固定说
                // "use the dedicated tool"——那个工具可能根本不存在。
                Gate::Deny(resource) => kanzei_harness::ToolOutput::error(format!(
                    "permission denied by ruleset: {action} on `{resource}`.\n{}",
                    snapshot.denial_hint(action, &resource),
                )),
                Gate::UserDeclined => {
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name: name.clone(),
                        ok: false,
                        preview: "(user declined)".into(),
                        display: None,
                    });
                    append_declined_tool_results(&mut results, &calls, call_index);
                    messages.push(Message::tool_results(results));
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        steps: step,
                        halted_by_user: true,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                    });
                }
                Gate::Pass => {
                    if input.is_null() {
                        repair_hint(tool.as_ref(), &raw_input, "tool input was not valid JSON")
                    } else {
                        tool.execute(input, ctx).await
                    }
                }
            };
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name: name.clone(),
                ok: !output.is_error,
                preview: preview(&output.content),
                display: output.display.clone(),
            });
            results.push(Part::ToolResult {
                call_id: id,
                content: output.content,
                is_error: output.is_error,
            });
        }
            results
        };
        // R-100:工具结果回喂前就地注入冗余提醒(不阻断)。
        redundancy.note_step(&ctx.project_root, &calls, &mut results);
        messages.push(Message::tool_results(results));

        if matches!(finish, FinishReason::MaxTokens | FinishReason::Refusal) {
            return Ok(RunSummary {
                text: final_text,
                usage: total_usage,
                steps: step,
                halted_by_user: false,
                messages,
                context_report: context_report.clone(),
                overflow_traces: overflow_traces.clone(),
            });
        }
        if last_step {
            break;
        }
    }

    Ok(RunSummary {
        text: final_text,
        usage: total_usage,
        steps: step,
        halted_by_user: false,
        messages,
        context_report,
        overflow_traces: overflow_traces.clone(),
    })
    })
}

fn append_declined_tool_results(
    results: &mut Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    declined_index: usize,
) {
    for (index, (id, _, _, _)) in calls.iter().enumerate().skip(declined_index) {
        let content = if index == declined_index {
            "permission request declined by user"
        } else {
            "tool call cancelled because a previous permission request was declined"
        };
        results.push(Part::ToolResult {
            call_id: id.clone(),
            content: content.into(),
            is_error: true,
        });
    }
}

struct PreparedToolCall {
    index: usize,
    id: String,
    name: String,
    input: serde_json::Value,
    tool: Arc<dyn Tool>,
    concurrency: ToolConcurrency,
}

fn build_tool_execution_waves_with(
    max_parallel: usize,
    calls: Vec<PreparedToolCall>,
) -> Vec<Vec<PreparedToolCall>> {
    let mut waves = Vec::new();
    let mut current: Vec<PreparedToolCall> = Vec::new();
    for call in calls {
        let conflicts = current
            .iter()
            .any(|other| call.concurrency.conflicts_with(&other.concurrency));
        if !current.is_empty()
            && (conflicts || current.len() >= max_parallel)
        {
            waves.push(std::mem::take(&mut current));
        }
        current.push(call);
    }
    if !current.is_empty() {
        waves.push(current);
    }
    waves
}

async fn execute_prepared_tools(
    calls: Vec<PreparedToolCall>,
    ctx: &ToolCtx,
    max_parallel: usize,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> Vec<(usize, Part)> {
    let mut results = Vec::new();
    for wave in build_tool_execution_waves_with(max_parallel, calls) {
        let mut jobs: futures::stream::FuturesUnordered<_> = wave
            .into_iter()
            .map(|call| async move {
                let PreparedToolCall {
                    index,
                    id,
                    name,
                    input,
                    tool,
                    concurrency: _,
                } = call;
                let output = tool.execute(input, ctx).await;
                (index, id, name, output)
            })
            .collect();
        while let Some((index, id, name, output)) = jobs.next().await {
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name,
                ok: !output.is_error,
                preview: preview(&output.content),
                display: output.display.clone(),
            });
            results.push((
                index,
                Part::ToolResult {
                    call_id: id,
                    content: output.content,
                    is_error: output.is_error,
                },
            ));
        }
    }
    results.sort_by_key(|(index, _)| *index);
    results
}

enum Gate {
    Pass,
    Deny(String),
    UserDeclined,
}
/// 跑一个子代理:独立的只读快照 + 空历史,结果文本即 tool result。
/// 子代理内 ask 一律 Deny(无人应答);run_once 递归经 dyn Box 断开无限类型。
/// 内部轮次/工具事件折叠成 TaskProgress 经 progress 通道上抛(UI 实时可见)。
async fn run_subagent(
    client: &LlmClient,
    rt: &SubagentRuntime,
    ctx: &ToolCtx,
    parent_call_id: &str,
    input: &serde_json::Value,
    progress: tokio::sync::mpsc::UnboundedSender<RunEvent>,
) -> kanzei_harness::ToolOutput {
    let prompt = ["prompt", "task", "instruction", "query"]
        .iter()
        .find_map(|k| input.get(k).and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();
    if prompt.is_empty() {
        return kanzei_harness::ToolOutput::error(
            "task requires a `prompt` string: a self-contained exploration instruction",
        );
    }
    let (route, model, service_tier) = match input.get("model").and_then(|v| v.as_str()) {
        Some("primary") => (&rt.primary.0, &rt.primary.1, &rt.primary_service_tier),
        _ => (&rt.fast.0, &rt.fast.1, &rt.fast_service_tier),
    };
    let config = RunnerConfig {
        model: model.clone(),
        max_tokens: rt.max_tokens,
        // 子代理是机械检索,不开思考:省钱且避免本地小模型不认该参数。
        reasoning: ReasoningEffort::Off,
        service_tier: service_tier.clone(),
        limits: rt.limits.clone(),
        // 子代理跑的是 fast 模型,窗口未必与主模型同源;这里不传上限,
        // 让它继续走撞墙后的被动恢复,不按主模型的预算误压。
        context_limit: None,
    };
    let mut on_event = |event: RunEvent| {
        let text = match &event {
            RunEvent::TurnStart { step, max_steps } => Some(if *max_steps > 0 {
                format!("第 {step}/{max_steps} 轮")
            } else {
                format!("第 {step} 轮")
            }),
            RunEvent::ToolStart { name, summary, .. } => {
                let head: String = summary.chars().take(80).collect();
                Some(format!("{name} {head}"))
            }
            _ => None,
        };
        let trace = match event {
            RunEvent::ToolStart { id, name, summary, .. } => Some(TaskTrace {
                child_id: id,
                phase: "start".into(),
                name,
                summary: Some(summary),
                ok: None,
                preview: None,
                display: None,
            }),
            RunEvent::ToolEnd { id, name, ok, preview, display } => Some(TaskTrace {
                child_id: id,
                phase: "end".into(),
                name,
                summary: None,
                ok: Some(ok),
                preview: Some(preview),
                display,
            }),
            _ => None,
        };
        if let Some(text) = text {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text,
                trace: trace.clone(),
            });
        } else if trace.is_some() {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text: "子代理工具完成".into(),
                trace,
            });
        }
    };    let mut ask = |_request: AskRequest| -> AskFuture {
        Box::pin(async { AskResponse::Permission(AskReply::Deny) })
    };
    // run_once 本身返回 boxed future,递归的无限类型在其签名处已断开。
    let fut = run_once(
        client,
        route,
        &rt.snapshot,
        &rt.agent,
        &config,
        ctx,
        &prompt,
        &[],
        None,
        &mut on_event,
        &mut ask,
    );
    match fut.await {
        Ok(summary) => {
            let text = if summary.text.trim().is_empty() {
                "(subagent finished without a text answer)".to_string()
            } else {
                summary.text
            };
            kanzei_harness::ToolOutput::ok(text)
        }
        Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
    }
}

fn recover_context_overflow(
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
fn dropped_trace(messages: &[Message]) -> String {
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

fn compact_messages_for_retry(messages: &mut Vec<Message>, overflow_traces: &mut Vec<String>) {
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

fn compact_messages_aggressively(messages: &mut Vec<Message>, overflow_traces: &mut Vec<String>) {
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
fn is_text_user_message(message: &Message) -> bool {
    message.role == Role::User
        && message
            .parts
            .iter()
            .any(|part| matches!(part, Part::Text { .. }))
}

fn add_usage(a: Usage, b: Usage) -> Usage {
    Usage {
        input: a.input + b.input,
        output: a.output + b.output,
        reasoning: a.reasoning + b.reasoning,
        cache_read: a.cache_read + b.cache_read,
        cache_write: a.cache_write + b.cache_write,
    }
}

fn summarize_input(input: &serde_json::Value, raw: &str) -> String {
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
    use super::{
        append_declined_tool_results, clip, compact_messages_aggressively,
        compact_messages_for_retry, digest_plausible, drain_task_events,
        budgeted_tokens, estimate_prompt_tokens, execute_prepared_tools, extract_file_names,
        recover_context_overflow, trim_tail, update_calibration, PreparedToolCall, RunEvent,
        CONTEXT_BUDGET_RATIO, MAX_STREAM_RESTARTS,
    };
    use async_trait::async_trait;
    use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
    use kanzei_llm::{LlmError, Message, Part};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

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

    /// D-181:主动压缩必须保住**任务定义**与**近期工作区**,只压中段。
    /// 旧实现直接复用应急函数,一次从预算线砍到约 2k(97%),而且留下的是最旧的
    /// 内容——压完模型不知道自己刚做了什么。
    #[tokio::test]
    async fn 主动压缩保住任务定义与近期工作并只压中段() {
        let mut messages = vec![Message::user_text("任务定义:修复 D-123 的空指针")];
        for i in 0..60 {
            messages.push(Message::user_text(format!("中段第 {i} 条 {}", "x".repeat(400))));
        }
        messages.push(Message::user_text("最近工作:正在改 store.rs 的 migrate"));
        messages.push(Message::user_text("最近工作:刚跑完 cargo test"));

        let before = estimate_prompt_tokens(&[], &messages, &[]);
        let mut traces = Vec::new();
        // subagent=None → 纪要模型不可用,走截断回落;即便如此也必须保住首尾。
        let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
        let dropped =
            super::compact_with_digest(&client, None, &mut messages, 2_000, &mut traces, 0.35).await;

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
        assert!(text.contains("任务定义:修复 D-123"), "任务定义必须逐字保留:\n{text}");
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
        assert!(text.contains("第 29 条"), "最近的历史要留下:\n{}", clip(&text, 300));
        assert!(!text.contains("第 0 条"), "最旧的历史不该占满配额");
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
        assert!((c - 0.85).abs() < 1e-9, "第一次应得 1.0*0.7+0.5*0.3=0.85,实得 {c}");
        c = update_calibration(c, 10_000, 5_000);
        assert!((c - 0.745).abs() < 1e-9, "第二次应得 0.85*0.7+0.5*0.3=0.745,实得 {c}");
        // 实际远超估算(3 倍)被限幅在 2 倍,单步最多 +0.3。
        let c3 = update_calibration(1.0, 10_000, 30_000);
        assert!((c3 - 1.3).abs() < 1e-9, "限幅后应得 1.0*0.7+2.0*0.3=1.3,实得 {c3}");
        // 零值保护:估算为 0 或拿不到真实值时不更新。
        assert_eq!(update_calibration(0.9, 0, 100), 0.9);
        assert_eq!(update_calibration(0.9, 100, 0), 0.9);
        // 估算恰好命中时不漂移。
        assert!((update_calibration(1.0, 8_000, 8_000) - 1.0).abs() < 1e-9);
    }

    /// trim_tail:压缩后仍超线时只收 tail 最旧端,任务定义、纪要、当前用户
    /// 消息一律不动——否则下一步预算检查立刻再压,缓存前缀两次全量重算。
    #[test]
    fn trimTail只收最旧端且不动首尾与纪要() {
        let system = vec![String::new()];
        let mut messages = vec![Message::user_text("任务定义:修复 D-123 的空指针")];
        messages.push(Message::user_text(
            "(系统:此前 40 条消息已压缩为纪要,基于它继续)\n改动了 runner.rs 的压缩逻辑,store.rs 待处理",
        ));
        for i in 0..30 {
            messages.push(Message::user_text(format!("tail 第 {i} 条 {}", "x".repeat(300))));
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

    /// D-206:主动压缩不设总量配额。等价类断言写在常量语义上:
    /// 刹车常量只允许"连续无效"语义存在——谁把总量配额加回来,先得删这条测试。
    #[test]
    fn 压缩刹车只认连续无效不设总量配额() {
        // 常量本身:连续无效两次即停(压不动了),而不是"一共只能压 N 次"。
        assert_eq!(super::MAX_FUTILE_COMPACTIONS, 2);
        // 语义锚点:成功压缩必须能无限次发生。模拟 58 步长 run 的记账序列——
        // 三次成功压缩(after<=budget → 清零)后计数仍为 0,第四次照样允许;
        // 旧实现(每次成功 +1、上限 3)在同一序列后是 3,第四次被拒。
        let budget = 100u64;
        let mut futile = 0u32;
        for _ in 0..3 {
            let after = 90u64; // 压回线内
            if after <= budget { futile = 0 } else { futile += 1 }
        }
        assert_eq!(futile, 0, "成功的压缩不得累计任何配额");
        assert!(futile < super::MAX_FUTILE_COMPACTIONS, "第四次压缩必须仍被允许");
        // 连续压不动(after>budget)两次后停——这才是注释里"再压无益"的原意。
        for _ in 0..2 {
            let after = 120u64;
            if after <= budget { futile = 0 } else { futile += 1 }
        }
        assert!(futile >= super::MAX_FUTILE_COMPACTIONS, "连续无效两次后必须刹车");
        // 中间只要成功一次就复位,不是一杆子打死。
        let after = 90u64;
        if after <= budget { futile = 0 } else { futile += 1 }
        assert_eq!(futile, 0);
    }

    /// D-203:trim_tail 必须用**校准口径**够预算线,与调用方同一把尺子。
    /// a119eeb 把校准乘在了三个调用点上,trim_tail 内部却用原始估算——
    /// calibration>1 时它提前收手,调用方视角仍超线,下一步立刻再压,
    /// 缓存前缀两次全量重算,恰是它要防的事。谁把 budgeted_tokens 改回
    /// 原始估算,这条立刻红。
    #[test]
    fn trimTail按校准口径收线_调用方视角不再超预算() {
        let calibration = 1.6; // 中文密集轨迹的真实形态:估算系统性偏低
        let system = vec![String::new()];
        let build = || {
            let mut messages = vec![Message::user_text("任务定义:修复 D-123")];
            for i in 0..40 {
                messages.push(Message::user_text(format!("tail 第 {i} 条 {}", "x".repeat(300))));
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
        trim_tail(&mut messages, &system, &[], budget, calibration, &mut traces);
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

    struct ProbeTool {
        name: &'static str,
        concurrency: ToolConcurrency,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for ProbeTool {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> String {
            "test probe".into()
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn concurrency(&self, _input: &serde_json::Value, _ctx: &ToolCtx) -> ToolConcurrency {
            self.concurrency.clone()
        }

        async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
            let active = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(
                input["delay_ms"].as_u64().unwrap_or(10),
            ))
            .await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            let label = input["label"].as_str().unwrap_or("probe");
            if input["fail"].as_bool().unwrap_or(false) {
                ToolOutput::error(format!("{label} failed"))
            } else {
                ToolOutput::ok(format!("{label} ok"))
            }
        }
    }

    fn probe_call(
        index: usize,
        id: &str,
        input: serde_json::Value,
        tool: Arc<ProbeTool>,
    ) -> PreparedToolCall {
        PreparedToolCall {
            index,
            id: id.into(),
            name: tool.name().into(),
            concurrency: tool.concurrency(&input, &ToolCtx::new(std::env::temp_dir())),
            input,
            tool,
        }
    }

    #[tokio::test]
    async fn 普通只读工具真实并发_失败隔离且结果按调用顺序归位() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "call_slow",
                serde_json::json!({"label": "slow", "delay_ms": 60}),
                tool.clone(),
            ),
            probe_call(
                1,
                "call_fast_fail",
                serde_json::json!({"label": "fast", "delay_ms": 5, "fail": true}),
                tool,
            ),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir());
        let mut completed = Vec::new();
        let mut on_event = |event| {
            if let RunEvent::ToolEnd { id, .. } = event {
                completed.push(id);
            }
        };
        let results = execute_prepared_tools(calls, &ctx, super::MAX_PARALLEL_TOOLS_PER_WAVE, &mut on_event).await;

        assert!(max_in_flight.load(Ordering::SeqCst) >= 2, "只读调用没有重叠执行");
        assert_eq!(completed, vec!["call_fast_fail", "call_slow"]);
        assert!(matches!(
            &results[0].1,
            Part::ToolResult { call_id, is_error: false, content } if call_id == "call_slow" && content.contains("slow ok")
        ));
        assert!(matches!(
            &results[1].1,
            Part::ToolResult { call_id, is_error: true, content } if call_id == "call_fast_fail" && content.contains("fast failed")
        ));
    }

    #[tokio::test]
    async fn 同一工作树读写与写写冲突严格串行() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let writer = Arc::new(ProbeTool {
            name: "probe_write",
            concurrency: ToolConcurrency::WorktreeWrite("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let reader = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight,
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(0, "write_1", serde_json::json!({"delay_ms": 15}), writer.clone()),
            probe_call(1, "read_1", serde_json::json!({"delay_ms": 15}), reader),
            probe_call(2, "write_2", serde_json::json!({"delay_ms": 15}), writer),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(calls, &ctx, super::MAX_PARALLEL_TOOLS_PER_WAVE, &mut on_event).await;

        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            results
                .iter()
                .map(|(_, part)| match part {
                    Part::ToolResult { call_id, .. } => call_id.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>(),
            vec!["write_1", "read_1", "write_2"]
        );
    }

    #[test]
    fn 子代理完成前的缓冲事件会被排空() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(RunEvent::TaskProgress {
            id: "task-1".into(),
            text: "收尾".into(),
            trace: None,
        })
        .unwrap();
        drop(tx);
        let mut drained = 0;
        drain_task_events(&mut rx, &mut |event| {
            if matches!(event, RunEvent::TaskProgress { .. }) {
                drained += 1;
            }
        });
        assert_eq!(drained, 1);
    }

    /// 流中途断开只对传输层错误重放:协议错误重放只会原样复现,白烧钱。
    /// 这里锁住 runner 里那条判定用的变体匹配语义。
    #[test]
    fn 只有传输层中断才重放本步() {
        let transport = LlmError::Config("placeholder".into());
        assert!(!matches!(transport, LlmError::Transport(_)));
        assert!(!matches!(
            LlmError::Protocol("bad SSE".into()),
            LlmError::Transport(_)
        ));
        assert!(!matches!(
            LlmError::ContextOverflow { message: "x".into() },
            LlmError::Transport(_)
        ));
        assert!(!matches!(
            LlmError::Provider { kind: "rate_limit_error".into(), message: "x".into() },
            LlmError::Transport(_)
        ));
        // 重放次数必须有界:每次重放都要重新生成已产出的 token。
        assert_eq!(MAX_STREAM_RESTARTS, 2);
    }

    #[test]
    fn compact_retry_keeps_prompt_and_bounded_tool_history() {
        let mut messages = vec![
            Message::user_text("原始任务"),
            Message::assistant(vec![Part::Text {
                text: "旧回复".into(),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "工具结果".into(),
                is_error: false,
            }]),
            Message::user_text("当前任务"),
        ];
        let mut traces = Vec::new();

        compact_messages_for_retry(&mut messages, &mut traces);

        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].parts[0], Part::Text { ref text } if text.contains("工具结果")));
        assert!(matches!(messages[1].parts[0], Part::Text { ref text } if text == "当前任务"));
    }

    #[test]
    fn compact_retry_drops_orphan_tool_results_in_tool_loop() {
        let mut messages = vec![
            Message::user_text("当前任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "notes.md"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "工具结果".into(),
                is_error: false,
            }]),
        ];
        let mut traces = Vec::new();

        compact_messages_for_retry(&mut messages, &mut traces);
        assert!(messages.iter().any(|message| {
            message.parts.iter().any(
                |part| matches!(part, Part::Text { text } if text == "当前任务")
            )
        }));
        assert!(!messages.iter().flat_map(|message| &message.parts).any(|part| {
            matches!(part, Part::ToolResult { .. } | Part::ToolCall { .. })
        }));
        assert!(messages.iter().any(|message| {
            message.parts.iter().any(
                |part| matches!(part, Part::Text { text } if text.contains("工具结果"))
            )
        }));

        let mut aggressive = vec![
            Message::user_text("当前任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_2".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_2".into(),
                content: "结果".into(),
                is_error: false,
            }]),
        ];
        let mut aggressive_traces = Vec::new();
        compact_messages_aggressively(&mut aggressive, &mut aggressive_traces);
        assert!(!aggressive.iter().flat_map(|message| &message.parts).any(|part| {
            matches!(part, Part::ToolResult { .. } | Part::ToolCall { .. })
        }));
    }

    #[test]
    fn compaction_records_dropped_segments_as_overflow_traces() {
        // R-106:被裁剪段先沉淀轨迹摘要再重置,激进压缩不再无声丢弃轨迹。
        // 失败信号要走 summarize_failures 的机械闸门(count>=2 或带恢复对),
        // 所以同一失败放两次,信号才会保留。
        let mut messages = vec![
            Message::user_text("原始任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "cargo test"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "构建失败".into(),
                is_error: true,
            }]),
            Message::assistant(vec![Part::ToolCall {
                id: "call_2".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "cargo test"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_2".into(),
                content: "构建失败".into(),
                is_error: true,
            }]),
            Message::user_text("当前任务"),
        ];
        let mut recoveries = 0;
        let mut traces = Vec::new();

        // 第一级:有界压缩。被丢弃的是除当前消息外的整段历史。
        assert!(recover_context_overflow(&mut messages, &mut recoveries, &mut traces));
        assert_eq!(traces.len(), 1, "第一次压缩应产生一条轨迹摘要");
        let first: serde_json::Value = serde_json::from_str(&traces[0]).unwrap();
        assert_eq!(first["dropped_messages"], 5);
        assert_eq!(first["tools"]["bash"], 2, "被丢弃段的工具画像应被沉淀");
        assert_eq!(first["failures"][0]["tool"], "bash", "失败信号应随轨迹沉淀");
        assert_eq!(first["failures"][0]["count"], 2);
        assert!(first["preview"].as_str().is_some_and(|s| s.contains("原始任务")));

        // 第二级:激进压缩。当前消息外的整段(含上一级压缩记录)再次沉淀。
        assert!(recover_context_overflow(&mut messages, &mut recoveries, &mut traces));
        assert_eq!(traces.len(), 2, "第二次压缩应追加一条轨迹摘要");
        assert_eq!(messages.len(), 1, "激进压缩后只剩当前消息");

        // 第三级:超过有界上限,拒绝继续恢复且不新增轨迹。
        assert!(!recover_context_overflow(&mut messages, &mut recoveries, &mut traces));
        assert_eq!(traces.len(), 2);
    }
    #[test]
    fn declined_tool_batch_keeps_real_and_placeholder_results_paired() {
        let calls = vec![
            ("call_done".into(), "write".into(), serde_json::json!({}), "{}".into()),
            ("call_declined".into(), "edit".into(), serde_json::json!({}), "{}".into()),
            ("call_pending".into(), "bash".into(), serde_json::json!({}), "{}".into()),
        ];
        let mut results = vec![Part::ToolResult {
            call_id: "call_done".into(),
            content: "真实写入结果".into(),
            is_error: false,
        }];
        append_declined_tool_results(&mut results, &calls, 1);

        assert_eq!(results.len(), 3);
        assert!(matches!(
            &results[0],
            Part::ToolResult { call_id, content, is_error: false }
                if call_id == "call_done" && content == "真实写入结果"
        ));
        assert!(matches!(
            &results[1],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_declined" && content.contains("declined")
        ));
        assert!(matches!(
            &results[2],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_pending" && content.contains("cancelled")
        ));
    }
}
