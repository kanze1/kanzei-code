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
        "global" => MemoryStore::global().ok_or_else(|| anyhow::anyhow!("no home dir for global scope")),
        other => anyhow::bail!("invalid scope `{other}`; valid: global | project"),
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
        "Create a durable memory entry. ALWAYS memory_search first. Params: scope(global|project), category, title, description (retrieval hook), body; optional source, force.".into()
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
        match store.add(
            &input.category,
            &input.title,
            &input.description,
            input.body.as_deref().unwrap_or(""),
            input.source.as_deref().unwrap_or("memory-manager"),
            input.force,
        ) {
            Ok(AddOutcome::Added(e)) => ToolOutput::ok(format!("added {} [{}] {}", e.id, e.category, e.title)),
            Ok(AddOutcome::Duplicate(e)) => ToolOutput::error(format!(
                "duplicate of existing {} `{}` — use memory_update/memory_merge instead, or retry with force=true if genuinely distinct",
                e.id, e.title
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
        match store.merge(
            &input.primary,
            &input.duplicates,
            None,
            input.description.as_deref(),
            input.body.as_deref(),
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
        let body_note = format!("\n\n(stale: {})", input.reason.trim());
        match store.update(&input.id, None, None, None, Some("stale")) {
            Ok(e) => {
                let appended = format!("{}{}", e.body, body_note);
                let _ = store.update(&input.id, None, None, Some(&appended), None);
                ToolOutput::ok(format!("staled {} — {}", e.id, input.reason.trim()))
            }
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct InboxClearInput {
    /// 固定 project(草稿箱只在项目域)
    #[serde(default)]
    scope: Option<String>,
}

pub struct MemoryInboxClearTool;

#[async_trait]
impl Tool for MemoryInboxClearTool {
    fn name(&self) -> &'static str {
        "memory_inbox_clear"
    }

    fn description(&self) -> String {
        "Clear the inbox after ALL notes are processed (added/updated/merged or judged NOOP).".into()
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
        draft.tools.insert("memory_search", Arc::new(super::MemorySearchTool));
        draft.tools.insert("memory_stats", Arc::new(super::MemoryStatsTool));
        draft.tools.insert("memory_add", Arc::new(MemoryAddTool));
        draft.tools.insert("memory_update", Arc::new(MemoryUpdateTool));
        draft.tools.insert("memory_merge", Arc::new(MemoryMergeTool));
        draft.tools.insert("memory_stale", Arc::new(MemoryStaleTool));
        draft
            .tools
            .insert("memory_inbox_clear", Arc::new(MemoryInboxClearTool));
        for tool in [
            "memory_search",
            "memory_stats",
            "memory_add",
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
        let ctx = ToolCtx { cwd: dir.clone(), project_root: dir.clone() };
        let store = MemoryStore::project(&dir);
        store.append_note("发版要走两条通道", "package.ps1 -Publish + 静默装", "sop").unwrap();

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
            .execute(json!({"scope": "project", "id": "M-001", "reason": "  "}), &ctx)
            .await;
        assert!(no_reason.is_error);
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
                 note decide NOOP (transient/junk/already known), ADD, UPDATE, or MERGE. \
                 ALWAYS memory_search before memory_add — the engine rejects exact-title \
                 duplicates. Scope rules: preference/habit → global, fact/sop → project. \
                 Write `description` as a retrieval hook: WHEN should a future agent recall \
                 this (e.g. \"处理 edit 替换失败/换行符问题时必读\"). Keep entries about \
                 durable facts, not next steps. \
                 A failure COUNT is signal strength, never content: \"edit failed 7 times\" \
                 means the same mistake recurred — it does NOT mean \"7 retries are needed\". \
                 Record the underlying constraint (quote the actual error text), not the \
                 retry count. If a note does not let you state a durable fact you could \
                 verify, judge it NOOP rather than inventing one. After processing ALL notes \
                 call memory_inbox_clear, then reply with one summary line."
            .into(),
    }
}
