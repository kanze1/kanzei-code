//! Cold-memory decisions require versioned opportunities; archive remains reversible.
use super::{entry_revision, MemoryEntry, MemoryScope, MemoryStore};
use std::collections::BTreeMap;

const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Copy)]
pub struct MemoryRetentionPolicy {
    pub cold_after_days: u32,
    pub minimum_opportunities: u32,
    pub quarantine_days: u32,
}

impl Default for MemoryRetentionPolicy {
    fn default() -> Self {
        Self {
            cold_after_days: 90,
            minimum_opportunities: 30,
            quarantine_days: 30,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct MemoryRetentionReport {
    pub promoted: Vec<String>,
    pub demoted: Vec<String>,
    pub archived: Vec<String>,
    pub protected: Vec<String>,
}

/// Content identity survives lifecycle transitions, but never a semantic edit.
pub fn memory_content_revision(entry: &MemoryEntry) -> String {
    let mut content = entry.clone();
    content.status.clear();
    content.updated.clear();
    content
        .extras
        .retain(|(key, _)| !key.starts_with("retention_") && key != "reviewed_content_revision");
    entry_revision(&content)
}

fn field<'a>(entry: &'a MemoryEntry, key: &str) -> Option<&'a str> {
    entry
        .extras
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn protected(entry: &MemoryEntry) -> bool {
    entry.source == "user"
        || entry.source.starts_with("user:")
        || matches!(entry.category.as_str(), "preference" | "habit")
        || ["protected", "pinned", "constraint"]
            .iter()
            .any(|key| field(entry, key) == Some("true"))
}

impl MemoryStore {
    pub fn record_availability(&self, run_id: &str) -> anyhow::Result<()> {
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("记忆机会缺少项目来源"))?;
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))?;
        let session = kanzei_core::project_session_id(root);
        store.create_session(&session, &root.display().to_string(), None)?;
        let entries = self
            .load_all_checked()?
            .into_iter()
            .filter(|(_, entry)| entry.status == "active")
            .map(|(_, entry)| kanzei_core::AvailableMemory {
                revision: memory_content_revision(&entry),
                id: entry.id,
            })
            .collect::<Vec<_>>();
        store.record_memory_availability(&session, run_id, &entries)?;
        Ok(())
    }

    pub fn reconcile_retention(
        &self,
        policy: MemoryRetentionPolicy,
        at_ms: i64,
    ) -> anyhow::Result<MemoryRetentionReport> {
        anyhow::ensure!(
            policy.minimum_opportunities > 0
                && policy.cold_after_days > 0
                && policy.quarantine_days > 0,
            "记忆保留策略必须有非零观察与隔离窗口"
        );
        anyhow::ensure!(
            self.scope == MemoryScope::Project,
            "全局记忆不能根据单个项目的未命中情况自动清退"
        );
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("记忆保留策略缺少项目来源"))?;
        let state = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))?;
        let session = kanzei_core::project_session_id(root);
        state.create_session(&session, &root.display().to_string(), None)?;
        let observations = state.memory_retention_counts()?;
        let feedback = state.memory_usage_counts()?;
        let _lock = self.tree_lock()?;
        let mut report = MemoryRetentionReport::default();
        for (path, mut entry) in self.load_all_checked()? {
            let before = entry.clone();
            let revision = memory_content_revision(&entry);
            let reviewed = feedback.get(&entry.id).filter(|usage| {
                usage.feedback_revision.as_deref() == Some(entry_revision(&entry).as_str())
            });
            let latest = reviewed.and_then(|usage| usage.last_feedback);
            let last_adopted = reviewed
                .filter(|usage| {
                    usage.last_feedback == Some(kanzei_core::MemoryFeedbackKind::Adopted)
                })
                .map(|usage| usage.last_feedback_at);
            let cold = field(&entry, "retention_reason") == Some("cold");
            let previous_review = field(&entry, "retention_previous_status")
                .filter(|status| matches!(*status, "active" | "candidate" | "shadow"))
                .map(str::to_owned);
            let withdrawn_review = previous_review.filter(|_| {
                latest == Some(kanzei_core::MemoryFeedbackKind::Cleared)
                    && matches!(
                        field(&entry, "retention_reason"),
                        Some("user_reviewed" | "user_corrected")
                    )
            });
            let reason = if let Some(previous_status) = withdrawn_review {
                entry.status = previous_status;
                if entry.status == "active" {
                    report.promoted.push(entry.id.clone());
                } else {
                    report.demoted.push(entry.id.clone());
                }
                Some("review_withdrawn")
            } else if matches!(entry.status.as_str(), "candidate" | "shadow")
                && latest == Some(kanzei_core::MemoryFeedbackKind::Adopted)
                && (entry.category != "sop" || cold)
            {
                entry.status = "active".into();
                report.promoted.push(entry.id.clone());
                Some("user_reviewed")
            } else if entry.status == "active"
                && latest == Some(kanzei_core::MemoryFeedbackKind::Corrected)
            {
                entry.status = "shadow".into();
                report.demoted.push(entry.id.clone());
                Some("user_corrected")
            } else if protected(&entry) {
                report.protected.push(entry.id.clone());
                None
            } else if entry.status == "active"
                && cold_enough(&entry, &observations, policy, at_ms, last_adopted)
            {
                entry.status = "shadow".into();
                report.demoted.push(entry.id.clone());
                Some("cold")
            } else if entry.status == "shadow"
                && cold
                && field(&entry, "retention_revision") == Some(revision.as_str())
                && field(&entry, "retention_at")
                    .and_then(|value| value.parse::<i64>().ok())
                    .is_some_and(|at| {
                        at_ms.saturating_sub(at) >= i64::from(policy.quarantine_days) * DAY_MS
                    })
            {
                entry.status = "deprecated".into();
                report.archived.push(entry.id.clone());
                Some("cold_quarantine_elapsed")
            } else {
                None
            };
            let Some(reason) = reason else {
                continue;
            };
            if entry.requires_review() {
                if entry.status == "active" && reason == "user_reviewed" {
                    entry
                        .extras
                        .retain(|(key, _)| key != "reviewed_content_revision");
                    entry
                        .extras
                        .push(("reviewed_content_revision".into(), revision.clone()));
                } else if entry.status == "candidate" {
                    entry
                        .extras
                        .retain(|(key, _)| key != "reviewed_content_revision");
                }
            }
            entry
                .extras
                .retain(|(key, _)| !key.starts_with("retention_"));
            if entry.status == "shadow" || matches!(reason, "user_reviewed" | "user_corrected") {
                entry.extras.extend([
                    ("retention_at".into(), at_ms.to_string()),
                    ("retention_reason".into(), reason.into()),
                    ("retention_revision".into(), revision.clone()),
                ]);
                if matches!(reason, "user_reviewed" | "user_corrected") {
                    entry
                        .extras
                        .push(("retention_previous_status".into(), before.status.clone()));
                }
            }
            entry.updated = super::today();
            let payload = serde_json::json!({"memory_id":entry.id,"before_revision":entry_revision(&before),"after_revision":entry_revision(&entry),"from":before.status,"to":entry.status,"reason":reason,"at_ms":at_ms,"observation":observations.get(&(entry.id.clone(),revision))});
            state.append_event(&session, "memory.retention.planned", &payload)?;
            self.write_entry(&entry, Some(&path))?;
            self.refresh_derived()?;
            state.append_event(&session, "memory.retention.completed", &payload)?;
        }
        Ok(report)
    }
}

fn cold_enough(
    entry: &MemoryEntry,
    observations: &BTreeMap<(String, String), kanzei_core::MemoryRetentionCounts>,
    policy: MemoryRetentionPolicy,
    now: i64,
    last_adopted: Option<i64>,
) -> bool {
    let Some(counts) = observations.get(&(entry.id.clone(), memory_content_revision(entry))) else {
        return false;
    };
    let updated = super::date_days(&entry.updated)
        .map(|days| (days - super::date_days("1970-01-01").expect("civil epoch")) * DAY_MS)
        .unwrap_or(now);
    let since = counts
        .last_retrieved_at
        .unwrap_or(counts.first_opportunity_at)
        .max(updated)
        .max(last_adopted.unwrap_or(i64::MIN));
    counts.opportunities >= u64::from(policy.minimum_opportunities)
        && now.saturating_sub(since) >= i64::from(policy.cold_after_days) * DAY_MS
}

#[cfg(test)]
mod tests {
    use super::super::AddOutcome;
    use super::*;

    fn create(store: &MemoryStore, title: &str, category: &str, source: &str) -> MemoryEntry {
        match store
            .add(
                category,
                title,
                "适用条件",
                &format!("{title} 的事实与依据"),
                source,
                &[],
                None,
                true,
            )
            .unwrap()
        {
            AddOutcome::Added(entry) => entry,
            outcome => panic!("{outcome:?}"),
        }
    }

    #[test]
    fn recent_explicit_adoption_refreshes_an_active_memories_cold_window() {
        let root = std::env::temp_dir().join(format!(
            "kz-retention-adopted-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        let store = MemoryStore::project(&root);
        let mut entry = create(&store, "旧项目事实", "fact", "memory-manager");
        entry.status = "active".into();
        entry.updated = "2020-01-01".into();
        let path = store.load_all_checked().unwrap().remove(0).0;
        store.write_entry(&entry, Some(&path)).unwrap();
        store.refresh_derived().unwrap();
        for index in 0..30 {
            store.record_availability(&format!("old-{index}")).unwrap();
        }
        let now = super::super::store::now_ms();
        let connection =
            rusqlite::Connection::open(kanzei_core::project_state_path(&root)).unwrap();
        connection
            .execute(
                "UPDATE session_events SET created_at=?1 WHERE event_type='memory.availability'",
                [now - 100 * DAY_MS],
            )
            .unwrap();
        drop(connection);
        let state =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        assert!(cold_enough(
            &entry,
            &state.memory_retention_counts().unwrap(),
            Default::default(),
            now,
            None
        ));
        store
            .record_user_feedback(
                &entry.id,
                &entry_revision(&entry),
                kanzei_core::MemoryFeedbackKind::Adopted,
            )
            .unwrap();
        assert!(store
            .reconcile_retention(Default::default(), now)
            .unwrap()
            .demoted
            .is_empty());
        assert_eq!(store.load_all_checked().unwrap()[0].1.status, "active");
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reviewed_candidate_can_be_adopted_and_withdrawn_without_losing_its_source() {
        let root = std::env::temp_dir().join(format!(
            "kz-retention-withdraw-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        let store = MemoryStore::project(&root);
        let provenance =
            serde_json::json!({"evidence":[{"quote":"喜欢简洁排版","source_id":"original"}]});
        let entry = match store
            .add_review_candidate(super::super::ReviewCandidateDraft {
                category: "preference",
                title: "简洁排版偏好",
                description: "编写文档时采用简洁排版",
                body: "喜欢简洁排版",
                idempotency_key: "proposal",
                provenance: &provenance,
            })
            .unwrap()
        {
            AddOutcome::Added(entry) => entry,
            other => panic!("{other:?}"),
        };
        assert!(store
            .update(&entry.id, None, None, None, Some("active"), None, false)
            .is_err());
        store
            .record_user_feedback(
                &entry.id,
                &entry_revision(&entry),
                kanzei_core::MemoryFeedbackKind::Adopted,
            )
            .unwrap();
        store
            .reconcile_retention(Default::default(), super::super::store::now_ms())
            .unwrap();
        let active = store.load_all_checked().unwrap().remove(0).1;
        assert_eq!(active.status, "active");
        store
            .record_user_feedback(
                &active.id,
                &entry_revision(&active),
                kanzei_core::MemoryFeedbackKind::Cleared,
            )
            .unwrap();
        store
            .reconcile_retention(Default::default(), super::super::store::now_ms())
            .unwrap();
        let reverted = store.load_all_checked().unwrap().remove(0).1;
        assert_eq!(reverted.status, "candidate");
        assert_eq!(reverted.body, entry.body);
        assert_eq!(field(&reverted, "provenance"), field(&entry, "provenance"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cold_memory_uses_observed_opportunities_then_quarantine_and_reversible_archive() {
        let root = std::env::temp_dir().join(format!(
            "kz-retention-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        let store = MemoryStore::project(&root);
        let cold = create(&store, "旧工具事实", "fact", "memory-manager");
        let user = create(&store, "用户明确限制", "fact", "user");
        let preference = create(&store, "长期审美偏好", "preference", "memory-manager");
        for entry in [&cold, &preference] {
            store
                .update(&entry.id, None, None, None, Some("active"), None, false)
                .unwrap();
        }
        let now = super::super::store::now_ms();
        assert!(
            store
                .reconcile_retention(Default::default(), now + 100 * DAY_MS)
                .unwrap()
                .demoted
                .is_empty(),
            "old age without measured opportunities is not evidence of disuse"
        );
        for index in 0..30 {
            store.record_availability(&format!("run-{index}")).unwrap();
        }
        assert!(store
            .reconcile_retention(Default::default(), now + 89 * DAY_MS)
            .unwrap()
            .demoted
            .is_empty());
        let report = store
            .reconcile_retention(Default::default(), now + 91 * DAY_MS)
            .unwrap();
        assert_eq!(report.demoted, vec![cold.id.clone()]);
        assert!(report.protected.contains(&user.id) && report.protected.contains(&preference.id));
        assert!(store
            .reconcile_retention(Default::default(), now + 120 * DAY_MS)
            .unwrap()
            .archived
            .is_empty());
        assert_eq!(
            store
                .reconcile_retention(Default::default(), now + 122 * DAY_MS)
                .unwrap()
                .archived,
            vec![cold.id.clone()]
        );
        let restored = store.restore_archived(&cold.id, "active").unwrap();
        assert_eq!(restored.body, cold.body);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_review_promotes_facts_but_cannot_invent_verified_sop_outcomes() {
        let root = std::env::temp_dir().join(format!(
            "kz-retention-review-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        let store = MemoryStore::project(&root);
        let fact = create(&store, "推断的项目事实", "fact", "memory-manager");
        let sop = create(&store, "尚待实证的方法", "sop", "memory-manager");
        for entry in [&fact, &sop] {
            store
                .record_user_feedback(
                    &entry.id,
                    &entry_revision(entry),
                    kanzei_core::MemoryFeedbackKind::Adopted,
                )
                .unwrap();
        }
        let report = store
            .reconcile_retention(Default::default(), super::super::store::now_ms())
            .unwrap();
        assert_eq!(report.promoted, vec![fact.id.clone()]);
        let entries = store.load_all();
        assert_eq!(
            entries
                .iter()
                .find(|(_, entry)| entry.id == sop.id)
                .unwrap()
                .1
                .status,
            "candidate"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
