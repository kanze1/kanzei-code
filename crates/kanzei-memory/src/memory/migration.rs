//! 旧版记忆迁移(R-255 第一刀,纯搬迁自 store.rs)。
//!
//! 独立理由:legacy 迁移是「R-098 的 .kanzei/project/memory.md(tracker M-条目)→
//! 一条一文件」的一次性历史搬迁,与运行期准入/检索/收件箱正交:迁完即不再触发,
//! 独立成域后,store 的日常读写路径不必背负历史格式知识(照 files_view.rs 模式)。
//!
//! 源文件只在条目和派生物全部成功后用 CAS 改写为指路牌。条目携带来源指纹,
//! 中断后按 ID/指纹继续,不覆盖其间已编辑或归档的条目;旧 stale 降为 deprecated。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;

use super::store::MemoryStore;
use super::{today, MemoryEntry};

const LEGACY_ENTRY_HASH: &str = "legacy_memory_hash";
const MIGRATED_NOTICE: &str =
    "# Memory\n\n(已迁移至 .kanzei/memory/,由 memory_search 检索;本文件不再使用。)\n";

impl MemoryStore {
    /// legacy 迁移:R-098 的 .kanzei/project/memory.md(tracker M-条目)→ 一条一文件。
    /// 幂等:legacy 文件不存在即跳过;迁移后原文件改写为指路牌。
    pub(crate) fn migrate_legacy(&self, project_root: &Path) -> anyhow::Result<()> {
        let legacy = project_root
            .join(".kanzei")
            .join("project")
            .join("memory.md");
        let text = match std::fs::read_to_string(&legacy) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("读取旧记忆源文件失败"),
        };
        if crate::docstore::parse(&crate::docstore::MEMORY, &text).is_empty() {
            return Ok(());
        }
        // 与围栏使用相同 source→tree 顺序,与 add/update/archive 共用树锁。
        // 锁内重新读取,外部不遵守锁的编辑还由最后的内容 CAS 检测。
        let _source_lock = crate::atomic_file::lock_exclusive(&legacy)?;
        let _tree_lock = self.tree_lock()?;
        let text = std::fs::read_to_string(&legacy).context("重新读取旧记忆源文件失败")?;
        let parsed = crate::docstore::parse_document(&crate::docstore::MEMORY, &text);
        let entries = parsed.entries;
        if entries.is_empty() {
            return Ok(());
        }
        let mut source_ids = BTreeSet::new();
        for entry in &entries {
            anyhow::ensure!(!entry.id.is_empty(), "旧记忆条目缺少 ID，源文件保留");
            anyhow::ensure!(
                source_ids.insert(&entry.id),
                "旧记忆编号重复: {}，源文件保留",
                entry.id
            );
        }
        let mut existing = BTreeMap::new();
        for (path, entry) in self.migration_entries()? {
            anyhow::ensure!(
                !existing.contains_key(&entry.id),
                "新记忆编号重复: {}，源文件保留",
                entry.id
            );
            existing.insert(entry.id.clone(), (path, entry));
        }
        let voided = match std::fs::read_to_string(self.voided_ledger_file()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error).context("读取已注销记忆编号失败"),
        };
        let now = today();
        for legacy_entry in &entries {
            let origin = crate::docstore::render_with_template(
                &crate::docstore::MEMORY,
                std::slice::from_ref(legacy_entry),
                &parsed.template,
            );
            let origin_hash = kanzei_base::content_hash(origin.as_bytes());
            if let Some((path, entry)) = existing.get(&legacy_entry.id) {
                anyhow::ensure!(
                    entry.field(LEGACY_ENTRY_HASH) == Some(origin_hash.as_str()),
                    "记忆 {} 已存在但迁移来源不匹配: {}；拒绝覆盖，旧源文件保留",
                    legacy_entry.id,
                    path.display()
                );
                continue;
            }
            anyhow::ensure!(
                !voided
                    .lines()
                    .filter_map(|line| line.trim().strip_prefix("- "))
                    .filter_map(|line| line.split_once(':'))
                    .any(|(id, _)| id.trim() == legacy_entry.id),
                "记忆 {} 已注销；拒绝重新创建，旧源文件保留",
                legacy_entry.id
            );
            let entry = MemoryEntry {
                id: legacy_entry.id.clone(),
                scope: self.scope.label().into(),
                category: "fact".into(),
                title: legacy_entry.title.clone(),
                description: format!("{}(迁移自 memory.md)", legacy_entry.title),
                status: if legacy_entry.status == "stale" {
                    "deprecated"
                } else {
                    "active"
                }
                .into(),
                created: now.clone(),
                updated: now.clone(),
                source: "migration".into(),
                extras: vec![(LEGACY_ENTRY_HASH.into(), origin_hash)],
                // 保留普通正文、字段续行及文档说明,不能仅提取结构化 fields。
                body: origin,
            };
            let target = self.root.join(format!("{}.md", entry.file_stem()));
            match target.try_exists() {
                Ok(false) => {}
                Ok(true) => anyhow::bail!(
                    "迁移目标已存在但未识别为对应记忆: {}，拒绝覆盖",
                    target.display()
                ),
                Err(error) => return Err(error).context("检查迁移目标失败"),
            }
            self.write_entry(&entry, None)
                .with_context(|| format!("迁移记忆 {} 失败，旧源文件保留", entry.id))?;
        }
        self.refresh_derived()
            .context("刷新迁移后的记忆索引失败，旧源文件保留")?;
        crate::atomic_file::write_atomic_cas(
            &legacy,
            MIGRATED_NOTICE,
            &kanzei_base::content_hash(text.as_bytes()),
            |text| kanzei_base::content_hash(text.as_bytes()),
        )
        .map_err(anyhow::Error::msg)
        .context("提交旧记忆迁移标记失败，旧源文件保留")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{render_entry, MemoryScope};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tracing::{field::Visit, span, Event, Metadata, Subscriber};

    const SOURCE: &str = "# Memory\n\n## M-001 第一条迁移记忆 [active]\n- 依据: 保留原始事实\n\n## M-002 第二条迁移记忆 [stale]\n- 备注: 旧结论\n";

    #[derive(Clone, Default)]
    struct EventLog(Arc<Mutex<Vec<String>>>);

    struct EventFields(String);
    impl Visit for EventFields {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write;
            writeln!(&mut self.0, "{}={value:?}", field.name()).unwrap();
        }
    }
    impl Subscriber for EventLog {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
            span::Id::from_u64(1)
        }
        fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
        fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
        fn event(&self, event: &Event<'_>) {
            let mut fields = EventFields(format!("{}:\n", event.metadata().level()));
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
        fn enter(&self, _: &span::Id) {}
        fn exit(&self, _: &span::Id) {}
    }

    struct Fixture {
        project: PathBuf,
        legacy: PathBuf,
        store: MemoryStore,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT_FIXTURE: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let project = std::env::temp_dir().join(format!(
                "kz-memory-migration-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let legacy = project.join(".kanzei/project/memory.md");
            std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
            std::fs::write(&legacy, SOURCE).unwrap();
            let store = MemoryStore::open(MemoryScope::Project, project.join(".kanzei/memory"));
            Self {
                project,
                legacy,
                store,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.project).unwrap();
        }
    }

    #[test]
    fn entry_write_failure_preserves_legacy_source() {
        let fixture = Fixture::new();
        std::fs::write(&fixture.store.root, "not a directory").unwrap();
        assert!(fixture.store.migrate_legacy(&fixture.project).is_err());
        assert_eq!(std::fs::read_to_string(&fixture.legacy).unwrap(), SOURCE);
        assert_eq!(
            std::fs::read_to_string(&fixture.store.root).unwrap(),
            "not a directory"
        );
    }

    #[cfg(windows)]
    #[test]
    fn interrupted_final_replace_resumes_without_overwriting_edited_entry() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        // Permit the legacy read but deny deletion/replacement until the handle is released.
        let guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&fixture.legacy)
            .unwrap();
        let failure = fixture.store.migrate_legacy(&fixture.project).unwrap_err();
        assert!(format!("{failure:#}").contains("提交旧记忆迁移标记失败"));
        assert_eq!(std::fs::read_to_string(&fixture.legacy).unwrap(), SOURCE);
        drop(guard);
        let (path, mut entry) = fixture
            .store
            .load_all()
            .into_iter()
            .find(|(_, entry)| entry.id == "M-001")
            .unwrap();
        entry.title = "迁移后人工修改的标题".into();
        entry.body = "迁移后人工补充的重要事实".into();
        let edited = render_entry(&entry);
        std::fs::write(&path, &edited).unwrap();
        fixture.store.migrate_legacy(&fixture.project).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), edited);
        assert_eq!(fixture.store.load_all().len(), 1);
        assert_eq!(fixture.store.load_archived().len(), 1);
    }

    #[test]
    fn derived_failure_preserves_source_and_retry_keeps_edited_archived_entry() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.store.root.join("index.db")).unwrap();
        let failure = fixture.store.migrate_legacy(&fixture.project).unwrap_err();
        assert!(format!("{failure:#}").contains("刷新迁移后的记忆索引失败"));
        assert_eq!(std::fs::read_to_string(&fixture.legacy).unwrap(), SOURCE);
        let (path, mut entry) = fixture
            .store
            .load_archived()
            .into_iter()
            .find(|(_, entry)| entry.id == "M-002")
            .unwrap();
        entry.body = "人工确认已归档事实，迁移不能还原旧内容".into();
        let edited = render_entry(&entry);
        std::fs::write(&path, &edited).unwrap();
        std::fs::remove_dir(fixture.store.root.join("index.db")).unwrap();
        fixture.store.migrate_legacy(&fixture.project).unwrap();
        assert_eq!(
            std::fs::read_to_string(&fixture.legacy).unwrap(),
            MIGRATED_NOTICE
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), edited);
        assert_eq!(fixture.store.load_all().len(), 1);
        assert_eq!(fixture.store.load_archived().len(), 1);
        fixture.store.migrate_legacy(&fixture.project).unwrap();
        assert_eq!(fixture.store.load_all().len(), 1);
        assert_eq!(fixture.store.load_archived().len(), 1);
    }

    #[test]
    fn unrecognized_archive_target_preserves_legacy_and_existing_file() {
        let fixture = Fixture::new();
        let archive = fixture.store.root.join("archive");
        std::fs::create_dir_all(&archive).unwrap();
        let target = archive.join("M-002-第二条迁移记忆.md");
        let damaged = "# 用户还未修复的归档文件\n保留这段内容\n";
        std::fs::write(&target, damaged).unwrap();
        assert!(fixture.store.migrate_legacy(&fixture.project).is_err());
        assert_eq!(std::fs::read_to_string(&fixture.legacy).unwrap(), SOURCE);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), damaged);
        std::fs::remove_file(target).unwrap();
        fixture.store.migrate_legacy(&fixture.project).unwrap();
        let archived = fixture.store.load_archived();
        assert_eq!(archived.len(), 1);
        assert!(archived[0].1.body.contains("旧结论"));
    }

    #[test]
    fn migration_preserves_unstructured_text_and_detects_its_source_changes() {
        let fixture = Fixture::new();
        let source = "# Memory\n迁移说明必须保留。\n\n## M-001 原文记忆 [active]\n普通正文必须保留。\n- 依据: 首行\n  后续行也必须保留。\n\n```text\n原始代码片段\n```\n";
        std::fs::write(&fixture.legacy, source).unwrap();
        // 在条目落盘后阻断派生索引,再更改非字段正文;恢复应识别真实来源冲突。
        std::fs::create_dir_all(fixture.store.root.join("index.db")).unwrap();
        assert!(fixture.store.migrate_legacy(&fixture.project).is_err());
        let entries = fixture.store.load_all();
        assert_eq!(entries.len(), 1);
        let (path, entry) = &entries[0];
        for retained in [
            "迁移说明必须保留。",
            "普通正文必须保留。",
            "后续行也必须保留。",
            "原始代码片段",
        ] {
            assert!(
                entry.body.contains(retained),
                "未保留 {retained}: {}",
                entry.body
            );
        }
        let original_target = std::fs::read_to_string(path).unwrap();
        let changed = source.replace("普通正文必须保留。", "用户已经修改这段普通正文。");
        std::fs::write(&fixture.legacy, &changed).unwrap();
        std::fs::remove_dir(fixture.store.root.join("index.db")).unwrap();
        let error = fixture.store.migrate_legacy(&fixture.project).unwrap_err();
        assert!(format!("{error:#}").contains("迁移来源不匹配"));
        assert_eq!(std::fs::read_to_string(&fixture.legacy).unwrap(), changed);
        assert_eq!(std::fs::read_to_string(path).unwrap(), original_target);
        std::fs::write(&fixture.legacy, source).unwrap();
        fixture.store.migrate_legacy(&fixture.project).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), original_target);
    }

    #[test]
    fn conflicting_origin_preserves_both_source_and_existing_memory() {
        let fixture = Fixture::new();
        fixture.store.migrate_legacy(&fixture.project).unwrap();
        let (path, entry) = fixture.store.load_all().pop().unwrap();
        let migrated = std::fs::read_to_string(&path).unwrap();
        let changed_source = SOURCE.replace("保留原始事实", "源文件后来添加了不同事实");
        std::fs::write(&fixture.legacy, &changed_source).unwrap();
        let error = fixture.store.migrate_legacy(&fixture.project).unwrap_err();
        assert!(format!("{error:#}").contains("迁移来源不匹配"));
        assert_eq!(
            std::fs::read_to_string(&fixture.legacy).unwrap(),
            changed_source
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), migrated);
        // 旧版部分迁移没有可验证指纹也必须保留,不能假定同 ID 就可覆盖或丢弃源。
        let mut unidentified = entry;
        unidentified.extras.clear();
        let prior = render_entry(&unidentified);
        std::fs::write(&path, &prior).unwrap();
        std::fs::write(&fixture.legacy, SOURCE).unwrap();
        assert!(fixture.store.migrate_legacy(&fixture.project).is_err());
        assert_eq!(std::fs::read_to_string(&fixture.legacy).unwrap(), SOURCE);
        assert_eq!(std::fs::read_to_string(path).unwrap(), prior);
    }

    #[test]
    fn duplicate_legacy_ids_and_unreadable_sources_are_explicit_errors() {
        let fixture = Fixture::new();
        let duplicate = SOURCE.replace("M-002", "M-001");
        std::fs::write(&fixture.legacy, &duplicate).unwrap();
        let error = fixture.store.migrate_legacy(&fixture.project).unwrap_err();
        assert!(format!("{error:#}").contains("旧记忆编号重复"));
        assert_eq!(std::fs::read_to_string(&fixture.legacy).unwrap(), duplicate);
        assert!(fixture.store.load_all().is_empty());
        std::fs::write(&fixture.legacy, [0xff, 0xfe, 0xfd]).unwrap();
        assert!(fixture.store.migrate_legacy(&fixture.project).is_err());
        assert_eq!(std::fs::read(&fixture.legacy).unwrap(), [0xff, 0xfe, 0xfd]);
    }

    #[test]
    fn project_constructor_reports_migration_failure_and_preserves_source() {
        // tracing 的 callsite interest 缓存是进程级状态;日志断言独占测试进程。
        crate::memory::test_home::run(
            "memory::migration::tests::project_constructor_reports_migration_failure_and_preserves_source",
            |_| {
                let fixture = Fixture::new();
                let invalid_source = [0xff, 0xfe, 0xfd];
                std::fs::write(&fixture.legacy, invalid_source).unwrap();
                let log = EventLog::default();
                tracing::subscriber::with_default(log.clone(), || {
                    let store = MemoryStore::project(&fixture.project);
                    assert_eq!(store.root, fixture.store.root);
                });
                let logged = log.0.lock().unwrap().join("\n");
                assert!(logged.contains("ERROR:"), "{logged}");
                assert!(logged.contains("旧记忆迁移未完成"), "{logged}");
                assert!(logged.contains("读取旧记忆源文件失败"), "{logged}");
                assert!(
                    logged.contains(&fixture.legacy.display().to_string()),
                    "{logged}"
                );
                assert_eq!(std::fs::read(&fixture.legacy).unwrap(), invalid_source);
            },
        );
    }

    #[test]
    fn migration_acquires_source_before_tree_like_managed_fence() {
        crate::memory::test_home::run(
            "memory::migration::tests::migration_acquires_source_before_tree_like_managed_fence",
            |_| {
                let fixture = Fixture::new();
                let log = EventLog::default();
                tracing::subscriber::with_default(log.clone(), || {
                    fixture.store.migrate_legacy(&fixture.project).unwrap();
                });
                let logged = log.0.lock().unwrap();
                let acquired: Vec<_> = logged
                    .iter()
                    .filter(|event| event.contains("file lock acquired"))
                    .collect();
                let legacy_lock = crate::atomic_file::lock_path_for(&fixture.legacy).unwrap();
                let tree_lock = crate::atomic_file::lock_path_for(&fixture.store.root).unwrap();
                assert!(
                    acquired[0].contains(&legacy_lock.display().to_string()),
                    "{acquired:?}"
                );
                assert!(
                    acquired[1].contains(&tree_lock.display().to_string()),
                    "{acquired:?}"
                );
            },
        );
    }

    #[test]
    fn concurrent_migrations_create_each_entry_once() {
        let fixture = Fixture::new();
        let start = std::sync::Barrier::new(3);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..2)
                .map(|_| {
                    let start = &start;
                    let fixture = &fixture;
                    scope.spawn(move || {
                        start.wait();
                        fixture.store.migrate_legacy(&fixture.project)
                    })
                })
                .collect();
            start.wait();
            for worker in workers {
                worker.join().unwrap().unwrap();
            }
        });
        assert_eq!(fixture.store.load_all().len(), 1);
        assert_eq!(fixture.store.load_archived().len(), 1);
        assert_eq!(
            std::fs::read_to_string(&fixture.legacy).unwrap(),
            MIGRATED_NOTICE
        );
    }
}
