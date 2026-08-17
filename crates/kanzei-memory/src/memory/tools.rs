//! 主 agent 侧的记忆工具(M1):search(读)/note(草稿投递)/stats(概览)。
//! 写路径(add/update/merge/stale)属于 M2 的 memory-manager 子代理,主 agent 不可直写。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{IndexQuery, MemoryStore, SearchHit, SqliteMemoryIndex};

fn stores_for(ctx: &ToolCtx, scope: &str) -> Vec<MemoryStore> {
    let mut out = Vec::new();
    if scope == "all" || scope == "project" {
        out.push(MemoryStore::project(&ctx.project_root));
    }
    // R-194:全局(用户级)记忆废弃——scope=global/all 不再返回全局 store。
    // 全局库 0 active、0 召回且候选与项目重复,跨项目偏好由配置文件与
    // 系统提示承载;保留显式报错让调用方知道该 scope 已不可用。
    out
}

/// R-161:读项目 state.db 的五段漏斗计数(与 episodes 同库,CLI/桌面同源写入)。
/// 库缺失或损坏时返回 None——遥测是诊断口径,不应让 stats 工具报错。
/// AVAILABLE 段由这里从记忆库文件真源统计(project + global 两级 active 条目数),
/// state.db 不知道文件真源——旧实现数恒空的 memory_sources,首段永远是 0。
fn project_funnel_counts(ctx: &ToolCtx) -> Option<kanzei_core::FunnelCounts> {
    let state = kanzei_core::project_state_path(&ctx.project_root);
    let store = kanzei_core::SessionStore::open(&state).ok()?;
    // R-194:全局记忆废弃,AVAILABLE 段只数项目 store 的 active 条目。
    let active = super::MemoryStore::project(&ctx.project_root)
        .load_all()
        .into_iter()
        .filter(|(_, e)| e.status == "active")
        .count() as u64;
    store.funnel_counts(active).ok()
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
        // R-194:全局记忆废弃——scope=global 不再检索任何内容,明确提示去向。
        if scope == "global" {
            return ToolOutput::ok(
                "(全局/用户级记忆已于 R-194 废弃:全局库 0 召回、0 active 且候选与项目重复,\
                 跨项目偏好由配置文件与系统提示承载。请用 scope=project 检索项目记忆)",
            );
        }
        let status = match input.status.as_deref() {
            None => Some("active"),
            Some("any") => None,
            Some(s) if super::STATUSES.contains(&s) => Some(s),
            Some(other) => {
                return ToolOutput::error(format!(
                    "invalid status `{other}`; valid: active | stale | any"
                ))
            }
        };
        let limit = input.limit.unwrap_or(5).clamp(1, 10);
        // D-366:检索走统一门面(index 是 ranking 唯一实现处),不再直调 store.search。
        let index = SqliteMemoryIndex::new(&ctx.project_root);
        let all_hits: Vec<SearchHit> = index.search_entries(
            &IndexQuery::text(&input.query),
            input.category.as_deref(),
            status,
            limit,
        );
        super::record_memory_search_telemetry(
            &ctx.project_root,
            &input.query,
            &all_hits,
            !all_hits.is_empty(),
            "lexical",
            &crate::memory::index::RetrievalTiming::default(),
        );
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
    /// R-070 来源引用:必须真实存在(R-/D-/A-/G-/S-/F-/M- 条目或项目内文件),
    /// 随草稿写入,manager 消化时带进正式条目。
    #[serde(default)]
    refs: Vec<String>,
}

pub struct MemoryNoteTool;

#[async_trait]
impl Tool for MemoryNoteTool {
    fn name(&self) -> &'static str {
        "memory_note"
    }

    fn description(&self) -> String {
        "Drop a draft note into the memory inbox (confirmed facts, pitfalls, user decisions worth remembering). The memory manager will consolidate it. Params: summary; optional detail, category_hint, refs (source IDs that must exist).".into()
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
        if let Err(e) = super::validate_source_refs(ctx, &input.refs) {
            return ToolOutput::error(e);
        }
        let store = MemoryStore::project(&ctx.project_root);
        // R-165 批2 novelty gate(验收④):投递前机械三档分流——
        // 明显重复直接 NOOP(不占 LLM run 与 inbox),记遥测;新/不确定才进 inbox。
        // R-216:返回 (判定, 候选),Uncertain 也直接进 inbox 交 manager(候选仅 add 硬闸用)。
        let (novelty, _candidates) =
            store.classify_novelty(&input.summary, input.detail.as_deref().unwrap_or(""), "");
        if novelty == super::Novelty::Duplicate {
            store.record_novelty(&novelty, "", &input.summary);
            return ToolOutput::ok(format!(
                "noted as duplicate (NOOP, {:.60}…) — already an active memory covers it; \
                 use memory_update to evolve that entry instead of re-adding",
                input.summary
            ));
        }
        store.record_novelty(&novelty, "", &input.summary);
        match store.append_note(
            &input.summary,
            input.detail.as_deref().unwrap_or(""),
            input.category_hint.as_deref().unwrap_or(""),
            &input.refs,
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
                let slot = by_category
                    .entry(
                        super::CATEGORIES
                            .iter()
                            .find(|c| **c == e.category)
                            .copied()
                            .unwrap_or("other"),
                    )
                    .or_insert((0, 0));
                if e.status == "active" {
                    slot.0 += 1;
                } else {
                    slot.1 += 1;
                }
            }
            out.push_str(&format!(
                "[{}] {} entries",
                store.scope.label(),
                entries.len()
            ));
            for (category, (active, stale)) in &by_category {
                out.push_str(&format!(" · {category} {active}a/{stale}s"));
            }
            // R-149 决策价值观测:召回→采纳转化是「记忆是否真进了决策」的机械口径。
            let profile = store.recall_profile();
            if !profile.is_empty() {
                let recalled: u64 = profile.values().map(|(r, _)| r).sum();
                let fetched: u64 = profile.values().map(|(_, f)| f).sum();
                out.push_str(&format!(" · 召回 {recalled}/采纳 {fetched}"));
            }
            // R-161 五段漏斗(与 episodes 同库,CLI/桌面端同源写入):A→R→I→U→Y。
            // 只在项目 scope 报一次,避免跨 store 重复计数(global store 命中也会
            // 记进项目 state.db 的 recall_events)。
            if store.scope.label() == "project" {
                if let Some(funnel) = project_funnel_counts(ctx) {
                    let outcome = if funnel.outcome_improved_available {
                        funnel.outcome_improved.to_string()
                    } else {
                        "N/A".into()
                    };
                    out.push_str(&format!(
                        "\n  漏斗 A→R→I→U→Y: {}/{}/{}/{}/{} (available/retrieved/injected/action_changed/outcome_improved)",
                        funnel.available,
                        funnel.retrieved,
                        funnel.injected,
                        funnel.action_changed,
                        outcome
                    ));
                    if let Ok(db) = kanzei_core::SessionStore::open(
                        &kanzei_core::project_state_path(&ctx.project_root),
                    ) {
                        if let Ok(metrics) = db.recall_metrics() {
                            for metric in metrics {
                                out.push_str(&format!(
                                    "\n  触发 {}: events={} retrieved={} injected={} precision={:.2} recall={:.2}",
                                    metric.trigger_type, metric.events, metric.retrieved_events,
                                    metric.injected_events, metric.precision, metric.recall
                                ));
                            }
                        }
                    }
                }
            }
            let pending = store.pending_notes();
            if pending > 0 {
                out.push_str(&format!(" · inbox {pending} pending"));
            }
            // R-165 批4 memory pressure(内容④):active 记忆过多会稀释检索注入,
            // 提示整理(归档/合并),引擎不自动删。
            let active_count = entries.iter().filter(|(_, e)| e.status == "active").count();
            if active_count > 500 {
                out.push_str(&format!(
                    " · ⚠ memory pressure: {active_count} active(>500) — 建议归档失效条目或合并重复"
                ));
            }
            for issue in store.integrity_issues() {
                out.push_str(&format!("\n  ⚠ {issue}"));
            }
            // 零采纳候选:召回≥3 从未拉正文 = 语义显著但决策无关的头号嫌疑,
            // 供空闲整理与 UI 消费;这里只报不删(淘汰决定留给人,墓碑可逆)。
            let mut flagged = 0usize;
            for (id, (recalled, fetched)) in &profile {
                if *recalled < 3 || *fetched > 0 || flagged >= 3 {
                    continue;
                }
                if let Some((_, e)) = entries
                    .iter()
                    .find(|(_, e)| &e.id == id && e.status == "active")
                {
                    out.push_str(&format!(
                        "\n  ⚠ 零采纳候选 {}《{}》召回 {} 次未被采纳",
                        id, e.title, recalled
                    ));
                    flagged += 1;
                }
            }
            // R-166 批5(内容⑥,验收④):deprecate 候选——F(m) 评估的
            // low value(effect_mean≤0)+ high confidence(样本足且 CI 窄)才报;
            // age 不参与。只报不删,真正的 deprecated 由 manager 按 reason 落。
            if store.scope.label() == "project" {
                if let Ok(db) =
                    kanzei_core::SessionStore::open(&store.root.join("..").join("state.db"))
                {
                    if let Ok(candidates) = db.deprecate_candidates(3, 0.34) {
                        for id in candidates.iter().take(3) {
                            if let Some((_, e)) = entries
                                .iter()
                                .find(|(_, e)| &e.id == id && e.status == "active")
                            {
                                out.push_str(&format!(
                                    "\n  ⚠ 反事实候选 {}《{}》F(m)≤0(拿掉不损失)+ 置信达标 — 可 memory_stale 归档",
                                    e.id, e.title
                                ));
                            }
                        }
                    }
                }
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
        (
            dir.clone(),
            ToolCtx {
                cwd: dir.clone(),
                project_root: dir,
                ..Default::default()
            },
        )
    }

    #[tokio::test]
    async fn search_note_stats_roundtrip_on_project_scope() {
        let (dir, ctx) = ctx();
        let store = MemoryStore::project(&ctx.project_root);
        match store
            .add(
                "sop",
                "发版 SOP 两条通道",
                "发版发布安装更新必读",
                "package.ps1 -Publish",
                "user",
                &[],
                None,
                false,
            )
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
        assert!(
            note.content.contains("pending notes: 1"),
            "{}",
            note.content
        );

        let stats = MemoryStatsTool.execute(json!({}), &ctx).await;
        assert!(!stats.is_error);
        assert!(stats.content.contains("[project]"), "{}", stats.content);
        assert!(stats.content.contains("sop 1a/0s"), "{}", stats.content);
        assert!(
            stats.content.contains("inbox 1 pending"),
            "{}",
            stats.content
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn stats_reports_recall_adoption_and_flags_zero_adoption_candidates() {
        // R-149:召回≥3 从未拉正文的条目要在 stats 里被点名(只报不删)。
        let (dir, ctx) = ctx();
        let store = MemoryStore::project(&ctx.project_root);
        match store
            .add(
                "fact",
                "发版通道甲",
                "发版发布安装更新必读",
                "正文",
                "user",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            crate::memory::AddOutcome::Added(_) => {}
            _ => panic!("expected add"),
        }
        // D-366:决策排序在检索门面,经 index 取命中。
        let index = SqliteMemoryIndex::new(&ctx.project_root);
        let hits = index.search_entries(&IndexQuery::text("发版"), None, Some("active"), 5);
        assert!(!hits.is_empty());
        for _ in 0..3 {
            crate::memory::record_memory_search_telemetry(
                &ctx.project_root,
                "要发版了",
                &hits,
                false,
                "lexical",
                &crate::memory::index::RetrievalTiming::default(),
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // R-161:用不属于文件真源的遥测 ID 单独验证漏斗，不能把 M-001 的
        // 零采纳样本改成已注入。
        let mut funnel_hit = hits[0].clone();
        funnel_hit.entry.id = "M-telemetry".into();
        crate::memory::record_memory_search_telemetry(
            &ctx.project_root,
            "要发版了",
            std::slice::from_ref(&funnel_hit),
            true,
            "lexical",
            &crate::memory::index::RetrievalTiming::default(),
        );
        let stats = MemoryStatsTool.execute(json!({}), &ctx).await;
        assert!(!stats.is_error);
        assert!(stats.content.contains("召回 4/采纳 1"), "{}", stats.content);
        assert!(
            stats.content.contains("零采纳候选 M-001"),
            "{}",
            stats.content
        );
        assert!(
            stats.content.contains("召回 3 次未被采纳"),
            "{}",
            stats.content
        );
        // AVAILABLE 段按记忆库文件真源统计(本测试恰有 1 条 active)——旧口径数
        // 恒空的 memory_sources,首段永远 0,漏斗两端全是死数据。
        assert!(
            stats.content.contains("漏斗 A→R→I→U→Y: 1/2/1/0/N/A"),
            "{}",
            stats.content
        );
        assert!(
            stats.content.contains(
                "触发 memory_search: events=4 retrieved=4 injected=1 precision=0.25 recall=1.00"
            ),
            "{}",
            stats.content
        );
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
        let telemetry =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&ctx.project_root))
                .unwrap();
        let metrics = telemetry.recall_metrics().unwrap();
        let miss = metrics
            .iter()
            .find(|metric| metric.trigger_type == "memory_search")
            .expect("空检索必须写入 telemetry");
        assert_eq!(miss.events, 1);
        assert_eq!(miss.retrieved_events, 0);
        assert_eq!(miss.injected_events, 0);
        let bad = MemorySearchTool
            .execute(json!({"query": "x", "scope": "银河系"}), &ctx)
            .await;
        assert!(bad.is_error);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_note_validates_refs_and_carries_them_into_inbox() {
        // R-070:memory_note 的 refs 也走硬校验,非法整体拒绝;合法引用写进草稿行。
        let (dir, ctx) = ctx();
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-070 示例 [todo]\n- 验收: 略\n",
        )
        .unwrap();
        let bad = MemoryNoteTool
            .execute(json!({"summary": "假引用", "refs": ["D-999"]}), &ctx)
            .await;
        assert!(bad.is_error);
        assert!(bad.content.contains("invalid refs"), "{}", bad.content);
        let ok = MemoryNoteTool
            .execute(
                json!({"summary": "真引用", "refs": ["R-070"], "category_hint": "fact"}),
                &ctx,
            )
            .await;
        assert!(!ok.is_error, "{}", ok.content);
        let store = MemoryStore::project(&ctx.project_root);
        let inbox = store.read_inbox();
        assert!(inbox.contains("- refs: R-070"), "{inbox}");
        let (hint, summary, detail) = store.pending_note_list().pop().unwrap();
        assert_eq!((hint.as_str(), summary.as_str()), ("fact", "真引用"));
        assert!(detail.contains("refs: R-070"), "{detail}");
        std::fs::remove_dir_all(dir).ok();
    }
}
