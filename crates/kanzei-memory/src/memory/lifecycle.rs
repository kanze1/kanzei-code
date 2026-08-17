//! 记忆生命周期(R-255 第二刀,从 store.promote/reconcile_candidates 提纯)。
//!
//! 独立理由:生命周期是「candidate → shadow → active / deprecated」的状态机策略——
//! candidate 老化、晋升、清退、provenance 门禁,是记忆研究里与准入同频变更的一层;
//! 提成独立结构后,改晋升/清退规则不必读懂存储落盘,且可不经 store 直接构造场景测
//! (验收②)(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):provenance 硬门禁——无来源不入 active,且「来源指向真实轮次」
//! 必须校验 episode_id 真实存在(manager 编造 id 不能蒙混过关);证据先落库、全部
//! 成功才置 active(写证据失败不产生 active 半成品);清退保留可追溯墓碑。

use std::path::Path;

use super::{today, MemoryEntry, MemoryScope, MemoryStore};

/// 记忆生命周期策略:纯判定(should_promote/should_deprecate)+ 带库的门禁
/// (promote_guard)。无自有状态。
pub(crate) struct MemoryLifecycle;

impl MemoryLifecycle {
    /// promote 的 provenance 硬门禁:非空证据、状态机(candidate|shadow 可进)、
    /// episode 真实存在(project scope 查 state.db)、证据先落库全部成功。
    /// 校验通过后由调用方置 active 并落盘。
    pub(crate) fn promote_guard(
        &self,
        id: &str,
        entry: &MemoryEntry,
        sources: &[(i64, Option<i64>, Option<i64>)],
        source_hash: Option<&str>,
        scope: MemoryScope,
        root: &Path,
    ) -> anyhow::Result<()> {
        if sources.is_empty() {
            anyhow::bail!(
                "cannot promote `{id}`: no memory_sources evidence — R-165 provenance \
                 hard constraint, a candidate needs at least one episode source"
            );
        }
        if entry.status != "candidate" && entry.status != "shadow" {
            anyhow::bail!(
                "cannot promote `{id}`: status is `{}`, only candidate|shadow can be promoted",
                entry.status
            );
        }
        // 证据落 state.db memory_sources 表(与 episodes 同库,可 join)。
        // 仅 project scope 有 state.db(global 记忆无 episode 证据源)。
        let hash = source_hash.unwrap_or("compiler").to_string();
        let store = if scope == MemoryScope::Project {
            let db_path = root.join("..").join("state.db");
            match kanzei_core::SessionStore::open(&db_path) {
                Ok(store) => Some(store),
                Err(e) => anyhow::bail!(
                    "cannot promote `{id}`: cannot open state.db for provenance check ({e}) — \
                     证据落库失败,拒绝晋升"
                ),
            }
        } else {
            None
        };
        // R-213:promote 前校验每个 episode_id 真实存在——「无来源不入 active」必须是
        // 「来源指向真实轮次」,而不是只看数组非空,否则 manager 编造 id 也能蒙混过关。
        if let Some(store) = &store {
            for (episode_id, _, _) in sources {
                if !store.episode_exists(*episode_id)? {
                    anyhow::bail!(
                        "cannot promote `{id}`: episode_id {episode_id} does not exist in \
                         state.db episodes — provenance requires real episodes, not fabricated ids"
                    );
                }
            }
        }
        // R-213:证据先落库、全部成功才置 active——写证据失败不产生 active 条目,
        // 也不留下「active 却无证据」的半成品(回滚由顺序天然保证,无需手动回滚)。
        if let Some(store) = &store {
            for (episode_id, event_start, event_end) in sources {
                if let Err(e) =
                    store.record_memory_source(id, *episode_id, *event_start, *event_end, &hash)
                {
                    anyhow::bail!(
                        "cannot promote `{id}`: failed to record memory_source evidence \
                         (episode {episode_id}): {e} — promotion aborted, entry stays {}",
                        entry.status
                    );
                }
            }
        }
        Ok(())
    }

    /// candidate 自动晋升判定(R-195):有真实当轮 episode、复发计数≥3 且带
    /// fingerprint → 尝试 promote。返回是否应尝试晋升(实际晋升仍走 promote 的
    /// provenance 门禁;失败时调用方落回清退判定)。
    pub(crate) fn should_promote(
        &self,
        entry: &MemoryEntry,
        recurrence: u32,
        current_episode_id: Option<i64>,
    ) -> bool {
        current_episode_id
            .is_some_and(|_episode_id| recurrence >= 3 && entry.fingerprint().is_some())
    }

    /// candidate 清退判定(R-195):超过 max_age_days 个日历日未处置 → deprecated
    /// 并归档。返回 None = 保持 candidate(未验证不注入边界不变);Some(reason)
    /// = 应清退,reason 带可追溯墓碑说明。
    pub(crate) fn should_deprecate(
        &self,
        age_days: Option<i64>,
        max_age_days: i64,
        path_display: &str,
    ) -> Option<String> {
        let age_limit = max_age_days.max(1);
        if age_days.is_some_and(|days| days >= age_limit) {
            Some(format!(
                "(auto-deprecated: candidate 超过 {age_limit} 个日历日未完成晋升，\
                 无满足条件的 recurrence/provenance；原路径 {path_display})"
            ))
        } else {
            None
        }
    }
}

impl MemoryStore {
    /// candidate → shadow(R-166):进入评估期,可被离线回放评估但不注入生产检索。
    /// 与 promote 不同,to_shadow 不需要 provenance——评估本身就是验证,
    /// 评估通过后 promote(带证据)才进 active。状态机:只有 candidate 可进 shadow。
    pub fn to_shadow(&self, id: &str) -> anyhow::Result<MemoryEntry> {
        let entries = self.load_all();
        let Some((path, mut entry)) = entries.into_iter().find(|(_, e)| e.id == id) else {
            anyhow::bail!("unknown memory id `{id}`");
        };
        if entry.status != "candidate" {
            anyhow::bail!(
                "cannot to_shadow `{id}`: status is `{}`, only candidate can enter shadow",
                entry.status
            );
        }
        entry.status = "shadow".into();
        entry.updated = today();
        self.write_entry(&entry, Some(&path))?;
        self.refresh_derived()?;
        let source_refs = entry.refs();
        super::record_memory_lifecycle_event(
            self.project_root.as_deref(),
            "memory_candidate_shadowed",
            Some(&entry.id),
            &[],
            Some(&entry.source),
            &source_refs,
            "shadow_evaluation_started",
            Some("candidate"),
            Some("shadow"),
        );
        Ok(entry)
    }

    /// candidate 派生索引计数(recency 报告/收尾统计用)。
    pub(crate) fn candidate_index_count(&self) -> usize {
        let Ok(conn) = self.open_db() else { return 0 };
        conn.query_row(
            "SELECT COUNT(*) FROM memory_fts WHERE status = 'candidate'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count.max(0) as usize)
        .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    // R-255 验收②:生命周期的独立可测入口——不经 store.promote/reconcile_candidates,
    // 直接构造 MemoryEntry 场景调 MemoryLifecycle 方法。
    use super::*;
    use std::path::PathBuf;

    fn entry(status: &str, body: &str) -> MemoryEntry {
        MemoryEntry {
            id: "M-001".into(),
            scope: "project".into(),
            category: "fact".into(),
            title: "t".into(),
            description: "d".into(),
            status: status.into(),
            created: "2026-01-01".into(),
            updated: "2026-01-01".into(),
            source: "user".into(),
            extras: Vec::new(),
            body: body.into(),
        }
    }

    #[test]
    fn 晋升判定_复发门槛与指纹与当轮episode() {
        let lifecycle = MemoryLifecycle;
        // 复发≥3 + 带指纹 + 有当轮 episode → 尝试晋升。
        assert!(lifecycle.should_promote(&entry("candidate", "踩坑 [fp:abc]"), 3, Some(7)));
        // 复发不足 / 无指纹 / 无当轮 episode → 不尝试。
        assert!(!lifecycle.should_promote(&entry("candidate", "踩坑 [fp:abc]"), 2, Some(7)));
        assert!(!lifecycle.should_promote(&entry("candidate", "无指纹"), 3, Some(7)));
        assert!(!lifecycle.should_promote(&entry("candidate", "踩坑 [fp:abc]"), 3, None));
    }

    #[test]
    fn 清退判定_超龄才deprecate() {
        let lifecycle = MemoryLifecycle;
        let path = "M-001-x.md";
        // 达到/超过 max_age_days → Some(应清退)。
        assert!(lifecycle.should_deprecate(Some(30), 30, path).is_some());
        assert!(lifecycle.should_deprecate(Some(31), 30, path).is_some());
        // 未超龄 / 无年龄信息 → None(保持 candidate)。
        assert!(lifecycle.should_deprecate(Some(29), 30, path).is_none());
        assert!(lifecycle.should_deprecate(None, 30, path).is_none());
        // max_age_days ≤ 0 时按 1 算(不无限期挂起)。
        assert!(lifecycle.should_deprecate(Some(1), 0, path).is_some());
    }

    #[test]
    fn lifecycle事件账本保留转换与溯源字段() {
        let project = std::env::temp_dir().join(format!(
            "kz-memory-lifecycle-events-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(project.join(".kanzei")).unwrap();
        let refs = vec!["R-286".to_string()];
        super::super::record_memory_lifecycle_event(
            Some(&project),
            "memory_candidate_promoted",
            Some("M-001"),
            &[7],
            Some("run:episode-7"),
            &refs,
            "provenance_verified",
            Some("shadow"),
            Some("active"),
        );
        let db =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project)).unwrap();
        let session_id = kanzei_core::project_session_id(&project);
        let events = db.list_events(&session_id, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "memory_candidate_promoted");
        assert_eq!(events[0].payload["memory_id"], "M-001");
        assert_eq!(events[0].payload["episode_ids"], serde_json::json!([7]));
        assert_eq!(events[0].payload["source_id"], "run:episode-7");
        assert_eq!(events[0].payload["reason_code"], "provenance_verified");
        assert_eq!(events[0].payload["transition_from"], "shadow");
        assert_eq!(events[0].payload["transition_to"], "active");
        std::fs::remove_dir_all(project).ok();
    }

    #[test]
    fn promote门禁_状态机与证据非空() {
        let lifecycle = MemoryLifecycle;
        let sources = [(7i64, None, None)];
        let root = PathBuf::from("no-such-project");
        // global scope:不落 state.db,只走状态机+非空(可脱离数据库独立测)。
        let cand = entry("candidate", "b");
        assert!(lifecycle
            .promote_guard("M-001", &cand, &sources, None, MemoryScope::Global, &root)
            .is_ok());
        // active 不能晋升(状态机)。
        let active = entry("active", "b");
        assert!(lifecycle
            .promote_guard("M-001", &active, &sources, None, MemoryScope::Global, &root)
            .is_err());
        // 空证据 → 拒绝(provenance 硬约束)。
        assert!(lifecycle
            .promote_guard("M-001", &cand, &[], None, MemoryScope::Global, &root)
            .is_err());
    }
}
