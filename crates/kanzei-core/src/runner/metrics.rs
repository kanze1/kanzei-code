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
    /// 工具按设计拒绝了调用(no-op/需要修正/需要确认),不计真实故障。
    #[serde(default)]
    pub tool_rejections: usize,
    #[serde(default)]
    pub edit_rejections: usize,
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
                Part::ToolResult {
                    call_id,
                    is_error,
                    content,
                    ..
                } => {
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
                    if is_expected_tool_rejection(content) {
                        metrics.tool_rejections += 1;
                        if let Some((name, _)) = calls.get(call_id) {
                            if name == "edit" || name == "multiedit" || name == "insert" {
                                metrics.edit_rejections += 1;
                            }
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

/// ToolOutput 回喂模型时为预期拒绝附加的稳定头。旧轨迹没有该头,自然按旧语义统计。
pub(crate) fn is_expected_tool_rejection(content: &str) -> bool {
    let Some(header) = content.lines().next() else {
        return false;
    };
    header.starts_with("[tool_outcome=noop ")
        || header.starts_with("[tool_outcome=needs_correction ")
        || header.starts_with("[tool_outcome=needs_confirmation ")
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
                Part::ToolResult {
                    call_id, is_error, ..
                } => {
                    let Some((name, input)) = calls.get(call_id) else {
                        continue;
                    };
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
                    let Some(id) = input.get("id").and_then(|v| v.as_str()) else {
                        continue;
                    };
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
                Part::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => {
                    let Some((tool, target)) = calls.get(call_id).cloned() else {
                        continue;
                    };
                    if *is_error {
                        if is_expected_tool_rejection(content) {
                            continue;
                        }
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

/// 抹掉错误原文里的易变载荷:反引号包住的片段(具体命令名、结构化 bash 的整段
/// 命令 JSON)与花括号包住的片段(裸 JSON)。没有配对收尾就吃到行尾——错误原文
/// 经常是被截断的。
///
/// 不抹会怎样(2026-08-12 实测):`permission requires user approval: bash on
/// `{"command":"Get-ChildItem …}`` 这类错误,命令换一个字指纹就变成新的一条,
/// index.db 的 recurrence_counts 里 11 个指纹**全部停在 1**,于是「第 2 次才建
/// candidate、第 3 次+才晋升 active」的三段晋升门永远打不开,记忆只进不出。
pub fn mask_volatile_payload(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '`' => {
                for c in chars.by_ref() {
                    if c == '`' {
                        break;
                    }
                }
            }
            '{' => {
                let mut depth = 1usize;
                for c in chars.by_ref() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => out.push(ch),
        }
    }
    out
}

/// 把 `[fp:tool|kind]` 标记归一到当前口径。既有记忆正文里存的是**改口径之前**
/// 生成的标记,比较时两侧都过一遍本函数才对得上——否则每次收紧指纹规则,
/// 全部存量记忆就集体失配,复发检测直接瞎掉。归一是幂等的。
pub fn normalize_fp_marker(marker: &str) -> String {
    let trimmed = marker.trim();
    let Some(rest) = trimmed.strip_prefix("[fp:") else {
        return trimmed.to_string();
    };
    let rest = rest.strip_suffix(']').unwrap_or(rest);
    let Some((tool, kind)) = rest.split_once('|') else {
        return trimmed.to_string();
    };
    let normalized = failure_kind(kind);
    let normalized = if is_usable_failure_kind(&normalized) {
        normalized
    } else {
        "__legacy_generic__".into()
    };
    format!("[fp:{}|{}]", tool.trim(), normalized)
}

/// 错误指纹:首行小写 → 抹掉易变载荷 → 抹掉含路径分隔符的 token 与全部数字 →
/// 折叠空白 → 截 80。目的是让「13 次 CRLF 未命中」塌成同一条,而不是 13 条。
/// R-162:从 summarize_failures 抽出为共享函数,RecallWatch(事件触发召回)复用
/// 同一 (tool, kind) 分类口径,离线度量的失败指纹与在线触发的失败指纹必须一致。
pub(crate) fn failure_kind(content: &str) -> String {
    // D-159:bash 批次常把多条命令的输出叠在一起(同一次调用里 `git add` 失败 +
    // `git commit` 因无暂存失败),机械取首行会把前置的 `fatal: pathspec ... did
    // not match` 根因丢掉,只留下末尾的 commit 症状——记忆把症状当根因。
    // 先扫全文本,命中 fatal:/pathspec/did not match 的行优先作为根因行。
    let first_line = content.lines().next().unwrap_or("");
    let root_line = content
        .lines()
        .find(|l| {
            let lower = l.to_lowercase();
            lower.contains("fatal:")
                || lower.contains("pathspec")
                || lower.contains("did not match")
        })
        .or_else(|| {
            content.lines().find(|line| {
                let lower = line.trim().to_lowercase();
                !lower.is_empty()
                    && !is_wrapper_failure_line(&lower)
                    && [
                        "assert",
                        "error",
                        "failed",
                        "failure",
                        "panic",
                        "traceback",
                        "not found",
                        "cannot",
                        "denied",
                        "expected",
                        "undefined",
                    ]
                    .iter()
                    .any(|marker| lower.contains(marker))
            })
        })
        .unwrap_or(first_line);
    let root_line = mask_volatile_payload(root_line);
    let scrubbed: Vec<String> = root_line
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

fn is_wrapper_failure_line(line: &str) -> bool {
    line.starts_with("exit code")
        || line.starts_with("process exited")
        || line.starts_with("command failed")
        || line == "test failed"
        || line == "tests failed"
}

/// 记忆写入侧的指纹质量闸:过短或只描述外层退出状态的值不能作为跨轮键。
pub fn is_usable_failure_kind(kind: &str) -> bool {
    let normalized = kind.trim().to_ascii_lowercase();
    normalized.chars().count() >= 8
        && !matches!(
            normalized.as_str(),
            "exit code:"
                | "exit code"
                | "test failed"
                | "tests failed"
                | "command failed"
                | "unknown error"
                | "*"
        )
}

/// 目标键:路径类取最后一段(跨平台分隔符都算),命令取首词,其余取 id。
/// R-162:同 failure_kind,提为共享函数供在线召回复用。
pub(crate) fn failure_target(input: &serde_json::Value) -> String {
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
mod tests {
    use super::*;
    use crate::runner::testutil::{bash, call, result, tracker_call};

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
    fn 受控拒绝不污染真实失败指标和记忆信号() {
        let messages = vec![
            call("e1", "edit", "path", "src/lib.rs"),
            result(
                "e1",
                "[tool_outcome=needs_correction code=EDIT_INSERTION_WOULD_REPLACE_ANCHOR]\n请改用 insert",
                true,
            ),
            call("e2", "edit", "path", "src/lib.rs"),
            result(
                "e2",
                "[tool_outcome=noop code=EDIT_IDENTICAL_INPUT]\n无需修改",
                true,
            ),
        ];
        let metrics = summarize_metrics(&messages);
        assert_eq!(metrics.tool_rejections, 2);
        assert_eq!(metrics.edit_rejections, 2);
        assert_eq!(metrics.failed_calls, 0);
        assert_eq!(metrics.edit_misses, 0);
        assert!(summarize_failures(&messages).is_empty());
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
        assert!(
            entry.tools.contains(&"edit".to_string()) && entry.tools.contains(&"bash".to_string())
        );

        // 反例一:纯查询轮 —— 没有任何实质动作,不构成可复用流程。
        let read_only = vec![
            call("c1", "read", "path", "src/lib.rs"),
            result("c1", "...", false),
            tracker_call("c2", "req", "R-124", "done"),
            result("c2", "updated", false),
        ];
        assert!(
            completed_entry(&read_only).is_none(),
            "纯查询轮不该提炼 SOP"
        );

        // 反例二:先勾完成再干活 —— 顺序反了,勾的那一刻并没有完成什么。
        let out_of_order = vec![
            tracker_call("c1", "req", "R-125", "done"),
            result("c1", "updated", false),
            call("c2", "edit", "path", "src/lib.rs"),
            result("c2", "ok", false),
        ];
        assert!(
            completed_entry(&out_of_order).is_none(),
            "先收口后干活不该提炼 SOP"
        );

        // 反例三:收口调用本身失败 —— 条目根本没进终态。
        let failed_close = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            tracker_call("c2", "req", "R-126", "done"),
            result("c2", "cannot move backward", true),
        ];
        assert!(
            completed_entry(&failed_close).is_none(),
            "收口失败不该提炼 SOP"
        );

        // 反例四:只是把状态推到 doing —— 不是终态。
        let in_progress = vec![
            call("c1", "edit", "path", "src/lib.rs"),
            result("c1", "ok", false),
            tracker_call("c2", "req", "R-127", "doing"),
            result("c2", "updated", false),
        ];
        assert!(
            completed_entry(&in_progress).is_none(),
            "推进到 doing 不是完成"
        );
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
            result(
                "c1",
                "old_string not found in C:/p/main.rs — it must match exactly",
                true,
            ),
            call("c2", "edit", "path", "C:/p/other.rs"),
            result(
                "c2",
                "old_string not found in C:/p/other.rs — it must match exactly",
                true,
            ),
        ];
        let signals = summarize_failures(&twice);
        assert_eq!(signals.len(), 1, "同类错误必须塌成一条: {signals:?}");
        assert_eq!(signals[0].tool, "edit");
        assert_eq!(signals[0].count, 2);
        // 指纹抹掉了路径,两次不同文件仍归一类
        assert!(
            !signals[0].kind.contains("main.rs"),
            "指纹不该含路径: {}",
            signals[0].kind
        );
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

    #[test]
    fn failure_kind_把路径与数字抹平_同坑塌成一条() {
        // R-162 B1:共享分类函数——离线度量与在线触发必须用同一指纹口径。
        // 路径 token 与数字抹掉后,"13 次 CRLF 未命中"与"7 次 CRLF 未命中"是同一条。
        let a = failure_kind("old_string not found in C:/p/main.rs");
        let b = failure_kind("old_string not found in D:/other/lib.rs");
        assert_eq!(a, b, "不同路径的同款错误必须塌成同一指纹");
        assert!(!a.contains('/'), "路径段必须被抹掉");
        assert!(!a.contains('\\'), "反斜杠路径段也必须被抹掉");

        let c = failure_kind("test failed: 13 assertions failed");
        let d = failure_kind("test failed: 7 assertions failed");
        assert_eq!(c, d, "数字差异必须抹平");
        assert!(
            !c.contains('1') && !d.contains('7'),
            "ASCII 数字必须全部抹掉"
        );

        // 首行截断 80 字符,长错误不撑爆索引。
        let long = failure_kind(&"x".repeat(300));
        assert_eq!(long.chars().count(), 80);
    }

    #[test]
    fn failure_kind_抹掉命令载荷_权限错误不再每次换指纹() {
        // 2026-08-12 实证:权限拒绝把整条命令 JSON 拼进错误原文,命令换一个字
        // 指纹就换一条,recurrence_counts 里 11 个指纹全停在 1,三段晋升门永不打开。
        let a = failure_kind(
            r#"permission requires user approval: bash on `{"command":"Get-ChildItem output"}`"#,
        );
        let b = failure_kind(
            r#"permission requires user approval: bash on `{"command":"git log --all --oneline"}`"#,
        );
        assert_eq!(a, b, "同一道权限墙必须塌成同一指纹");
        assert!(a.contains("permission requires user approval"), "{a}");

        // 反引号里的具体子命令同样是载荷:bash 里做 git mutation 是同一个坑。
        let merge = failure_kind(
            "`git merge` is blocked in bash: git mutations must use the structured `git` tool",
        );
        let restore = failure_kind(
            "`git restore` is blocked in bash: git mutations must use the structured `git` tool",
        );
        assert_eq!(merge, restore, "同族 git mutation 拦截必须塌成同一指纹");

        // 但不同的坑不能被抹成一条:整文件重写与 git mutation 是两条规则。
        let rewrite = failure_kind(
            "`Set-Content` is blocked: whole-file rewrites bypass the edit tool's validation",
        );
        assert_ne!(merge, rewrite, "尾部语义不同的拦截不得误并");
    }

    #[test]
    fn normalize_fp_marker_老口径标记归一到新口径且幂等() {
        // 存量记忆正文里的标记是收紧口径之前生成的,不归一就集体失配。
        let stored = "[fp:bash|`git merge` is blocked in bash: git mutations must use the structured `git` tool]";
        let fresh = format!(
            "[fp:bash|{}]",
            failure_kind("`git restore` is blocked in bash: git mutations must use the structured `git` tool")
        );
        assert_eq!(normalize_fp_marker(stored), normalize_fp_marker(&fresh));
        // 幂等:归一结果再归一不变。
        let once = normalize_fp_marker(stored);
        assert_eq!(normalize_fp_marker(&once), once);
        // 不是 fp 标记的串原样返回,不制造假指纹。
        assert_eq!(normalize_fp_marker("随便一句话"), "随便一句话");
    }

    /// D-159:bash 批次多行输出时,前置 `fatal: pathspec` 根因优先于首行的
    /// commit 症状——不能把「Changes not staged」当根因记成忘记 add。
    #[test]
    fn failure_kind_多行bash批次_优先取pathspec根因行() {
        let content = "Changes not staged for commit:\n  (use \"git add <file>...\" to update what will be committed)\nfatal: pathspec 'src/foo.rs' did not match any files";
        let kind = failure_kind(content);
        assert!(
            kind.contains("fatal: pathspec") && kind.contains("did not match"),
            "应取 pathspec 根因行,而非首行 commit 症状: {kind}"
        );
        assert!(
            !kind.contains("changes not staged"),
            "commit 症状行不得成为根因: {kind}"
        );

        // 无根因行时退回首行(既有行为不回归)。
        let single = failure_kind("old_string not found in C:/p/main.rs");
        assert!(single.contains("old_string not found"), "{single}");
    }

    #[test]
    fn failure_kind_bash测试输出跳过退出包装行并拒绝通配键() {
        let kind = failure_kind("exit code: 101\nerror: assertion `left == right` failed");
        assert!(
            kind.contains("error:") && kind.contains("assertion"),
            "{kind}"
        );
        assert!(!is_usable_failure_kind("exit code:"));
        assert!(!is_usable_failure_kind("error"));
        assert!(is_usable_failure_kind(&kind));
        assert_eq!(
            normalize_fp_marker("[fp:bash|exit code:]"),
            "[fp:bash|__legacy_generic__]"
        );
    }

    #[test]
    fn failure_target_路径取尾段_命令取首词() {
        // R-162 B1:目标抽取——RecallWatch 用它做同目标去重与 ReRetrieve 的 query 拼装。
        let path = serde_json::json!({ "path": "C:/p/main.rs" });
        assert_eq!(failure_target(&path), "main.rs");
        let backslash = serde_json::json!({ "file_path": "C:\\p\\lib.rs" });
        assert_eq!(
            failure_target(&backslash),
            "lib.rs",
            "Windows 反斜杠路径取尾段"
        );
        let command = serde_json::json!({ "command": "cargo test -p foo --all-features" });
        assert_eq!(failure_target(&command), "cargo");
        let id = serde_json::json!({ "id": "M-001" });
        assert_eq!(failure_target(&id), "m-001");
        assert!(failure_target(&serde_json::json!({ "other": 1 })).is_empty());
    }
}
