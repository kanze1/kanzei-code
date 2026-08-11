//! memory-manager 子代理(M2/R-105):记忆的唯一写路径。
//! 主 agent 只投递草稿(memory_note),本组件持有的迷你 run 决定
//! ADD/UPDATE/MERGE/STALE/NOOP——写读分离,防止主 agent 顺手写出垃圾记忆。

use std::sync::Arc;

use async_trait::async_trait;
use kanzei_harness::{
    rule, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileScope, ResolveCtx, Tool,
    ToolCtx, ToolOutput,
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{AddOutcome, MemoryStore};

fn store_for(ctx: &ToolCtx, scope: &str) -> anyhow::Result<MemoryStore> {
    match scope {
        "project" => Ok(MemoryStore::project(&ctx.project_root)),
        "global" => {
            MemoryStore::global().ok_or_else(|| anyhow::anyhow!("no home dir for global scope"))
        }
        other => anyhow::bail!("invalid scope `{other}`; valid: global | project"),
    }
}

#[derive(Deserialize, JsonSchema)]
struct PromoteInput {
    /// candidate 记忆的 id(manager 先 memory_add 得到)
    id: String,
    /// 至少一条 episode 证据(provenance 硬约束,R-165)。
    /// episode_id 必须真实存在于 state.db episodes 表;event_start/end 可空。
    sources: Vec<PromoteSource>,
    /// 证据哈希,默认 "compiler"
    #[serde(default)]
    source_hash: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct PromoteSource {
    episode_id: i64,
    #[serde(default)]
    event_start: Option<i64>,
    #[serde(default)]
    event_end: Option<i64>,
}

/// R-165 PROMOTE:candidate → active,provenance 硬约束(至少一条 memory_sources)。
/// manager 编译出记忆后必须先 memory_add(candidate)再 memory_promote(带证据)——
/// 无来源证据的记忆永远无法进入检索注入。
pub struct MemoryPromoteTool;

#[async_trait]
impl Tool for MemoryPromoteTool {
    fn name(&self) -> &'static str {
        "memory_promote"
    }

    fn description(&self) -> String {
        "Promote a candidate memory to active with episode evidence (R-165 provenance hard \
         constraint). Params: id, sources=[{episode_id, event_start?, event_end?}], \
         source_hash?. A candidate with no episode source can never become active — this is \
         the engine-enforced evidence gate, not advisory."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(PromoteInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: PromoteInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let store = match store_for(ctx, "project") {
            Ok(s) => s,
            Err(e) => return ToolOutput::error(e.to_string()),
        };
        let sources: Vec<(i64, Option<i64>, Option<i64>)> = input
            .sources
            .iter()
            .map(|s| (s.episode_id, s.event_start, s.event_end))
            .collect();
        match store.promote(&input.id, &sources, input.source_hash.as_deref()) {
            Ok(e) => ToolOutput::ok(format!(
                "promoted {} [{}] {} → active (evidence: {} source(s))",
                e.id,
                e.category,
                e.title,
                sources.len()
            )),
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct AddInput {
    /// global(preference/habit) | project(fact/sop)
    scope: String,
    /// preference | habit | fact | sop
    category: String,
    /// 简洁标题(中文优先)
    title: String,
    /// 召回钩子:什么时候该想起这条("处理 X 问题时必读")
    description: String,
    /// 正文(证据、命令、路径、结论)
    #[serde(default)]
    body: Option<String>,
    /// 溯源标注,如 run:<session> 或 user
    #[serde(default)]
    source: Option<String>,
    /// R-070 来源引用:必须真实存在(R-/D-/A-/G-/S-/F-/M- 条目或项目内文件),
    /// 硬校验不通过整体拒绝,防止记忆与来源脱钩。
    #[serde(default)]
    refs: Vec<String>,
    /// 状态型事实的稳定主题键(R-149,如「安装通道」「当前开发分支」):
    /// 同 scope+category+subject 至多一条 active,冲突须 memory_update 既有条目——
    /// 状态就地覆盖,绝不并存,force 不可绕。
    #[serde(default)]
    subject: Option<String>,
    /// 与既有条目标题精确重复时仍强制新增
    #[serde(default)]
    force: bool,
}

pub struct MemoryAddTool;

#[async_trait]
impl Tool for MemoryAddTool {
    fn name(&self) -> &'static str {
        "memory_add"
    }

    fn description(&self) -> String {
        "Create a durable memory entry. ALWAYS memory_search first. Params: scope(global|project), category, title, description (retrieval hook), body; optional source, refs (source IDs that must exist), force.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(AddInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: AddInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let store = match store_for(ctx, &input.scope) {
            Ok(s) => s,
            Err(e) => return ToolOutput::error(e.to_string()),
        };
        if let Err(e) = super::validate_source_refs(ctx, &input.refs) {
            return ToolOutput::error(e);
        }
        match store.add(
            &input.category,
            &input.title,
            &input.description,
            input.body.as_deref().unwrap_or(""),
            input.source.as_deref().unwrap_or("memory-manager"),
            &input.refs,
            input.subject.as_deref(),
            input.force,
        ) {
            Ok(AddOutcome::Added(e)) => ToolOutput::ok(format!("added {} [{}] {}", e.id, e.category, e.title)),
            Ok(AddOutcome::Duplicate(e)) => ToolOutput::error(format!(
                "duplicate of existing {} `{}` — use memory_update/memory_merge instead, or retry with force=true if genuinely distinct",
                e.id, e.title
            )),
            Ok(AddOutcome::SubjectConflict(e)) => ToolOutput::error(format!(
                "subject `{}` is already held by active {} `{}` — state supersedes in place: memory_update {} with the new state (force cannot bypass this)",
                input.subject.as_deref().unwrap_or(""),
                e.id,
                e.title,
                e.id
            )),
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct UpdateInput {
    scope: String,
    /// 如 "M-013"
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

pub struct MemoryUpdateTool;

#[async_trait]
impl Tool for MemoryUpdateTool {
    fn name(&self) -> &'static str {
        "memory_update"
    }

    fn description(&self) -> String {
        "Evolve an existing memory entry (title/description/body). Params: scope, id; optional title, description, body.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UpdateInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: UpdateInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let store = match store_for(ctx, &input.scope) {
            Ok(s) => s,
            Err(e) => return ToolOutput::error(e.to_string()),
        };
        // D-215 引擎兜底:manager 改正文时不许弄丢复发指纹(它是引擎的检测键,
        // 丢了「记了但没用」就再也看不见)。只闸 manager 写路径——UI 用户直写
        // (memory_entry_save)不受此限,用户有权删任何东西(A-005)。
        if let Some(new_body) = input.body.as_deref() {
            if let Some((_, existing)) =
                store.load_all().into_iter().find(|(_, e)| e.id == input.id)
            {
                let lost: Vec<String> = super::fp_markers(&existing.body)
                    .into_iter()
                    .filter(|marker| !new_body.contains(marker.as_str()))
                    .collect();
                if !lost.is_empty() {
                    return ToolOutput::error(format!(
                        "body update would drop recurrence marker(s) {} — they are engine \
                         keys for recurrence detection; keep them verbatim in the new body",
                        lost.join(" ")
                    ));
                }
            }
        }
        match store.update(
            &input.id,
            input.title.as_deref(),
            input.description.as_deref(),
            input.body.as_deref(),
            None,
        ) {
            Ok(e) => ToolOutput::ok(format!("updated {} [{}] {}", e.id, e.status, e.title)),
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct MergeInput {
    scope: String,
    /// 保留的主条目(最老引用优先)
    primary: String,
    /// 被并入的重复条目(将 stale 并链接 superseded_by)
    duplicates: Vec<String>,
    /// 合并后的正文(通常是两者的并集提炼)
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// R-165 保守闸:用户已确认这次合并(评估器落地前,无确认则要求共享 fingerprint)。
    #[serde(default)]
    confirmed: bool,
}

pub struct MemoryMergeTool;

#[async_trait]
impl Tool for MemoryMergeTool {
    fn name(&self) -> &'static str {
        "memory_merge"
    }

    fn description(&self) -> String {
        "Merge duplicate entries into `primary`; duplicates become stale with a superseded_by link. Params: scope, primary, duplicates[]; optional body, description.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(MergeInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: MergeInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let store = match store_for(ctx, &input.scope) {
            Ok(s) => s,
            Err(e) => return ToolOutput::error(e.to_string()),
        };
        // R-166 批4:合并守恒 D(S→m')<ε 把关——有离线评估数据时,合并前后
        // 行为必须等价(D 小);失真(D≥ε)拒绝。无评估数据(评估器没跑过该记忆
        // 的合并对照)退化为既有保守闸(fingerprint/用户确认),不拦。
        // ε=0.5:允许小扰动(单个 case 的 success 翻转会被均值摊薄),
        // 但「合并后普遍失败」这种失真必须拦下。
        const EPSILON: f64 = 0.5;
        let merge_distortion = {
            let db_path = store.root.join("..").join("state.db");
            kanzei_core::SessionStore::open(&db_path)
                .ok()
                .and_then(|db| {
                    db.merge_conservation_delta(&input.primary, "", "")
                        .ok()
                        .flatten()
                })
        };
        if let Some((delta, n)) = merge_distortion {
            if delta >= EPSILON {
                return ToolOutput::error(format!(
                    "merge 守恒拒绝: D(S→m')={delta:.3} ≥ ε={EPSILON} (配对 {n} case)——\
                     合并会把决策质量显著改变,压缩不是行为等价变换。请先跑合并对照回放 \
                     (merged 臂)确认等价,或拆分合并"
                ));
            }
            // 放行:记录判定依据(供验收②的「判定依据落库」审计)。
            tracing::info!(
                primary = %input.primary,
                delta,
                n,
                "merge 守恒放行: D(S→m') < ε"
            );
        }
        match store.merge(
            &input.primary,
            &input.duplicates,
            None,
            input.description.as_deref(),
            input.body.as_deref(),
            input.confirmed,
        ) {
            Ok(e) => ToolOutput::ok(format!(
                "merged {} ← [{}]",
                e.id,
                input.duplicates.join(", ")
            )),
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct StaleInput {
    scope: String,
    id: String,
    /// 为什么失效(被推翻/过期/不再适用)——墓碑必须可追溯
    reason: String,
}

pub struct MemoryStaleTool;

#[async_trait]
impl Tool for MemoryStaleTool {
    fn name(&self) -> &'static str {
        "memory_stale"
    }

    fn description(&self) -> String {
        "Mark an entry stale (disproven/expired). Params: scope, id, reason (required).".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(StaleInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: StaleInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        if input.reason.trim().is_empty() {
            return ToolOutput::error("reason must not be empty");
        }
        let store = match store_for(ctx, &input.scope) {
            Ok(s) => s,
            Err(e) => return ToolOutput::error(e.to_string()),
        };
        // D-217:墓碑必须随条目进 archive。原实现先 update 状态(触发 archive_dead
        // 把文件搬走),再想追加 reason 正文——此时 load_all 已找不到条目,reason
        // 永远不落档,归档文件没有「为什么失效」的追溯。正确做法:先读原 body,
        // 追加墓碑文本,再一次性 update(body + status),rename 时文件已带墓碑。
        let reason = input.reason.trim();
        let (found_id, found_body) =
            match store.load_all().into_iter().find(|(_, e)| e.id == input.id) {
                Some((_, e)) => (e.id.clone(), e.body),
                None => return ToolOutput::error(format!("unknown memory id `{}`", input.id)),
            };
        let appended = format!("{}\n\n(stale: {reason})", found_body.trim_end());
        match store.update(&found_id, None, None, Some(&appended), Some("stale")) {
            Ok(e) => ToolOutput::ok(format!("staled {} — {reason}", e.id)),
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct InboxClearInput;

pub struct MemoryInboxClearTool;

#[async_trait]
impl Tool for MemoryInboxClearTool {
    fn name(&self) -> &'static str {
        "memory_inbox_clear"
    }

    fn description(&self) -> String {
        "Clear the inbox after ALL notes are processed (added/updated/merged or judged NOOP)."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(InboxClearInput)).unwrap()
    }

    async fn execute(&self, _input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let store = MemoryStore::project(&ctx.project_root);
        match store.clear_inbox() {
            Ok(()) => ToolOutput::ok("inbox cleared"),
            Err(e) => ToolOutput::error(format!("cannot clear inbox: {e}")),
        }
    }
}

/// manager 专属装配:全套写工具+检索,无 bash/read/write——它只能操作记忆。
pub struct MemoryManagerComponent;

impl Component for MemoryManagerComponent {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft
            .tools
            .insert("memory_search", Arc::new(super::MemorySearchTool));
        draft
            .tools
            .insert("memory_stats", Arc::new(super::MemoryStatsTool));
        draft.tools.insert("memory_add", Arc::new(MemoryAddTool));
        draft
            .tools
            .insert("memory_promote", Arc::new(MemoryPromoteTool));
        draft
            .tools
            .insert("memory_update", Arc::new(MemoryUpdateTool));
        draft
            .tools
            .insert("memory_merge", Arc::new(MemoryMergeTool));
        draft
            .tools
            .insert("memory_stale", Arc::new(MemoryStaleTool));
        draft
            .tools
            .insert("memory_inbox_clear", Arc::new(MemoryInboxClearTool));
        for tool in [
            "memory_search",
            "memory_stats",
            "memory_add",
            "memory_promote",
            "memory_update",
            "memory_merge",
            "memory_stale",
            "memory_inbox_clear",
        ] {
            draft.permissions.push(rule(tool, "*", Effect::Allow));
        }
        Ok(())
    }
}

#[allow(clippy::items_after_test_module)] // 保持公开 manager_agent 紧邻其实现，测试仍集中在本模块。
#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{Harness, KanzeiConfig, ProfileKind};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn manager_snapshot_has_full_write_toolset_and_no_shell() {
        let root = PathBuf::from("C:/kz-memory-manager-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(MemoryManagerComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|t| t.name())
            .collect();
        for tool in [
            "memory_search",
            "memory_add",
            "memory_promote",
            "memory_update",
            "memory_merge",
            "memory_stale",
            "memory_inbox_clear",
        ] {
            assert!(names.contains(&tool), "missing {tool} in {names:?}");
            assert_eq!(snapshot.evaluate(tool, "*"), Effect::Allow);
        }
        assert!(!names.contains(&"bash"), "manager 不得有 shell");
        assert!(!names.contains(&"write"), "manager 不得有 write");
    }

    #[tokio::test]
    async fn manager_tools_consolidate_a_note_end_to_end() {
        let dir = std::env::temp_dir().join(format!(
            "kz-manager-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let store = MemoryStore::project(&dir);
        store
            .append_note(
                "发版要走两条通道",
                "package.ps1 -Publish + 静默装",
                "sop",
                &[],
            )
            .unwrap();

        // 模拟 manager 的一轮决策:add → inbox_clear。
        let added = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "sop", "title": "发版 SOP:两条通道",
                       "description": "做发版/发布/安装更新相关任务时必读",
                       "body": "package.ps1 -Publish 后静默装 setup"}),
                &ctx,
            )
            .await;
        assert!(!added.is_error, "{}", added.content);
        // 引擎去重:同标题重复 add 被拒并指路
        let dup = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "sop", "title": "发版 SOP:两条通道",
                       "description": "x", "body": "y"}),
                &ctx,
            )
            .await;
        assert!(dup.is_error);
        assert!(dup.content.contains("memory_update"), "{}", dup.content);
        let cleared = MemoryInboxClearTool.execute(json!({}), &ctx).await;
        assert!(!cleared.is_error);
        assert_eq!(store.pending_notes(), 0);
        // stale 需要 reason
        let no_reason = MemoryStaleTool
            .execute(
                json!({"scope": "project", "id": "M-001", "reason": "  "}),
                &ctx,
            )
            .await;
        assert!(no_reason.is_error);
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-217:memory_stale 的 reason 必须随条目进 archive/ 墓碑(原实现先搬走文件
    /// 再追加正文,load_all 找不到条目,reason 永远不落档)。验证:标 stale 后条目
    /// 离开主目录,archive/ 里文件正文含 `(stale: reason)`,ID 由归档侧保留。
    #[tokio::test]
    async fn stale_墓碑_reason随条目进归档() {
        let dir = std::env::temp_dir().join(format!(
            "kz-manager-stale-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let store = MemoryStore::project(&dir);
        let added = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "fact", "title": "墓碑测试条目",
                       "description": "测试 reason 落档", "body": "原始正文"}),
                &ctx,
            )
            .await;
        assert!(!added.is_error, "{}", added.content);

        let staled = MemoryStaleTool
            .execute(
                json!({"scope": "project", "id": "M-001", "reason": "被新结论推翻"}),
                &ctx,
            )
            .await;
        assert!(!staled.is_error, "{}", staled.content);
        assert!(
            staled.content.contains("被新结论推翻"),
            "{}",
            staled.content
        );

        // 主目录不再有 M-001(archive_dead 已搬走),归档侧保留 ID。
        assert!(
            !store.load_all().iter().any(|(_, e)| e.id == "M-001"),
            "stale 条目应离开主目录"
        );
        let archive_dir = dir.join(".kanzei").join("memory").join("archive");
        let names: Vec<String> = std::fs::read_dir(&archive_dir)
            .unwrap()
            .flatten()
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("M-001-")),
            "归档侧应保留 ID: {names:?}"
        );
        // 墓碑正文:归档文件里必须能看到 reason(可追溯)。
        let bodies: Vec<String> = std::fs::read_dir(&archive_dir)
            .unwrap()
            .flatten()
            .map(|f| std::fs::read_to_string(f.path()).unwrap_or_default())
            .collect();
        assert!(
            bodies.iter().any(|b| b.contains("(stale: 被新结论推翻)")),
            "归档墓碑必须含 reason: {bodies:?}"
        );
        assert!(
            bodies.iter().any(|b| b.contains("原始正文")),
            "归档墓碑必须保留原正文: {bodies:?}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-166 批4(验收②):merge 守恒 D(S→m')<ε 把关——state.db 里有失真评估
    /// (current 全成功、merged 全失败,D=1≥0.5)时,memory_merge 被拒绝并给出依据。
    #[tokio::test]
    async fn merge_gate_rejects_distorting_merge_with_delta() {
        let dir = std::env::temp_dir().join(format!(
            "kz-manager-mergegate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let store = MemoryStore::project(&dir);
        // 建两条可合并的记忆(同 fingerprint 才能过保守闸;守恒闸在保守闸之前)。
        for (title, desc, body) in [
            ("合并守恒测试主", "钩子主", "内容 [fp:abc]"),
            ("合并守恒测试副", "钩子副", "重复内容 [fp:abc]"),
        ] {
            let out = MemoryAddTool
                .execute(
                    json!({"scope": "project", "category": "fact", "title": title,
                           "description": desc, "body": body}),
                    &ctx,
                )
                .await;
            assert!(!out.is_error, "{}", out.content);
        }
        // state.db 写失真评估:M-001 current 全成功、merged 全失败 → D=1 ≥ ε=0.5。
        let db_path = dir.join(".kanzei").join("state.db");
        let db = kanzei_core::SessionStore::open(&db_path).unwrap();
        for (case, c_ok, m_ok) in [
            ("c1", true, false),
            ("c2", true, false),
            ("c3", true, false),
        ] {
            db.record_memory_eval("M-001", case, "current", "m", "v1", c_ok, 1, 0, 0, 1, None)
                .unwrap();
            db.record_memory_eval("M-001", case, "merged", "m", "v1", m_ok, 1, 0, 0, 1, None)
                .unwrap();
        }
        drop(db);
        // merge 应被守恒闸拒绝。
        let out = MemoryMergeTool
            .execute(
                json!({"scope": "project", "primary": "M-001", "duplicates": ["M-002"],
                       "confirmed": false}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "失真合并必须被拒: {}", out.content);
        assert!(
            out.content.contains("守恒拒绝") && out.content.contains("D(S→m')"),
            "拒绝文案要带守恒依据: {}",
            out.content
        );
        // 两条都还在,未被并掉。
        let entries = store.load_all();
        assert_eq!(entries.len(), 2, "拒绝后不落盘: {}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_update_不许弄丢正文里的复发指纹() {
        // D-215:manager 修订条目时把 [fp:...] 弄丢会让复发检测静默失效,引擎拒绝。
        let dir = std::env::temp_dir().join(format!(
            "kz-manager-fpgate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let added = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "fact", "title": "edit 未命中先 read",
                       "description": "edit 失败必读", "body": "判据 [fp:edit|not found]"}),
                &ctx,
            )
            .await;
        assert!(!added.is_error, "{}", added.content);

        let dropped = MemoryUpdateTool
            .execute(
                json!({"scope": "project", "id": "M-001", "body": "重写后的判据(忘了带指纹)"}),
                &ctx,
            )
            .await;
        assert!(dropped.is_error, "{}", dropped.content);
        assert!(
            dropped.content.contains("[fp:edit|not found]"),
            "{}",
            dropped.content
        );

        // 带着指纹改正文、或只改 description 都放行。
        let kept = MemoryUpdateTool
            .execute(
                json!({"scope": "project", "id": "M-001",
                       "body": "更锋利的判据 [fp:edit|not found]"}),
                &ctx,
            )
            .await;
        assert!(!kept.is_error, "{}", kept.content);
        let desc_only = MemoryUpdateTool
            .execute(
                json!({"scope": "project", "id": "M-001", "description": "edit 替换失败必读:先 read 再改"}),
                &ctx,
            )
            .await;
        assert!(!desc_only.is_error, "{}", desc_only.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_add_subject_conflict_points_to_update_and_ignores_force() {
        // R-149:状态型事实同 subject 至多一条 active,冲突指路 memory_update,force 不可绕。
        let dir = std::env::temp_dir().join(format!(
            "kz-manager-subject-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };

        let first = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "fact", "title": "安装通道:NSIS 安装版",
                       "description": "查安装/更新通道时必读", "body": "AppData 下",
                       "subject": "安装通道"}),
                &ctx,
            )
            .await;
        assert!(!first.is_error, "{}", first.content);
        // subject 落进 frontmatter。
        let store = MemoryStore::project(&dir);
        let (_, entry) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == "M-001")
            .unwrap();
        assert!(entry
            .extras
            .iter()
            .any(|(k, v)| k == "subject" && v == "安装通道"));
        // R-165:subject 状态不变量只约束 active——先 promote 带证据升 active,冲突才触发。
        store
            .promote(&entry.id, &[(1, None, None)], Some("test"))
            .unwrap();

        let conflict = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "fact", "title": "安装通道改为便携版",
                       "description": "查安装通道必读", "body": "新状态",
                       "subject": "安装通道", "force": true}),
                &ctx,
            )
            .await;
        assert!(conflict.is_error, "{}", conflict.content);
        assert!(
            conflict.content.contains("memory_update M-001"),
            "{}",
            conflict.content
        );
        assert!(
            conflict.content.contains("force cannot bypass"),
            "{}",
            conflict.content
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_add_validates_source_refs_hard() {
        // R-070:refs 必须真实存在——未知 ID 整体拒绝;合法 ID 写入条目 frontmatter。
        let dir = std::env::temp_dir().join(format!(
            "kz-manager-refs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-070 示例 [todo]\n- 验收: 略\n",
        )
        .unwrap();
        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };

        let bad = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "fact", "title": "假引用",
                       "description": "测试", "body": "x", "refs": ["R-999"]}),
                &ctx,
            )
            .await;
        assert!(bad.is_error);
        assert!(bad.content.contains("invalid refs"), "{}", bad.content);

        let good = MemoryAddTool
            .execute(
                json!({"scope": "project", "category": "fact", "title": "真引用",
                       "description": "测试", "body": "x", "refs": ["R-070"]}),
                &ctx,
            )
            .await;
        assert!(!good.is_error, "{}", good.content);
        assert!(good.content.contains("M-001"), "{}", good.content);
        let store = MemoryStore::project(&dir);
        let (_, entry) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == "M-001")
            .unwrap();
        assert_eq!(entry.refs(), vec!["R-070".to_string()]);
        std::fs::remove_dir_all(dir).ok();
    }
}

/// manager 迷你 run 的 agent 定义(fast 档,调用方 fast 失败可升级 primary)。
pub fn manager_agent() -> AgentDef {
    AgentDef {
        name: "memory-manager".into(),
        profile: ProfileScope::Dev,
        model: "fast".into(),
        mode: AgentMode::Subagent,
        steps: 10,
        system: "You are the memory manager. Input: draft notes from the inbox. For EACH \
                 note decide NOOP, ADD, UPDATE, or MERGE. The single criterion is DECISION \
                 VALUE, not semantic richness: keep a note only if a future agent would ACT \
                 DIFFERENTLY for lack of it (wrong command, repeated dead end, violated \
                 user constraint). If you cannot name the concrete action it changes, judge \
                 NOOP — never invent one. \
                 Write `description` as retrieval hook PLUS decision: WHEN to recall it AND \
                 what to do differently (e.g. \"处理 edit 替换失败/换行符问题时必读:先 \
                 read 重读再改\"). \
                 ALWAYS memory_search before memory_add — the engine rejects exact-title \
                 duplicates. Scope rules: preference/habit → global, fact/sop → project. \
                 EXCEPTION (D-214): SOP candidates whose detail explicitly says \
                 \"scope=global\" (候选 SOP 落库目标) must be ADDed with scope=global — \
                 they are cross-project workflow templates; the detail line overrides \
                 the default scope rule. \
                 SOP candidates ask for a structured, followable procedure (D-204): \
                 ALWAYS emit 适用场景 + 操作步骤(每步做什么 AND 判断依据) + 边界与例外. \
                 A bare tool-name sequence is NOT a SOP — rephrase from the evidence or \
                 NOOP it. If the flow only fits this one entry (one-off debugging, tied to \
                 a specific id), NOOP — do not generalize an unrepeatable trace. \
                 STATEFUL facts describing the CURRENT world (当前安装通道/当前分支/当前 \
                 版本…) must pass `subject` (a stable topic key, e.g. \"安装通道\"); the \
                 engine keeps at most one active entry per subject — on conflict \
                 memory_update the existing entry: state supersedes in place, never \
                 accumulates. \
                 Failure notes carry a `[fp:tool|kind]` marker: copy it VERBATIM into the \
                 entry body — it is the engine's recurrence-detection key. A note saying an \
                 entry 已有记忆但仍复发 means that memory failed to enter decisions: \
                 memory_update that entry with a sharper 判据/description (keep the marker \
                 in the body); only ADD if it is truly a different pitfall. \
                 Notes may carry a `- refs: R-012 D-044` line: pass those IDs verbatim to \
                 memory_add's `refs` parameter (R-070 source contract; invalid IDs are \
                 rejected by the engine). \
                 BEFORE merging, ask the three conversion questions (R-165): \
                 COVERAGE (does the merged entry cover all key facts?), PRESERVATION \
                 (does it keep accurate details from the old entries?), FAITHFULNESS \
                 (does it state anything NOT in the evidence?). memory_merge is \
                 engine-gated: without `confirmed: true` it only merges entries sharing a \
                 [fp:...] marker — pass confirmed=true ONLY when the user explicitly \
                 approved this merge. \
                 A failure COUNT is signal strength, never content: \"edit failed 7 times\" \
                 means the same mistake recurred — it does NOT mean \"7 retries are needed\". \
                 Record the underlying constraint (quote the actual error text), not the \
                 retry count. After processing ALL notes call memory_inbox_clear, then \
                 reply with one summary line."
            .into(),
    }
}
