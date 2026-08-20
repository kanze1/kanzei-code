//! 通用追踪工具:req / defect / source / finding 共用一套 CRUD。
//! 硬门禁:ID 引擎分配、状态机受限、格式引擎序列化、引用必须存在——模型只提供字段值。

use std::collections::BTreeMap;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};

// R-204:取活调度域独立成模块(scheduling.rs),供 auto_run/CLI/docs/memory 四方统一消费。
// 本文件保留既有 pub 面(re-export),消费方调用点零改动;行为零变更。
pub mod scheduling;

#[cfg(test)]
mod scheduling_tests;

// R-204:每个 action 独立函数(actions.rs),execute 只剩路由。
mod actions;

#[cfg(test)]
pub(crate) use scheduling::block_reasons;
pub use scheduling::{
    append_progress, backlog_status, coupling_signals, dependency_states_from_documents,
    dependents_map, dependents_map_with_states, dispatch_verdict, schedule_for_display,
    schedule_for_display_with_states, workable_titles, DependencyStates, ScheduledEntry,
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
    /// add/update 的受控标签；写入文档字段「标签」。
    #[serde(default)]
    tag: Option<String>,
    /// requirement add/update 的复杂度(小|中|大)；写入文档字段「复杂度」。
    #[serde(default)]
    complexity: Option<String>,
    /// 自由字段,如 {"验收": "...", "复现": "..."}
    #[serde(default)]
    fields: BTreeMap<String, String>,
    /// 引用的条目 ID(finding 必须引用 source)
    #[serde(default)]
    refs: Vec<String>,
    /// R-248:先行调研工件的项目内相对路径。独立于 refs，避免把文件路径混入
    /// R-/D-/T- 引用契约。
    #[serde(default)]
    prior_art: Option<String>,
    /// R-248:用户明确豁免先行调研时的审计理由；与 prior_art 互斥。
    #[serde(default)]
    prior_art_waiver: Option<String>,
    /// B2:source/finding 所属课题目录(kebab-case)。
    #[serde(default)]
    topic: Option<String>,
    /// void_id 必填:这个编号为什么不该有条目、依据是什么
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
        if matches!(self.kind.prefix, "S" | "F") {
            d.push_str(" `topic` (lowercase kebab-case) is REQUIRED for source/finding actions; artifacts are stored under `.kanzei/research/<topic>/`.");
        }
        if self.kind.tags.is_some() {
            d.push_str(" On add, pass the controlled `tag` as a top-level field.");
        }
        if self.kind.priorities.is_some() && self.kind.severities.is_none() {
            d.push_str(" Requirement add also requires top-level `complexity` (小|中|大).");
        }
        if self.kind.prefix == "R" {
            d.push_str(" A core requirement with empty refs triggers R-248: pass top-level `prior_art` pointing to a validated `.kanzei/research/<topic>/prior-art.md`, or `prior_art_waiver` with the user's explicit reason. These fields are independent from refs.");
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
            if let Some(properties) = schema
                .pointer_mut("/properties")
                .and_then(|value| value.as_object_mut())
            {
                properties.remove("prior_art");
                properties.remove("prior_art_waiver");
            }
        }
        let mut required = vec!["title"];
        if self.kind.priorities.is_some() {
            required.push("priority");
        }
        if self.kind.severities.is_some() {
            required.push("severity");
        }
        if self.kind.tags.is_some() {
            required.push("tag");
        }
        if self.kind.priorities.is_some() && self.kind.severities.is_none() {
            required.push("complexity");
        }
        if self.requires_refs.is_some() {
            required.push("refs");
        }
        if matches!(self.kind.prefix, "S" | "F") {
            required.push("topic");
        }
        let mut then_schema = serde_json::json!({ "required": required });
        if self.requires_refs.is_some() {
            then_schema["properties"] = serde_json::json!({ "refs": { "minItems": 1 } });
        }
        schema["allOf"] = serde_json::json!([{
            "if": {
                "properties": { "action": { "const": "add" } },
                "required": ["action"]
            },
            "then": then_schema
        }]);
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
        let mut input: TrackerInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
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
        if WRITE_ACTIONS.contains(&input.action.as_str())
            && !REPAIR_ACTIONS.contains(&input.action.as_str())
            && input.action != FIX_TERMINAL_ACTION
            && input.action != "normalize"
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
            "list" => actions::list(self, input, ctx, &store, &mut entries),
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
        // R-268 批2:写动作成功且确实落盘后,记一条写日志(路径+写后指纹+身份)。
        // 这是围栏收口对账的归因凭据——bash 窗口内看到这个文档变了,查日志即可
        // 区分「专用工具的合法写入」与「shell 越界写」,写者从此不必等全局 bash
        // 静默。先写文档再记日志(「写后」凭据,见 write_log 模块头契约)。
        if !output.is_error && WRITE_ACTIONS.contains(&action_str.as_str()) {
            // 活动文件记写日志(D-398 收敛到共享 helper;路径+写后指纹+身份)。
            if let Ok(relative) = store.path.strip_prefix(&ctx.project_root) {
                crate::record_write_log(ctx, &relative.display().to_string(), &store.path);
            }
            // D-569:fix_terminal/archive_fill/normalize 也可能改写归档文件,必须和
            // archive 一样记录归档侧凭据,否则 bash 围栏会把合法修复回滚。
            if ["archive", FIX_TERMINAL_ACTION, "archive_fill", "normalize"]
                .contains(&action_str.as_str())
            {
                let archive_file = store.archive_file();
                if let Ok(relative) = archive_file.strip_prefix(&ctx.project_root) {
                    crate::record_write_log(ctx, &relative.display().to_string(), &archive_file);
                }
            }
        }
        output
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
        let mut description = format!(
            "Research 回流桥接，只允许 `get(id)` 读取既有 {}，或 `add(title, fields)` 登记一条待 dev 确认的草稿；禁止 list/update/close/archive。",
            self.inner.noun
        );
        if self.inner.kind.priorities.is_some() {
            description.push_str(" add 必须提供 priority；");
        }
        if self.inner.kind.severities.is_some() {
            description.push_str(" add 必须提供 severity；");
        }
        if self.inner.kind.tags.is_some() {
            description.push_str(" add 必须提供受控 tag；");
        }
        if self.inner.kind.priorities.is_some() && self.inner.kind.severities.is_none() {
            description.push_str(" requirement add 必须提供 complexity；");
        }
        description.push_str("草稿首状态由 tracker 固定为 todo/open，不得代替 dev 修改既有条目。");
        description
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
        if let Some(marker) = crate::docstore::title_status_marker(title) {
            return Some(format!(
                "title must not carry a status marker `[{marker}]` — the status lives in the \
                 header bracket (engine-managed); writing it into the title produces \
                 double-terminal headers like `[dropped] [fixed]` (D-331). Remove the marker \
                 from the title."
            ));
        }
        crate::docstore::invalid_severity_marker(self.kind, title).map(|severity| {
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
mod tests {
    use super::TrackerTool;
    use crate::docstore::{DocStore, Entry, DEFECTS, IDEAS, REQUIREMENTS};
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
    fn research_tracker_schema_only_exposes_get_and_add() {
        let tool = super::ResearchTrackerTool::new("req", "requirement", &REQUIREMENTS, None);
        let schema = tool.input_schema();
        let actions = schema
            .pointer("/properties/action/enum")
            .and_then(|value| value.as_array())
            .unwrap();
        assert_eq!(actions, &[json!("get"), json!("add")]);
        assert!(tool.description().contains("只允许 `get(id)` 读取"));
        assert!(tool
            .description()
            .contains("禁止 list/update/close/archive"));
    }

    #[tokio::test]
    async fn research_tracker_add_marks_todo_and_rejects_update() {
        let dir = std::env::temp_dir().join(format!(
            "kz-research-bridge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let tool = super::ResearchTrackerTool::new("req", "requirement", &REQUIREMENTS, None);
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let added = tool
            .execute(
                json!({
                    "action": "add",
                    "title": "研究回流草稿",
                    "priority": "P2",
                    "fields": {"复杂度": "小", "标签": "流程"}
                }),
                &ctx,
            )
            .await;
        assert!(!added.is_error, "{}", added.content);
        let entry = store
            .load()
            .unwrap()
            .into_iter()
            .find(|entry| entry.title == "研究回流草稿")
            .expect("research add 应落一条需求草稿");
        assert_eq!(entry.status, "todo");
        assert!(entry
            .fields
            .iter()
            .any(|(key, value)| key == "回流" && value == "[todo]"));

        let rejected = tool
            .execute(
                json!({"action": "update", "id": entry.id, "fields": {"进展": "越权"}}),
                &ctx,
            )
            .await;
        assert!(rejected.is_error, "research wrapper 不得允许 update");
        let fetched = tool
            .execute(json!({"action": "get", "id": entry.id}), &ctx)
            .await;
        assert!(
            !fetched.is_error,
            "research get 应读取已登记条目: {}",
            fetched.content
        );
        assert!(fetched.content.contains("研究回流草稿"));
        std::fs::remove_dir_all(dir).unwrap();
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

    /// R-232 验收①:同值 update 返回 no-op 且文件字节不变。
    /// 验收②:变更 update 返回 旧→新 摘要。
    #[tokio::test]
    async fn 同值update返回noop且文件字节不变_变更返回旧到新摘要() {
        let dir = std::env::temp_dir().join(format!("kz-idem-update-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("R-001");
        e.status = "doing".into();
        e.fields = vec![
            ("优先级".into(), "P1".into()),
            ("进展".into(), "2026-08-10 既有".into()),
        ];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let path = dir.join(".kanzei/project/requirements.md");

        // 同值 update:no-op + 文件字节不变。
        let before_bytes = std::fs::read(&path).unwrap();
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001",
                       "fields": {"优先级": "P1", "进展": "2026-08-10 既有"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("no-op"), "{}", out.content);
        let after_bytes = std::fs::read(&path).unwrap();
        assert_eq!(before_bytes, after_bytes, "同值 update 必须零写入");

        // 变更 update:返回 旧→新 摘要且落盘。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"优先级": "P2"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("优先级: P1 → P2"), "{}", out.content);
        let after_bytes = std::fs::read(&path).unwrap();
        assert!(
            after_bytes.windows(b"P2".len()).any(|w| w == b"P2"),
            "变更必须落盘"
        );
        assert_ne!(before_bytes, after_bytes, "变更必须改变文件");

        // 变更后同值再 update:又是 no-op。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"优先级": "P2"}}),
                &ctx,
            )
            .await;
        assert!(out.content.contains("no-op"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-306 验收⑤:未合并观测 head 的活动条目不得关闭。
    #[tokio::test]
    async fn close拒绝未进入当前祖先链的observed_head() {
        let dir = std::env::temp_dir().join(format!("kz-close-ancestor-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("R-001");
        e.fields = vec![
            ("标签".into(), "核心".into()),
            ("批次".into(), "1/1".into()),
            (
                "observed_head".into(),
                "0000000000000000000000000000000000000000".into(),
            ),
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
        assert!(out.is_error, "{}", out.content);
        assert!(
            out.content.contains("不在当前 HEAD 祖先链"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("拒绝关闭") && out.content.contains("收编"),
            "{}",
            out.content
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-232 验收③:close 幂等重入安全——已 done 条目再次 close 返回 no-op,
    /// 不再跑关闭门禁(前端冒烟/批次/分类断言),且文件字节不变。
    #[tokio::test]
    async fn close幂等重入_已终态条目再次关闭返回noop() {
        let dir = std::env::temp_dir().join(format!("kz-idem-close-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("R-001");
        e.status = "done".into();
        e.fields = vec![
            ("标签".into(), "前端".into()),
            ("批次".into(), "1/1".into()),
        ];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let path = dir.join(".kanzei/project/requirements.md");
        let before_bytes = std::fs::read(&path).unwrap();

        // 已 done 的前端标签条目:无前端冒烟 passed 也不应被 R-228 拦(重入非新关闭)。
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("no-op"), "{}", out.content);
        let after_bytes = std::fs::read(&path).unwrap();
        assert_eq!(before_bytes, after_bytes, "重入 close 必须零写入");
        let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(saved[0].status, "done", "状态不得回退");

        // 重入带字段补写:不是纯 no-op,允许落盘(有真实变更)。
        let out = tool
            .execute(
                json!({"action": "close", "id": "R-001", "fields": {"进展": "2026-08-16 补记"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("进展"), "{}", out.content);
        let after_bytes = std::fs::read(&path).unwrap();
        assert_ne!(before_bytes, after_bytes, "补字段应落盘");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 2026-08-16 审计门禁:验收条款对账——带圈条款号必须在关闭进展中逐条覆盖并带
    /// 证据锚(T-/file:line/提交号),否则显式降级;沉默跳过即拒。真伪由波次审计另查。
    #[tokio::test]
    async fn 验收条款对账_沉默降级拒关_带锚或显式降级放行() {
        let dir = std::env::temp_dir().join(format!("kz-acc-rec-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let store = || DocStore::open(&dir, &REQUIREMENTS);
        let with_fields = |fields: Vec<(&str, &str)>| {
            let mut e = entry("R-001");
            e.fields = fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            e
        };

        // ② 在进展中完全未提及:拒关并点名 ②。
        let e = with_fields(vec![
            ("验收", "①编译成功有实测;②截图被模型消费"),
            ("进展", "关闭:①T-1786000000 实测通过"),
        ]);
        store().save(&[e]).unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains('②'), "{}", out.content);
        assert!(out.content.contains("未提及"), "{}", out.content);

        // 条款提及了但 400 字符邻域内没有任何证据锚:拒关。
        let e = with_fields(vec![
            ("验收", "①编译成功有实测"),
            ("进展", "关闭:①做完了,效果很好,大家都说好"),
        ]);
        store().save(&[e]).unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("无证据锚"), "{}", out.content);

        // 逐条带锚(T- 记录 / file:line / 提交号)放行。
        let e = with_fields(vec![
            ("验收", "①编译成功有实测;②截图被模型消费;③发版"),
            (
                "进展",
                "关闭:①T-1786000000 实测;②见 crates/kanzei-tools/src/plot_tool.rs:185;\
                 ③随 build-9a06e05 发布",
            ),
        ]);
        store().save(&[e]).unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);

        // 显式降级与「由用户」缓办是合法覆盖形态:放行。
        let e = with_fields(vec![
            ("验收", "①真机全链路实测;②锁屏恢复"),
            (
                "进展",
                "关闭:①验收降级: 真机实测改为 viewport 自检,真机部分待补;②由用户执行",
            ),
        ]);
        store().save(&[e]).unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);

        // 无编号条款的验收不受影响(门禁只认带圈数字)。
        let e = with_fields(vec![
            ("验收", "一句话验收,无编号条款"),
            ("进展", "关闭:做完了"),
        ]);
        store().save(&[e]).unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-227 验收②配套:archive_fill 通过 tracker 动作回填归档条目占位符。
    #[tokio::test]
    async fn archive_fill_回填归档占位符_缺参报错() {
        let dir = std::env::temp_dir().join(format!("kz-archive-fill-tool-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        // 走正常归档路径:活动条目标记 done → archive_terminal 落归档,满足完整性门禁。
        // 注意用 R-001 低位 id:UNACCOUNTED 检查是 1..=max 连续序列,R-198 会触发
        // R-001..R-197 缺失(execute 入口有完整性门禁;docstore 层测试可直接用 R-198)。
        let mut e = entry("R-001");
        e.status = "done".into();
        e.fields = vec![(
            "进展".into(),
            "全量 cargo test --workspace 全绿(T-1786565xxx,harness 118)".into(),
        )];
        store.save(&[e]).unwrap();
        store.archive_terminal().unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        // 缺 old/new → 报错。
        let out = tool
            .execute(json!({"action": "archive_fill", "id": "R-001"}), &ctx)
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("old"), "{}", out.content);

        // 正常回填。
        let out = tool
            .execute(
                json!({"action": "archive_fill", "id": "R-001",
                       "old": "T-1786565xxx", "new": "T-1786565346"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("T-1786565346"), "{}", out.content);
        let archived = store.load_archive().unwrap();
        let r001 = archived.iter().find(|e| e.id == "R-001").unwrap();
        assert!(r001
            .fields
            .iter()
            .any(|(_, v)| v.contains("T-1786565346") && !v.contains("xxx")));

        // 找不到占位符 → 报错。
        let out = tool
            .execute(
                json!({"action": "archive_fill", "id": "R-001",
                       "old": "T-9999999999xxx", "new": "T-1786565346"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
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

    /// R-228 验收①③:带「前端」标签的条目关闭前必须已有前端冒烟 passed 测试记录
    /// ——没有则拒绝(验收①);非前端标签条目不受影响(验收③)。
    #[tokio::test]
    async fn 前端标签关闭需前端冒烟passed_非前端不受影响() {
        let dir = std::env::temp_dir().join(format!("kz-frontend-close-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut front = entry("R-001");
        front.status = "doing".into();
        front.fields = vec![("标签".into(), "前端".into())];
        let mut backend = entry("R-002");
        backend.status = "doing".into();
        backend.fields = vec![("标签".into(), "核心".into())];
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[front, backend])
            .unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        // 无任何前端冒烟 passed:前端标签条目关闭被拒,非前端放行。
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(
            out.is_error && out.content.contains("前端"),
            "前端标签条目无前端冒烟 passed 必须拒绝关闭: {}",
            out.content
        );
        let out = tool
            .execute(json!({"action": "close", "id": "R-002"}), &ctx)
            .await;
        assert!(
            !out.is_error,
            "非前端标签条目不受前端门禁影响: {}",
            out.content
        );

        // 补一条前端冒烟 passed:前端标签条目可关闭。
        let rec_dir = dir.clone();
        crate::test_record::append_test_run(
            &rec_dir,
            "node scripts/ui-runtime-smoke.mjs (R-228)",
            "passed",
            Some("node scripts/ui-runtime-smoke.mjs"),
            None,
            None,
        )
        .unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(
            !out.is_error,
            "有前端冒烟 passed 后前端标签条目应可关闭: {}",
            out.content
        );
        let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(saved[0].status, "done", "R-001 应已关闭");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-229 验收①:关闭证据含「剩余/其余 N 处」式分类断言但引证数不足 → 拒绝关闭。
    /// 验收②:R-199 式未核实分类断言(「剩余 3 处均为 X」无任何 file:line)在门禁层不可复现。
    #[tokio::test]
    async fn 分类断言引证不足拒绝关闭_r199式无引证不可复现() {
        let dir = std::env::temp_dir().join(format!("kz-class-close-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("R-001");
        e.status = "doing".into();
        // R-199 式原文:断言剩余 3 处,但没有任何 file:line 引证。
        e.fields = vec![(
            "进展".into(),
            "剩余 3 处 autoContinueAllowed 为「开关启动门禁」(勾选时提示),非续跑否决".into(),
        )];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };

        // 断言声称 3 处,引证 0 处 → 拒。
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(out.is_error, "引证不足必须拒绝: {}", out.content);
        assert!(out.content.contains("R-229"), "{}", out.content);
        assert!(out.content.contains("3"), "{}", out.content);

        // 引证数不足(2 < 3)仍拒。
        let mut e = entry("R-001");
        e.status = "doing".into();
        e.fields = vec![(
            "进展".into(),
            "剩余 3 处 autoContinueAllowed 为「开关启动门禁」:ui/08-compose.js:155、\
             ui/07-events.js:88 两处已核,第三处待核"
                .into(),
        )];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(out.is_error, "引证 2 < 3 必须拒绝: {}", out.content);

        // 引证数足够(3 == 3)→ 放行。
        let mut e = entry("R-001");
        e.status = "doing".into();
        e.fields = vec![(
            "进展".into(),
            "剩余 3 处 autoContinueAllowed 为「开关启动门禁」(勾选时提示),非续跑否决: \
             ①ui/08-compose.js:155 ②ui/07-events.js:88 ③ui/07-events.js:90"
                .into(),
        )];
        DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
        let out = tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(!out.is_error, "引证足够应放行: {}", out.content);
        let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(saved[0].status, "done", "R-001 应已关闭");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-229 验收③:无分类断言的关闭不受影响;「剩余价值」这类无数字用法不算断言。
    #[tokio::test]
    async fn 无分类断言关闭不受影响() {
        let dir = std::env::temp_dir().join(format!("kz-class-close-free-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let mut e = entry("R-001");
        e.status = "doing".into();
        // 无「剩余/其余 N 处」断言,仅普通叙述;含「剩余价值」非断言用法。
        e.fields = vec![("进展".into(),
            "实现已落地,验证通过。剩余价值是声明与检测入口。crates/kanzei-tools/src/tracker.rs:100 为既有实现。".into())];
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
        assert!(!out.is_error, "无分类断言不受影响: {}", out.content);
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

    #[tokio::test]
    async fn normalize_dry_run_reports_and_apply_fixes() {
        // D-332 验收②:normalize 是统一 repair surface——dry-run 只报告,
        // apply 机械修复(重复字段去重、标题标记剥离、显式 status 修正非法 lifecycle),
        // 幂等(重复 apply 无新变化)。
        let dir = std::env::temp_dir().join(format!(
            "kz-normalize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        // 构造污染:R-001 非法 lifecycle [open] + 标题带 [done] 标记 + 重复「优先级」字段;
        // R-002 合法 todo。
        let e1 = Entry {
            id: "R-001".into(),
            title: "某需求 [done] 标题".into(),
            status: "open".into(), // requirement 无 open,非法
            severity: None,
            fields: vec![
                ("优先级".into(), "P1".into()),
                ("优先级".into(), "P2".into()), // 重复
                ("标签".into(), "核心".into()),
            ],
        };
        let e2 = Entry {
            id: "R-002".into(),
            title: "正常需求".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![("优先级".into(), "P0".into())],
        };
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store.save(&[e1, e2]).unwrap();

        // dry-run:只报告,不写入。
        let out = tool.execute(json!({"action": "normalize"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("dry-run"), "{}", out.content);
        assert!(out.content.contains("R-001"), "{}", out.content);
        assert!(
            out.content.contains("invalid lifecycle `open`"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("duplicate field `优先级`"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("title status marker `[done]`"),
            "{}",
            out.content
        );
        // dry-run 不写盘
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded[0].status, "open",
            "dry-run 不得改状态: {:?}",
            loaded[0].status
        );
        assert_eq!(
            loaded[0].fields.len(),
            3,
            "dry-run 不得去重: {:?}",
            loaded[0].fields
        );

        // apply + status 修正:非法 lifecycle → todo,标题标记剥离,重复字段去重。
        let out = tool
            .execute(
                json!({"action": "normalize", "apply": true, "status": "todo"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("apply"), "{}", out.content);
        assert!(
            out.content.contains("lifecycle `open` → `todo`"),
            "{}",
            out.content
        );
        assert!(out.content.contains("deduplicated"), "{}", out.content);
        assert!(
            out.content.contains("stripped title status marker"),
            "{}",
            out.content
        );

        let loaded = store.load().unwrap();
        let r1 = loaded.iter().find(|x| x.id == "R-001").unwrap();
        assert_eq!(r1.status, "todo", "非法 lifecycle 应被修正: {}", r1.status);
        assert!(
            !r1.title.contains("[done]"),
            "标题标记应被剥离: {}",
            r1.title
        );
        assert_eq!(r1.title, "某需求 标题", "标题应保留其余文字: {}", r1.title);
        let prio: Vec<_> = r1.fields.iter().filter(|(k, _)| k == "优先级").collect();
        assert_eq!(prio.len(), 1, "重复字段应去重: {:?}", r1.fields);

        // 幂等:再次 dry-run 应无待修项。
        let out = tool.execute(json!({"action": "normalize"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("无待修项") || !out.content.contains("R-001"),
            "{}",
            out.content
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn normalize_archived_non_terminal_reports_and_keeps_status() {
        // D-336:归档区非终态 lifecycle 仍只报告(修复通道是 fix_terminal 纠错),
        // apply 不改归档 status;归档**重复字段**由 apply 走 dedupe_archived_fields
        // 修复(见 dedupe_archived_fields_merges_progress_and_keeps_first_of_others)。
        let dir = std::env::temp_dir().join(format!(
            "kz-normalize-arch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        let store = DocStore::open(&dir, &DEFECTS);
        // 直接写归档文件,模拟手改产生的归档区非终态条目。
        std::fs::write(
            store.archive_file(),
            "# Defects\n\n## D-001 已修缺陷 [open] (medium)\n",
        )
        .unwrap();
        std::fs::write(store.path.clone(), "# Defects\n").unwrap();

        let out = tool
            .execute(json!({"action": "normalize", "apply": true}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("archived D-001") && out.content.contains("non-terminal"),
            "归档区非终态应被报告: {}",
            out.content
        );
        // 归档非终态 status 不被 apply 改动(fix_terminal 才是纠错通道)
        let archived = store.load_archive().unwrap();
        assert_eq!(
            archived[0].status, "open",
            "apply 不得改动归档 status: {}",
            archived[0].status
        );

        std::fs::remove_dir_all(dir).ok();
    }

    /// D-358:apply 真去重了就得报出来。归档去重原先跑在输出拼装**之后**,
    /// push 进 fixed 的条目一条也进不了 content——实测修了 6 条却报「0 fix(es)」、
    /// 连「已修复」段都没有。工具少报自己的工作,在证据驱动的流程里等于说谎:
    /// 上一轮正是照着这个输出把 D-333 验收③判成不可修、挂了个用户阻塞。
    /// dry-run 的 findings 文案同步改成「apply 可自动收敛」,不再说「需手动整理归档」。
    #[tokio::test]
    async fn normalize_apply_如实报出归档去重条数() {
        let dir = std::env::temp_dir().join(format!(
            "kz-normalize-report-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let tool = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        };
        let store = DocStore::open(&dir, &DEFECTS);
        // 归档条目带两份「进展」:D-330/D-331 修复前的存量形态。
        std::fs::write(
            store.archive_file(),
            "# Defects\n\n## D-001 已修缺陷 [fixed] (medium)\n\
             - 进展: 第一段证据\n- 进展: 第二段证据\n",
        )
        .unwrap();
        std::fs::write(store.path.clone(), "# Defects\n").unwrap();

        // dry-run:报告要指向 apply 可修,不能再说「需手动整理归档」。
        let dry = tool.execute(json!({"action": "normalize"}), &ctx).await;
        assert!(!dry.is_error, "{}", dry.content);
        assert!(
            dry.content.contains("apply 可自动收敛") && !dry.content.contains("需手动整理归档"),
            "dry-run 文案不得否认 apply 的能力: {}",
            dry.content
        );

        // apply:计数与「已修复」段都要如实带上归档去重。
        let out = tool
            .execute(json!({"action": "normalize", "apply": true}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            !out.content.contains("0 fix(es)"),
            "真去重了就不能报 0 fix(es): {}",
            out.content
        );
        assert!(
            out.content.contains("已修复") && out.content.contains("D-001"),
            "「已修复」段必须列出被去重的归档条目: {}",
            out.content
        );
        // 复查:重复字段真的收敛了(报告与事实一致,不是只改了输出)。
        let again = tool.execute(json!({"action": "normalize"}), &ctx).await;
        assert!(
            !again.content.contains("duplicate field"),
            "apply 之后不该再有重复字段: {}",
            again.content
        );

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
                json!({"action": "add", "title": "另一条", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "批次": "0/5"}}),
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
                json!({"action": "add", "title": "十批条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "批次": "0/10"}}),
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
                json!({"action": "add", "title": "新条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端"}}),
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
            .execute(json!({"action": "add", "title": "恢复后可写", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端"}}), &ctx)
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
            project.join("ideas.md"),
            "# Ideas\n\n## I-001 原始想法 [inbox]\n- 进展: 旧\n\n## I-002 新想法 [inbox]\n- 来源: R-093\n",
        )
        .unwrap();
        std::fs::write(
            project.join("ideas-archive.md"),
            "# Ideas Archive\n\n## I-002 旧想法 [split]\n- 验收: 拆解即 `idea update I-002 split`\n\n手写归档说明 I-002 不能丢\n\n## I-003 另一想法 [split]\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        let repaired = tool
            .execute(json!({"action": "repair_reused_id", "id": "I-002"}), &ctx)
            .await;
        assert!(!repaired.is_error, "{}", repaired.content);
        assert!(repaired.content.contains("I-004"), "{}", repaired.content);
        let active = std::fs::read_to_string(project.join("ideas.md")).unwrap();
        let archive = std::fs::read_to_string(project.join("ideas-archive.md")).unwrap();
        assert!(active.contains("## I-002 新想法 [inbox]"));
        assert!(archive.contains("## I-004 旧想法 [split]"));
        assert!(archive.contains("idea update I-004 split"));
        assert!(archive.contains("手写归档说明 I-004 不能丢"));

        let updated = tool
            .execute(
                json!({"action": "update", "id": "I-001", "fields": {"进展": "新"}}),
                &ctx,
            )
            .await;
        assert!(
            !updated.is_error,
            "修复后普通写操作应恢复: {}",
            updated.content
        );
        let store = DocStore::open(&dir, &IDEAS);
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
            .execute(json!({"action": "add", "title": "恢复后可写", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端"}}), &ctx)
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
        assert!(schema["properties"].get("complexity").is_none());
        let defect_required = schema["allOf"][0]["then"]["required"].as_array().unwrap();
        for field in ["title", "severity", "priority", "tag"] {
            assert!(defect_required.iter().any(|value| value == field));
        }
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
        assert_eq!(
            schema["properties"]["complexity"]["enum"],
            json!(["小", "中", "大"])
        );
        assert_eq!(
            schema["properties"]["tag"]["enum"],
            json!(["核心", "后端", "前端", "模型", "发布", "流程"])
        );
        let req_required = schema["allOf"][0]["then"]["required"].as_array().unwrap();
        for field in ["title", "priority", "tag", "complexity"] {
            assert!(req_required.iter().any(|value| value == field));
        }
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

    /// D-398:tracker archive 动作写活动+归档两个文件——都记写日志
    /// (归档侧不记则新增被围栏回滚,条目从两份同时消失 D-112)。
    #[tokio::test]
    async fn archive_写日志含活动与归档文件() {
        let dir = std::env::temp_dir().join(format!("kz-arch-log-{}", std::process::id()));
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
        let logs = crate::write_log::entries_after(&dir, 0);
        let active_rel = store
            .path
            .strip_prefix(&dir)
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        let archive_rel = store
            .archive_file()
            .strip_prefix(&dir)
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        let paths: Vec<String> = logs.iter().map(|l| l.path.clone()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with(&active_rel)),
            "活动文件应有写日志: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.ends_with(&archive_rel)),
            "归档文件应有写日志(否则归档侧新增被回滚): {paths:?}"
        );
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
    // 新建 defect 缺 severity 即拒;补全后放行;idea(severities/priorities/tags
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
                       "fields": {"复杂度": "中", "标签": "后端"}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);

        // schema 暴露的顶层字段必须直接可执行，不能再逼模型猜 fields 中文键。
        let out = req_tool
            .execute(
                json!({"action": "add", "title": "顶层字段", "priority": "P2",
                       "complexity": "小", "tag": "后端"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let req_text = std::fs::read_to_string(dir.join(REQUIREMENTS.rel_path)).unwrap();
        assert!(req_text.contains("- 复杂度: 小"), "{req_text}");
        assert!(req_text.contains("- 标签: 后端"), "{req_text}");
        let invalid = req_tool
            .execute(
                json!({"action": "add", "title": "非法复杂度", "priority": "P2",
                       "complexity": "巨大", "tag": "后端"}),
                &ctx,
            )
            .await;
        assert!(invalid.is_error);
        assert!(
            invalid.content.contains("valid: 小 | 中 | 大"),
            "{}",
            invalid.content
        );

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

        // idea(无必填 kind 字段):裸 add 不受影响。
        let idea_tool = TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        };
        let out = idea_tool
            .execute(json!({"action": "add", "title": "想法"}), &ctx)
            .await;
        assert!(!out.is_error, "idea 裸 add 不应被拦: {}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn tag_validation_skips_documents_without_vocabulary() {
        let dir = std::env::temp_dir().join(format!(
            "kz-tag-idea-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &IDEAS);
        store.save(&[entry("I-001")]).unwrap();
        let tool = TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        // 无词表的文档:任意标签值放行,不受校验约束。
        let out = tool
            .execute(
                json!({"action": "update", "id": "I-001", "fields": {"标签": "任意值"}}),
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
                            json!({"action": "add", "title": format!("并发条目 {n}"), "priority": "P2", "fields": {"复杂度": "中", "标签": "后端"}}),
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

    /// R-252 验收③:想法转 split 的 refs 硬门禁——refs 空拒、指向不存在的 ID 拒、
    /// 指向 requirements/defects 活跃或归档条目放行。走真实 tool.execute 链路。
    #[tokio::test]
    async fn idea_split_refs_gate_positive_and_negative() {
        let dir = std::env::temp_dir().join(format!(
            "kz-idea-split-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = dir.join(".kanzei/project");
        std::fs::create_dir_all(&project).unwrap();
        // 想法线:一条 inbox 待拆解。
        std::fs::write(
            project.join("ideas.md"),
            "# Ideas\n\n## I-001 待拆解的想法 [inbox]\n",
        )
        .unwrap();
        // requirements:活跃 R-001 + 归档 R-002(defects 同构,归档放行依赖它)。
        std::fs::write(
            project.join("requirements.md"),
            "# Requirements\n\n## R-001 活跃需求 [todo]\n",
        )
        .unwrap();
        std::fs::write(
            project.join("requirements-archive.md"),
            "# Requirements Archive\n\n## R-002 已归档需求 [done]\n",
        )
        .unwrap();
        std::fs::write(
            project.join("defects.md"),
            "# Defects\n\n## D-001 活跃缺陷 [open]\n",
        )
        .unwrap();
        std::fs::write(
            project.join("defects-archive.md"),
            "# Defects Archive\n\n## D-002 已归档缺陷 [fixed]\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());

        // 反面 1:refs 为空 → 拒转 split。
        let out = tool
            .execute(
                json!({"action": "update", "id": "I-001", "status": "split", "refs": []}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "refs 空必须拒: {}", out.content);
        assert!(out.content.contains("refs"), "{}", out.content);
        // 反面 2:refs 指向不存在的 ID → 拒。
        let out = tool
            .execute(
                json!({"action": "update", "id": "I-001", "status": "split", "refs": ["R-999"]}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "指向不存在的 ID 必须拒: {}", out.content);
        assert!(out.content.contains("R-999"), "{}", out.content);
        // 反面 3:refs 指向非 R/D 编号 → 拒。
        let out = tool
            .execute(
                json!({"action": "update", "id": "I-001", "status": "split", "refs": ["S-001"]}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "非 R/D 编号必须拒: {}", out.content);
        // 正面 1:refs 指向活跃 R-001 + D-001 → 放行,状态转 split。
        let out = tool
            .execute(
                json!({"action": "update", "id": "I-001", "status": "split", "refs": ["R-001", "D-001"]}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "指向活跃条目应放行: {}", out.content);
        let store = DocStore::open(&dir, &IDEAS);
        let entries = store.load().unwrap();
        assert_eq!(entries[0].status, "split");
        assert_eq!(
            entries[0].refs(),
            vec!["R-001", "D-001"],
            "转 split 的 refs 必须落到条目字段"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-252 验收③:refs 指向归档条目放行——拆解产出的 R/D 可能随后完成并归档,
    /// 不能因时序拒绝合法拆解。独立夹具,不受上一条测试写入影响。
    #[tokio::test]
    async fn idea_split_refs_gate_allows_archived_targets() {
        let dir = std::env::temp_dir().join(format!(
            "kz-idea-split-gate-arch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = dir.join(".kanzei/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("ideas.md"),
            "# Ideas\n\n## I-001 待拆解的想法 [inbox]\n",
        )
        .unwrap();
        // requirements 活动区为空,只有归档里的 R-002。
        std::fs::write(project.join("requirements.md"), "# Requirements\n").unwrap();
        std::fs::write(
            project.join("requirements-archive.md"),
            "# Requirements Archive\n\n## R-002 已归档需求 [done]\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let out = tool
            .execute(
                json!({"action": "update", "id": "I-001", "status": "split", "refs": ["R-002"]}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "指向归档条目应放行: {}", out.content);
        let entries = DocStore::open(&dir, &IDEAS).load().unwrap();
        assert_eq!(entries[0].status, "split");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-252 验收③:非想法线不受 split 门禁约束——req 转 split 不是合法状态,
    /// 但门禁逻辑只对 prefix==I 触发,req 的既有状态机校验照常兜底(不因门禁报
    /// 错被误伤)。
    #[tokio::test]
    async fn idea_split_gate_skips_non_idea_lines() {
        let dir = std::env::temp_dir().join(format!(
            "kz-idea-split-gate-req-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = dir.join(".kanzei/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("requirements.md"),
            "# Requirements\n\n## R-001 活跃需求 [todo]\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        // split 不是 req 的合法状态:应被状态机拒,而不是被 split 门禁拦(无 refs 也
        // 不该报 refs 相关错误——门禁对非 I 线完全跳过)。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "status": "split"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "req 不能转 split: {}", out.content);
        assert!(
            !out.content.contains("refs"),
            "非想法线不应触发 refs 门禁: {}",
            out.content
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-268 批2:写动作(add)成功后必须产出写日志——围栏收口对账的归因凭据,
    /// 「专用工具写文档」与「bash 越界写」由此可区分。
    #[tokio::test]
    async fn 写动作产出写日志_路径指纹与身份齐备() {
        let dir = std::env::temp_dir().join(format!(
            "kz-writelog-{}-{}",
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone()).with_identity(
            "wt".into(),
            "key".into(),
            "run-1".into(),
            "proc-1".into(),
        );
        let out = tool
            .execute(
                json!({"action": "add", "title": "写日志验证条目", "priority": "P3", "tag": "后端", "complexity": "小"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);

        // 写日志落盘,且路径/指纹/身份与本次写入一致。
        let logs = crate::write_log::entries_after(&dir, 0);
        assert_eq!(logs.len(), 1, "一次 add 应产出一条写日志");
        let log = &logs[0];
        assert_eq!(
            log.path, ".kanzei/project/requirements.md",
            "日志路径必须指向被写文档"
        );
        let on_disk = std::fs::read(&store.path).unwrap();
        assert_eq!(
            log.fingerprint,
            crate::content_hash(&on_disk),
            "日志指纹必须等于写后内容指纹"
        );
        assert_eq!(log.run_id.as_deref(), Some("run-1"));
        assert_eq!(log.process_id.as_deref(), Some("proc-1"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-248 验收①④⑥:核心+空 refs 机械触发，完整工件/用户豁免可审计放行；
    /// 普通条目保持既有登记路径。
    #[tokio::test]
    async fn 核心空refs触发prior_art门禁_普通条目与豁免不受阻() {
        let dir = std::env::temp_dir().join(format!(
            "kz-prior-art-tracker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::create_dir_all(dir.join("docs/design")).unwrap();
        std::fs::write(dir.join("docs/design/base.md"), "# 基线\n已有设计\n").unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let core_input = json!({
            "action": "add",
            "title": "新的核心方向",
            "priority": "P1",
            "complexity": "中",
            "tag": "核心"
        });
        let blocked = tool.execute(core_input.clone(), &ctx).await;
        assert!(blocked.is_error, "缺 prior-art 必须拒绝");
        assert!(blocked
            .content
            .contains("CORE_REQUIREMENT_PRIOR_ART_REQUIRED"));
        let topic = crate::prior_art::requirement_topic("R-001", "新的核心方向");
        let relative = format!(".kanzei/research/{topic}/prior-art.md");
        let artifact = dir.join(&relative);
        assert!(artifact.is_file(), "拒绝时必须同时创建可继续填写的骨架");
        std::fs::write(
            &artifact,
            format!(
                "---\nkind: prior_art\ntopic: {topic}\nstatus: complete\ntrigger: core_requirement\nentry_refs: R-001\nwebsearch_round_limit: 3\n---\n\n# 对照\n\n## 外部已有实现\n\n### upstream\n- 出处: https://example.test/upstream\n- 证据等级: V1\n- 差异: 上游只覆盖单机\n- 决策: 采用数据结构\n\n## 仓内既有设计\n\n### baseline\n- 出处: file:docs/design/base.md:2\n- 证据等级: V2\n- 差异: 仓内缺少自动触发\n- 决策: 保留现有 tracker 并补门禁\n"
            ),
        )
        .unwrap();
        let mut with_artifact = core_input;
        with_artifact["prior_art"] = json!(relative);
        let added = tool.execute(with_artifact, &ctx).await;
        assert!(!added.is_error, "有效工件应放行: {}", added.content);
        let entries = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(entries[0].id, "R-001");
        assert!(entries[0]
            .fields
            .iter()
            .any(|(key, value)| key == "先行调研" && value.ends_with("prior-art.md")));

        let ordinary = tool
            .execute(
                json!({"action": "add", "title": "普通后端需求", "priority": "P2", "complexity": "小", "tag": "后端"}),
                &ctx,
            )
            .await;
        assert!(!ordinary.is_error, "普通条目不应触发: {}", ordinary.content);

        let waived = tool
            .execute(
                json!({"action": "add", "title": "用户要求直接推进", "priority": "P1", "complexity": "小", "tag": "核心", "prior_art_waiver": "用户明确要求复用已知内部方案并直接实施"}),
                &ctx,
            )
            .await;
        assert!(!waived.is_error, "明确豁免应放行: {}", waived.content);
        let entries = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert!(entries[2]
            .fields
            .iter()
            .any(|(key, value)| key == "先行调研豁免" && value.contains("用户明确")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn prior_art字段只出现在requirement_schema() {
        let requirement = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        }
        .input_schema();
        let defect = TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        }
        .input_schema();
        assert!(requirement.pointer("/properties/prior_art").is_some());
        assert!(requirement
            .pointer("/properties/prior_art_waiver")
            .is_some());
        assert!(defect.pointer("/properties/prior_art").is_none());
        assert!(defect.pointer("/properties/prior_art_waiver").is_none());
    }
}
