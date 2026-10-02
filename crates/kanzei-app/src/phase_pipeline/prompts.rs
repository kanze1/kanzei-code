//! 阶段角色上下文、勘察/复核提示以及结果简报的文本呈现。

use super::{RoleReport, NO_ISSUES};

/// 把裁决快照渲染成给角色看的「本轮条目」块。没有 selected 就没有块。
///
/// 只挑对「这次要动什么」有信息量的字段:全量字段会把角色的注意力冲散,而角色
/// 要回答的是「本任务碰哪里」,不是复述条目。
pub(crate) fn render_task_context(state: &kanzei_tools::ResolvedControlState) -> Option<String> {
    let item = state.selected.as_ref()?;
    let Some(context) = item.work_unit_context.as_ref() else {
        let mut out = format!("{} [{}] {}", item.id, item.lifecycle_status, item.title);
        for field in &item.fields {
            if matches!(
                field.name.as_str(),
                "进展" | "内容" | "验收" | "复现" | "改动面"
            ) {
                out.push_str(&format!("\n- {}: {}", field.name, field.value));
            }
        }
        return Some(out);
    };

    let unit = &context.unit;
    let mut out = format!(
        "{} [{}] {}\n[当前任务]\n- 目标: {}",
        unit.unit_id,
        unit.status.as_str(),
        unit.objective,
        unit.objective
    );
    if !unit.scope.is_empty() {
        out.push_str(&format!("\n- 范围: {}", unit.scope.join(", ")));
    }
    if let Some(checkpoint) = &unit.last_checkpoint {
        out.push_str(&format!(
            "\n[实质进展]\n- 摘要: {}\n- 下一步: {}",
            checkpoint.summary, checkpoint.next_action
        ));
        if !checkpoint.decisions.is_empty() {
            out.push_str(&format!("\n- 决策: {}", checkpoint.decisions.join("；")));
        }
        if !checkpoint.retrieval_refs.is_empty() {
            out.push_str(&format!(
                "\n[记忆来源]\n- 声明来源: {}",
                checkpoint.retrieval_refs.join(", ")
            ));
        }
    } else {
        out.push_str("\n[实质进展]\n- 尚无 checkpoint");
    }
    out.push_str("\n[验证结果]");
    if unit.verification.is_empty() {
        out.push_str("\n- 尚未声明验证命令");
    } else {
        out.push_str(&format!("\n- 声明命令: {}", unit.verification.join("；")));
    }
    if unit.evidence.is_empty() {
        out.push_str("\n- 证据: 尚未登记（状态未知）");
    } else {
        out.push_str(&format!("\n- 证据: {} 条", unit.evidence.len()));
    }
    Some(out)
}

/// 角色提示里的指代锚。没有它,固定角色表的 brief(「本次任务会写到哪里」)就没有
/// 指代物,scout 只能回答**本仓库**的写入面——D-368 那轮 write_surface_scout 返回
/// kanzei-core/src/store/processes.rs 就是这么来的:它忠实回答了一个通用问题。
const TASK_ANCHOR: &str = "下面这一条 tracker 条目就是「本次任务」。判断「与本次任务相关」时以它为准;下方 prompt 原文在自主推进轮里可能只是通用推进指令,与用户显式指令冲突时以指令为准。";

fn task_context_block(task_context: Option<&str>) -> String {
    match task_context {
        Some(context) => format!("{TASK_ANCHOR}\n[本轮条目]\n{context}\n\n"),
        None => String::new(),
    }
}

pub(super) fn scout_prompt(
    role: &str,
    brief: &str,
    task_prompt: &str,
    task_context: Option<&str>,
) -> String {
    let context_block = task_context_block(task_context);
    format!(
        "你是只读勘察代理 `{role}`,工具只有 read/glob/grep。\n\
         职责:{brief}\n\n\
         {context_block}\
         本轮任务(原文):\n{task_prompt}\n\n\
         只回事实,每条都带文件路径与行号。不要提改动建议,不要下结论说该怎么做。\n\
         与本轮任务无关的发现不要写。什么都没找到就直接说没找到。"
    )
}

pub(super) fn review_prompt(
    role: &str,
    brief: &str,
    task_prompt: &str,
    run_summary: &str,
    task_context: Option<&str>,
) -> String {
    // 有了条目上下文,复核方向从「自述是否属实」升级为「自述是否兑现了这一条的
    // 验收」——验收字段本来就在上下文块里。
    let context_block = task_context_block(task_context);
    format!(
        "你是只读复核代理 `{role}`,工具只有 read/glob/grep。\n\
         职责:{brief}\n\n\
         {context_block}\
         本轮任务(原文):\n{task_prompt}\n\n\
         本轮执行方的自述结果:\n{run_summary}\n\n\
         去代码里核对上面的自述是否属实。只报**确有问题**的地方,每条带文件路径与行号。\n\
         来源核对必须使用上面的同一 run 起点证据；现时 ?? 不是原有用户稿证据，覆盖事故还须对应成功 write/edit/insert 事实。证据不够说未知，不编事故。\n\
         验收以本任务明确范围和既定验证为准，已通过后按用户要求交接收口；只针对实际失败或未满足验收提出修复。\n\
         没有发现问题就只回 `{NO_ISSUES}` 四个字,不要写任何别的东西。"
    )
}

/// 把角色产出拼成给模型看的一块文本。
pub(super) fn render_brief(title: &str, reports: &[RoleReport], notice: Option<String>) -> String {
    let mut out = format!("[{title}]");
    if let Some(notice) = notice {
        // 反静默降级:失败/超时/零结果必须出现在模型看得见的地方。
        out.push('\n');
        out.push_str(&notice);
    }
    for report in reports {
        if !report.ok {
            continue; // 失败的已经由 notice 点名,正文不再塞错误串
        }
        let text = report.text.trim();
        if text.is_empty() {
            continue;
        }
        out.push_str(&format!("\n\n## {}\n{}", report.role, text));
    }
    out
}

/// 复核发现。全部角色都回 `NO_ISSUES` 且没有失败 → `None`(不需要修正段)。
///
/// 判据刻意**失败即有发现**:复核代理失败/超时时返回 `Some`,让修正段跑一次而不是
/// 当成"没问题"放过去。宁可多跑一段,不可把没复核过的东西当成复核通过。
pub(super) fn findings(reports: &[RoleReport], notice: Option<String>) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    for report in reports {
        if !report.ok {
            continue; // 失败由 notice 承载
        }
        let text = report.text.trim();
        if text.is_empty() || text == NO_ISSUES {
            continue;
        }
        blocks.push(format!("## {}\n{}", report.role, text));
    }
    if blocks.is_empty() && notice.is_none() {
        return None;
    }
    let mut out = String::from("[复核发现]");
    if let Some(notice) = notice {
        out.push('\n');
        out.push_str(&notice);
    }
    for block in blocks {
        out.push_str("\n\n");
        out.push_str(&block);
    }
    Some(out)
}

/// 修正段给模型的提示。
pub(crate) fn fixup_prompt(findings: &str) -> String {
    format!(
        "{findings}\n\n\
         (system) 以上是只读复核代理在你**释放写权之后**、对稳定快照做的核对。\n\
         逐条判断:确有问题的就修掉并给出最小验证;判断为误报的,明确说出理由。\n\
         不要为了显得做了事而改无关的地方。全部都是误报就直说,不用改任何文件。"
    )
}
