//! MemoryStore 的归档侧:读取 archive/ 下的归档条目与 ID、归档计数、把失效条目搬进 archive/,
//! 以及记忆图谱写 frontmatter `area:` 的入口。与 store.rs 同一个 `impl MemoryStore`,
//! 只是按职责拆到子模块(子模块可直接用父模块的私有方法,如 archive_dir / record_write_log)。

use std::path::PathBuf;

use super::super::admission::MemoryAdmission;
use super::super::{parse_entry, render_entry, today, MemoryEntry};
use super::MemoryStore;

impl MemoryStore {
    /// 记忆图谱:扫描 archive/ 加载全部归档条目(与 load_all 同写法;解析不了的跳过)。
    /// 只读,不触发归档搬移或派生物重建。
    pub fn load_archived(&self) -> Vec<(PathBuf, MemoryEntry)> {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir(self.archive_dir()) else {
            return out;
        };
        for item in dir.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                let entry = parse_entry(&text);
                if !entry.id.is_empty() {
                    out.push((path, entry));
                }
            }
        }
        out.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        out
    }

    /// 记忆图谱:写 frontmatter `area:`(规范区域 id,空格分隔);空切片 = 删除该键。
    /// 归一与存在性校验归调用方(AreaRegistry::resolve_token),这里只落盘。与 update 走同一条
    /// 写路径(update_with_area:持记忆树锁、一次写盘、写后 refresh_derived)。只改活动条目:归档条目只读。
    pub fn set_area(&self, id: &str, areas: &[String]) -> anyhow::Result<MemoryEntry> {
        self.update_with_area(id, None, None, None, None, Some(areas), None, false)
    }

    pub fn has_archived_id(&self, id: &str) -> bool {
        self.load_archived_ids()
            .iter()
            .any(|archived| archived == id)
    }

    pub(crate) fn load_archived_ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir(self.archive_dir()) else {
            return out;
        };
        for item in dir.flatten() {
            if let Some(name) = item.path().file_stem().and_then(|n| n.to_str()) {
                if let Some(id) = name.split('-').take(2).collect::<Vec<_>>().get(0..2) {
                    out.push(format!("{}-{}", id[0], id[1]));
                }
            }
        }
        out
    }

    /// 归档条数(D-217):stale/失效条目经 archive_dead 搬入 archive/ 后在此计数,
    /// 供整理清单展示「已归档待复查」积压。只读,不触发扫描副作用。
    pub fn archived_count(&self) -> usize {
        let Ok(dir) = std::fs::read_dir(self.archive_dir()) else {
            return 0;
        };
        dir.flatten()
            .filter(|p| p.path().extension().and_then(|e| e.to_str()) == Some("md"))
            .count()
    }

    /// 归档失效条目(D-231/R-165 验收③):deprecated/invalid 移入 archive/ 带墓碑。
    /// 返回归档条数。引擎强制:任何 refresh_derived(写操作后)都会先归档,
    /// 主目录只留 active/candidate——归档条目不在 load_all/FTS/检索范围内,
    /// ID 由 load_archived_ids 保留永不复用。
    pub fn archive_dead(&self) -> usize {
        let entries = self.load_all();
        let mut archived = 0usize;
        for (path, entry) in &entries {
            if entry.status != "deprecated" && entry.status != "invalid" {
                continue;
            }
            let archive_dir = self.archive_dir();
            std::fs::create_dir_all(&archive_dir).ok();
            let dest = archive_dir.join(format!("{}.md", entry.file_stem()));
            // 墓碑:保留文件(内容即追溯),目标已存在则跳过(防重复归档覆盖)。
            if dest.exists() {
                if std::fs::remove_file(path).is_ok() {
                    self.record_write_log(path, Vec::new());
                }
            } else if std::fs::rename(path, &dest).is_ok() {
                archived += 1;
                // D-480:rename 同时改变源路径和 archive 目标路径。两条日志都要记，
                // 围栏才能把「源删除 + 墓碑落盘」识别为同一次合法 memory_stale。
                self.record_write_log(path, Vec::new());
                self.record_write_log(&dest, render_entry(entry).into_bytes());
            }
        }
        archived
    }

    /// 撤销归档 / 从归档恢复:把 archive/ 里的条目搬回主目录并改成 `status`(candidate|shadow|active)。
    /// 界面「归档」按钮的反向操作:归档是 deprecated + 搬移,没有它用户误点就只能手改文件。
    /// 状态不变量照旧:同 category+subject 已有 active/candidate 时拒绝(绝不并存,报错点明冲突条目)。
    pub fn restore_archived(&self, id: &str, status: &str) -> anyhow::Result<MemoryEntry> {
        let status = super::super::normalize_status(status);
        if !matches!(status, "candidate" | "shadow" | "active") {
            anyhow::bail!("恢复后的状态只能是 candidate | shadow | active,收到 `{status}`");
        }
        let _tree_lock = self.tree_lock()?;
        let Some((path, mut entry)) = self
            .load_archived()
            .into_iter()
            .find(|(_, entry)| entry.id == id)
        else {
            anyhow::bail!("归档里没有记忆 `{id}`(可能已被恢复或不在这个范围)");
        };
        let live = self.load_all();
        if live.iter().any(|(_, existing)| existing.id == id) {
            anyhow::bail!("记忆 `{id}` 在主目录里已有同编号条目,拒绝覆盖;请先处理重复文件");
        }
        let subject = entry.field("subject").map(str::to_string);
        if matches!(status, "candidate" | "active") {
            if let Some(conflict) =
                MemoryAdmission::find_subject_conflict(&live, &entry.category, subject.as_deref())
            {
                anyhow::bail!(
                    "同主题的 {} 正在使用({}),恢复会让两条并存;先归档它或用管理对话合并",
                    conflict.id,
                    conflict.status
                );
            }
        }
        entry.status = status.into();
        entry.updated = today();
        // 先写主目录再删归档:中途失败只会留下重复(archive_dead 会按「目标已存在」收拾),不会丢条目。
        self.write_entry(&entry, None)?;
        std::fs::remove_file(&path)?;
        self.record_write_log(&path, Vec::new());
        self.refresh_derived()?;
        Ok(entry)
    }
}

/// 把规范区域 id 列表写进条目的 `area:` 额外字段(去空、去重、保序,空列表即删除该键)。
pub(super) fn set_area_field(entry: &mut MemoryEntry, areas: &[String]) {
    let mut seen = std::collections::BTreeSet::new();
    let value = areas
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty() && seen.insert(a.to_string()))
        .collect::<Vec<_>>()
        .join(" ");
    entry.extras.retain(|(key, _)| key != "area");
    if !value.is_empty() {
        entry.extras.push(("area".to_string(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::super::AddOutcome;
    use super::*;

    fn temp_store() -> (PathBuf, MemoryStore) {
        let dir = std::env::temp_dir().join(format!(
            "kz-memory-archive-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        (dir, store)
    }

    fn add_active(
        store: &MemoryStore,
        title: &str,
        desc: &str,
        subject: Option<&str>,
    ) -> MemoryEntry {
        let entry = match store
            .add("fact", title, desc, "正文", "user", &[], subject, true)
            .unwrap()
        {
            AddOutcome::Added(entry) => entry,
            other => panic!("expected added, got {other:?}"),
        };
        store
            .update(&entry.id, None, None, None, Some("active"), None, false)
            .unwrap()
    }

    #[test]
    fn restore_archived_moves_entry_back_and_rejects_bad_status() {
        let (dir, store) = temp_store();
        let entry = add_active(&store, "归档恢复条目", "撤销归档时必读", None);
        store
            .update(&entry.id, None, None, None, Some("deprecated"), None, false)
            .unwrap();
        assert!(store.load_all().iter().all(|(_, e)| e.id != entry.id));
        assert_eq!(store.load_archived().len(), 1);

        assert!(store.restore_archived(&entry.id, "deprecated").is_err());
        assert!(store.restore_archived("M-999", "active").is_err());
        let restored = store.restore_archived(&entry.id, "active").unwrap();
        assert_eq!(restored.status, "active");
        assert!(store.load_archived().is_empty(), "归档里不再有它");
        let live = store.load_all();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].1.status, "active");
        assert!(live[0].0.is_file());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn restore_archived_refuses_to_duplicate_a_live_subject() {
        let (dir, store) = temp_store();
        let old = add_active(
            &store,
            "旧版主题条目",
            "同主题旧条目钩子",
            Some("restore-subject"),
        );
        store
            .update(&old.id, None, None, None, Some("deprecated"), None, false)
            .unwrap();
        let newer = add_active(
            &store,
            "新版主题条目",
            "同主题新条目钩子",
            Some("restore-subject"),
        );
        let error = store.restore_archived(&old.id, "active").unwrap_err();
        assert!(error.to_string().contains(&newer.id), "{error}");
        assert_eq!(store.load_archived().len(), 1, "拒绝时归档原样保留");
        std::fs::remove_dir_all(dir).ok();
    }
}
