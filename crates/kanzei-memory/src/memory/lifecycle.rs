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

use super::{MemoryEntry, MemoryScope};

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
