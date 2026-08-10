//! 通用追踪工具:req / defect / source / finding 共用一套 CRUD。
//! 硬门禁:ID 引擎分配、状态机受限、格式引擎序列化、引用必须存在——模型只提供字段值。

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};

pub struct TrackerTool {
    pub tool_name: &'static str,
    pub noun: &'static str,
    pub kind: &'static DocKind,
    /// Some(kind) = add/update 时 refs 必须非空且全部存在于该文档(finding → sources)。
    pub requires_refs: Option<&'static DocKind>,
}

/// 可在完整性破损时执行的修复动作:它们的存在就是为了修复完整性,
/// 用完整性门禁挡住它们会形成死锁。
const REPAIR_ACTIONS: &[&str] = &["repair_reused_id", "repair_missing_id", "void_id"];

const WRITE_ACTIONS: &[&str] = &[
    "add",
    "update",
    "close",
    "archive",
    "reorder",
    "repair_reused_id",
    "repair_missing_id",
    "void_id",
];

#[derive(Deserialize, JsonSchema)]
struct TrackerInput {
    /// 动作(取值见 enum)
    action: String,
    /// reorder 用:完整的 ID 新顺序(必须恰好覆盖当前全部条目)
    #[serde(default)]
    order: Vec<String>,
    /// get/update/close/repair_*/void_id 必填,如 "R-012"
    #[serde(default)]
    id: Option<String>,
    /// add 必填
    #[serde(default)]
    title: Option<String>,
    /// 生命周期状态(取值见 enum),与 priority/severity 是三个不同维度
    #[serde(default)]
    status: Option<String>,
    /// 影响程度(取值见 enum);与 priority 不是一回事,别互相代填
    #[serde(default)]
    severity: Option<String>,
    /// 排期优先级(取值见 enum);与 severity 不是一回事,别互相代填
    #[serde(default)]
    priority: Option<String>,
    /// 自由字段,如 {"验收": "...", "复现": "..."}
    #[serde(default)]
    fields: BTreeMap<String, String>,
    /// 引用的条目 ID(finding 必须引用 source)
    #[serde(default)]
    refs: Vec<String>,
    /// void_id 必填:这个编号为什么不该有条目、依据是什么
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl Tool for TrackerTool {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn description(&self) -> String {
        // 三个维度必须在描述里一次说清:实测模型会把 severity 的值填进 priority
        // (`priority: high`),失败一次、再补一个只改字段的提交,纯粹是描述没写全的代价。
        let mut d = format!(
            "Track {}s in the project doc. Actions: list, get(id), add(title, fields), \
             update(id, status/fields), close(id), archive (move terminal entries to the archive \
             file), reorder(order: complete id list — file order IS the user's dev order), \
             repair_reused_id(id), repair_missing_id(id, title, ...) (put back an entry recovered \
             from git history at its original id), void_id(id, reason) (record that an allocated \
             id legitimately has no entry — the ONLY sanctioned way to settle a gap; never \
             fabricate a placeholder entry). status: {}.",
            self.noun,
            self.kind.statuses.join(" → "),
        );
        if let Some(severities) = self.kind.severities {
            d.push_str(&format!(" severity (impact): {}.", severities.join(" | ")));
        }
        if let Some(priorities) = self.kind.priorities {
            d.push_str(&format!(
                " priority (scheduling, a SEPARATE field from severity): {}.",
                priorities.join(" | ")
            ));
        }
        if self.requires_refs.is_some() {
            d.push_str(" `refs` (source IDs) is REQUIRED on add.");
        }
        d
    }

    /// schema 按文档类型动态收窄:action/status/severity/priority 全部给 enum。
    /// 合法取值只写在描述里是不够的——provider 不会据此校验,模型猜错就是一次
    /// 失败调用加一个补丁提交。放进 schema 才有约束力。
    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(TrackerInput)).unwrap();
        let mut actions = vec![
            "list", "get", "add", "update", "close", "archive", "reorder",
        ];
        actions.extend_from_slice(REPAIR_ACTIONS);
        let enums: [(&str, Option<Vec<String>>); 4] = [
            (
                "action",
                Some(actions.iter().map(|s| s.to_string()).collect()),
            ),
            (
                "status",
                Some(self.kind.statuses.iter().map(|s| s.to_string()).collect()),
            ),
            (
                "severity",
                self.kind
                    .severities
                    .map(|values| values.iter().map(|s| s.to_string()).collect()),
            ),
            (
                "priority",
                self.kind
                    .priorities
                    .map(|values| values.iter().map(|s| s.to_string()).collect()),
            ),
        ];
        for (field, values) in enums {
            let Some(values) = values else {
                // 该文档类型没有这个维度:直接从 schema 里删掉,别让模型以为可以填。
                if let Some(properties) = schema
                    .pointer_mut("/properties")
                    .and_then(|v| v.as_object_mut())
                {
                    properties.remove(field);
                }
                continue;
            };
            if let Some(slot) = schema
                .pointer_mut(&format!("/properties/{field}"))
                .and_then(|v| v.as_object_mut())
            {
                // Option<String> 会渲染成 anyOf/type 数组,直接盖掉更稳。
                slot.remove("anyOf");
                slot.remove("type");
                slot.insert("enum".into(), serde_json::json!(values));
            }
        }
        schema
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: TrackerInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let store = DocStore::open(&ctx.project_root, self.kind);
        let mut entries = match store.load() {
            Ok(e) => e,
            Err(e) => {
                return ToolOutput::error(format!("cannot read {}: {e}", store.path.display()))
            }
        };

        // D-140:完整性告警原先只是拼在输出末尾的一行 warn,模型可以完全忽略——
        // 实测 R-104/107/108/110 从活动与归档同时消失后,告警连响 5 个提交无人处理,
        // 最后靠旧副本偶然捞回。改为:发现缺号/重复时**拒绝一切写操作**,读操作照常放行,
        // 迫使当轮先把数据找回来。错误文本直接给出可执行的恢复路径,避免变成死锁。
        if WRITE_ACTIONS.contains(&input.action.as_str())
            && !REPAIR_ACTIONS.contains(&input.action.as_str())
        {
            let issues = store.integrity_issues(&entries);
            if !issues.is_empty() {
                return ToolOutput::error(format!(
                    "REFUSING to write {}: tracker integrity is broken.\n{}\n\
                     Fix it first (reads still work) with one of the repair actions — all three \
                     stay available while the gate is closed:\n\
                     · `{tool} repair_reused_id <id>` — the same id means different things in the \
                     active and archive files;\n\
                     · `{tool} repair_missing_id <id> …` — you recovered the lost entry from git \
                     history and are putting it back at its original id;\n\
                     · `{tool} void_id <id> reason=…` — the id was allocated but legitimately \
                     never carried an entry.\n\
                     Do NOT fabricate a placeholder entry to get past this gate: it silences the \
                     alarm by corrupting the very statistics the gate protects.",
                    self.kind.rel_path,
                    issues.join("\n"),
                    tool = self.tool_name,
                ));
            }
        }

        let mut output = match input.action.as_str() {
            "list" => {
                if entries.is_empty() {
                    return ToolOutput::ok(format!("(no {}s yet)", self.noun));
                }
                let dependency_states = match dependency_states(ctx, self.kind, &entries) {
                    Ok(states) => states,
                    Err(e) => {
                        return ToolOutput::error(format!(
                            "cannot read scheduler dependencies: {e}"
                        ))
                    }
                };
                let scheduled = schedule_entries(&entries, &dependency_states);
                let mut lines: Vec<String> = scheduled
                    .iter()
                    .map(|(entry, reasons)| render_scheduled_line(entry, reasons))
                    .collect();
                // 饥饿保护:一条可执行都没有是队列的异常状态,不是"没活干"。不加这条横幅时
                // agent 只看到满屏 [blocked:...] 就会判定无可推进项并停住,而阻塞理由多半是
                // 它自己历轮写下的"需先确认方案"(D-163)。
                if scheduled.iter().all(|(_, reasons)| !reasons.is_empty()) {
                    lines.insert(0, deadlock_banner(scheduled.len(), self.noun));
                }
                ToolOutput::ok(lines.join("\n"))
            }
            "get" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required for get");
                };
                match entries.iter().find(|e| &e.id == id) {
                    Some(e) => ToolOutput::ok(render_full(e)),
                    // 已归档条目仍可读:回落到 archive 文件(只读,不可 update)。
                    None => match store
                        .load_archive()
                        .ok()
                        .and_then(|arch| arch.into_iter().find(|e| &e.id == id))
                    {
                        Some(e) => ToolOutput::ok(format!("{} (archived)", render_full(&e))),
                        None => ToolOutput::error(unknown_id(id, &entries)),
                    },
                }
            }
            "repair_reused_id" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required for repair_reused_id");
                };
                match store.repair_reused_archived_id(id) {
                    Ok(new_id) => ToolOutput::ok(format!(
                        "repaired reused ID: archived {id} → {new_id}; active {id} kept unchanged. Commit `{}` and its archive file together.",
                        self.kind.rel_path,
                    )),
                    Err(e) => ToolOutput::error(format!("repair_reused_id failed: {e}")),
                }
            }
            // 补回从 git 历史里捞回来的条目:只允许补真空洞,并插回原编号位置。
            "repair_missing_id" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required for repair_missing_id");
                };
                let Some(title) = input.title.as_deref().filter(|t| !t.trim().is_empty()) else {
                    return ToolOutput::error(
                        "`title` is required for repair_missing_id — restore the entry's real \
                         title from git history, do not invent one",
                    );
                };
                if let Some(sev_err) = self.check_severity(&input.severity) {
                    return ToolOutput::error(sev_err);
                }
                if let Some(priority_err) = self.check_priority(&input.priority) {
                    return ToolOutput::error(priority_err);
                }
                if let Some(tag_err) = self.check_tag(&input.fields) {
                    return ToolOutput::error(tag_err);
                }
                let mut fields: Vec<(String, String)> = input.fields.into_iter().collect();
                if !input.refs.is_empty() {
                    fields.push(("refs".into(), input.refs.join(" ")));
                }
                if let Some(priority) = input.priority {
                    fields.push(("优先级".into(), priority));
                }
                let status = input
                    .status
                    .clone()
                    .unwrap_or_else(|| self.kind.statuses[0].to_string());
                if !self.kind.statuses.contains(&status.as_str()) {
                    return ToolOutput::error(format!(
                        "unknown status `{status}`; valid: {}",
                        self.kind.statuses.join(" | ")
                    ));
                }
                let entry = Entry {
                    id: id.clone(),
                    title: title.trim().to_string(),
                    status,
                    severity: if self.kind.severities.is_some() {
                        input.severity
                    } else {
                        None
                    },
                    fields,
                };
                match store.restore_entry(entry) {
                    Ok(()) => ToolOutput::ok(format!(
                        "restored {id} into {} at its original position. Verify it against the \
                         git-history version you recovered before committing.",
                        self.kind.rel_path,
                    )),
                    Err(e) => ToolOutput::error(format!("repair_missing_id failed: {e}")),
                }
            }
            // 主动注销一个编号:唯一合法的"缺号交代"通道,理由必填、留档可审计。
            "void_id" => {
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
                        self.kind.rel_path,
                    )),
                    Err(e) => ToolOutput::error(format!("void_id failed: {e}")),
                }
            }
            "archive" => match store.archive_terminal() {
                Ok(moved) if moved.is_empty() => {
                    ToolOutput::ok("nothing to archive (no terminal entries)")
                }
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
                        self.noun,
                        moved.join(", "),
                        store.archive_file().display(),
                        self.kind.rel_path,
                    ))
                }
                Err(e) => ToolOutput::error(format!("archive failed: {e}")),
            },
            "add" => {
                let Some(title) = input.title.as_deref().filter(|t| !t.trim().is_empty()) else {
                    return ToolOutput::error("`title` is required for add");
                };
                if let Some(sev_err) = self.check_severity(&input.severity) {
                    return ToolOutput::error(sev_err);
                }
                if let Some(priority_err) = self.check_priority(&input.priority) {
                    return ToolOutput::error(priority_err);
                }
                if let Some(tag_err) = self.check_tag(&input.fields) {
                    return ToolOutput::error(tag_err);
                }
                if let Err(e) = self.check_refs(ctx, &input.refs, true) {
                    return ToolOutput::error(e);
                }
                let id = store.next_id(&entries);
                let mut fields: Vec<(String, String)> = input.fields.into_iter().collect();
                if !input.refs.is_empty() {
                    fields.push(("refs".into(), input.refs.join(" ")));
                }
                if let Some(priority) = input.priority {
                    fields.push(("优先级".into(), priority));
                }
                let severity = input
                    .severity
                    .or_else(|| self.kind.severities.map(|s| s[s.len() / 2].to_string()));
                entries.push(Entry {
                    id: id.clone(),
                    title: title.trim().to_string(),
                    status: self.kind.statuses[0].to_string(),
                    severity: if self.kind.severities.is_some() {
                        severity
                    } else {
                        None
                    },
                    fields,
                });
                if let Err(e) = store.save(&entries) {
                    return ToolOutput::error(format!(
                        "cannot write {}: {e}",
                        store.path.display()
                    ));
                }
                ToolOutput::ok(format!("added {id} [{}] {title}", self.kind.statuses[0]))
            }
            "update" | "close" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required");
                };
                let Some(pos) = entries.iter().position(|e| &e.id == id) else {
                    return ToolOutput::error(unknown_id(id, &entries));
                };
                if let Some(sev_err) = self.check_severity(&input.severity) {
                    return ToolOutput::error(sev_err);
                }
                if let Some(priority_err) = self.check_priority(&input.priority) {
                    return ToolOutput::error(priority_err);
                }
                if let Some(tag_err) = self.check_tag(&input.fields) {
                    return ToolOutput::error(tag_err);
                }
                if let Err(e) = self.check_refs(ctx, &input.refs, false) {
                    return ToolOutput::error(e);
                }
                let target_status = if input.action == "close" {
                    // 批次没走完不能关:格子是给人看进度的,关闭时还剩空格,要么是漏了批次,
                    // 要么是总数当初估多了——两种都要说清楚,不能默默把空格留在那儿。
                    let merged = {
                        let mut probe = entries[pos].clone();
                        for (key, value) in &input.fields {
                            match probe.fields.iter_mut().find(|(k, _)| k == key) {
                                Some((_, slot)) => *slot = value.clone(),
                                None => probe.fields.push((key.clone(), value.clone())),
                            }
                        }
                        probe
                    };
                    let derived_done = crate::git_batches::completed_batches(&ctx.project_root, id)
                        .ok()
                        .filter(|done| *done > 0);
                    if let (Some((declared_done, declared_total)), Some(derived_done)) = (
                        crate::docstore::declared_batch_progress(&merged),
                        derived_done,
                    ) {
                        if declared_done != derived_done {
                            return ToolOutput::error(format!(
                                "{id} 的手写批次是 {declared_done}/{declared_total},但 Git 提交历史标记数为 {derived_done};请先核对并更新批次字段后再关闭。"
                            ));
                        }
                    }
                    let (done, total) =
                        crate::docstore::batch_progress_with_derived_done(&merged, derived_done);
                    if total > 1 && done < total {
                        return ToolOutput::error(format!(
                            "{id} 批次未走完({done}/{total}),不能关闭。真做完了就把总数改成实际批数\
                             (`批次: {done}/{done}`——当初估多了是正常的,改它比留着空格诚实);\
                             还有批次没做就先做完再关。"
                        ));
                    }
                    // 关闭前必须先收尾该条目名下的测试记录:一条挂着的 running 与"根本没跑"
                    // 无法区分,带着它关闭等于把未验证当成已验证入账。
                    let unclosed = crate::test_record::unclosed_running_for(&ctx.project_root, id);
                    if !unclosed.is_empty() {
                        return ToolOutput::error(format!(
                            "{id} 名下还有 {} 条 running 测试记录没收尾,不能关闭:\n{}\n\
                             跑完就用 test_record 带上对应 id 记终态(passed/failed/skipped);\
                             确实不跑了记 skipped 并写明原因。",
                            unclosed.len(),
                            unclosed
                                .iter()
                                .map(|(rid, title)| format!("  - {rid} {title}"))
                                .collect::<Vec<_>>()
                                .join("\n")
                        ));
                    }
                    let status = input
                        .status
                        .clone()
                        .unwrap_or_else(|| self.kind.terminal[0].to_string());
                    if !self.kind.terminal.contains(&status.as_str()) {
                        return ToolOutput::error(format!(
                            "close target must be terminal: {}",
                            self.kind.terminal.join(" | ")
                        ));
                    }
                    Some(status)
                } else {
                    input.status.clone()
                };
                let entry = &mut entries[pos];
                if let Some(status) = target_status {
                    if let Err(e) = store.transition_allowed(&entry.status, &status) {
                        return ToolOutput::error(e);
                    }
                    entry.status = status;
                }
                if let Some(title) = input.title.filter(|t| !t.trim().is_empty()) {
                    entry.title = title.trim().to_string();
                }
                if input.severity.is_some() && self.kind.severities.is_some() {
                    entry.severity = input.severity;
                }
                if let Some(priority) = input.priority {
                    match entry
                        .fields
                        .iter_mut()
                        .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
                    {
                        Some((_, value)) => *value = priority,
                        None => entry.fields.push(("优先级".into(), priority)),
                    }
                }
                for (key, value) in input.fields {
                    match entry.fields.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, slot)) => *slot = value,
                        None => entry.fields.push((key, value)),
                    }
                }
                if !input.refs.is_empty() {
                    let joined = input.refs.join(" ");
                    match entry.fields.iter_mut().find(|(k, _)| k == "refs") {
                        Some((_, slot)) => *slot = joined,
                        None => entry.fields.push(("refs".into(), joined)),
                    }
                }
                let line = render_line(&entries[pos]);
                if let Err(e) = store.save(&entries) {
                    return ToolOutput::error(format!(
                        "cannot write {}: {e}",
                        store.path.display()
                    ));
                }
                ToolOutput::ok(format!("updated: {line}"))
            }
            // R-054:整表重排(文件顺序 = 开发顺序)。要求 order 是现有条目的完整置换,
            // 缺一多一都拒绝——引擎整读整写,天然与并发的状态更新互斥。
            "reorder" => {
                if input.order.is_empty() {
                    return ToolOutput::error("`order` (complete id list) is required for reorder");
                }
                let mut seen = std::collections::HashSet::new();
                for id in &input.order {
                    if !seen.insert(id.as_str()) {
                        return ToolOutput::error(format!("duplicate id `{id}` in order"));
                    }
                    if !entries.iter().any(|e| &e.id == id) {
                        return ToolOutput::error(unknown_id(id, &entries));
                    }
                }
                if input.order.len() != entries.len() {
                    let missing: Vec<&str> = entries
                        .iter()
                        .filter(|e| !input.order.iter().any(|id| id == &e.id))
                        .map(|e| e.id.as_str())
                        .collect();
                    return ToolOutput::error(format!(
                        "order must cover ALL {} entries; missing: {}",
                        entries.len(),
                        missing.join(", ")
                    ));
                }
                let mut reordered = Vec::with_capacity(entries.len());
                for id in &input.order {
                    if let Some(pos) = entries.iter().position(|e| &e.id == id) {
                        reordered.push(entries.remove(pos));
                    }
                }
                if let Err(e) = store.save(&reordered) {
                    return ToolOutput::error(format!(
                        "cannot write {}: {e}",
                        store.path.display()
                    ));
                }
                ToolOutput::ok(format!(
                    "reordered {} {}s: {}",
                    reordered.len(),
                    self.noun,
                    input.order.join(" → ")
                ))
            }
            other => ToolOutput::error(format!(
                "unknown action `{other}`; valid: list | get | add | update | close | archive | \
                 reorder | {}",
                REPAIR_ACTIONS.join(" | ")
            )),
        };
        // D-112 门禁:每次调用后对活动∪归档做缺号/重复检测,数据丢失立刻可见,
        // 而不是等 requirements 的依赖引用悬空才被发现。
        if !output.is_error {
            if let Ok(current) = store.load() {
                let issues = store.integrity_issues(&current);
                if !issues.is_empty() {
                    output.content.push_str(&format!(
                        "\n⚠ tracker integrity ({}): {}",
                        self.kind.rel_path,
                        issues.join("; ")
                    ));
                }
            }
        }
        output
    }
}

impl TrackerTool {
    fn check_severity(&self, severity: &Option<String>) -> Option<String> {
        let (Some(sev), Some(valid)) = (severity.as_deref(), self.kind.severities) else {
            return None;
        };
        if valid.contains(&sev) {
            None
        } else {
            Some(format!(
                "invalid severity `{sev}`; valid: {}",
                valid.join(" | ")
            ))
        }
    }

    fn check_priority(&self, priority: &Option<String>) -> Option<String> {
        let (Some(value), Some(valid)) = (priority.as_deref(), self.kind.priorities) else {
            return None;
        };
        if valid.contains(&value) {
            None
        } else {
            Some(format!(
                "invalid priority `{value}`; valid: {}",
                valid.join(" | ")
            ))
        }
    }

    /// 标签受控词表校验(R-112):「标签:」值必须命中 conventions §1.35 词表,
    /// 词表外拒绝并提示合法值。fields 键兼容 标签/tags/tag,值按空白/逗号拆分逐词校验。
    fn check_tag(&self, fields: &BTreeMap<String, String>) -> Option<String> {
        let Some(valid) = self.kind.tags else {
            return None;
        };
        let Some(value) = fields.iter().find(|(key, _)| {
            **key == "标签" || key.eq_ignore_ascii_case("tags") || key.eq_ignore_ascii_case("tag")
        }) else {
            return None;
        };
        let bad: Vec<&str> = value
            .1
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::trim)
            .filter(|t| !t.is_empty() && !valid.contains(t))
            .collect();
        if bad.is_empty() {
            None
        } else {
            Some(format!(
                "invalid tag `{}`; valid: {}",
                bad.join(" "),
                valid.join(" | ")
            ))
        }
    }

    fn check_refs(&self, ctx: &ToolCtx, refs: &[String], adding: bool) -> Result<(), String> {
        let Some(ref_kind) = self.requires_refs else {
            return Ok(());
        };
        if refs.is_empty() {
            if adding {
                let available = DocStore::open(&ctx.project_root, ref_kind)
                    .load()
                    .map(|entries| {
                        entries
                            .iter()
                            .map(render_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                return Err(format!(
                    "every {} MUST cite at least one source via `refs`. Existing sources:\n{}",
                    self.noun,
                    if available.is_empty() {
                        "(none — record a source first)"
                    } else {
                        &available
                    },
                ));
            }
            return Ok(());
        }
        let existing = DocStore::open(&ctx.project_root, ref_kind)
            .load()
            .map_err(|e| e.to_string())?;
        for id in refs {
            if !existing.iter().any(|e| &e.id == id) {
                return Err(format!(
                    "ref `{id}` does not exist. {}",
                    unknown_id(id, &existing)
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ScheduledEntry {
    pub entry: Entry,
    pub block_reasons: Vec<String>,
}

/// 为桌面端文档快照提供与 req/defect list 相同的阻塞判断和稳定后置顺序。
pub fn schedule_for_display(
    ctx: &ToolCtx,
    kind: &'static DocKind,
    entries: &[Entry],
) -> Result<Vec<ScheduledEntry>, String> {
    let states = dependency_states(ctx, kind, entries)?;
    let scheduled = schedule_entries(entries, &states);
    Ok(scheduled
        .into_iter()
        .map(|(entry, block_reasons)| ScheduledEntry {
            entry: entry.clone(),
            block_reasons,
        })
        .collect())
}

#[derive(Default)]
struct DependencyStates {
    terminal: BTreeMap<String, bool>,
    deps: BTreeMap<String, Vec<String>>,
}

impl DependencyStates {
    fn get(&self, id: &str) -> Option<&bool> {
        self.terminal.get(id)
    }

    fn is_terminal(&self, id: &str) -> bool {
        self.terminal.get(id).copied().unwrap_or(false)
    }

    /// 沿**未完成**依赖从 start 出发,能走回 start 就返回环路径。已归档依赖不构成
    /// 阻塞,自然也不参与成环。返回的路径首尾都是 start,方便直接打印。
    fn cycle_from(&self, start: &str) -> Option<Vec<String>> {
        let mut path = vec![start.to_string()];
        let mut visited = BTreeSet::new();
        self.walk(start, start, &mut path, &mut visited)
            .then_some(path)
    }

    fn walk(
        &self,
        node: &str,
        start: &str,
        path: &mut Vec<String>,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        let Some(deps) = self.deps.get(node) else {
            return false;
        };
        for dep in deps {
            if self.is_terminal(dep) {
                continue;
            }
            path.push(dep.clone());
            if dep == start {
                return true;
            }
            if visited.insert(dep.clone()) && self.walk(dep, start, path, visited) {
                return true;
            }
            path.pop();
        }
        false
    }
}

fn dependency_states(
    ctx: &ToolCtx,
    current_kind: &DocKind,
    current_entries: &[Entry],
) -> Result<DependencyStates, String> {
    let mut states = DependencyStates::default();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let active = if kind.rel_path == current_kind.rel_path {
            current_entries.to_vec()
        } else {
            DocStore::open(&ctx.project_root, kind)
                .load()
                .map_err(|e| format!("{}: {e}", kind.rel_path))?
        };
        let archived = DocStore::open(&ctx.project_root, kind)
            .load_archive()
            .map_err(|e| format!("{} archive: {e}", kind.rel_path))?;
        for entry in active.into_iter().chain(archived) {
            let deps: Vec<String> = entry
                .fields
                .iter()
                .filter(|(key, _)| is_dependency_key(key))
                .flat_map(|(_, value)| tracker_ids(value))
                .collect();
            states.terminal.insert(
                entry.id.clone(),
                kind.terminal.contains(&entry.status.as_str()),
            );
            if !deps.is_empty() {
                states.deps.insert(entry.id, deps);
            }
        }
    }
    Ok(states)
}

/// 反向依赖图(R-111 验收②「条目详情含正反向链接」):id → 依赖它的条目 id 列表。
/// 与 dependency_states 共用同一份「依赖:」字段解析(依赖=阻塞关系,refs 不在此列),
/// 供桌面端 docs_snapshot 输出反向链接与文档页依赖视图使用。
/// 返回 (正向图 id→deps, 反向图 id→dependents)。
pub fn dependents_map(
    ctx: &ToolCtx,
    current_kind: &DocKind,
    current_entries: &[Entry],
) -> Result<
    (
        std::collections::BTreeMap<String, Vec<String>>,
        std::collections::BTreeMap<String, Vec<String>>,
    ),
    String,
> {
    let states = dependency_states(ctx, current_kind, current_entries)?;
    let mut dependents: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (from, deps) in &states.deps {
        for dep in deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .push(from.clone());
        }
    }
    let mut deps_map: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (from, deps) in &states.deps {
        deps_map.insert(from.clone(), deps.clone());
    }
    for list in dependents.values_mut().chain(deps_map.values_mut()) {
        list.sort();
        list.dedup();
    }
    Ok((deps_map, dependents))
}


fn schedule_entries<'a>(
    entries: &'a [Entry],
    states: &DependencyStates,
) -> Vec<(&'a Entry, Vec<String>)> {
    // 稳定分区:不改写 Markdown,只在取活输出中把当前不可执行项后置；
    // 因此阻塞解除后会自动回到原文档顺序。
    let mut executable = Vec::with_capacity(entries.len());
    let mut blocked = Vec::new();
    for entry in entries {
        let reasons = block_reasons(entry, states);
        if reasons.is_empty() {
            executable.push((entry, reasons));
        } else {
            blocked.push((entry, reasons));
        }
    }
    executable.extend(blocked);
    executable
}

fn block_reasons(entry: &Entry, states: &DependencyStates) -> Vec<String> {
    let mut reasons = Vec::new();
    // 环上的条目永远等不到依赖完成。只报"未完成依赖"会让 agent 一轮轮空等一个
    // 不可能到来的前置,所以直接点出环并要求断边(D-163)。
    let cycle = states.cycle_from(&entry.id);
    for (key, value) in &entry.fields {
        if is_blocker_key(key) && is_present_blocker(value) {
            reasons.push(format!("阻塞字段: {}", value.trim()));
        }
        if is_dependency_key(key) && cycle.is_none() {
            for id in tracker_ids(value) {
                match states.get(&id) {
                    Some(true) => {}
                    Some(false) => reasons.push(format!("未完成依赖: {id}")),
                    None => reasons.push(format!("依赖不存在: {id}")),
                }
            }
        }
        if is_stage_key(key) && is_deferred_stage(value) {
            reasons.push(format!("阶段门槛: {}", value.trim()));
        }
    }
    if let Some(path) = cycle {
        reasons.push(format!(
            "循环依赖: {} —— 环上没有条目能先完成,必须断掉其中一条边(把不成立的依赖移入 refs)",
            path.join(" → ")
        ));
    }
    reasons
}

/// 全部条目被判阻塞时置顶的横幅。措辞是刻意的:先要求复核既有阻塞(多数是自记的),
/// 复核后仍全阻塞才允许升级给用户,并且必须点名缺哪个决策——"无可执行条目"不是合格收尾。
fn deadlock_banner(total: usize, noun: &str) -> String {
    format!(
        "[调度死锁] {total} 条{noun}全部带阻塞标记,可执行数为 0。这是队列异常,不是没活干。\n\
         1. 先逐条复核阻塞是否仍成立:依赖已归档、条件已满足、方案其实早已确认的,\
         清空该条的「阻塞」字段再取活——自己历轮写下的「需先确认方案」不算外部阻塞。\n\
         2. 复核后仍全阻塞,才回复用户,并逐条点名缺的是哪一个具体决策。\n\
         禁止以「没有可执行条目」「本轮停止」之类的纯文本收尾。"
    )
}

fn render_scheduled_line(entry: &Entry, reasons: &[String]) -> String {
    let line = render_line(entry);
    if reasons.is_empty() {
        line
    } else {
        format!("{line} [blocked: {}]", reasons.join("；"))
    }
}

fn is_blocker_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.contains("阻塞") || matches!(lower.as_str(), "blocked" | "blocker" | "blocking")
}

fn is_dependency_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.trim() == "依赖" || matches!(lower.as_str(), "dependency" | "dependencies" | "depends_on")
}

fn is_stage_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.trim() == "阶段" || matches!(lower.as_str(), "stage" | "phase")
}

fn is_present_blocker(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "无" | "否" | "none" | "no" | "false" | "未阻塞" | "暂无"
        )
}

fn is_deferred_stage(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    value.contains("后")
        || value.contains("以后")
        || lower.contains("after")
        || lower.contains("later")
}

/// 从自由文本中提取 R-001/D-002 形式的追踪 ID,兼容中文标点和说明文字。
fn tracker_ids(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut ids = Vec::new();
    let mut i = 0;
    while i + 2 < chars.len() {
        let prefix = chars[i].to_ascii_uppercase();
        if !matches!(prefix, 'R' | 'D') || chars[i + 1] != '-' || !chars[i + 2].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut end = i + 2;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        let id: String = chars[i..end].iter().collect();
        if !ids.contains(&id) {
            ids.push(id);
        }
        i = end;
    }
    ids
}

fn render_line(e: &Entry) -> String {
    let sev = e
        .severity
        .as_ref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    format!("{} [{}]{sev} {}", e.id, e.status, e.title)
}

fn render_full(e: &Entry) -> String {
    let mut out = render_line(e);
    for (key, value) in &e.fields {
        out.push_str(&format!("\n- {key}: {value}"));
    }
    out
}

fn unknown_id(id: &str, entries: &[Entry]) -> String {
    let known: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    format!(
        "unknown id `{id}`; existing: {}",
        if known.is_empty() {
            "(none)".into()
        } else {
            known.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::TrackerTool;
    use crate::docstore::{DocStore, Entry, GOALS, REQUIREMENTS};
    use kanzei_harness::{Tool, ToolCtx};
    use serde_json::json;
    use std::process::Command;

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.into(),
            title: format!("t-{id}"),
            status: "todo".into(),
            severity: None,
            fields: vec![],
        }
    }

    #[tokio::test]
    async fn 批次没走完不能关闭_改小总数或做完都放行() {
        let dir = std::env::temp_dir().join(format!("kz-batch-close-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("R-001");
        e.status = "doing".into();
        e.fields = vec![
            ("复杂度".into(), "大".into()),
            ("批次".into(), "3/11".into()),
        ];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(out.is_error, "还剩 8 格空着就该拦下来: {}", out.content);
        assert!(out.content.contains("3/11"), "{}", out.content);

        // 当初估多了:把总数改成实际批数即可关闭——比留着空格诚实。
        let out = tool
            .execute(
                json!({"action": "close", "id": "R-001", "fields": {"批次": "3/3"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "总数改成实际批数后应放行: {}", out.content);
        let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(saved[0].status, "done");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn 关闭时拒绝手写批次与_git_提交真源不一致() {
        let dir = std::env::temp_dir().join(format!(
            "kz-batch-git-close-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Kanzei Test"],
            vec![
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                "R-001 B1: first batch",
            ],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap()
                .success());
        }
        let mut e = entry("R-001");
        e.status = "doing".into();
        e.fields = vec![("批次".into(), "2/2".into())];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(
            out.is_error,
            "Git 与字段不一致必须拒绝关闭: {}",
            out.content
        );
        assert!(
            out.content.contains("Git 提交历史标记数为 1"),
            "{}",
            out.content
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn reorder_rewrites_file_order_and_rejects_partial() {
        let dir = std::env::temp_dir().join(format!("kz-reorder-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store
            .save(&[entry("R-001"), entry("R-002"), entry("R-003")])
            .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        // 完整置换:成功且文件顺序改变。
        let out = tool
            .execute(
                json!({"action": "reorder", "order": ["R-003", "R-001", "R-002"]}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["R-003", "R-001", "R-002"]);

        // 不完整清单:拒绝且顺序不变。
        let out = tool
            .execute(json!({"action": "reorder", "order": ["R-001"]}), &ctx)
            .await;
        assert!(out.is_error);
        let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["R-003", "R-001", "R-002"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn add_preserves_handwritten_free_text_and_unknown_blocks() {
        let dir = std::env::temp_dir().join(format!("kz-preserve-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let path = dir.join(REQUIREMENTS.rel_path);
        std::fs::write(
            &path,
            "# Requirements\n\n手写说明: 不应被引擎删除\n- 就是个备注\n\n## R-001 已有条目 [todo]\n### 子标题\n```text\n用户代码块\n```\n- 验收: 原字段\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let output = tool
            .execute(
                json!({"action": "add", "title": "新条目"}),
                &ToolCtx::new(dir.clone()),
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        let saved = std::fs::read_to_string(path).unwrap();
        for line in [
            "手写说明: 不应被引擎删除",
            "- 就是个备注",
            "### 子标题",
            "用户代码块",
        ] {
            assert!(saved.contains(line), "missing preserved line: {line}");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn archive_preserves_handwritten_free_text_and_unknown_blocks() {
        let dir = std::env::temp_dir().join(format!("kz-archive-preserve-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let path = dir.join(REQUIREMENTS.rel_path);
        std::fs::write(
            &path,
            "# Requirements\n\n## R-001 已完成条目 [done]\n终态说明: 归档时也不能丢\n- 手写备注\n### 归档子标题\n```text\n归档代码块\n```\n- 验收: 原字段\n\n## R-002 进行中条目 [doing]\n- 验收: 保留在活动文档\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let output = tool
            .execute(json!({"action": "archive"}), &ToolCtx::new(dir.clone()))
            .await;
        assert!(!output.is_error, "{}", output.content);

        let archive =
            std::fs::read_to_string(dir.join(".kanzei/project/requirements-archive.md")).unwrap();
        for line in [
            "终态说明: 归档时也不能丢",
            "- 手写备注",
            "### 归档子标题",
            "归档代码块",
        ] {
            assert!(archive.contains(line), "missing preserved line: {line}");
        }
        let active = std::fs::read_to_string(path).unwrap();
        assert!(!active.contains("终态说明: 归档时也不能丢"));
        assert!(active.contains("## R-002 进行中条目 [doing]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn 完整性破损时拒绝写操作但读操作放行() {
        // D-140:告警是 warn-only 时无人处理,实测缺号连响 5 个提交。
        let dir = std::env::temp_dir().join(format!(
            "kz-integrity-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        // 制造缺号:R-001 与 R-003 存在,R-002 缺失。
        store.save(&[entry("R-001"), entry("R-003")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        // 读操作必须仍然可用——否则模型无法诊断,就成了死锁。
        for action in ["list", "get"] {
            let out = tool
                .execute(json!({"action": action, "id": "R-001"}), &ctx)
                .await;
            assert!(!out.is_error, "读操作不该被拦: {action} -> {}", out.content);
        }
        // 写操作全部被拒,且错误里给出可执行的恢复路径。
        for input in [
            json!({"action": "add", "title": "新条目"}),
            json!({"action": "update", "id": "R-001", "status": "doing"}),
            json!({"action": "close", "id": "R-001"}),
            json!({"action": "archive"}),
            json!({"action": "reorder", "order": ["R-003", "R-001"]}),
        ] {
            let action = input["action"].as_str().unwrap().to_string();
            let out = tool.execute(input, &ctx).await;
            assert!(out.is_error, "写操作必须被拒: {action}");
            assert!(
                out.content.contains("R-002"),
                "要指名缺失的 id: {}",
                out.content
            );
            assert!(
                out.content.contains("git log -S"),
                "要给恢复路径: {}",
                out.content
            );
        }
        // 补齐缺号后写操作恢复正常。
        store
            .save(&[entry("R-001"), entry("R-002"), entry("R-003")])
            .unwrap();
        let out = tool
            .execute(json!({"action": "add", "title": "恢复后可写"}), &ctx)
            .await;
        assert!(!out.is_error, "完整性恢复后应放行: {}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn 复用历史_id_可改号修复且不会丢归档自由内容() {
        let dir = std::env::temp_dir().join(format!(
            "kz-reused-id-repair-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = dir.join(".kanzei/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("goals.md"),
            "# Goals\n\n## G-001 长期目标 [active]\n- 进展: 旧\n\n## G-002 新长期目标 [active]\n- 来源: R-093\n",
        )
        .unwrap();
        std::fs::write(
            project.join("goals-archive.md"),
            "# Goals Archive\n\n## G-002 旧短期目标 [achieved]\n- 验收: 达成即 `goal update G-002 achieved`\n\n手写归档说明 G-002 不能丢\n\n## G-003 另一目标 [achieved]\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "goal",
            noun: "goal",
            kind: &GOALS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let repaired = tool
            .execute(json!({"action": "repair_reused_id", "id": "G-002"}), &ctx)
            .await;
        assert!(!repaired.is_error, "{}", repaired.content);
        assert!(repaired.content.contains("G-004"), "{}", repaired.content);
        let active = std::fs::read_to_string(project.join("goals.md")).unwrap();
        let archive = std::fs::read_to_string(project.join("goals-archive.md")).unwrap();
        assert!(active.contains("## G-002 新长期目标 [active]"));
        assert!(archive.contains("## G-004 旧短期目标 [achieved]"));
        assert!(archive.contains("goal update G-004 achieved"));
        assert!(archive.contains("手写归档说明 G-004 不能丢"));

        let updated = tool
            .execute(
                json!({"action": "update", "id": "G-001", "fields": {"进展": "新"}}),
                &ctx,
            )
            .await;
        assert!(
            !updated.is_error,
            "修复后普通写操作应恢复: {}",
            updated.content
        );
        let store = DocStore::open(&dir, &GOALS);
        let entries = store.load().unwrap();
        assert!(store.integrity_issues(&entries).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-173:缺号有两条**结构化**出路,不必再靠伪造 `[wontfix]` 墓碑骗过门禁。
    #[tokio::test]
    async fn 缺号可注销或补回_两条修复通道都在门禁关闭时可用() {
        let dir = std::env::temp_dir().join(format!(
            "kz-id-ledger-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        // 两个空洞:R-002 其实是撤销的分配,R-003 是真丢了。
        store.save(&[entry("R-001"), entry("R-004")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        // 门禁关闭:普通写被拒,且措辞点名三条修复通道、并禁止伪造墓碑。
        let out = tool
            .execute(json!({"action": "add", "title": "x"}), &ctx)
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("void_id"), "{}", out.content);
        assert!(out.content.contains("repair_missing_id"), "{}", out.content);
        assert!(out.content.contains("fabricate"), "{}", out.content);

        // 注销必须给理由。
        let out = tool
            .execute(json!({"action": "void_id", "id": "R-002"}), &ctx)
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("reason"), "{}", out.content);
        // 不能拿它注销一个还活着的条目。
        let out = tool
            .execute(
                json!({"action": "void_id", "id": "R-001", "reason": "想清掉它"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);

        let out = tool
            .execute(
                json!({"action": "void_id", "id": "R-002", "reason": "分配后当场撤销,git log -S 全历史无此条目"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        // 补回:必须给真实标题,并插回原编号位置。
        let out = tool
            .execute(
                json!({"action": "repair_missing_id", "id": "R-003", "title": "从 git 捞回的条目", "status": "doing"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["R-001", "R-003", "R-004"], "必须插回原位");

        // 两个空洞都交代完 → 完整性恢复,普通写放行,且注销过的号不再被复用。
        assert!(store.integrity_issues(&store.load().unwrap()).is_empty());
        let out = tool
            .execute(json!({"action": "add", "title": "恢复后可写"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("R-005"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 已注销的编号后来又冒出条目 = 账实不符的另一半,同样要报。
    #[test]
    fn 注销后又出现条目会被报为账实不符() {
        let dir = std::env::temp_dir().join(format!(
            "kz-ledger-conflict-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store.save(&[entry("R-001")]).unwrap();
        store.void_id("R-002", "撤销的分配,有据可查").unwrap();
        store.save(&[entry("R-001"), entry("R-002")]).unwrap();
        let issues = store.integrity_issues(&store.load().unwrap());
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("voided"), "{issues:?}");
        assert!(issues[0].contains("R-002"), "{issues:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// priority 与 severity 是两个维度,合法取值必须进 schema 而不是只写在描述里。
    #[test]
    fn schema_gives_real_enums_for_each_document_kind() {
        use crate::docstore::DEFECTS;
        let defects = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        let schema = defects.input_schema();
        assert_eq!(
            schema["properties"]["severity"]["enum"],
            json!(["high", "medium", "low"])
        );
        assert_eq!(
            schema["properties"]["priority"]["enum"],
            json!(["P0", "P1", "P2", "P3"])
        );
        assert_eq!(
            schema["properties"]["status"]["enum"],
            json!(["open", "fixing", "fixed", "wontfix"])
        );
        let actions = schema["properties"]["action"]["enum"].as_array().unwrap();
        for expected in ["list", "add", "void_id", "repair_missing_id"] {
            assert!(
                actions.iter().any(|a| a == expected),
                "action enum 缺 {expected}: {actions:?}"
            );
        }
        let description = defects.description();
        assert!(
            description.contains("severity (impact): high | medium | low"),
            "{description}"
        );
        assert!(description.contains("P0 | P1 | P2 | P3"), "{description}");

        // 需求没有 severity 维度:schema 里根本不该出现这个字段。
        let requirements = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let schema = requirements.input_schema();
        assert!(schema["properties"].get("severity").is_none(), "{schema}");
        assert_eq!(
            schema["properties"]["priority"]["enum"],
            json!(["P0", "P1", "P2", "P3"])
        );
    }

    #[tokio::test]
    async fn integrity_warning_surfaces_in_tool_output() {
        // D-112:缺号(R-002)必须出现在每次成功调用的输出里。
        let dir = std::env::temp_dir().join(format!(
            "kz-tracker-integrity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store.save(&[entry("R-001"), entry("R-003")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let out = tool
            .execute(json!({"action": "list"}), &ToolCtx::new(dir.clone()))
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("tracker integrity"), "{}", out.content);
        assert!(out.content.contains("R-002"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn archive_reports_moved_ids_and_requires_paired_commit() {
        // D-112:归档输出必须列出移动的 ID 并要求两文件同一提交。
        let dir = std::env::temp_dir().join(format!(
            "kz-tracker-archive-msg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut done = entry("R-001");
        done.status = "done".into();
        store.save(&[done, entry("R-002")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let out = tool
            .execute(json!({"action": "archive"}), &ToolCtx::new(dir.clone()))
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("R-001"), "{}", out.content);
        assert!(out.content.contains("SAME commit"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn priority_update_reuses_existing_english_field() {
        let dir = std::env::temp_dir().join(format!("kz-priority-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut item = entry("R-001");
        item.fields.push(("priority".into(), "P2".into()));
        store.save(&[item]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let output = tool
            .execute(
                json!({"action": "update", "id": "R-001", "priority": "P0"}),
                &ToolCtx::new(dir.clone()),
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        let fields = &store.load().unwrap()[0].fields;
        assert_eq!(fields, &[("priority".into(), "P0".into())]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn list_stably_postpones_blocked_entries_and_restores_order() {
        let dir = std::env::temp_dir().join(format!(
            "kz-scheduler-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut blocked = entry("R-001");
        blocked.fields.push(("阻塞".into(), "等待用户确认".into()));
        let mut dependency_blocked = entry("R-003");
        dependency_blocked
            .fields
            .push(("依赖".into(), "R-002".into()));
        let mut stage_blocked = entry("R-004");
        stage_blocked.fields.push(("阶段".into(), "5 后".into()));
        store
            .save(&[blocked, entry("R-002"), dependency_blocked, stage_blocked])
            .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        let lines: Vec<&str> = out.content.lines().collect();
        assert!(lines[0].starts_with("R-002"), "{}", out.content);
        assert!(lines[1].contains("R-001") && lines[1].contains("阻塞字段"));
        assert!(lines[2].contains("R-003") && lines[2].contains("未完成依赖: R-002"));
        assert!(lines[3].contains("R-004") && lines[3].contains("阶段门槛"));

        // 解除两个阻塞后，原文档顺序自动恢复；没有持久化一份临时排序。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"阻塞": ""}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-002", "status": "done"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        let lines: Vec<&str> = out.content.lines().collect();
        assert!(lines[0].starts_with("R-001"), "{}", out.content);
        assert!(lines[1].starts_with("R-002"), "{}", out.content);
        assert!(lines[2].starts_with("R-003"), "{}", out.content);
        assert!(lines[3].contains("R-004") && lines[3].contains("阶段门槛"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn list_banners_deadlock_when_nothing_is_executable() {
        let dir = std::env::temp_dir().join(format!(
            "kz-deadlock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut first = entry("R-001");
        first
            .fields
            .push(("阻塞".into(), "等待用户确认方案".into()));
        let mut second = entry("R-002");
        second.fields.push(("阻塞".into(), "依赖外部服务".into()));
        store.save(&[first, second]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.starts_with("[调度死锁] 2 条requirement"),
            "{}",
            out.content
        );
        // 横幅必须堵死"没有可执行条目"这条收尾话术,否则鞭挞照旧静默停。
        assert!(
            out.content.contains("禁止以「没有可执行条目」"),
            "{}",
            out.content
        );
        assert!(out.content.contains("R-001") && out.content.contains("R-002"));

        // 只要有一条恢复可执行,横幅就消失——它只在真死锁时出现,不构成日常噪音。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"阻塞": ""}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(!out.content.contains("[调度死锁]"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn list_names_dependency_cycles_instead_of_endless_waiting() {
        let dir = std::env::temp_dir().join(format!(
            "kz-cycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut a = entry("R-001");
        a.fields.push(("依赖".into(), "R-002".into()));
        let mut b = entry("R-002");
        b.fields.push(("依赖".into(), "R-001".into()));
        // 环外的普通未完成依赖仍应照旧报"未完成依赖",不被环检测吞掉。
        let mut c = entry("R-003");
        c.fields.push(("依赖".into(), "R-004".into()));
        store.save(&[a, b, c, entry("R-004")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        let cycle_line = out
            .content
            .lines()
            .find(|l| l.starts_with("R-001"))
            .unwrap_or_default();
        assert!(
            cycle_line.contains("循环依赖: R-001 → R-002 → R-001"),
            "{}",
            out.content
        );
        assert!(cycle_line.contains("断掉其中一条边"), "{}", out.content);
        let plain = out
            .content
            .lines()
            .find(|l| l.starts_with("R-003"))
            .unwrap_or_default();
        assert!(plain.contains("未完成依赖: R-004"), "{}", out.content);
        assert!(!plain.contains("循环依赖"), "{}", out.content);

        // 断边后环消失,两条都回到可执行。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-002", "fields": {"依赖": ""}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(!out.content.contains("循环依赖"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn dependents_map_reports_forward_and_reverse_links() {
        let dir = std::env::temp_dir().join(format!(
            "kz-dependents-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut a = entry("R-001");
        a.fields.push(("依赖".into(), "R-002 R-003".into()));
        let b = entry("R-002");
        let mut c = entry("R-003");
        c.fields.push(("依赖".into(), "R-002".into()));
        store.save(&[a, b, c]).unwrap();
        let ctx = ToolCtx::new(dir.clone());
        let loaded = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();

        let (deps, dependents) = super::dependents_map(&ctx, &REQUIREMENTS, &loaded).unwrap();
        // 正向:R-001 → [R-002, R-003],R-003 → [R-002],R-002 无依赖。
        assert_eq!(deps.get("R-001").unwrap(), &vec!["R-002".to_string(), "R-003".to_string()]);
        assert_eq!(deps.get("R-003").unwrap(), &vec!["R-002".to_string()]);
        assert!(deps.get("R-002").is_none());
        // 反向:R-002 被 R-001 与 R-003 依赖;R-003 只被 R-001 依赖;R-001 无人依赖。
        assert_eq!(
            dependents.get("R-002").unwrap(),
            &vec!["R-001".to_string(), "R-003".to_string()]
        );
        assert_eq!(dependents.get("R-003").unwrap(), &vec!["R-001".to_string()]);
        assert!(dependents.get("R-001").is_none());

        // 去重:同一依赖写两遍只出现一次。
        let mut dup = entry("R-004");
        dup.fields.push(("依赖".into(), "R-002 R-002".into()));
        let mut a2 = entry("R-001");
        a2.fields.push(("依赖".into(), "R-002 R-003".into()));
        let mut c2 = entry("R-003");
        c2.fields.push(("依赖".into(), "R-002".into()));
        store.save(&[a2, entry("R-002"), c2, dup]).unwrap();
        let loaded = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        let (_, dependents) = super::dependents_map(&ctx, &REQUIREMENTS, &loaded).unwrap();
        assert_eq!(
            dependents.get("R-002").unwrap(),
            &vec!["R-001".to_string(), "R-003".to_string(), "R-004".to_string()]
        );
        std::fs::remove_dir_all(dir).ok();
    }

    // R-112:标签受控词表校验——add/update 时词表外拒绝并提示合法值,词表内放行;
    // 无标签字段或不参与分类的文档不受影响。
    #[tokio::test]
    async fn tag_validation_rejects_out_of_vocabulary_on_add_and_update() {
        let dir = std::env::temp_dir().join(format!(
            "kz-tag-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store.save(&[entry("R-001")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        // add:词表外标签被拒,错误里带合法词表。
        let out = tool
            .execute(
                json!({"action": "add", "title": "t", "fields": {"标签": "杂项"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("invalid tag `杂项`"), "{}", out.content);
        assert!(out.content.contains("核心"), "{}", out.content);
        assert!(out.content.contains("后端"), "{}", out.content);

        // add:词表内标签放行。
        let out = tool
            .execute(
                json!({"action": "add", "title": "t", "fields": {"标签": "前端"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);

        // update:词表外标签被拒。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"标签": "网络"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("invalid tag `网络`"), "{}", out.content);

        // update:多值含一个非法词也被拒(按空白/逗号拆分逐词校验)。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"标签": "核心 杂项"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("invalid tag `杂项`"), "{}", out.content);

        // update:词表内多值放行。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"标签": "核心 流程"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);

        // 无标签字段的更新不受影响(close 走 fields 合并,不应误伤)。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"进展": "x"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn tag_validation_skips_documents_without_vocabulary() {
        let dir = std::env::temp_dir().join(format!(
            "kz-tag-goal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &GOALS);
        store.save(&[entry("G-001")]).unwrap();
        let tool = TrackerTool {
            tool_name: "goal",
            noun: "goal",
            kind: &GOALS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());
        // 无词表的文档:任意标签值放行,不受校验约束。
        let out = tool
            .execute(
                json!({"action": "update", "id": "G-001", "fields": {"标签": "任意值"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }
}
