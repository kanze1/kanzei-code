//! User feedback is an authoritative, versioned fact, separate from recall telemetry.

use super::{entry_revision, MemoryStore};

impl MemoryStore {
    pub fn record_user_feedback(
        &self,
        id: &str,
        expected_revision: &str,
        kind: kanzei_core::MemoryFeedbackKind,
    ) -> anyhow::Result<kanzei_core::MemoryUsageCounts> {
        let _tree_lock = self.tree_lock()?;
        let entries = self.load_all_checked()?;
        let (_, entry) = entries
            .iter()
            .find(|(_, entry)| entry.id == id)
            .ok_or_else(|| anyhow::anyhow!("记忆 {id} 不在当前活动集合中"))?;
        anyhow::ensure!(
            entry_revision(entry) == expected_revision,
            "记忆内容已变化，请刷新后评价"
        );
        let project_root = self
            .project_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("记忆反馈缺少所属项目，不能推断会话来源"))?;
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(project_root))?;
        let session_id = kanzei_core::project_session_id(project_root);
        store.create_session(&session_id, &project_root.display().to_string(), None)?;
        store.record_memory_feedback(&session_id, id, expected_revision, kind)?;
        Ok(store.memory_usage_counts()?.remove(id).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_review_is_bound_to_existing_memory_content_and_survives_reopening() {
        let root = std::env::temp_dir().join(format!(
            "kz-memory-feedback-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        let store = MemoryStore::project(&root);
        let entry = match store
            .add(
                "fact",
                "反馈版本检查",
                "读取反馈版本检查时",
                "保留来源与版本",
                "user",
                &[],
                None,
                true,
            )
            .unwrap()
        {
            super::super::AddOutcome::Added(entry) => entry,
            other => panic!("{other:?}"),
        };
        assert!(store
            .record_user_feedback(&entry.id, "stale", kanzei_core::MemoryFeedbackKind::Adopted)
            .is_err());
        let counts = store
            .record_user_feedback(
                &entry.id,
                &entry_revision(&entry),
                kanzei_core::MemoryFeedbackKind::Adopted,
            )
            .unwrap();
        assert_eq!(counts.adopted, 1);
        assert_eq!(counts.injected, 0);
        let reopened = MemoryStore::project(&root);
        assert_eq!(reopened.usage_counts()[&entry.id].adopted, 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
