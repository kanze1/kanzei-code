//! 主 agent 侧的记忆工具(M1):search(读)/note(草稿投递)/stats(概览)。
//! 写路径(add/update/merge/stale)属于 M2 的 memory-manager 子代理,主 agent 不可直写。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{MemoryStore, SearchHit};

fn stores_for(ctx: &ToolCtx, scope: &str) -> Vec<MemoryStore> {
    let mut out = Vec::new();
    if scope == "all" || scope == "project" {
        out.push(MemoryStore::project(&ctx.project_root));
    }
    if scope == "all" || scope == "global" {
        if let Some(global) = MemoryStore::global() {
            out.push(global);
        }
    }
    out
}

#[derive(Deserialize, JsonSchema)]
struct SearchInput {
    /// 检索词(空格分词,FTS 全文匹配 title/description/body)
    query: String,
    /// all(默认) | global | project
    #[serde(default)]
    scope: Option<String>,
    /// preference | habit | fact | sop
    #[serde(default)]
    category: Option<String>,
    /// active(默认) | stale | any
    #[serde(default)]
    status: Option<String>,
    /// 默认 5,上限 10
    #[serde(default)]
    limit: Option<usize>,
}

pub struct MemorySearchTool;

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory_search"
    }

    fn description(&self) -> String {
        "Search long-term memory (facts, habits, SOPs, preferences) across project and global scopes. Params: query; optional scope(all|global|project), category, status, limit. Read the returned file path for the full entry.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(SearchInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: SearchInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let scope = input.scope.as_deref().unwrap_or("all");
        if !["all", "global", "project"].contains(&scope) {
            return ToolOutput::error("scope must be all | global | project");
        }
        let status = match input.status.as_deref() {
            None => Some("active"),
            Some("any") => None,
            Some(s) if super::STATUSES.contains(&s) => Some(s),
            Some(other) => {
                return ToolOutput::error(format!("invalid status `{other}`; valid: active | stale | any"))
            }
        };
        let limit = input.limit.unwrap_or(5).clamp(1, 10);
        let mut all_hits: Vec<SearchHit> = Vec::new();
        for store in stores_for(ctx, scope) {
            match store.search(&input.query, input.category.as_deref(), status, limit) {
                Ok(hits) => all_hits.extend(hits),
                Err(e) => return ToolOutput::error(format!("memory search failed: {e}")),
            }
        }
        all_hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        all_hits.truncate(limit);
        if all_hits.is_empty() {
            return ToolOutput::ok(format!(
                "(no memory matched `{}` — if you learn something reusable here, record it with memory_note)",
                input.query
            ));
        }
        let rendered: Vec<String> = all_hits
            .iter()
            .map(|hit| {
                format!(
                    "{} [{}/{}] {} — {}\n  {}\n  file: {}",
                    hit.entry.id,
                    hit.entry.scope,
                    hit.entry.category,
                    hit.entry.title,
                    hit.entry.description,
                    hit.snippet.replace('\n', " "),
                    hit.path.display(),
                )
            })
            .collect();
        ToolOutput::ok(rendered.join("\n"))
    }
}

#[derive(Deserialize, JsonSchema)]
struct NoteInput {
    /// 一句话:学到了什么/踩了什么坑/用户定了什么调
    summary: String,
    /// 可选详情(证据、命令、路径)
    #[serde(default)]
    detail: Option<String>,
    /// 建议分类:preference | habit | fact | sop(manager 最终裁定)
    #[serde(default)]
    category_hint: Option<String>,
}

pub struct MemoryNoteTool;

#[async_trait]
impl Tool for MemoryNoteTool {
    fn name(&self) -> &'static str {
        "memory_note"
    }

    fn description(&self) -> String {
        "Drop a draft note into the memory inbox (confirmed facts, pitfalls, user decisions worth remembering). The memory manager will consolidate it. Params: summary; optional detail, category_hint.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(NoteInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: NoteInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        if input.summary.trim().is_empty() {
            return ToolOutput::error("summary must not be empty");
        }
        let store = MemoryStore::project(&ctx.project_root);
        match store.append_note(
            &input.summary,
            input.detail.as_deref().unwrap_or(""),
            input.category_hint.as_deref().unwrap_or(""),
        ) {
            Ok(path) => ToolOutput::ok(format!(
                "noted → {} (pending notes: {})",
                path.display(),
                store.pending_notes()
            )),
            Err(e) => ToolOutput::error(format!("cannot append note: {e}")),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct StatsInput {}

pub struct MemoryStatsTool;

#[async_trait]
impl Tool for MemoryStatsTool {
    fn name(&self) -> &'static str {
        "memory_stats"
    }

    fn description(&self) -> String {
        "Overview of the memory system: per-scope entry counts by category/status, pending inbox notes, integrity issues.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(StatsInput)).unwrap()
    }

    async fn execute(&self, _input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let mut out = String::new();
        for store in stores_for(ctx, "all") {
            let entries = store.load_all();
            let mut by_category: std::collections::BTreeMap<&str, (usize, usize)> =
                std::collections::BTreeMap::new();
            for (_, e) in &entries {
                let slot = by_category.entry(super::CATEGORIES
                    .iter()
                    .find(|c| **c == e.category)
                    .copied()
                    .unwrap_or("other"))
                    .or_insert((0, 0));
                if e.status == "active" {
                    slot.0 += 1;
                } else {
                    slot.1 += 1;
                }
            }
            out.push_str(&format!("[{}] {} entries", store.scope.label(), entries.len()));
            for (category, (active, stale)) in &by_category {
                out.push_str(&format!(" · {category} {active}a/{stale}s"));
            }
            let pending = store.pending_notes();
            if pending > 0 {
                out.push_str(&format!(" · inbox {pending} pending"));
            }
            for issue in store.integrity_issues() {
                out.push_str(&format!("\n  ⚠ {issue}"));
            }
            out.push('\n');
        }
        if out.is_empty() {
            out = "(no memory stores available)".into();
        }
        ToolOutput::ok(out.trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;
    use serde_json::json;

    fn ctx() -> (std::path::PathBuf, ToolCtx) {
        let dir = std::env::temp_dir().join(format!(
            "kz-memtool-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (dir.clone(), ToolCtx { cwd: dir.clone(), project_root: dir })
    }

    #[tokio::test]
    async fn search_note_stats_roundtrip_on_project_scope() {
        let (dir, ctx) = ctx();
        let store = MemoryStore::project(&ctx.project_root);
        match store
            .add("sop", "发版 SOP 两条通道", "发版发布安装更新必读", "package.ps1 -Publish", "user", false)
            .unwrap()
        {
            crate::memory::AddOutcome::Added(_) => {}
            _ => panic!("expected add"),
        }

        let hits = MemorySearchTool
            .execute(json!({"query": "发版 更新", "scope": "project"}), &ctx)
            .await;
        assert!(!hits.is_error, "{}", hits.content);
        assert!(hits.content.contains("M-001"), "{}", hits.content);
        assert!(hits.content.contains("file:"), "{}", hits.content);

        let note = MemoryNoteTool
            .execute(
                json!({"summary": "纯 ui 改动只跑 node 检查", "category_hint": "habit"}),
                &ctx,
            )
            .await;
        assert!(!note.is_error, "{}", note.content);
        assert!(note.content.contains("pending notes: 1"), "{}", note.content);

        let stats = MemoryStatsTool.execute(json!({}), &ctx).await;
        assert!(!stats.is_error);
        assert!(stats.content.contains("[project]"), "{}", stats.content);
        assert!(stats.content.contains("sop 1a/0s"), "{}", stats.content);
        assert!(stats.content.contains("inbox 1 pending"), "{}", stats.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn empty_result_nudges_note_and_invalid_enum_errors() {
        let (dir, ctx) = ctx();
        let none = MemorySearchTool
            .execute(json!({"query": "不存在的词条", "scope": "project"}), &ctx)
            .await;
        assert!(!none.is_error);
        assert!(none.content.contains("memory_note"), "{}", none.content);
        let bad = MemorySearchTool
            .execute(json!({"query": "x", "scope": "银河系"}), &ctx)
            .await;
        assert!(bad.is_error);
        std::fs::remove_dir_all(dir).ok();
    }
}
