//! 通用追踪工具:req / defect / source / finding 共用一套 CRUD。
//! 硬门禁:ID 引擎分配、状态机受限、格式引擎序列化、引用必须存在——模型只提供字段值。

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};

type DependencyMap = BTreeMap<String, Vec<String>>;

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

/// D-241:fixing 推不动时退回 open 的合法动作。与 repair_* 同族但不修完整性,
/// 所以仍走完整性门禁(拒绝在破损状态下 reopen)。
const REOPEN_ACTION: &str = "reopen";

/// D-331:归档终态纠错(fixed↔wontfix)。只改终态、强制 reason、条目保持归档,
/// 写回时清除标题里的跨 DocKind 状态标记污染。走完整性门禁(破损文档不接受纠错)。
const FIX_TERMINAL_ACTION: &str = "fix_terminal";

const WRITE_ACTIONS: &[&str] = &[
    "add",
    "update",
    "close",
    "archive",
    "reorder",
    REOPEN_ACTION,
    FIX_TERMINAL_ACTION,
    "raw_delete",
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
    /// raw_delete 必填:raw_lines 输出里的 [n] 序号(要删除的第 n 条游离行)
    #[serde(default)]
    ordinal: Option<usize>,
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
             raw_lines(id) (list this entry's stray non-addressable lines as [n] markers), \
             raw_delete(id, ordinal=n) (delete exactly that stray line; all fields and other \
             lines stay byte-identical), \
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
            "list",
            "get",
            "raw_lines",
            "add",
            "update",
            "close",
            "archive",
            "reorder",
            REOPEN_ACTION,
            "raw_delete",
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

    /// tracker 是一个工具承载读写两类动作。F11 只关闭分支线上的写入,所以资源必须
    /// 带上动作类别;继续使用默认 `*` 会让任意 deny 把 list/get 也一并摘掉。
    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let access = if WRITE_ACTIONS.contains(&action) {
            "write"
        } else {
            "read"
        };
        vec![format!("{access}:{action}")]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: TrackerInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        if input.action == "list"
            && matches!(self.kind.prefix, "R" | "D")
            && !matches!(
                input.reason.as_deref(),
                Some("deduplicate_registration" | "human_cli")
            )
        {
            return ToolOutput::error(
                "完整 requirement/defect 队列不是执行期上下文。取活请调用 `work next`；\
                 只有登记前查重可用 reason=deduplicate_registration 显式读取。",
            );
        }
        let store = DocStore::open(&ctx.project_root, self.kind);
        // 需求/缺陷的任何写都先拿双队列选择锁，再拿各自文档锁。`work claim`
        // 使用相同顺序，因此“读两队列 → 判 WIP → 写一个队列”与普通 close/reopen
        // 不会交错出两个 WIP。其余 tracker 文档不参与取活，不承担这笔串行成本。
        let work_selection_path = ctx.project_root.join(".kanzei/project/work-selection");
        let _work_selection_lock = if WRITE_ACTIONS.contains(&input.action.as_str())
            && matches!(self.kind.prefix, "R" | "D")
        {
            match crate::atomic_file::lock_exclusive(&work_selection_path) {
                Ok(lock) => Some(lock),
                Err(error) => {
                    return ToolOutput::error(format!(
                        "cannot lock requirement/defect work selection: {error}"
                    ))
                }
            }
        } else {
            None
        };
        // R-138:写事务的锁必须罩住 **load … next_id … save** 整段,不能只锁 save。
        // 两个进程(kzapp / kz CLI / 自举循环)各自 load 到同一份条目、各自算出
        // 同一个 next_id、再各自整文件写回——后写的那个把前一个的新条目连同 ID
        // 一起覆盖掉。只在 save 里加锁挡不住这个:两次 save 本来就不重叠,
        // 丢失发生在它们各自的读与写之间。读动作(list/get)不取锁,照常并行。
        let _write_lock = if WRITE_ACTIONS.contains(&input.action.as_str()) {
            match store.lock() {
                Ok(lock) => Some(lock),
                Err(e) => {
                    return ToolOutput::error(format!(
                        "cannot lock {} for writing: {e}",
                        store.path.display()
                    ))
                }
            }
        } else {
            None
        };
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
                    return ToolOutput::ok(
                        serde_json::json!({
                            "schema_version": 1,
                            "kind": self.noun,
                            "deadlocked": false,
                            "entries": [],
                        })
                        .to_string(),
                    );
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
                let items: Vec<serde_json::Value> = scheduled
                    .iter()
                    .map(|(entry, reasons)| structured_entry(entry, reasons, false))
                    .collect();
                // 饥饿保护:一条可执行都没有是队列的异常状态,不是"没活干"。不加这条横幅时
                // agent 只看到满屏 [blocked:...] 就会判定无可推进项并停住,而阻塞理由多半是
                // 它自己历轮写下的"需先确认方案"(D-163)。
                let deadlocked = scheduled.iter().all(|(_, reasons)| !reasons.is_empty());
                ToolOutput::ok(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "kind": self.noun,
                        "deadlocked": deadlocked,
                        "deadlock_guidance": deadlocked.then(|| deadlock_banner(scheduled.len(), self.noun)),
                        "entries": items,
                    }))
                    .unwrap(),
                )
            }
            "get" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required for get");
                };
                match entries.iter().find(|e| &e.id == id) {
                    Some(e) => ToolOutput::ok(
                        serde_json::to_string_pretty(&structured_entry(e, &[], false)).unwrap(),
                    ),
                    // 已归档条目仍可读:回落到 archive 文件(只读,不可 update)。
                    None => match store
                        .load_archive()
                        .ok()
                        .and_then(|arch| arch.into_iter().find(|e| &e.id == id))
                    {
                        Some(e) => ToolOutput::ok(
                            serde_json::to_string_pretty(&structured_entry(&e, &[], true)).unwrap(),
                        ),
                        None => ToolOutput::error(unknown_id(id, &entries)),
                    },
                }
            }
            // R-201:列出条目的游离行——模板里不可寻址的 Raw 行,update 永远删不到。
            // 每条给 [n] 序号 + 原文,序号即 raw_delete 的键;空行显式标出避免看不见。
            "raw_lines" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required for raw_lines");
                };
                if !entries.iter().any(|e| &e.id == id) {
                    return ToolOutput::error(unknown_id(id, &entries));
                }
                let raws = store.raw_lines(id);
                if raws.is_empty() {
                    return ToolOutput::ok(format!("{id} 没有游离行(条目内每一行都是可寻址字段)"));
                }
                let rendered = raws
                    .iter()
                    .map(|raw| {
                        let body = if raw.text.trim().is_empty() {
                            "(空行)".to_string()
                        } else {
                            raw.text.clone()
                        };
                        format!("[{:>2}] {}", raw.ordinal, body)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolOutput::ok(format!(
                    "{id} 的游离行共 {} 条(历史多行写法/手改留下的不可寻址内容,update 删不到)。\
                     用 `{tool} raw_delete id={id} ordinal=<n>` 按序号删除单条:\n{rendered}",
                    raws.len(),
                    tool = self.tool_name,
                ))
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
                if let Some(title_err) = self.check_title(title) {
                    return ToolOutput::error(title_err);
                }
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
                // D-330:priority 参数与 fields 里「优先级」键去重——调用方可能同时传两者,
                // 直接 push 会双写同名字段(值相同冗余、值不同语义歧义)。语义与 update 分支
                // (:664-673)一致:已存在(中文键或大小写不敏感的 priority)则覆盖,否则追加。
                if let Some(priority) = input.priority {
                    match fields
                        .iter_mut()
                        .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
                    {
                        Some((_, value)) => *value = priority,
                        None => fields.push(("优先级".into(), priority)),
                    }
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
                if let Some(title_err) = self.check_title(title) {
                    return ToolOutput::error(title_err);
                }
                // R-191:登记硬约束(缺必填字段即拒,提示补什么)。
                if let Some(required_err) = self.check_add_required(&input) {
                    return ToolOutput::error(required_err);
                }
                if let Some(sev_err) = self.check_severity(&input.severity) {
                    return ToolOutput::error(sev_err);
                }
                if let Some(priority_err) = self.check_priority(&input.priority) {
                    return ToolOutput::error(priority_err);
                }
                if let Some(tag_err) = self.check_tag(&input.fields) {
                    return ToolOutput::error(tag_err);
                }
                // 新建没有既有批次值:严格按上限约束。
                if let Some(batch_err) = self.check_batches(&input.fields, None) {
                    return ToolOutput::error(batch_err);
                }
                if let Err(e) = self.check_refs(ctx, &input.refs, true) {
                    return ToolOutput::error(e);
                }
                let id = store.next_id(&entries);
                let mut fields: Vec<(String, String)> = input.fields.into_iter().collect();
                if !input.refs.is_empty() {
                    fields.push(("refs".into(), input.refs.join(" ")));
                }
                // D-330:priority 参数与 fields 里「优先级」键去重——调用方可能同时传两者,
                // 直接 push 会双写同名字段(值相同冗余、值不同语义歧义)。语义与 update 分支
                // (:664-673)一致:已存在(中文键或大小写不敏感的 priority)则覆盖,否则追加。
                if let Some(priority) = input.priority {
                    match fields
                        .iter_mut()
                        .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
                    {
                        Some((_, value)) => *value = priority,
                        None => fields.push(("优先级".into(), priority)),
                    }
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
                    return ToolOutput::error(archived_or_unknown(
                        id,
                        &entries,
                        &store,
                        self.tool_name,
                    ));
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
                // 以条目现有的批次总数为基准:存量 >10 的条目照常逐批推进,只拦抬高。
                if let Some(batch_err) = self.check_batches(&input.fields, Some(&entries[pos])) {
                    return ToolOutput::error(batch_err);
                }
                if let Err(e) = self.check_refs(ctx, &input.refs, false) {
                    return ToolOutput::error(e);
                }
                let updates_progress = input.fields.contains_key("进展");
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
                    if let Some(title_err) = self.check_title(&title) {
                        return ToolOutput::error(title_err);
                    }
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
                // 每次落「进展」都同时保存仓库锚点。后续取活会机械比较 HEAD /
                // worktree 指纹，把历史叙事标为 current/stale/future/unanchored，
                // 不再让一段未对齐当前代码的文字冒充事实。
                if updates_progress {
                    for (key, value) in crate::work::progress_anchor_fields(&ctx.cwd) {
                        match entry
                            .fields
                            .iter_mut()
                            .find(|(candidate, _)| *candidate == key)
                        {
                            Some((_, slot)) => *slot = value,
                            None => entry.fields.push((key, value)),
                        }
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
                // D-276 修复方向③:update 后自检游离段落并告警。push_field(D-294)
                // 保证本次写入不新增游离段落,但历史多行/手改残留仍在字段体系外、
                // update 触及不到——返回里点名并指路 raw_lines/raw_delete,否则
                // 残留段落会一直藏到有人用 git 手工翻。
                let raws = store.raw_lines(id);
                if raws.is_empty() {
                    ToolOutput::ok(format!("updated: {line}"))
                } else {
                    ToolOutput::ok(format!(
                        "updated: {line}\n⚠ {id} 仍携带 {} 条不可寻址的游离段落(历史多行写法/手改残留,本次 update 不新增也不清除)。\
                         用 `{tool} raw_lines id={id}` 查看、`{tool} raw_delete id={id} ordinal=<n>` 按序号清理。",
                        raws.len(),
                        tool = self.tool_name
                    ))
                }
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
            // R-201:按序号删除一条游离行。删除走 docstore 的模板手术:只移除那一条
            // Raw,字段与其余行一字不动,二次保存幂等(行已不在模板里,不会再生)。
            "raw_delete" => {
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
                        tool = self.tool_name,
                    )),
                    Err(e) => ToolOutput::error(format!("raw_delete failed: {e}")),
                }
            }
            // D-241:fixing 推不动时的合法退路。要求 id + reason(强制写理由),
            // 状态必须命中该文档类型的 reopen_from 集合,退回初始态并落进展。
            // 与「手改 markdown」的区别:reopen 走引擎,理由进文档,调度器下次
            // 扫到的是 open 而不是冒充「正在做」的僵尸 fixing。
            REOPEN_ACTION => {
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
                    return ToolOutput::error(archived_or_unknown(
                        id,
                        &entries,
                        &store,
                        self.tool_name,
                    ));
                };
                let current = &entries[pos];
                if !self.kind.reopen_from.contains(&current.status.as_str()) {
                    return ToolOutput::error(format!(
                        "cannot reopen {id}: status `{}` is not in the reopen set ({}). \
                         Reopen pulls a non-terminal item back to `{}`; closed items stay closed.",
                        current.status,
                        self.kind.reopen_from.join(" | "),
                        self.kind.statuses[0],
                    ));
                }
                let back_to = self.kind.statuses[0].to_string();
                entries[pos].status = back_to.clone();
                // 退回理由必须留在条目里,不能只出现在工具输出——否则下轮上下文
                // 一滚动就没人知道这条为什么被退回来(D-241 验收②「处置依据逐条写进进展」)。
                // 追加新的一行进展,而不是拼进已有字段值:docstore 按行解析,
                // 值里嵌 \n 的重载会被拆成 Raw 行而丢失(D-241 实测)。
                let note = format!("[reopen {}] {}", crate::memory::today(), reason);
                entries[pos].fields.push(("进展".into(), note.clone()));
                if let Err(e) = store.save(&entries) {
                    return ToolOutput::error(format!(
                        "cannot write {}: {e}",
                        store.path.display()
                    ));
                }
                ToolOutput::ok(format!(
                    "reopened {id} [{}] {}\n{note}",
                    back_to, entries[pos].title
                ))
            }
            // D-331:归档终态纠错——只允许终态到终态(fixed↔wontfix),强制 reason,
            // 条目保持归档、原子写入、进展留审计。归档 ID 不再是死胡同(D-267 的
            // [dropped] [fixed] 双终态就是没有此通道时留下的)。
            FIX_TERMINAL_ACTION => {
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
                        self.kind.rel_path,
                    )),
                    Err(e) => ToolOutput::error(format!("fix_terminal failed: {e}")),
                }
            }
            other => ToolOutput::error(format!(
                "unknown action `{other}`; valid: list | get | raw_lines | add | update | close | \
                 archive | reorder | raw_delete | {} | {} | {}",
                REOPEN_ACTION,
                FIX_TERMINAL_ACTION,
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
    /// 批次字段的写入侧门禁(2026-08-10 用户定调:批数由 agent 按工作量自定,上限 10)。
    ///
    /// 只校验**本次调用传入的字段值**,绝不拿合并后的整条目校验:归档里真实存在
    /// 11/11、16/16 的历史条目,按整条目校验会让它们一被触碰就再也关不掉。
    /// 判据本身在 docstore::check_declared_batches——上限、格式、done>total 都在那儿,
    /// 这里只负责把它接到写入路径上(没有调用方的门禁等于没有门禁)。
    ///
    /// `existing` = 被改的那条目前在文件里的样子(add 这类新建传 None)。它只提供
    /// 上限的基准:既有 3/11 推进到 4/11 是正常逐批推进要放行,抬到 3/16 才是新声明
    /// 要拒——不给基准的话,门禁会把存量 >10 条目的每一次推进都误伤掉。
    fn check_batches(
        &self,
        fields: &BTreeMap<String, String>,
        existing: Option<&Entry>,
    ) -> Option<String> {
        let (_, value) = fields
            .iter()
            .find(|(key, _)| **key == "批次" || key.eq_ignore_ascii_case("batches"))?;
        let existing_total = existing
            .and_then(crate::docstore::declared_batch_progress)
            .map(|(_, total)| total);
        crate::docstore::check_declared_batches(value, existing_total).err()
    }

    /// D-331:标题不得携带跨 DocKind 状态标记(`[done]`/`[dropped]` 等)——状态的家是
    /// header 方括号(引擎维护),写进标题会渲染成 `[dropped] [fixed]` 双终态污染。
    fn check_title(&self, title: &str) -> Option<String> {
        crate::docstore::title_status_marker(title).map(|marker| {
            format!(
                "title must not carry a status marker `[{marker}]` — the status lives in the \
                 header bracket (engine-managed); writing it into the title produces \
                 double-terminal headers like `[dropped] [fixed]` (D-331). Remove the marker \
                 from the title."
            )
        })
    }

    fn check_tag(&self, fields: &BTreeMap<String, String>) -> Option<String> {
        let valid = self.kind.tags?;
        let value = fields.iter().find(|(key, _)| {
            **key == "标签" || key.eq_ignore_ascii_case("tags") || key.eq_ignore_ascii_case("tag")
        })?;
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

    /// R-191 登记硬约束:新建条目缺关键登记字段直接拒绝,并提示补什么,不静默放行。
    ///
    /// 触发:另一个项目的 agent 登记需求时漏掉复杂度评估——根因就是 add 只校验 title,
    /// 「复杂度/severity/priority/标签」全凭自觉。跨项目一致性不能靠每个项目各自记,
    /// 要在这里硬拦:req 必带 复杂度(小|中|大)+ 优先级 + 标签;defect 必带
    /// severity + 优先级 + 标签。goal/source/finding(severities/priorities/tags 均 None)
    /// 不受影响。
    fn check_add_required(&self, input: &TrackerInput) -> Option<String> {
        // 只有带 priorities 的追踪文档(req/defect)有登记硬约束;
        // goal/source/finding/memory/decision(priorities None)不受影响。
        self.kind.priorities?;
        let mut missing: Vec<&str> = Vec::new();
        if self.kind.severities.is_some() {
            if input.severity.is_none() {
                missing.push("severity (high|medium|low)");
            }
        } else {
            let has_complexity = input.fields.iter().any(|(k, v)| {
                (*k == "复杂度" || k.eq_ignore_ascii_case("complexity")) && !v.trim().is_empty()
            });
            if !has_complexity {
                missing.push("复杂度 (小|中|大)");
            }
        }
        if input.priority.is_none() {
            missing.push("priority (P0|P1|P2|P3)");
        }
        if self.kind.tags.is_some() {
            let has_tag = input.fields.iter().any(|(k, v)| {
                (*k == "标签" || k.eq_ignore_ascii_case("tags") || k.eq_ignore_ascii_case("tag"))
                    && !v.trim().is_empty()
            });
            if !has_tag {
                missing.push("标签 (核心|后端|前端|模型|发布|流程)");
            }
        }
        if missing.is_empty() {
            None
        } else {
            Some(format!(
                "{} add 缺少必填登记字段: {}. 新建条目必须先补这些字段再登记——\
                 缺字段即拒是跨项目硬约束(R-191),不静默放行。",
                self.noun,
                missing.join("、")
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

/// R-184 批5(收活格5):合并成功后,把线的交付回写**主根** tracker。
///
/// 追加「进展」与仓库 provenance 锚点、不改状态、不改标题——收活回写是事实记录,
/// 条目该不该 done/open 仍由取活判定负责,这里不越权。走与 `TrackerTool::execute` 相同
/// 的跨进程锁与完整性门禁(load … find … save 整段罩在锁里,两个 worktree
/// 各自登记撞 D-267 的教训),所以绝不绕过 docstore 直接改文件。
///
/// 返回更新后的条目,让调用方能复核回写是否落在预期的行上。
pub fn append_progress(
    project_root: &std::path::Path,
    kind: &'static DocKind,
    id: &str,
    note: &str,
) -> Result<Entry, String> {
    let store = DocStore::open(project_root, kind);
    let _lock = store
        .lock()
        .map_err(|e| format!("cannot lock {} for writing: {e}", store.path.display()))?;
    let mut entries = store
        .load()
        .map_err(|e| format!("cannot read {}: {e}", store.path.display()))?;
    let issues = store.integrity_issues(&entries);
    if !issues.is_empty() {
        return Err(format!(
            "REFUSING to write {}: tracker integrity is broken.\n{}",
            kind.rel_path,
            issues.join("\n")
        ));
    }
    let Some(pos) = entries.iter().position(|e| e.id == id) else {
        return Err(unknown_id(id, &entries));
    };
    let entry = &mut entries[pos];
    let now = crate::memory::today();
    let line = format!("{now} 收活回写: {note}");
    match entry.fields.iter_mut().find(|(k, _)| k == "进展") {
        Some((_, slot)) => {
            slot.push('\n');
            slot.push_str(&line);
        }
        None => entry.fields.push(("进展".into(), line)),
    }
    for (key, value) in crate::work::progress_anchor_fields(project_root) {
        match entry
            .fields
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            Some((_, slot)) => *slot = value,
            None => entry.fields.push((key, value)),
        }
    }
    store
        .save(&entries)
        .map_err(|e| format!("cannot write {}: {e}", store.path.display()))?;
    Ok(entries[pos].clone())
}

/// 为桌面端文档快照提供与 req/defect list 相同的阻塞判断和稳定后置顺序。
pub fn schedule_for_display(
    ctx: &ToolCtx,
    kind: &'static DocKind,
    entries: &[Entry],
) -> Result<Vec<ScheduledEntry>, String> {
    let states = dependency_states(ctx, kind, entries)?;
    Ok(schedule_for_display_with_states(entries, &states))
}

/// 已由调用方读取完整文档快照时,复用同一份依赖状态,避免 req/defect 各自重新扫盘。
pub fn schedule_for_display_with_states(
    entries: &[Entry],
    states: &DependencyStates,
) -> Vec<ScheduledEntry> {
    let scheduled = schedule_entries(entries, states);
    scheduled
        .into_iter()
        .map(|(entry, block_reasons)| ScheduledEntry {
            entry: entry.clone(),
            block_reasons,
        })
        .collect()
}

/// 当前可推进条目的「ID 标题」,按调度顺序取前 limit 条(阻塞的跳过)。
///
/// 用途:自主推进轮的记忆召回查询键。自动轮的 prompt 是固定模板,拿它去检索
/// 等于每轮都用同一个常量查询——2026-08-12 实测,224 轮召回里 161 轮是自动轮,
/// 反复注入同一批条目(M-006 被注入 101 次只被拉取 18 次),采纳率 22.5%,
/// 而用户真实提问轮是 46.5%。取活条目的标题才是这一轮真正在做的事。
pub fn workable_titles(project_root: &std::path::Path, limit: usize) -> Vec<String> {
    let ctx = ToolCtx::new(project_root.to_path_buf(), project_root.to_path_buf());
    let mut out = Vec::new();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let Ok(entries) = DocStore::open(project_root, kind).load() else {
            continue;
        };
        let Ok(scheduled) = schedule_for_display(&ctx, kind, &entries) else {
            continue;
        };
        for item in scheduled {
            if out.len() >= limit {
                return out;
            }
            if kind.terminal.contains(&item.entry.status.as_str()) {
                continue;
            }
            // D-332:非法 lifecycle(未知/畸形状态)不参与取活候选——调度器对
            // 控制面脏数据 fail-closed,不让污染条目混进可推进标题。
            if !item.entry.status.is_empty() && !kind.statuses.contains(&item.entry.status.as_str())
            {
                continue;
            }
            if !item.block_reasons.is_empty() {
                continue;
            }
            out.push(format!("{} {}", item.entry.id, item.entry.title));
        }
    }
    out
}

/// R-169:自主推进(鞭挞)的 backlog 判定——桌面端与 CLI 共用同一实现
/// (D-229 类「能力只在桌面端」的架构债消除)。活动条目里存在可推进项 →
/// Workable;无活动条目 → Empty;全部阻塞 → AllBlocked。
/// block_reasons 与 docs_snapshot 的 `blocked` 字段同源(schedule_for_display)。
pub fn backlog_status(project_root: &std::path::Path) -> kanzei_harness::auto_run::BacklogStatus {
    use kanzei_harness::auto_run::BacklogStatus;
    // R-141:调用方给的就是主根,不再从它二次发现(worktree 线传下来的主根
    // 与代码树不同,发现一次就会拐回分支副本)。
    let ctx = ToolCtx::new(project_root.to_path_buf(), project_root.to_path_buf());
    let mut active = 0usize;
    let mut workable = false;
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let entries = match DocStore::open(project_root, kind).load() {
            Ok(entries) => entries,
            // 自动推进宁可继续等下一轮，也不能把暂时的读取故障伪装成“已清空”。
            Err(_) => return BacklogStatus::Unknown,
        };
        let scheduled = match schedule_for_display(&ctx, kind, &entries) {
            Ok(scheduled) => scheduled,
            Err(_) => return BacklogStatus::Unknown,
        };
        for item in scheduled {
            if kind.terminal.contains(&item.entry.status.as_str()) {
                continue;
            }
            // D-332:非法 lifecycle 不算活动条目——它已被隔离为 integrity 错误,
            // 不能计入「active」,否则 backlog 判定会把它误算成可推进项。
            if !item.entry.status.is_empty() && !kind.statuses.contains(&item.entry.status.as_str())
            {
                continue;
            }
            active += 1;
            if item.block_reasons.is_empty() {
                workable = true;
            }
        }
    }
    if workable {
        BacklogStatus::Workable
    } else if active == 0 {
        BacklogStatus::Empty
    } else {
        BacklogStatus::AllBlocked
    }
}

#[derive(Default)]
pub struct DependencyStates {
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

pub fn dependency_states_from_documents(
    requirements: (&[Entry], &[Entry]),
    defects: (&[Entry], &[Entry]),
) -> DependencyStates {
    let mut states = DependencyStates::default();
    for (kind, (active, archived)) in [(&REQUIREMENTS, requirements), (&DEFECTS, defects)] {
        for entry in active.iter().chain(archived.iter()) {
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
                states.deps.insert(entry.id.clone(), deps);
            }
        }
    }
    states
}

fn dependency_states(
    ctx: &ToolCtx,
    current_kind: &DocKind,
    current_entries: &[Entry],
) -> Result<DependencyStates, String> {
    let mut documents: [Vec<Entry>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
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
        let offset = if kind.rel_path == REQUIREMENTS.rel_path {
            0
        } else {
            2
        };
        documents[offset] = active;
        documents[offset + 1] = archived;
    }
    Ok(dependency_states_from_documents(
        (&documents[0], &documents[1]),
        (&documents[2], &documents[3]),
    ))
}

/// 反向依赖图(R-111 验收②「条目详情含正反向链接」):id → 依赖它的条目 id 列表。
/// 与 dependency_states 共用同一份「依赖:」字段解析(依赖=阻塞关系,refs 不在此列),
/// 供桌面端 docs_snapshot 输出反向链接与文档页依赖视图使用。
/// 返回 (正向图 id→deps, 反向图 id→dependents)。
pub fn dependents_map(
    ctx: &ToolCtx,
    current_kind: &DocKind,
    current_entries: &[Entry],
) -> Result<(DependencyMap, DependencyMap), String> {
    let states = dependency_states(ctx, current_kind, current_entries)?;
    Ok(dependents_map_with_states(&states))
}

/// 从已缓存的依赖状态生成正向/反向链接,不再触发任何文件读取。
pub fn dependents_map_with_states(states: &DependencyStates) -> (DependencyMap, DependencyMap) {
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
    (deps_map, dependents)
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

fn structured_entry(entry: &Entry, reasons: &[String], archived: bool) -> serde_json::Value {
    let priority = entry
        .fields
        .iter()
        .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
        .map(|(_, value)| value.clone());
    let fields: Vec<serde_json::Value> = entry
        .fields
        .iter()
        .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
        .collect();
    serde_json::json!({
        "id": entry.id,
        "title": entry.title,
        "lifecycle_status": entry.status,
        "archived": archived,
        "severity": entry.severity,
        "priority": priority,
        "blocked": !reasons.is_empty(),
        "block_reasons": reasons,
        "fields": fields,
    })
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

/// 活动 entries 找不到时:区分「已归档」与「真不存在」。归档条目不是 unknown——
/// 误导 agent 以为 ID 不存在而绕过专用工具手改托管文档,会破坏原子写入与审计链
/// (D-331:reopen 对归档 ID 误报 unknown id,把 D-267 的 [dropped] [fixed] 留在归档)。
fn archived_or_unknown(id: &str, entries: &[Entry], store: &DocStore, tool: &str) -> String {
    let archived = store.load_archive().unwrap_or_default();
    if archived.iter().any(|e| e.id == id) {
        format!(
            "`{id}` is archived — this action does not apply to terminal entries. \
             To correct a wrong terminal status (e.g. fixed should be wontfix), use \
             `{tool} fix_terminal id={id} status=<fixed|wontfix> reason=<why>`."
        )
    } else {
        unknown_id(id, entries)
    }
}

#[cfg(test)]
mod tests {
    use super::TrackerTool;
    use crate::docstore::{DocStore, Entry, DEFECTS, GOALS, REQUIREMENTS};
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

    #[test]
    fn tracker_permission_resource_distinguishes_reads_and_writes() {
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        assert_eq!(tool.resources(&json!({"action": "list"})), ["read:list"]);
        assert_eq!(tool.resources(&json!({"action": "get"})), ["read:get"]);
        assert_eq!(
            tool.resources(&json!({"action": "raw_lines"})),
            ["read:raw_lines"]
        );
        assert_eq!(tool.resources(&json!({"action": "add"})), ["write:add"]);
        assert_eq!(
            tool.resources(&json!({"action": "raw_delete"})),
            ["write:raw_delete"]
        );
        assert_eq!(
            tool.resources(&json!({"action": "repair_missing_id"})),
            ["write:repair_missing_id"]
        );
    }

    /// D-330:add/repair_missing_id 时 priority 参数与 fields 里「优先级」键只落一条——
    /// 同传两者不再双写同名字段(值不同语义歧义),参数优先覆盖(fields 里值被顶掉)。
    #[tokio::test]
    async fn add_and_repair_dedupe_priority_param_with_fields_key() {
        let dir = std::env::temp_dir().join(format!(
            "kz-add-prio-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        // add:priority 参数 P1 + fields 里「优先级: P2」——只落一条,参数优先。
        let out = tool
            .execute(
                json!({"action": "add", "title": "双写优先级测试", "priority": "P1",
                       "fields": {"复杂度": "小", "优先级": "P2", "标签": "后端"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let entry = store
            .load()
            .unwrap()
            .into_iter()
            .find(|e| e.title == "双写优先级测试")
            .expect("条目应存在");
        let prio: Vec<_> = entry.fields.iter().filter(|(k, _)| k == "优先级").collect();
        assert_eq!(
            prio.len(),
            1,
            "add 优先级字段只应有一条: {:?}",
            entry.fields
        );
        assert_eq!(prio[0].1, "P1", "priority 参数应覆盖 fields 里的值");
        // repair_missing_id:同型——恢复缺失编号同时传两处优先级。
        let out2 = tool
            .execute(
                json!({"action": "repair_missing_id", "id": "R-002", "title": "恢复条目",
                       "priority": "P2", "fields": {"优先级": "P3", "标签": "后端"}}),
                &ctx,
            )
            .await;
        assert!(!out2.is_error, "{}", out2.content);
        let e2 = store
            .load()
            .unwrap()
            .into_iter()
            .find(|e| e.id == "R-002")
            .expect("恢复条目应存在");
        let prio2: Vec<_> = e2.fields.iter().filter(|(k, _)| k == "优先级").collect();
        assert_eq!(
            prio2.len(),
            1,
            "repair 优先级字段只应有一条: {:?}",
            e2.fields
        );
        assert_eq!(prio2[0].1, "P2", "repair 的 priority 参数应覆盖 fields 值");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn requirement_defect_full_list_requires_explicit_dedup_reason() {
        let dir = std::env::temp_dir().join(format!(
            "kz-list-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[entry("R-001")])
            .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let rejected = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(rejected.is_error);
        assert!(rejected.content.contains("work next"));
        let allowed = tool
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ctx,
            )
            .await;
        assert!(!allowed.is_error, "{}", allowed.content);
        let value: serde_json::Value = serde_json::from_str(&allowed.content).unwrap();
        assert_eq!(value["entries"][0]["id"], "R-001");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-184 批5 收活格5:append_progress 追加进展与锚点,不改状态/标题/业务字段。
    #[test]
    fn append_progress_only_appends_progress_field_and_keeps_state() {
        let dir = std::env::temp_dir().join(format!(
            "kz-append-progress-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut e = entry("R-001");
        e.status = "doing".into();
        e.fields.push(("进展".into(), "2026-08-10 既有进展".into()));
        e.fields.push(("优先级".into(), "P1".into()));
        store.save(&[e]).unwrap();

        let updated =
            super::append_progress(&dir, &REQUIREMENTS, "R-001", "由 M 线交付合并").unwrap();
        assert_eq!(updated.status, "doing", "回写不得改状态");
        assert_eq!(updated.title, "t-R-001", "回写不得改标题");
        let progress = updated
            .fields
            .iter()
            .find(|(k, _)| k == "进展")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert!(progress.starts_with("2026-08-10 既有进展\n"), "{progress}");
        assert!(
            progress.ends_with("收活回写: 由 M 线交付合并"),
            "{progress}"
        );
        // 优先级字段原样保留。
        assert!(updated
            .fields
            .iter()
            .any(|(k, v)| k == "优先级" && v == "P1"));
        for anchor in ["recorded_at", "observed_head", "observed_worktree_hash"] {
            assert!(
                updated.fields.iter().any(|(key, _)| key == anchor),
                "缺进展 provenance 锚点 {anchor}: {:?}",
                updated.fields
            );
        }
        assert!(updated
            .fields
            .iter()
            .any(|(key, value)| key == "recorded_at" && value.parse::<u128>().is_ok()));
        assert!(updated.fields.iter().any(|(key, value)| {
            key == "observed_worktree_hash" && value.starts_with("fnv1a64:")
        }));

        // 无既有进展字段的条目:直接新建该字段。保存必须带上 R-001,否则覆盖丢条目。
        let e2 = entry("R-002");
        let mut both = store.load().unwrap();
        both.push(e2);
        store.save(&both).unwrap();
        let updated2 = super::append_progress(&dir, &REQUIREMENTS, "R-002", "第二条").unwrap();
        let progress2 = updated2
            .fields
            .iter()
            .find(|(k, _)| k == "进展")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert!(progress2.starts_with("20"), "{progress2}");
        assert!(progress2.contains("收活回写: 第二条"), "{progress2}");

        // 未知 ID 拒绝且不写盘。
        let before = store.load().unwrap();
        let err = super::append_progress(&dir, &REQUIREMENTS, "R-999", "x").unwrap_err();
        assert!(err.contains("unknown id `R-999`"), "{err}");
        let after = store.load().unwrap();
        assert_eq!(after.len(), before.len(), "未知 ID 不得写盘");

        std::fs::remove_dir_all(dir).ok();
    }

    /// R-184 批5:完整性破损时 append_progress 必须拒绝(与 TrackerTool 同一门禁)。
    #[test]
    fn append_progress_refuses_when_integrity_broken() {
        let dir = std::env::temp_dir().join(format!(
            "kz-append-progress-integrity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        // 编号断层(R-001 缺 R-002,直接上 R-003)→ 完整性门禁必须拦。
        let mut broken = entry("R-001");
        broken.fields.push(("进展".into(), "x".into()));
        store.save(&[broken, entry("R-003")]).unwrap();
        let err = super::append_progress(&dir, &REQUIREMENTS, "R-001", "x").unwrap_err();
        assert!(
            err.contains("tracker integrity is broken"),
            "完整性破损必须拒绝回写: {err}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-201 端到端:raw_lines 列出游离行(raw_delete 的标识来源),raw_delete
    /// 删除单条后字段体系与文件其余字节不受影响。
    #[tokio::test]
    async fn raw_lines_raw_delete_清理游离行且字段不受影响() {
        let dir = std::env::temp_dir().join(format!(
            "kz-rawlines-tool-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let path = dir.join(REQUIREMENTS.rel_path);
        // 直接手写带游离行的文档:引擎渲染路径不会产生游离行,必须从历史文件形态进入。
        std::fs::write(
            &path,
            "\
# Requirements

## R-001 条目 [todo]
- 进展: 第一行
历史手写段落一
- 验收: 有验收
",
        )
        .unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        // ①列出:输出带 [n] 序号 + 原文,同时给删除指引。
        let listed = tool
            .execute(json!({"action": "raw_lines", "id": "R-001"}), &ctx)
            .await;
        assert!(!listed.is_error, "{}", listed.content);
        assert!(listed.content.contains("[ 1]"), "{}", listed.content);
        assert!(
            listed.content.contains("历史手写段落一"),
            "{}",
            listed.content
        );
        assert!(
            listed.content.contains("raw_delete"),
            "输出要指明删除动作: {}",
            listed.content
        );

        // 未知 ID:拒绝并给出可用 ID。
        let missing = tool
            .execute(json!({"action": "raw_lines", "id": "R-999"}), &ctx)
            .await;
        assert!(missing.is_error, "{}", missing.content);

        // ②删除第 1 条:文件里只少那一行,字段一行不动。
        let deleted = tool
            .execute(
                json!({"action": "raw_delete", "id": "R-001", "ordinal": 1}),
                &ctx,
            )
            .await;
        assert!(!deleted.is_error, "{}", deleted.content);
        assert!(
            deleted.content.contains("已删除 R-001 的第 1 条游离行"),
            "{}",
            deleted.content
        );
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("历史手写段落一"), "{after}");
        assert!(after.contains("## R-001 条目 [todo]"), "{after}");
        assert!(after.contains("- 进展: 第一行"), "{after}");
        assert!(after.contains("- 验收: 有验收"), "{after}");

        // ④字段解析不受影响。
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let parsed = store.load().unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].fields.len(), 2, "{:?}", parsed[0].fields);
        assert!(parsed[0]
            .fields
            .iter()
            .any(|(k, v)| k == "进展" && v == "第一行"));

        // 缺少 ordinal:raw_delete 拒绝。
        let no_ordinal = tool
            .execute(json!({"action": "raw_delete", "id": "R-001"}), &ctx)
            .await;
        assert!(no_ordinal.is_error, "{}", no_ordinal.content);

        std::fs::remove_dir_all(dir).ok();
    }

    /// D-276 端到端:update 传**多行**进展值时——①不新增游离段落(push_field 折行,
    /// D-294 既有能力);②若条目已有历史游离段落,返回里自检告警并指路
    /// raw_lines/raw_delete(修复方向③,本次交付);③raw_delete 清完后 update
    /// 不再告警。
    #[tokio::test]
    async fn update多行值不新增游离段落且已有残留被自检点名() {
        let dir = std::env::temp_dir().join(format!(
            "kz-d276-update-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let path = dir.join(REQUIREMENTS.rel_path);
        // 手写带一条历史游离段落的文档(引擎渲染路径不会产生游离行)。
        std::fs::write(
            &path,
            "\
# Requirements

## R-001 条目 [todo]
- 进展: 第一行
历史手写段落一
- 验收: 有验收
",
        )
        .unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        // ①update 传多行进展值:第一段是首行,第二段会折行进同一字段(不新增游离段落)。
        let updated = tool
            .execute(
                json!({"action": "update", "id": "R-001",
                       "fields": {"进展": "第一段\n第二段\n第三段"}}),
                &ctx,
            )
            .await;
        assert!(!updated.is_error, "{}", updated.content);
        // 自检告警:仍有 1 条历史游离段落被点名 + 指路清理工具。
        assert!(
            updated.content.contains("游离段落"),
            "update 后应自检告警残留: {}",
            updated.content
        );
        assert!(
            updated.content.contains("raw_lines") && updated.content.contains("raw_delete"),
            "告警要指路清理通道: {}",
            updated.content
        );
        // 文件里多行值被折成单行,且游离段落数量不变(仍是那 1 条历史残留)。
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("- 进展: 第一段 第二段 第三段"),
            "多行值必须折成单行字段: {after}"
        );
        assert!(
            after.contains("历史手写段落一"),
            "历史游离段落仍存在(update 不新增也不清除): {after}"
        );
        assert!(
            !after.contains("第二段\n-"),
            "第二段不能变成新的游离段落: {after}"
        );

        // ②raw_delete 清掉历史残留后再 update:不再告警。
        let deleted = tool
            .execute(
                json!({"action": "raw_delete", "id": "R-001", "ordinal": 1}),
                &ctx,
            )
            .await;
        assert!(!deleted.is_error, "{}", deleted.content);
        let updated2 = tool
            .execute(
                json!({"action": "update", "id": "R-001",
                       "fields": {"进展": "只有一段"}}),
                &ctx,
            )
            .await;
        assert!(!updated2.is_error, "{}", updated2.content);
        assert!(
            !updated2.content.contains("游离段落"),
            "清完后 update 不应再告警: {}",
            updated2.content
        );

        std::fs::remove_dir_all(dir).ok();
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
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

    /// D-241 机制项:fixing 推不动时可以合法退回 open。要求 id + reason,
    /// 理由必须落进条目进展(不能只出现在工具输出),调度器下次看到的是 open。
    #[tokio::test]
    async fn reopen_把fixing退回open_并强制写理由进进展() {
        let dir = std::env::temp_dir().join(format!("kz-reopen-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("D-001");
        e.status = "fixing".into();
        e.fields = vec![("进展".into(), "修复方向已落地,真机复测卡在外部环境".into())];
        DocStore::open(&dir, &DEFECTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };

        // 不带理由 → 拒绝。
        let out = tool
            .execute(json!({"action": "reopen", "id": "D-001"}), &ctx)
            .await;
        assert!(out.is_error, "reopen 必须给理由: {}", out.content);
        assert!(out.content.contains("reason"), "{}", out.content);

        // 状态不在 reopen 集合(open 不是) → 拒绝。
        let saved = DocStore::open(&dir, &DEFECTS).load().unwrap();
        assert_eq!(saved[0].status, "fixing", "被拒的 reopen 不能动状态");
        let out = tool
            .execute(
                json!({"action": "reopen", "id": "D-001", "reason": "推不动,退回"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "fixing 应可退回 open: {}", out.content);
        assert!(out.content.contains("open"), "{}", out.content);

        let saved = DocStore::open(&dir, &DEFECTS).load().unwrap();
        assert_eq!(saved[0].status, "open", "状态必须退回 open");
        let progress_lines: Vec<&str> = saved[0]
            .fields
            .iter()
            .filter(|(k, _)| k == "进展")
            .map(|(_, v)| v.as_str())
            .collect();
        assert!(
            progress_lines.iter().any(|p| p.contains("推不动,退回")),
            "理由必须落进进展(任意一行): {:?}",
            progress_lines
        );
        assert!(
            progress_lines.iter().any(|p| p.contains("[reopen")),
            "进展要有 reopen 落款: {:?}",
            progress_lines
        );
        // 追加语义:原始进展行原样保留,新理由独立成行(与 docstore 按行解析一致)。
        assert!(
            progress_lines.iter().any(|p| p.contains("修复方向已落地")),
            "原始进展不能被覆盖: {:?}",
            progress_lines
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-241 边界:closed(终态)条目不能 reopen,requirement 的 done 同理。
    #[tokio::test]
    async fn reopen_拒绝终态与不在集合的状态() {
        let dir = std::env::temp_dir().join(format!("kz-reopen-edge-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("D-001");
        e.status = "fixed".into();
        DocStore::open(&dir, &DEFECTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        let out = tool
            .execute(
                json!({"action": "reopen", "id": "D-001", "reason": "不该退"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "终态不可 reopen: {}", out.content);
        assert!(
            out.content.contains("not in the reopen set"),
            "{}",
            out.content
        );

        // requirement 的 doing 不在 reopen 集合(REQUIREMENTS.reopen_from = ["doing"] 时
        // 才可退;这里验证 todo 态也被拒),终态 done 更不行。
        let mut r = entry("R-001");
        r.status = "done".into();
        DocStore::open(&dir, &REQUIREMENTS).save(&[r]).unwrap();
        let rtool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let out = rtool
            .execute(
                json!({"action": "reopen", "id": "R-001", "reason": "不该退"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "done 不可 reopen: {}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-331 验收①:add/update/repair_missing_id 拒绝标题里的跨 DocKind 状态标记
    /// (`[dropped]`/`[done]` 等)——状态的家是 header 方括号,标题带标记会渲染成
    /// 双终态污染(如 D-267 的 `[dropped] [fixed]`)。
    #[tokio::test]
    async fn title_status_marker_rejected_on_all_write_actions() {
        let dir = std::env::temp_dir().join(format!("kz-title-marker-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        // add:dropped 是 requirements/findings 的状态,不是缺陷状态 → 标题不得携带。
        let out = tool
            .execute(
                json!({"action": "add", "title": "某缺陷 [dropped]", "priority": "P2", "severity": "medium",
                       "fields": {"复杂度": "小", "标签": "后端"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "add 应拒绝标题状态标记: {}", out.content);
        assert!(out.content.contains("status marker"), "{}", out.content);
        // update:改标题携带 [done] → 拒绝。
        let mut e = entry("D-001");
        e.status = "open".into();
        DocStore::open(&dir, &DEFECTS).save(&[e]).unwrap();
        let out = tool
            .execute(
                json!({"action": "update", "id": "D-001", "title": "完成 [done] 的标题"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "update 应拒绝标题状态标记: {}", out.content);
        assert!(out.content.contains("status marker"), "{}", out.content);
        // repair_missing_id 同型。
        let out = tool
            .execute(
                json!({"action": "repair_missing_id", "id": "D-002", "title": "恢复 [fixed]",
                       "priority": "P2", "fields": {"标签": "后端"}}),
                &ctx,
            )
            .await;
        assert!(
            out.is_error,
            "repair_missing_id 应拒绝标题状态标记: {}",
            out.content
        );
        assert!(out.content.contains("status marker"), "{}", out.content);
        // 合法标题照常放行(不带方括号状态标记)。
        let out = tool
            .execute(
                json!({"action": "add", "title": "正常标题", "priority": "P2", "severity": "medium",
                       "fields": {"复杂度": "小", "标签": "后端"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "合法标题应放行: {}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-331 验收②:reopen/update 命中归档 ID 时不再报 unknown id,而是明确 archived
    /// 且该动作不适用——agent 不会误以为 ID 不存在而绕过专用工具手改托管文档。
    #[tokio::test]
    async fn archived_id_reports_archived_not_unknown() {
        let dir = std::env::temp_dir().join(format!("kz-archived-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        // 归档里放一个终态条目,活动文件为空。
        let mut e = entry("D-001");
        e.status = "fixed".into();
        e.title = "已归档缺陷".into();
        let store = DocStore::open(&dir, &DEFECTS);
        store.save(&[e]).unwrap();
        store.archive_terminal().unwrap();
        assert!(store.load().unwrap().is_empty(), "活动文件应为空");
        assert_eq!(store.load_archive().unwrap()[0].id, "D-001");
        // reopen 归档 ID:不再 unknown id,而是 archived + 指向纠错动作。
        let out = tool
            .execute(
                json!({"action": "reopen", "id": "D-001", "reason": "想拉回来"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "归档条目 reopen 应被拒: {}", out.content);
        assert!(out.content.contains("archived"), "{}", out.content);
        assert!(!out.content.contains("unknown id"), "{}", out.content);
        assert!(out.content.contains("fix_terminal"), "{}", out.content);
        // update 归档 ID 同理。
        let out = tool
            .execute(
                json!({"action": "update", "id": "D-001", "fields": {"进展": "x"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "归档条目 update 应被拒: {}", out.content);
        assert!(out.content.contains("archived"), "{}", out.content);
        // 真不存在的 ID 仍报 unknown id(回归⑤)。
        let out = tool
            .execute(
                json!({"action": "reopen", "id": "D-999", "reason": "x"}),
                &ctx,
            )
            .await;
        assert!(out.content.contains("unknown id"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-331 验收③④:fix_terminal 把归档终态从 fixed 纠为 wontfix,保持归档、
    /// 清除标题里的跨 DocKind 状态标记污染(D-267 的 [dropped])、进展留审计;
    /// 非法终态/缺理由/归档无此 ID 都拒绝。
    #[tokio::test]
    async fn fix_terminal_corrects_archived_status_and_strips_title_marker() {
        let dir = std::env::temp_dir().join(format!("kz-fix-terminal-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        // 归档一个 fixed 条目,标题带 [dropped] 污染(复现 D-267 形态)。
        let mut e = entry("D-001");
        e.status = "fixed".into();
        e.title = "某缺陷 [dropped] 标题".into();
        let store = DocStore::open(&dir, &DEFECTS);
        store.save(&[e]).unwrap();
        store.archive_terminal().unwrap();
        // 纠为 wontfix。
        let out = tool
            .execute(
                json!({"action": "fix_terminal", "id": "D-001", "status": "wontfix",
                       "reason": "用户 2026-08-11 定调不做中间档,应记 wontfix 而非 fixed"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("fixed") && out.content.contains("wontfix"),
            "{}",
            out.content
        );
        // 条目仍在归档、状态已改、标题标记被清、进展留审计。
        let archived = store.load_archive().unwrap();
        let entry = archived
            .iter()
            .find(|x| x.id == "D-001")
            .expect("条目应留在归档");
        assert_eq!(entry.status, "wontfix");
        assert!(
            !entry.title.contains("[dropped]"),
            "标题状态标记应被清除: {}",
            entry.title
        );
        assert!(
            entry.title.contains("某缺陷") && entry.title.contains("标题"),
            "其余标题逐字保留: {}",
            entry.title
        );
        assert!(
            entry.fields.iter().any(|(k, v)| k == "进展"
                && v.contains("[terminal-fix")
                && v.contains("fixed")
                && v.contains("wontfix")),
            "进展应留纠错审计: {:?}",
            entry.fields
        );
        assert!(store.load().unwrap().is_empty(), "活动文件不应出现该条目");
        // 非法终态(open 不是终态)拒绝。
        let out = tool
            .execute(
                json!({"action": "fix_terminal", "id": "D-001", "status": "open", "reason": "想退回"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "非终态不得作为纠错目标: {}", out.content);
        assert!(out.content.contains("terminal"), "{}", out.content);
        // 缺理由拒绝。
        let out = tool
            .execute(
                json!({"action": "fix_terminal", "id": "D-001", "status": "fixed"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "纠错必须给理由: {}", out.content);
        // 归档里不存在的 ID 拒绝。
        let out = tool
            .execute(
                json!({"action": "fix_terminal", "id": "D-999", "status": "fixed", "reason": "x x x"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "归档无此 ID 应拒绝: {}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    /// 写入侧门禁必须真的接上:docstore::check_declared_batches 有实现有单测,但没有
    /// 调用方时「上限 10」只是提示词里的一句话(§1.25:没有消费者不算交付)。
    /// 同时钉死作用域——只校验**本次传入**的字段值,归档里 11/11 的历史条目照常关得掉。
    #[tokio::test]
    async fn 声明批数超过十批_写入侧拒绝_但不牵连已有的历史批数() {
        let dir = std::env::temp_dir().join(format!(
            "kz-batch-cap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("R-001");
        e.status = "doing".into();
        e.fields = vec![("批次".into(), "3/11".into())];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        // ① 历史 3/11 的正常逐批推进(只改已完成数)必须放行:这是存量条目唯一能往前
        //    走的动作,拦掉它等于逼 agent 先篡改总数才能动这条。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"批次": "4/11"}}),
                &ctx,
            )
            .await;
        assert!(
            !out.is_error,
            "历史 3/11 推进到 4/11 应放行: {}",
            out.content
        );
        let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(
            saved[0]
                .fields
                .iter()
                .find(|(k, _)| k == "批次")
                .map(|(_, v)| v.as_str()),
            Some("4/11"),
            "推进后的批次要真的落盘"
        );

        // ② 把既有的 11 抬到 16 是货真价实的新声明:拒绝,且错误里要同时给出上限、
        //    既有基准与出路(拆后续条目)。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"批次": "3/16"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "抬高总数到 16 应被拒: {}", out.content);
        assert!(out.content.contains("10"), "{}", out.content);
        assert!(out.content.contains("11"), "{}", out.content);
        assert!(out.content.contains("后续条目"), "{}", out.content);

        // 既有值没超上限时,抬到 12 照旧撞门(基准不是免死金牌)。
        let out = tool
            .execute(
                json!({"action": "add", "title": "另一条", "priority": "P2", "fields": {"复杂度": "中", "标签": "核心", "批次": "0/5"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-002", "fields": {"批次": "0/12"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "既有 5 批抬到 12 应被拒: {}", out.content);

        // ③ 新建没有既有值,按 <=10 严格约束(两条写入路径都要接上,不能只堵一半)。
        let out = tool
            .execute(
                json!({"action": "add", "title": "新条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "核心", "批次": "0/11"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "add 携 11 批应被拒: {}", out.content);

        // ④ 新建 0/10 是合法上界,照常放行。
        let out = tool
            .execute(
                json!({"action": "add", "title": "十批条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "核心", "批次": "0/10"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "add 携 10 批应放行: {}", out.content);

        // ⑤ 作用域:文件里已有的 11 是历史真值,把它做完(按实际改小成 4/4)必须照常
        //    关得掉——若门禁错误地校验合并后的整条目,归档里 11/11、16/16 的条目会永久
        //    关不掉。
        let out = tool
            .execute(
                json!({"action": "close", "id": "R-001", "fields": {"批次": "4/4"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "历史 11 批条目收口应放行: {}", out.content);
        assert_eq!(
            DocStore::open(&dir, &REQUIREMENTS).load().unwrap()[0].status,
            "done"
        );
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

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
                json!({"action": "add", "title": "新条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "核心"}}),
                &ToolCtx::new(dir.clone(), dir.clone()),
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
            .execute(
                json!({"action": "archive"}),
                &ToolCtx::new(dir.clone(), dir.clone()),
            )
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        // 读操作必须仍然可用——否则模型无法诊断,就成了死锁。
        for action in ["list", "get"] {
            let mut input = json!({"action": action, "id": "R-001"});
            if action == "list" {
                input["reason"] = json!("deduplicate_registration");
            }
            let out = tool.execute(input, &ctx).await;
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
            .execute(json!({"action": "add", "title": "恢复后可写", "priority": "P2", "fields": {"复杂度": "中", "标签": "核心"}}), &ctx)
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

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
            .execute(json!({"action": "add", "title": "恢复后可写", "priority": "P2", "fields": {"复杂度": "中", "标签": "核心"}}), &ctx)
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
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ToolCtx::new(dir.clone(), dir.clone()),
            )
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
            .execute(
                json!({"action": "archive"}),
                &ToolCtx::new(dir.clone(), dir.clone()),
            )
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
                &ToolCtx::new(dir.clone(), dir.clone()),
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        let out = tool
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        let items = listed["entries"].as_array().unwrap();
        assert_eq!(items[0]["id"], "R-002");
        assert_eq!(items[1]["id"], "R-001");
        assert!(items[1]["block_reasons"][0]
            .as_str()
            .unwrap()
            .contains("阻塞字段"));
        assert_eq!(items[2]["id"], "R-003");
        assert!(items[2]["block_reasons"][0]
            .as_str()
            .unwrap()
            .contains("未完成依赖: R-002"));
        assert_eq!(items[3]["id"], "R-004");
        assert!(items[3]["block_reasons"][0]
            .as_str()
            .unwrap()
            .contains("阶段门槛"));

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
        let out = tool
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        let items = listed["entries"].as_array().unwrap();
        assert_eq!(items[0]["id"], "R-001");
        assert_eq!(items[1]["id"], "R-002");
        assert_eq!(items[2]["id"], "R-003");
        assert_eq!(items[3]["id"], "R-004");
        assert!(items[3]["block_reasons"][0]
            .as_str()
            .unwrap()
            .contains("阶段门槛"));
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        let out = tool
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(listed["deadlocked"], true);
        assert!(listed["deadlock_guidance"]
            .as_str()
            .unwrap()
            .starts_with("[调度死锁] 2 条requirement"));
        // 横幅必须堵死"没有可执行条目"这条收尾话术,否则鞭挞照旧静默停。
        assert!(
            listed["deadlock_guidance"]
                .as_str()
                .unwrap()
                .contains("禁止以「没有可执行条目」"),
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
        let out = tool
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(listed["deadlocked"], false, "{}", out.content);
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        let out = tool
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        let items = listed["entries"].as_array().unwrap();
        let cycle_line = items
            .iter()
            .find(|item| item["id"] == "R-001")
            .and_then(|item| item["block_reasons"][0].as_str())
            .unwrap_or_default();
        assert!(
            cycle_line.contains("循环依赖: R-001 → R-002 → R-001"),
            "{}",
            out.content
        );
        assert!(cycle_line.contains("断掉其中一条边"), "{}", out.content);
        let plain = items
            .iter()
            .find(|item| item["id"] == "R-003")
            .and_then(|item| item["block_reasons"][0].as_str())
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
        let out = tool
            .execute(
                json!({"action": "list", "reason": "deduplicate_registration"}),
                &ctx,
            )
            .await;
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let loaded = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();

        let (deps, dependents) = super::dependents_map(&ctx, &REQUIREMENTS, &loaded).unwrap();
        // 正向:R-001 → [R-002, R-003],R-003 → [R-002],R-002 无依赖。
        assert_eq!(
            deps.get("R-001").unwrap(),
            &vec!["R-002".to_string(), "R-003".to_string()]
        );
        assert_eq!(deps.get("R-003").unwrap(), &vec!["R-002".to_string()]);
        assert!(!deps.contains_key("R-002"));
        // 反向:R-002 被 R-001 与 R-003 依赖;R-003 只被 R-001 依赖;R-001 无人依赖。
        assert_eq!(
            dependents.get("R-002").unwrap(),
            &vec!["R-001".to_string(), "R-003".to_string()]
        );
        assert_eq!(dependents.get("R-003").unwrap(), &vec!["R-001".to_string()]);
        assert!(!dependents.contains_key("R-001"));

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
            &vec![
                "R-001".to_string(),
                "R-003".to_string(),
                "R-004".to_string()
            ]
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        // add:词表外标签被拒,错误里带合法词表。
        let out = tool
            .execute(
                json!({"action": "add", "title": "t", "priority": "P2", "fields": {"复杂度": "中", "标签": "杂项"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("invalid tag `杂项`"),
            "{}",
            out.content
        );
        assert!(out.content.contains("核心"), "{}", out.content);
        assert!(out.content.contains("后端"), "{}", out.content);

        // add:词表内标签放行。
        let out = tool
            .execute(
                json!({"action": "add", "title": "t", "priority": "P2", "fields": {"复杂度": "中", "标签": "前端"}}),
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
        assert!(
            out.content.contains("invalid tag `网络`"),
            "{}",
            out.content
        );

        // update:多值含一个非法词也被拒(按空白/逗号拆分逐词校验)。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"标签": "核心 杂项"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("invalid tag `杂项`"),
            "{}",
            out.content
        );

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

    // R-191:登记硬约束——新建 req 缺 复杂度/优先级/标签 即拒并提示补什么,
    // 新建 defect 缺 severity 即拒;补全后放行;goal(severities/priorities/tags
    // 全 None)不受影响。跨项目一致性的机械门禁:登记缺字段不再静默放行。
    #[tokio::test]
    async fn add_requires_registration_fields() {
        let dir = std::env::temp_dir().join(format!(
            "kz-r191-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        // req:缺 复杂度/优先级/标签 → 拒绝,错误提示补什么。
        let req_tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let out = req_tool
            .execute(json!({"action": "add", "title": "裸登记"}), &ctx)
            .await;
        assert!(out.is_error, "裸 req add 必须被拒");
        assert!(
            out.content.contains("复杂度")
                && out.content.contains("priority")
                && out.content.contains("标签"),
            "报错应提示缺哪些字段: {}",
            out.content
        );

        // req:只带 复杂度 仍缺 priority/标签 → 拒绝。
        let out = req_tool
            .execute(
                json!({"action": "add", "title": "半裸", "fields": {"复杂度": "中"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("priority"), "{}", out.content);

        // req:字段补全 → 放行。
        let out = req_tool
            .execute(
                json!({"action": "add", "title": "完整", "priority": "P2",
                       "fields": {"复杂度": "中", "标签": "核心"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);

        // defect:缺 severity → 拒绝;补全 → 放行。
        let def_tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        let out = def_tool
            .execute(
                json!({"action": "add", "title": "缺陷裸登记", "priority": "P2",
                       "fields": {"标签": "前端"}}),
                &ctx,
            )
            .await;
        assert!(
            out.is_error,
            "缺 severity 的 defect add 必须被拒: {}",
            out.content
        );
        assert!(out.content.contains("severity"), "{}", out.content);
        let out = def_tool
            .execute(
                json!({"action": "add", "title": "缺陷完整", "severity": "medium",
                       "priority": "P2", "fields": {"标签": "前端"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);

        // goal(无必填 kind 字段):裸 add 不受影响。
        let goal_tool = TrackerTool {
            tool_name: "goal",
            noun: "goal",
            kind: &GOALS,
            requires_refs: None,
        };
        let out = goal_tool
            .execute(json!({"action": "add", "title": "目标"}), &ctx)
            .await;
        assert!(!out.is_error, "goal 裸 add 不应被拦: {}", out.content);
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
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

    /// R-138 验收③,打在**真实写入口**上:8 个并发的 `req add` 必须落 8 条、
    /// ID 互不相同。这是 D-064 一族 lost-update 的直接回归锁——`kz` CLI、桌面端
    /// 与自举循环是三个各自独立的 OS 进程,谁也看不见谁的内存态协调器,
    /// 唯一挡得住的就是 execute 顶部那把跨进程锁。
    #[test]
    fn 并发新建不丢条目也不撞编号() {
        let dir = std::env::temp_dir().join(format!(
            "kz-tracker-concurrent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        DocStore::open(&dir, &REQUIREMENTS).save(&[]).unwrap();

        let 并发 = 8usize;
        let handles: Vec<_> = (0..并发)
            .map(|n| {
                let dir = dir.clone();
                // 每个线程一个独立 runtime:模拟各自跑一个进程,不共享任何内存态。
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    rt.block_on(async move {
                        let tool = TrackerTool {
                            tool_name: "req",
                            noun: "requirement",
                            kind: &REQUIREMENTS,
                            requires_refs: None,
                        };
                        let ctx = ToolCtx::new(dir.clone(), dir.clone());
                        tool.execute(
                            json!({"action": "add", "title": format!("并发条目 {n}"), "priority": "P2", "fields": {"复杂度": "中", "标签": "核心"}}),
                            &ctx,
                        )
                        .await
                    })
                })
            })
            .collect();
        for handle in handles {
            let out = handle.join().unwrap();
            assert!(!out.is_error, "并发 add 不该失败: {}", out.content);
        }

        let store = DocStore::open(&dir, &REQUIREMENTS);
        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 并发, "有条目被覆盖掉了: {entries:?}");
        let ids: std::collections::BTreeSet<&String> = entries.iter().map(|e| &e.id).collect();
        assert_eq!(ids.len(), 并发, "分配出了重复 ID: {ids:?}");
        assert!(
            store.integrity_issues(&entries).is_empty(),
            "并发写之后完整性必须干净: {:?}",
            store.integrity_issues(&entries)
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn backlog_status_三态判定_桌面端与_cli共用同一实现() {
        use kanzei_harness::auto_run::BacklogStatus;
        use std::path::Path;
        use std::time::{SystemTime, UNIX_EPOCH};
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kz-backlog-{}-{uniq}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let root: &Path = &dir;
        // ① 全阻塞:带「阻塞:」字段的活动条目 → AllBlocked。
        let mut blocked = entry("R-001");
        blocked.status = "doing".into();
        blocked.fields = vec![("阻塞".into(), "等用户回复方案".into())];
        DocStore::open(root, &REQUIREMENTS)
            .save(&[blocked])
            .unwrap();
        DocStore::open(root, &DEFECTS).save(&[]).unwrap();
        assert!(matches!(
            super::backlog_status(root),
            BacklogStatus::AllBlocked
        ));
        // ② 存在可推进条目 → Workable(即使有另一条被阻塞)。
        DocStore::open(root, &REQUIREMENTS)
            .save(&[entry("R-002")])
            .unwrap();
        assert!(matches!(
            super::backlog_status(root),
            BacklogStatus::Workable
        ));
        // ③ 无活动条目 → Empty。
        DocStore::open(root, &REQUIREMENTS).save(&[]).unwrap();
        DocStore::open(root, &DEFECTS).save(&[]).unwrap();
        assert!(matches!(super::backlog_status(root), BacklogStatus::Empty));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn backlog_status_读取失败时返回未知而非误报清空() {
        use kanzei_harness::auto_run::BacklogStatus;
        use std::time::{SystemTime, UNIX_EPOCH};
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "kz-backlog-read-fail-{}-{uniq}",
            std::process::id()
        ));
        let project = dir.join(".kanzei/project");
        std::fs::create_dir_all(project.join("defects.md")).unwrap();

        assert!(matches!(
            super::backlog_status(&dir),
            BacklogStatus::Unknown
        ));
        std::fs::remove_dir_all(dir).ok();
    }
}
