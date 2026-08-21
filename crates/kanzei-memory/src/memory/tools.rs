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
    /// note(默认):一句话:学到了什么/踩了什么坑/用户定了什么调。
    /// correct:只修正既有条目的一个文本字段,不进入 inbox。
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    summary: Option<String>,
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
    /// correct action 专用:project|global,既有条目 id,以及单一 title/description/body 字段。
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    old_value: Option<String>,
    #[serde(default)]
    new_value: Option<String>,
    /// correct action 必须提供当前渲染 hash 与人工可复核的修正依据。
    #[serde(default)]
    expected_hash: Option<String>,
    #[serde(default)]
    basis: Option<String>,
}

fn required_correction<'a>(value: Option<&'a String>, name: &str) -> Result<&'a str, String> {
    value
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("correct action requires {name}"))
}

pub struct MemoryNoteTool;

#[async_trait]
impl Tool for MemoryNoteTool {
    fn name(&self) -> &'static str {
        "memory_note"
    }

    fn description(&self) -> String {
        "Record a draft note in the memory inbox, or synchronously correct one existing title/description/body field with action=correct. note params: summary; optional detail, category_hint, refs. correct params: scope, id, field, old_value, new_value, expected_hash, basis. correct never adds/deletes entries or changes status/extra fields and writes an audit record with actor, basis, old/new values.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(NoteInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: NoteInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let action = input.action.as_deref().unwrap_or("note");
        if action == "correct" {
            let scope = input.scope.as_deref().unwrap_or("project");
            let store = match scope {
                "project" => MemoryStore::project(&ctx.project_root),
                "global" => match MemoryStore::global() {
                    Some(store) => store,
                    None => return ToolOutput::error("no home dir for global scope"),
                },
                other => {
                    return ToolOutput::error(format!(
                        "invalid correction scope `{other}`; valid: project | global"
                    ))
                }
            };
            let id = match required_correction(input.id.as_ref(), "id") {
                Ok(value) => value,
                Err(error) => return ToolOutput::error(error),
            };
            let field = match required_correction(input.field.as_ref(), "field") {
                Ok(value) => value,
                Err(error) => return ToolOutput::error(error),
            };
            let old_value = match required_correction(input.old_value.as_ref(), "old_value") {
                Ok(value) => value,
                Err(error) => return ToolOutput::error(error),
            };
            let new_value = match required_correction(input.new_value.as_ref(), "new_value") {
                Ok(value) => value,
                Err(error) => return ToolOutput::error(error),
            };
            let expected_hash =
                match required_correction(input.expected_hash.as_ref(), "expected_hash") {
                    Ok(value) => value,
                    Err(error) => return ToolOutput::error(error),
                };
            let basis = match required_correction(input.basis.as_ref(), "basis") {
                Ok(value) => value,
                Err(error) => return ToolOutput::error(error),
            };
            return match store.correct_text(id, field, old_value, new_value, expected_hash, basis) {
                Ok(entry) => ToolOutput::ok(format!(
                    "corrected {} {} [{}]; audit: {}",
                    entry.id,
                    field,
                    entry.title,
                    store.root.join("corrections.jsonl").display()
                )),
                Err(error) => ToolOutput::error(error.to_string()),
            };
        }
        if action != "note" {
            return ToolOutput::error("action must be note or correct");
        }
        let Some(summary) = input
            .summary
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            return ToolOutput::error("summary must not be empty");
        };

        if let Err(e) = super::validate_source_refs(ctx, &input.refs) {
            return ToolOutput::error(e);
        }
        let store = MemoryStore::project(&ctx.project_root);
        // R-165 批2 novelty gate(验收④):投递前机械三档分流——
        // 明显重复直接 NOOP(不占 LLM run 与 inbox),记遥测;新/不确定才进 inbox。
        // R-216:返回 (判定, 候选),Uncertain 也直接进 inbox 交 manager(候选仅 add 硬闸用)。
        let (novelty, _candidates) =
            store.classify_novelty(summary, input.detail.as_deref().unwrap_or(""), "");
        if novelty == super::Novelty::Duplicate {
            store.record_novelty(&novelty, "", summary);
            return ToolOutput::ok(format!(
                "noted as duplicate (NOOP, {:.60}…) — already an active memory covers it; \
                 use memory_update to evolve that entry instead of re-adding",
                summary
            ));
        }
        store.record_novelty(&novelty, "", summary);
        match store.append_note(
            summary,
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
            let decisions = store.manager_decision_counts();
            if !decisions.is_empty() {
                out.push_str(&format!(
                    " · manager decisions noop={} produced={} rejected={}",
                    decisions.get("noop").copied().unwrap_or(0),
                    decisions.get("produced").copied().unwrap_or(0),
                    decisions.get("rejected").copied().unwrap_or(0)
                ));
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

    #[tokio::test]
    async fn memory_note_correct_synchronously_updates_and_audits_existing_text() {
        // R-316:真实 memory_note 调用验证同步落盘、CAS 与旧/新值审计。
        let (dir, ctx) = ctx();
        let store = MemoryStore::project(&ctx.project_root);
        match store
            .add(
                "fact",
                "原始标题",
                "原始描述",
                "原始正文",
                "user",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            super::super::AddOutcome::Added(_) => {}
            other => panic!("expected add, got {other:?}"),
        }
        let before = store
            .load_all()
            .into_iter()
            .find(|(_, entry)| entry.title == "原始标题")
            .expect("seeded memory entry");
        let id = before.1.id.clone();
        let expected_hash =
            kanzei_base::content_hash(crate::memory::render_entry(&before.1).as_bytes());
        let corrected = MemoryNoteTool
            .execute(
                json!({
                    "action": "correct",
                    "scope": "project",
                    "id": id,
                    "field": "description",
                    "old_value": "原始描述",
                    "new_value": "修正后的描述",
                    "expected_hash": expected_hash,
                    "basis": "与 git 历史真源逐行比对"
                }),
                &ctx,
            )
            .await;
        assert!(!corrected.is_error, "{}", corrected.content);
        assert!(corrected.content.contains("corrections.jsonl"));
        let after = store
            .load_all()
            .into_iter()
            .find(|(_, entry)| entry.id == id)
            .expect("corrected memory entry");
        assert_eq!(after.1.description, "修正后的描述");
        assert_eq!(after.1.status, before.1.status);
        let audit = std::fs::read_to_string(store.root.join("corrections.jsonl")).unwrap();
        let record: serde_json::Value =
            serde_json::from_str(audit.lines().next().unwrap()).unwrap();
        assert_eq!(record["event"], "memory_text_corrected");
        assert_eq!(record["actor"], "main-agent");
        assert_eq!(record["basis"], "与 git 历史真源逐行比对");
        assert_eq!(record["old_value"], "原始描述");
        assert_eq!(record["new_value"], "修正后的描述");

        let stale = MemoryNoteTool
            .execute(
                json!({
                    "action": "correct",
                    "id": id,
                    "field": "description",
                    "old_value": "修正后的描述",
                    "new_value": "错误并发覆盖",
                    "expected_hash": expected_hash,
                    "basis": "stale CAS must reject"
                }),
                &ctx,
            )
            .await;
        assert!(
            stale.is_error,
            "过期 expected_hash 不得覆盖: {}",
            stale.content
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn memory_note_correct_rolls_back_when_audit_write_fails() {
        // D-675:审计目标不可写时，正文修改必须回滚而不是留下无审计新值。
        let (dir, ctx) = ctx();
        let store = MemoryStore::project(&ctx.project_root);
        match store
            .add(
                "fact",
                "审计失败回滚",
                "旧描述",
                "旧正文",
                "user",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            super::super::AddOutcome::Added(_) => {}
            other => panic!("expected add, got {other:?}"),
        }
        let (_, entry) = store
            .load_all()
            .into_iter()
            .find(|(_, entry)| entry.title == "审计失败回滚")
            .unwrap();
        let expected_hash =
            kanzei_base::content_hash(crate::memory::render_entry(&entry).as_bytes());
        std::fs::create_dir(store.root.join("corrections.jsonl")).unwrap();
        let output = MemoryNoteTool
            .execute(
                json!({
                    "action": "correct",
                    "id": entry.id,
                    "field": "description",
                    "old_value": "旧描述",
                    "new_value": "不应留下",
                    "expected_hash": expected_hash,
                    "basis": "故障注入：审计目标不可写"
                }),
                &ctx,
            )
            .await;
        assert!(output.is_error, "审计失败必须返回错误: {}", output.content);
        let (_, restored) = store
            .load_all()
            .into_iter()
            .find(|(_, candidate)| candidate.title == "审计失败回滚")
            .unwrap();
        assert_eq!(restored.description, "旧描述");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn d568_two_corrupted_descriptions_are_corrected_in_one_session() {
        // R-316/D-568:同一 ToolCtx 连续修正两条既有记忆，验证不进 inbox 且 INDEX 同步。
        let (dir, ctx) = ctx();
        let store = MemoryStore::project(&ctx.project_root);
        for index in 1..=15 {
            let id = format!("M-{index:03}");
            let title = match index {
                14 => "HTML 静态文案必须登记进资源表,否则断言测试失败".to_string(),
                15 => "SSE 流内 context overflow 恢复须重建请求,OpenAI 错误分类须同查 type/code"
                    .to_string(),
                _ => format!("D568 占位 {index}"),
            };
            match store
                .add(
                    "fact",
                    &title,
                    &format!("错误描述 {id}"),
                    &format!("正文真源 {id}"),
                    "user",
                    &[],
                    None,
                    false,
                )
                .unwrap()
            {
                super::super::AddOutcome::Added(entry) => assert_eq!(entry.id, id),
                other => panic!("expected {id} add, got {other:?}"),
            }
        }

        let correct = [
            (
                "M-014",
                "编辑旧字符串不存在时必读：先 read 重读文件排版再精确构造 old_string — match exactly including whitespace;多处匹配勿用 replace_all 盲改。",
                "与 M-014 正文真源逐行比对",
            ),
            (
                "M-015",
                "所有 git mutation 在 bash 都被拦截，必须走结构化 git 工具 — 处理任何 Git 分支/索引变更时不要换别的 git 子命令重试。",
                "与 M-015 正文真源逐行比对",
            ),
        ];
        for (id, new_value, basis) in correct {
            let (_, entry) = store
                .load_all()
                .into_iter()
                .find(|(_, entry)| entry.id == id)
                .expect("D-568 entry");
            let expected_hash =
                kanzei_base::content_hash(crate::memory::render_entry(&entry).as_bytes());
            let output = MemoryNoteTool
                .execute(
                    json!({
                        "action": "correct",
                        "scope": "project",
                        "id": id,
                        "field": "description",
                        "old_value": entry.description,
                        "new_value": new_value,
                        "expected_hash": expected_hash,
                        "basis": basis
                    }),
                    &ctx,
                )
                .await;
            assert!(!output.is_error, "{id}: {}", output.content);
        }

        let index = std::fs::read_to_string(store.root.join("INDEX.md")).unwrap();
        assert!(index.contains("M-014") && index.contains("编辑旧字符串不存在时必读"));
        assert!(index.contains("M-015") && index.contains("所有 git mutation 在 bash 都被拦截"));
        let audit = std::fs::read_to_string(store.root.join("corrections.jsonl")).unwrap();
        assert_eq!(audit.lines().count(), 2);
        assert!(audit.contains("M-014") && audit.contains("M-015"));
        assert!(audit.contains("与 M-014 正文真源逐行比对"));
        assert!(audit.contains("与 M-015 正文真源逐行比对"));
        std::fs::remove_dir_all(dir).ok();
    }
}
