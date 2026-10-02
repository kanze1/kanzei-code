//! Tracker maintenance actions: ledger/archive repair and raw-line operations.
//!
//! These actions keep the existing `TrackerTool` routing and `DocStore` write paths;
//! this module only separates the action implementations from the main CRUD flow.

use crate::docstore::{DocStore, Entry};
use kanzei_harness::{ToolCtx, ToolOutput};

use super::action_helpers::archived_or_unknown;
use super::{TrackerInput, TrackerTool};

// 主动注销一个编号:唯一合法的"缺号交代"通道,理由必填、留档可审计。
pub(crate) fn void_id(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    _entries: &mut Vec<Entry>,
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for void_id");
    };
    let Some(reason) = input.reason.as_deref().filter(|r| !r.trim().is_empty()) else {
        return ToolOutput::error(
            "`reason` is required for void_id — state why this allocated id \
             legitimately has no entry, and what evidence says so (e.g. which commit \
             range you searched). An unexplained void is indistinguishable from \
             hiding data loss.",
        );
    };
    match store.void_id(id, reason) {
        Ok(()) => ToolOutput::ok(format!(
            "voided {id} in {}. It will never be reallocated. Commit the ledger \
             together with {}.",
            store.ledger_file().display(),
            tool.kind.rel_path,
        )),
        Err(e) => ToolOutput::error(format!("void_id failed: {e}")),
    }
}

pub(crate) fn archive(
    tool: &TrackerTool,
    _input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    _entries: &mut Vec<Entry>,
) -> ToolOutput {
    match store.archive_terminal() {
        Ok(moved) if moved.is_empty() => ToolOutput::ok("nothing to archive (no terminal entries)"),
        Ok(moved) => {
            // 归档后回读校验(D-112):移动的 ID 必须真的落在归档文件里。
            let archived = store.load_archive().unwrap_or_default();
            let lost: Vec<&String> = moved
                .iter()
                .filter(|id| !archived.iter().any(|e| &&e.id == id))
                .collect();
            if !lost.is_empty() {
                return ToolOutput::error(format!(
                    "archive verification FAILED: {} missing from {} after the move — \
                     do NOT commit; the entries may be lost, investigate immediately",
                    lost.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    store.archive_file().display()
                ));
            }
            ToolOutput::ok(format!(
                "archived {} terminal {}(s): {} → {}\n\
                 IMPORTANT: `{}` and its archive file were BOTH modified — commit them \
                 together in the SAME commit. Committing only one (or reverting the \
                 archive) permanently loses these entries (D-112).",
                moved.len(),
                tool.noun,
                moved.join(", "),
                store.archive_file().display(),
                tool.kind.rel_path,
            ))
        }
        Err(e) => ToolOutput::error(format!("archive failed: {e}")),
    }
}

// R-201:按序号删除一条游离行。删除走 docstore 的模板手术:只移除那一条
// Raw,字段与其余行一字不动,二次保存幂等(行已不在模板里,不会再生)。
pub(crate) fn raw_delete(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    _entries: &mut Vec<Entry>,
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for raw_delete");
    };
    let Some(ordinal) = input.ordinal else {
        return ToolOutput::error(
            "`ordinal` is required for raw_delete(取值见 raw_lines 输出的 [n])",
        );
    };
    match store.delete_raw_line(id, ordinal) {
        Ok(()) => ToolOutput::ok(format!(
            "已删除 {id} 的第 {ordinal} 条游离行;其余内容与字段一字不变。\
             可再 `{tool} raw_lines id={id}` 复查剩余游离行。",
            tool = tool.tool_name,
        )),
        Err(e) => ToolOutput::error(format!("raw_delete failed: {e}")),
    }
}

// D-241:fixing 推不动时的合法退路。要求 id + reason(强制写理由),
// 状态必须命中该文档类型的 reopen_from 集合,退回初始态并落进展。
// 与「手改 markdown」的区别:reopen 走引擎,理由进文档,调度器下次
// 扫到的是 open 而不是冒充「正在做」的僵尸 fixing。
pub(crate) fn reopen(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    entries: &mut [Entry],
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for reopen");
    };
    let Some(reason) = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    else {
        return ToolOutput::error(
            "`reason` is required for reopen: say why this item is being pulled back",
        );
    };
    let Some(pos) = entries.iter().position(|e| &e.id == id) else {
        return ToolOutput::error(archived_or_unknown(id, entries, store, tool.tool_name));
    };
    let current = &entries[pos];
    if !tool.kind.reopen_from.contains(&current.status.as_str()) {
        return ToolOutput::error(format!(
            "cannot reopen {id}: status `{}` is not in the reopen set ({}). \
             Reopen pulls a non-terminal item back to `{}`; closed items stay closed.",
            current.status,
            tool.kind.reopen_from.join(" | "),
            tool.kind.statuses[0],
        ));
    }
    let back_to = tool.kind.statuses[0].to_string();
    entries[pos].status = back_to.clone();
    // 退回理由必须留在条目里,不能只出现在工具输出——否则下轮上下文
    // 一滚动就没人知道这条为什么被退回来(D-241 验收②「处置依据逐条写进进展」)。
    // 追加新的一行进展,而不是拼进已有字段值:docstore 按行解析,
    // 值里嵌 \n 的重载会被拆成 Raw 行而丢失(D-241 实测)。
    let note = format!("[reopen {}] {}", crate::memory::today(), reason);
    entries[pos].fields.push(("进展".into(), note.clone()));
    if let Err(e) = store.save(entries) {
        return ToolOutput::error(format!("cannot write {}: {e}", store.path.display()));
    }
    ToolOutput::ok(format!(
        "reopened {id} [{back_to}] {}\n{note}",
        entries[pos].title
    ))
}

// D-331:归档终态纠错——只允许终态到终态(fixed↔wontfix),强制 reason,
// 条目保持归档、原子写入、进展留审计。归档 ID 不再是死胡同(D-267 的
// [dropped] [fixed] 双终态就是没有此通道时留下的)。
pub(crate) fn fix_terminal(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    _entries: &mut Vec<Entry>,
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for fix_terminal");
    };
    let Some(status) = input
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return ToolOutput::error(
            "`status` is required for fix_terminal (one of the terminal statuses)",
        );
    };
    let Some(reason) = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    else {
        return ToolOutput::error(
            "`reason` is required for fix_terminal: say why the archived terminal \
             status is being corrected",
        );
    };
    match store.correct_archived_terminal(id, status, reason) {
        Ok((old, new)) => ToolOutput::ok(format!(
            "corrected archived {id} terminal: {old} → {new} (stays archived).\n\
             Commit `{}` and its archive file together.",
            tool.kind.rel_path,
        )),
        Err(e) => ToolOutput::error(format!("fix_terminal failed: {e}")),
    }
}

// R-227:归档条目字段里的测试 ID 占位符回填。占位符是
// 「全量跑过但没记 test_record、隔时凭记忆写证据」的产物(R-198/R-199/
// D-219/D-266/D-279/D-281/D-282/D-316 关闭证据存量 8 处)。回填 =
// 把占位符替换为 test_record 落盘的真实 ID;docstore 侧要求恰好命中一次。
pub(crate) fn archive_fill(
    tool: &TrackerTool,
    input: TrackerInput,
    _ctx: &ToolCtx,
    store: &DocStore,
    _entries: &mut Vec<Entry>,
) -> ToolOutput {
    let Some(id) = &input.id else {
        return ToolOutput::error("`id` is required for archive_fill");
    };
    let (Some(old), Some(new)) = (input.old.as_deref(), input.new.as_deref()) else {
        return ToolOutput::error(
            "`old` and `new` are required for archive_fill: old = 归档中的旧文本, \
             new = test_record 落盘的真实 ID(如 `T-1786565346`)",
        );
    };
    match store.fill_archived_placeholder(id, old, new) {
        Ok(0) => ToolOutput::error(format!(
            "archive_fill: 在归档 {id} 里没找到 `{old}`,没有可回填的占位符"
        )),
        Ok(count) => ToolOutput::ok(format!(
            "archive_fill: 归档 {id} 回填 {count} 处占位符 `{old}` → `{new}`。\n\
             与 test_record 落盘的真实 ID 对齐(关门禁 R-227)。\n\
             提交时带上 `{}` 及其归档文件。",
            tool.kind.rel_path,
        )),
        Err(e) => ToolOutput::error(format!("archive_fill failed: {e}")),
    }
}
