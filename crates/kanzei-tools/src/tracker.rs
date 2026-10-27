//! 通用追踪工具:req / defect / source / finding 共用一套 CRUD。
//! 硬门禁:ID 引擎分配、状态机受限、格式引擎序列化、引用必须存在——模型只提供字段值。

use std::collections::BTreeMap;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use kanzei_memory::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};

// R-204:取活调度域独立成模块(scheduling.rs),供 auto_run/CLI/docs/memory 四方统一消费。
// 本文件保留既有 pub 面(re-export),消费方调用点零改动;行为零变更。
pub mod scheduling;

#[cfg(test)]
mod scheduling_tests;

#[cfg(test)]
mod maintenance_tests;

// R-311:close action 在状态迁移前写入收尾链遥测。
// R-204:每个 action 独立函数(actions.rs),execute 只剩路由。
mod actions;
mod fields;
mod requirement_contract;
mod validation;

#[cfg(test)]
pub(crate) use scheduling::block_reasons;
pub use scheduling::{
    append_progress, backlog_status, backlog_status_for, coupling_signals,
    dependency_states_from_documents, dependents_map, dependents_map_with_states, dispatch_verdict,
    schedule_for_display, schedule_for_display_with_states, workable_titles, DependencyStates,
    ScheduledEntry,
};
pub(crate) use scheduling::{is_prerequisite_key, park_reason, tracker_ids};

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
    "archive_fill",
    "raw_delete",
    "repair_reused_id",
    "repair_missing_id",
    "void_id",
    "normalize",
];

/// UI2-0926 #13:批次字段写成 `批次: 0/5; B1 核心模型…` 时归一成 `0/5`,说明放进「批次计划」
/// (已有该字段就追加)。开头不是 `k/N` 的值原样留给 check_batches 报错。
fn normalize_batch_field(fields: &mut BTreeMap<String, String>) {
    let Some(key) = fields
        .keys()
        .find(|key| *key == "批次" || key.eq_ignore_ascii_case("batches"))
        .cloned()
    else {
        return;
    };
    let Some((done, total, note)) = kanzei_memory::docstore::split_batch_value(&fields[&key])
    else {
        return;
    };
    fields.insert(key, format!("{done}/{total}"));
    if let Some(note) = note {
        match fields.get_mut("批次计划") {
            Some(existing) if existing.contains(&note) => {}
            Some(existing) => {
                existing.push('；');
                existing.push_str(&note);
            }
            None => {
                fields.insert("批次计划".into(), note);
            }
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct TrackerInput {
    /// Set only by the desktop command, never by model-supplied JSON.
    #[serde(skip)]
    user_action: bool,
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
    /// add/update 的受控标签；写入文档字段「标签」。
    #[serde(default)]
    tag: Option<String>,
    /// requirement add/update 的复杂度(小|中|大)；写入文档字段「复杂度」。
    #[serde(default)]
    complexity: Option<String>,
    /// 自由字段,如 {"验收": "...", "复现": "..."}
    #[serde(default)]
    fields: BTreeMap<String, String>,
    /// Fixed requirement contract. Unknown inputs stay draft; replace this object on spec edits.
    #[serde(default)]
    requirement: Option<kanzei_memory::docstore::requirement::RequirementSpec>,
    /// Evidence is separate from acceptance. Copy the current revision from req get.
    #[serde(default)]
    evidence: Option<Vec<kanzei_memory::docstore::requirement::AcceptanceEvidence>>,
    /// 引用的条目 ID(finding 必须引用 source)
    #[serde(default)]
    refs: Vec<String>,
    /// add/update 可选：已完成且验证通过的 `.kanzei/research/<topic>/prior-art.md`。
    /// 核心空 refs 的 add 不传本参数会保存 todo 与 pending 工件；先完成调研，
    /// 再 update 或 work claim 开始实施。独立于 refs，不放进 fields。
    #[serde(default)]
    prior_art: Option<String>,
    /// add/update 可选：用户明确豁免先行调研的理由，至少 8 字；不替用户生成。
    /// 与 prior_art 互斥，不放进 fields。
    #[serde(default)]
    prior_art_waiver: Option<String>,
    /// B2:source/finding 所属课题目录(kebab-case)。
    #[serde(default)]
    topic: Option<String>,
    /// list 使用 backlog_maintenance 核对、清理全队列，deduplicate_registration 登记前查重。
    /// close/update 到 dropped/wontfix 必填：过期、取消、重复或不修复的具体原因。
    /// void_id/reopen/fix_terminal 也使用此字段记录原因。
    #[serde(default)]
    reason: Option<String>,
    /// raw_delete 必填:raw_lines 输出里的 [n] 序号(要删除的第 n 条游离行)
    #[serde(default)]
    ordinal: Option<usize>,
    /// archive_fill 必填:归档条目字段里要被替换的占位符原文(如 `T-1786565xxx`)
    #[serde(default)]
    old: Option<String>,
    /// archive_fill 必填:替换后的真实值(如 `T-1786565346`)
    #[serde(default)]
    new: Option<String>,
    /// normalize 用:false(默认)= dry-run 只报告待修项;true = 实际写入修复。
    #[serde(default)]
    apply: bool,
}

#[async_trait]
impl Tool for TrackerTool {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn description(&self) -> String {
        let mut description = format!(
            "Read and maintain project {} records. get returns the current record; update uses a top-level id. \
             reorder requires the complete ID order; archive moves terminal records. Repairs preserve historical IDs and require supporting records.", self.noun
        );
        if self.kind.severities.is_some() && self.kind.priorities.is_some() {
            description.push_str(" severity is impact; priority is scheduling.");
        }
        if self.requires_refs.is_some() {
            description.push_str(" Adding a record requires source refs.");
        }
        if matches!(self.kind.prefix, "S" | "F") {
            description.push_str(
                " All actions require topic; records belong to .kanzei/research/<topic>/.",
            );
        }
        if self.kind.prefix == "R" {
            description.push_str(" requirement carries statement, acceptance and source; missing information remains draft. Preserve acceptance IDs on edits. Core additions with empty refs require prior_art or an explicit prior_art_waiver. close(done) requires criterion evidence tied to the current revision.");
        }
        if matches!(self.kind.prefix, "R" | "D") {
            description.push_str(" Maintenance lists use reason=backlog_maintenance. Retirement states require a reason; completion states require delivery evidence. awaiting_external records pending external checks without declaring acceptance.");
        }
        description
    }

    /// schema 按文档类型动态收窄:action/status/severity/priority 全部给 enum。
    /// 合法取值只写在描述里是不够的——provider 不会据此校验,模型猜错就是一次
    /// 失败调用加一个补丁提交。放进 schema 才有约束力。
    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(TrackerInput)).unwrap();
        let mut actions = vec![
            "list",
            "audit_acceptance_scope",
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
        let enums: [(&str, Option<Vec<String>>); 6] = [
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
            (
                "tag",
                self.kind
                    .tags
                    .map(|values| values.iter().map(|s| s.to_string()).collect()),
            ),
            (
                "complexity",
                (self.kind.priorities.is_some() && self.kind.severities.is_none())
                    .then(|| ["小", "中", "大"].into_iter().map(str::to_string).collect()),
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
        if self.kind.prefix != "R" {
            if let Some(definitions) = schema
                .get_mut("definitions")
                .and_then(|v| v.as_object_mut())
            {
                for key in [
                    "RequirementSpec",
                    "RequirementKind",
                    "RequirementSource",
                    "AcceptanceCriterion",
                    "RequirementLink",
                    "RequirementRelation",
                    "AcceptanceEvidence",
                ] {
                    definitions.remove(key);
                }
            }
            if let Some(properties) = schema
                .pointer_mut("/properties")
                .and_then(|value| value.as_object_mut())
            {
                properties.remove("prior_art");
                properties.remove("prior_art_waiver");
                properties.remove("requirement");
                properties.remove("evidence");
            }
        }
        let mut required = vec!["title"];
        if self.kind.priorities.is_some() && self.kind.prefix != "R" {
            required.push("priority");
        }
        if self.kind.severities.is_some() {
            required.push("severity");
        }
        if self.kind.tags.is_some() && self.kind.prefix != "R" {
            required.push("tag");
        }
        if self.requires_refs.is_some() {
            required.push("refs");
        }
        if matches!(self.kind.prefix, "S" | "F") {
            required.push("topic");
            schema["required"]
                .as_array_mut()
                .expect("TrackerInput has required fields")
                .push(serde_json::json!("topic"));
        }
        let mut then_schema = serde_json::json!({ "required": required });
        if self.requires_refs.is_some() {
            then_schema["properties"] = serde_json::json!({ "refs": { "minItems": 1 } });
        }
        schema["allOf"] = serde_json::json!([
            {
                "if": {
                    "properties": { "action": { "const": "add" } },
                    "required": ["action"]
                },
                "then": then_schema
            },
            {
                "if": {
                    "properties": { "action": { "const": "update" } },
                    "required": ["action"]
                },
                "then": {
                    "required": ["id"],
                    "properties": { "id": { "type": "string", "minLength": 1 } }
                }
            }
        ]);
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
        self.execute_with_origin(input, ctx, false).await
    }
}

impl TrackerTool {
    /// A user's explicit completion decision does not require agent delivery evidence.
    /// Locking, document integrity and lifecycle validation still share the tool path.
    pub async fn execute_user_action(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        self.execute_with_origin(input, ctx, true).await
    }

    async fn execute_with_origin(
        &self,
        input: serde_json::Value,
        ctx: &ToolCtx,
        user_action: bool,
    ) -> ToolOutput {
        let mut input: TrackerInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        input.user_action = user_action;
        // R-268:写日志判定在 match(input.action) 之后还要用 action——先缓存字符串
        // (match 会部分 move input,函数尾再读 input.action 会 borrow-after-move)。
        let action_str = input.action.clone();
        // 顶层结构化字段落到既有文档字段体系；顶层值优先，避免同一调用双写冲突。
        if let Some(tag) = input.tag.take() {
            input.fields.insert("标签".into(), tag);
        }
        if let Some(complexity) = input.complexity.take() {
            input.fields.insert("复杂度".into(), complexity);
        }
        if matches!(input.action.as_str(), "add" | "update" | "close") {
            normalize_batch_field(&mut input.fields);
        }
        if WRITE_ACTIONS.contains(&input.action.as_str()) {
            if let Some(error) = fields::reject_engine_writes(&input.fields) {
                return ToolOutput::needs_correction("TRACKER_ENGINE_FIELD", error);
            }
            if let Err(error) = scheduling::validate_write_fields(&input.fields) {
                return ToolOutput::needs_correction("TRACKER_INPUT_INVALID", error);
            }
        }
        if input.action == "list"
            && ctx.project_workflow
            && matches!(self.kind.prefix, "R" | "D")
            && !matches!(
                input.reason.as_deref(),
                Some("deduplicate_registration" | "backlog_maintenance" | "human_cli")
            )
        {
            return ToolOutput::needs_correction(
                "TRACKER_LIST_PURPOSE_REQUIRED",
                "完整 requirement/defect 队列不是执行期上下文。取活请调用 `work next`；\
                 用户要求核对或清理队列时用 reason=backlog_maintenance；\
                 登记前查重用 reason=deduplicate_registration。",
            );
        }
        let store = if matches!(self.kind.prefix, "S" | "F") {
            let Some(topic) = input
                .topic
                .as_deref()
                .filter(|topic| !topic.trim().is_empty())
            else {
                return ToolOutput::error(
                    "source/finding 操作必须提供 topic(小写 kebab-case)，工件落点是 `.kanzei/research/<topic>/`",
                );
            };
            match DocStore::open_topic(&ctx.project_root, self.kind, topic) {
                Ok(store) => store,
                Err(error) => return ToolOutput::error(error.to_string()),
            }
        } else {
            DocStore::open(&ctx.project_root, self.kind)
        };
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
        if WRITE_ACTIONS.contains(&input.action.as_str()) {
            let issues = match store.integrity_issues(&entries) {
                Ok(issues) => issues,
                Err(error) => {
                    return ToolOutput::error(format!(
                        "REFUSING to write {}: cannot verify tracker integrity: {error}",
                        self.kind.rel_path,
                    ))
                }
            };
            if !REPAIR_ACTIONS.contains(&input.action.as_str())
                && input.action != FIX_TERMINAL_ACTION
                && input.action != "normalize"
                && !issues.is_empty()
            {
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

        if self.kind.prefix == "R" && matches!(input.action.as_str(), "add" | "update" | "close") {
            let previous = input
                .id
                .as_ref()
                .and_then(|id| entries.iter().find(|entry| &entry.id == id));
            if let Err(error) = requirement_contract::prepare(&mut input, previous) {
                return ToolOutput::needs_correction("TRACKER_INPUT_INVALID", error);
            }
        }
        let output = match input.action.as_str() {
            "list" => actions::list(self, input, ctx, &store, &mut entries),
            "audit_acceptance_scope" => {
                actions::audit_acceptance_scope(self, input, ctx, &store, &mut entries)
            }
            "get" => actions::get(self, input, ctx, &store, &mut entries),
            // R-201:列出条目的游离行——模板里不可寻址的 Raw 行,update 永远删不到。
            // 每条给 [n] 序号 + 原文,序号即 raw_delete 的键;空行显式标出避免看不见。
            "raw_lines" => actions::raw_lines(self, input, ctx, &store, &mut entries),
            "repair_reused_id" => actions::repair_reused_id(self, input, ctx, &store, &mut entries),
            // 补回从 git 历史里捞回来的条目:只允许补真空洞,并插回原编号位置。
            "repair_missing_id" => {
                actions::repair_missing_id(self, input, ctx, &store, &mut entries)
            }
            // 主动注销一个编号:唯一合法的"缺号交代"通道,理由必填、留档可审计。
            "void_id" => actions::maintenance::void_id(self, input, ctx, &store, &mut entries),
            "archive" => actions::maintenance::archive(self, input, ctx, &store, &mut entries),
            "add" => actions::add(self, input, ctx, &store, &mut entries),
            "update" | "close" => actions::update_close(self, input, ctx, &store, &mut entries),
            // R-054:整表重排(文件顺序 = 开发顺序)。要求 order 是现有条目的完整置换,
            // 缺一多一都拒绝——引擎整读整写,天然与并发的状态更新互斥。
            "reorder" => actions::reorder(self, input, ctx, &store, &mut entries),
            // R-201:按序号删除一条游离行。删除走 docstore 的模板手术:只移除那一条
            // Raw,字段与其余行一字不动,二次保存幂等(行已不在模板里,不会再生)。
            "raw_delete" => {
                actions::maintenance::raw_delete(self, input, ctx, &store, &mut entries)
            }
            // D-241:fixing 推不动时的合法退路。要求 id + reason(强制写理由),
            // 状态必须命中该文档类型的 reopen_from 集合,退回初始态并落进展。
            // 与「手改 markdown」的区别:reopen 走引擎,理由进文档,调度器下次
            // 扫到的是 open 而不是冒充「正在做」的僵尸 fixing。
            REOPEN_ACTION => actions::maintenance::reopen(self, input, ctx, &store, &mut entries),
            // D-331:归档终态纠错——只允许终态到终态(fixed↔wontfix),强制 reason,
            // 条目保持归档、原子写入、进展留审计。归档 ID 不再是死胡同(D-267 的
            // [dropped] [fixed] 双终态就是没有此通道时留下的)。
            FIX_TERMINAL_ACTION => {
                actions::maintenance::fix_terminal(self, input, ctx, &store, &mut entries)
            }
            // R-227:归档条目字段里的占位符测试 ID 回填。占位符 `T-<数字>xxx` 是
            // 「全量跑过但没记 test_record、隔时凭记忆写证据」的产物(R-198/R-199/
            // D-219/D-266/D-279/D-281/D-282/D-316 关闭证据存量 8 处)。回填 =
            // 把占位符替换为 test_record 落盘的真实 ID;docstore 侧要求恰好命中一次。
            "archive_fill" => {
                actions::maintenance::archive_fill(self, input, ctx, &store, &mut entries)
            }
            // D-332 验收②:统一 repair surface——把散落在 fix_terminal / 手改 markdown /
            // raw_delete 之间的修复动作收敛成一个机械、幂等、dry-run-first 的入口。
            // 扫描活动 + 归档区,报告/修复:
            //   ① invalid lifecycle(非空但不在合法枚举)——报告,apply 不自动猜(缺语义);
            //   ② duplicate fields(同 key 多次出现,key 大小写不敏感)——apply 保留首条;
            //   ③ 标题状态标记污染(title_status_marker 命中)——apply 剥离;
            //   ④ 活动区出现终态 / 归档区出现非终态——报告,提示用 close/archive/reopen。
            // dry-run 默认:只报告不写入;apply=true 才落盘。幂等:重复 apply 无新变化。
            "normalize" => actions::normalize(self, input, ctx, &store, &mut entries),
            other => ToolOutput::error(format!(
                "unknown action `{other}`; valid: list | get | raw_lines | add | update | close | \
                 archive | reorder | raw_delete | {} | {} | {}",
                REOPEN_ACTION,
                FIX_TERMINAL_ACTION,
                REPAIR_ACTIONS.join(" | ")
            )),
        };
        self.finalize_action_result(&store, &action_str, ctx, output)
    }
}

/// Research 档回流桥接：只暴露既有条目的 get 与新草稿 add。
/// 真实写入仍复用 TrackerTool，确保 R-191 登记字段、ID、状态和原子写日志门禁不分叉。
pub struct ResearchTrackerTool {
    inner: TrackerTool,
}

impl ResearchTrackerTool {
    pub fn new(
        tool_name: &'static str,
        noun: &'static str,
        kind: &'static DocKind,
        requires_refs: Option<&'static DocKind>,
    ) -> Self {
        Self {
            inner: TrackerTool {
                tool_name,
                noun,
                kind,
                requires_refs,
            },
        }
    }
}

#[async_trait]
impl Tool for ResearchTrackerTool {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn description(&self) -> String {
        format!(
            "Read an existing {} with get(id), or add a draft for development review. \
             Existing record status cannot be changed through this research feedback tool.",
            self.inner.noun
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = self.inner.input_schema();
        if let Some(action) = schema
            .pointer_mut("/properties/action")
            .and_then(|value| value.as_object_mut())
        {
            action.insert("enum".into(), serde_json::json!(["get", "add"]));
        }
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        self.inner.resources(input)
    }

    async fn execute(&self, mut input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let action = input.get("action").and_then(|value| value.as_str());
        if !matches!(action, Some("get") | Some("add")) {
            return ToolOutput::error(
                "research 档的 req/defect 回流只允许 get 或 add；既有条目不可 list/update/close/archive",
            );
        }
        if action == Some("add") {
            let Some(object) = input.as_object_mut() else {
                return ToolOutput::error("research req/defect add input must be an object");
            };
            let fields = object
                .entry("fields")
                .or_insert_with(|| serde_json::json!({}));
            let Some(fields) = fields.as_object_mut() else {
                return ToolOutput::error("research req/defect add fields must be an object");
            };
            fields.insert("回流".into(), serde_json::json!("[todo]"));
        }
        self.inner.execute(input, ctx).await
    }
}

impl TrackerTool {
    fn finalize_action_result(
        &self,
        store: &DocStore,
        action: &str,
        ctx: &ToolCtx,
        mut output: ToolOutput,
    ) -> ToolOutput {
        // D-112 门禁:每次调用后对活动∪归档做缺号/重复检测,数据丢失立刻可见,
        // 而不是等 requirements 的依赖引用悬空才被发现。
        let action_succeeded = !output.is_error;
        if action_succeeded {
            match store
                .load()
                .and_then(|current| store.integrity_issues(&current))
            {
                Ok(issues) => {
                    if !issues.is_empty() {
                        output.content.push_str(&format!(
                            "\n⚠ tracker integrity ({}): {}",
                            self.kind.rel_path,
                            issues.join("; ")
                        ));
                    }
                }
                Err(error) => {
                    let state = if WRITE_ACTIONS.contains(&action) {
                        "tracker action already wrote its result, but post-write integrity verification failed; do not blindly retry"
                    } else {
                        "tracker result was read, but integrity verification failed"
                    };
                    output = ToolOutput::error(format!("{state}: {error}"));
                }
            }
        }
        // Preserve the action's actual successful write even if postcheck failed.
        if action_succeeded && WRITE_ACTIONS.contains(&action) {
            if let Err(error) = self.record_action_writes(store, action, ctx) {
                return ToolOutput::error(error);
            }
        }
        output
    }

    // R-268/D-569: one shared record of the real post-write fingerprints.
    // The caller decides whether a write succeeded; verification is a separate fact.
    fn record_action_writes(
        &self,
        store: &DocStore,
        action: &str,
        ctx: &ToolCtx,
    ) -> Result<(), String> {
        // 活动文件记写日志(D-398 收敛到共享 helper;路径+写后指纹+身份)。
        if let Ok(relative) = store.path.strip_prefix(&ctx.project_root) {
            crate::record_write_log(ctx, &relative.display().to_string(), &store.path)?;
        }
        // D-569:fix_terminal/archive_fill/normalize 也可能改写归档文件,必须和
        // archive 一样记录归档侧凭据,否则 bash 围栏会把合法修复回滚。
        if ["archive", FIX_TERMINAL_ACTION, "archive_fill", "normalize"].contains(&action) {
            let archive_file = store.archive_file();
            if let Ok(relative) = archive_file.strip_prefix(&ctx.project_root) {
                crate::record_write_log(ctx, &relative.display().to_string(), &archive_file)?;
            }
        }
        Ok(())
    }

    fn finalize_archive_move(
        &self,
        store: &DocStore,
        ctx: &ToolCtx,
        moved: Vec<String>,
    ) -> ToolOutput {
        if moved.is_empty() {
            return ToolOutput::ok("nothing to archive (no terminal entries)");
        }
        // Lower archive_terminal has succeeded. Still verify every moved ID,
        // including the last entry: integrity alone cannot detect its disappearance.
        let failure = match store.load_archive() {
            Err(error) => Some(format!("cannot verify archived entries: {error}")),
            Ok(archived) => {
                let lost: Vec<&String> = moved
                    .iter()
                    .filter(|id| !archived.iter().any(|entry| &&entry.id == id))
                    .collect();
                (!lost.is_empty()).then(|| format!(
                    "archive verification FAILED: {} missing from {} after the move — the entries may be lost, investigate immediately",
                    lost.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", "),
                    store.archive_file().display()
                ))
            }
        };
        if let Some(failure) = failure {
            // Unified finalizer sees an error below, so record the actual successful
            // lower write here once before returning that verification failure.
            if let Err(error) = self.record_action_writes(store, "archive", ctx) {
                return ToolOutput::error(error);
            }
            return ToolOutput::error(format!(
                "archive already wrote its result, but verification failed; do not blindly retry: {failure}"
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
            .and_then(kanzei_memory::docstore::declared_batch_progress)
            .map(|(_, total)| total);
        kanzei_memory::docstore::check_declared_batches(value, existing_total).err()
    }

    /// D-331:标题不得携带跨 DocKind 状态标记(`[done]`/`[dropped]` 等)——状态的家是
    /// header 方括号(引擎维护),写进标题会渲染成 `[dropped] [fixed]` 双终态污染。
    fn check_title(&self, title: &str) -> Option<String> {
        if let Some(marker) = kanzei_memory::docstore::title_status_marker(title) {
            return Some(format!(
                "title must not carry a status marker `[{marker}]` — the status lives in the \
                 header bracket (engine-managed); writing it into the title produces \
                 double-terminal headers like `[dropped] [fixed]` (D-331). Remove the marker \
                 from the title."
            ));
        }
        kanzei_memory::docstore::invalid_severity_marker(self.kind, title).map(|severity| {
            format!(
                "title must not carry an invalid severity suffix `({severity})` — defect severity \
                 is a separate engine-managed field and only accepts high | medium | low (D-569)."
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

    fn check_complexity(&self, fields: &BTreeMap<String, String>) -> Option<String> {
        if self.kind.priorities.is_none() || self.kind.severities.is_some() {
            return None;
        }
        let (_, value) = fields
            .iter()
            .find(|(key, _)| **key == "复杂度" || key.eq_ignore_ascii_case("complexity"))?;
        let value = value.trim();
        if ["小", "中", "大"].contains(&value) {
            None
        } else {
            Some(format!("invalid complexity `{value}`; valid: 小 | 中 | 大"))
        }
    }

    /// R-248:只有「核心 + refs 为空」的 requirement add 是机械新方向触发；
    /// 普通条目和已有引用的核心条目保持原路径。工件和豁免都是独立顶层字段，
    /// refs 继续只承载追踪编号。
    fn check_prior_art(
        &self,
        input: &TrackerInput,
        ctx: &ToolCtx,
        id: &str,
        title: &str,
    ) -> Result<Option<(String, String)>, String> {
        crate::prior_art::check_registration(
            ctx,
            crate::prior_art::RegistrationCheck {
                requirement: self.kind.prefix == "R",
                fields: &input.fields,
                refs_empty: input.refs.is_empty(),
                artifact: input.prior_art.as_deref(),
                waiver: input.prior_art_waiver.as_deref(),
                id,
                title,
            },
        )
    }

    /// R-191 登记硬约束:新建条目缺关键登记字段直接拒绝,并提示补什么,不静默放行。
    ///
    /// 触发:另一个项目的 agent 登记需求时漏掉复杂度评估——根因就是 add 只校验 title,
    /// 「复杂度/severity/priority/标签」全凭自觉。跨项目一致性不能靠每个项目各自记,
    /// 要在这里硬拦:req 必带 复杂度(小|中|大)+ 优先级 + 标签;defect 必带
    /// severity + 优先级 + 标签。idea/source/finding(severities/priorities/tags 均 None)
    /// 不受影响。
    fn check_add_required(&self, input: &TrackerInput) -> Option<String> {
        if self.kind.prefix == "R" {
            return None;
        }
        // 只有带 priorities 的追踪文档(req/defect)有登记硬约束;
        // idea/source/finding/memory/decision(priorities None)不受影响。
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

    /// R-252 验收④(内容④):想法转 split 的 refs 硬门禁——refs 必须非空,且每个
    /// ID 在 requirements/defects 的活跃或归档里真实存在;否则「已拆解」就是一句
    /// 空话(拆解没有产出任何真实 R/D,却声称 split)。只在想法线转 split 时触发。
    /// 2026-08-16 用户定调:B2 先只提交方法本身,接线(actions.rs update_close)
    /// 与正反测试在用户调整代码后继续——此期间方法无调用方,标 allow(dead_code)。
    #[allow(dead_code)]
    fn check_idea_split_gate(&self, ctx: &ToolCtx, refs: &[String]) -> Result<(), String> {
        if self.kind.prefix != "I" {
            return Ok(());
        }
        if refs.is_empty() {
            return Err(
                "想法转 split 必须携带 refs(产出的 R-/D- 编号);refs 为空时「已拆解」是空话,拒绝。"
                    .into(),
            );
        }
        // 硬门禁真源:requirements/defects 的活跃 ∪ 归档。归档条目也放行——
        // 想法拆解出的 R/D 可能随后就完成并归档了,不能因时序拒绝合法拆解。
        let mut existing: Vec<String> = Vec::new();
        for kind in [&REQUIREMENTS, &DEFECTS] {
            let store = DocStore::open(&ctx.project_root, kind);
            if let Ok(entries) = store.load() {
                existing.extend(entries.iter().map(|e| e.id.clone()));
            }
            if let Ok(entries) = store.load_archive() {
                existing.extend(entries.iter().map(|e| e.id.clone()));
            }
        }
        for id in refs {
            let id = id.trim();
            if id.is_empty() {
                continue;
            }
            if !(id.starts_with("R-") || id.starts_with("D-")) {
                return Err(format!(
                    "ref `{id}` 不是 R-/D- 编号;想法拆解只能产出需求或缺陷。"
                ));
            }
            if !existing.iter().any(|e| e == id) {
                return Err(format!(
                    "ref `{id}` 在 requirements/defects 的活跃或归档中都不存在;拆解必须指向真实条目。"
                ));
            }
        }
        Ok(())
    }

    fn check_refs(
        &self,
        ctx: &ToolCtx,
        refs: &[String],
        adding: bool,
        topic: Option<&str>,
    ) -> Result<(), String> {
        let Some(ref_kind) = self.requires_refs else {
            return Ok(());
        };
        let source_store = || {
            topic
                .filter(|_| matches!(ref_kind.prefix, "S" | "F"))
                .map(|topic| DocStore::open_topic(&ctx.project_root, ref_kind, topic))
                .transpose()
                .map(|store| store.unwrap_or_else(|| DocStore::open(&ctx.project_root, ref_kind)))
        };
        if refs.is_empty() {
            if adding {
                let available = source_store()
                    .map_err(|error| error.to_string())?
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
        let existing = source_store()
            .map_err(|error| error.to_string())?
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

#[cfg(test)]
mod tests;
