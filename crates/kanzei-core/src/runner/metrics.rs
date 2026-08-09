//! 运行指标域(R-155 B2):summarize_* / FailureSignal / RunMetrics / CompletedEntry /
//! is_git_query。is_git_query 提 pub(crate)(双归属:指标+冗余,R-100 机械闸门)。

use kanzei_llm::{Message, Part};

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

pub(crate) fn is_git_query(input: &serde_json::Value) -> bool {
    const QUERIES: &[&str] = &["git status", "git diff", "git log", "git show", "git blame"];
    let command = input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    QUERIES.iter().any(|q| command.contains(q))
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

