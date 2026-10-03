//! processes 域(R-178 D3):线/进程注册与线级状态持久化。
//! v11 表,存线/进程注册 + 模型 / profile / reasoning / 勘察复核开关 /
//! tracker 写入开关。默认进程(d|)同样落库,以恢复线级设置。

use rusqlite::{params, OptionalExtension};

use super::{SessionStore, StoreError};

/// 与 `processes` 表一行的映射。桌面端 `ProcessHandle` 的持久化投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProcess {
    pub process_id: String,
    pub origin_project: String,
    pub project_dir: String,
    pub worktree_path: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub research_topic: Option<String>,
    pub reasoning: Option<String>,
    /// 项目级手填模型候选(provider:model 列表)。R-178 批3 起由默认进程行承载,
    /// 前端下拉的「手填」候选从后端读,不再以 localStorage 为真源。
    pub manual_models: Vec<String>,
    pub phase_pipeline: bool,
    pub subagents_enabled: bool,
    pub tracker_writes_enabled: bool,
    pub updated_at: i64,
}

fn process_id_forms(id: &str) -> [String; 3] {
    match id.split_once('|') {
        Some((prefix, path)) => {
            super::path_migration::path_forms(path).map(|path| format!("{prefix}|{path}"))
        }
        None => std::array::from_fn(|_| id.to_string()),
    }
}

fn normalize_process(mut record: StoredProcess) -> StoredProcess {
    record.process_id =
        super::path_migration::simplify_process_id(&record.process_id).unwrap_or(record.process_id);
    record.origin_project = super::path_migration::simplify_text(&record.origin_project);
    record.project_dir = super::path_migration::simplify_text(&record.project_dir);
    record.worktree_path = record
        .worktree_path
        .map(|path| super::path_migration::simplify_text(&path));
    record
}

impl SessionStore {
    /// 插入或覆盖一条线/进程注册。已退役的身份返回错误，不能由旧快照复活。
    /// `phase_pipeline` 以 bool 投影成 INTEGER。
    pub fn upsert_process(&self, process: &StoredProcess) -> Result<(), StoreError> {
        let manual_models = serde_json::to_string(&process.manual_models)?;
        let forms = process_id_forms(&process.process_id);
        let affected = self.connection.execute(
            "INSERT INTO processes
                 (process_id, origin_project, project_dir, worktree_path,
                  model, profile, reasoning, manual_models, phase_pipeline, subagents_enabled, tracker_writes_enabled, updated_at, research_topic)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
             WHERE NOT EXISTS (SELECT 1 FROM retired_processes WHERE process_id IN (?1, ?14, ?15))
             ON CONFLICT(process_id) DO UPDATE SET
                 origin_project = excluded.origin_project,
                 project_dir = excluded.project_dir,
                 worktree_path = excluded.worktree_path,
                 model = excluded.model,
                 profile = excluded.profile,
                 research_topic = excluded.research_topic,
                 reasoning = excluded.reasoning,
                 manual_models = excluded.manual_models,
                 phase_pipeline = excluded.phase_pipeline,
                 subagents_enabled = excluded.subagents_enabled,
                 tracker_writes_enabled = excluded.tracker_writes_enabled,
                 updated_at = excluded.updated_at",
            params![
                process.process_id,
                process.origin_project,
                process.project_dir,
                process.worktree_path,
                process.model,
                process.profile,
                process.reasoning,
                manual_models,
                process.phase_pipeline,
                process.subagents_enabled,
                process.tracker_writes_enabled,
                process.updated_at,
                process.research_topic,
                forms[1],
                forms[2],
            ],
        )?;
        if affected == 0 {
            return Err(StoreError::InvalidInput(format!(
                "线路 {} 已关闭，不能保存或复用退役身份",
                process.process_id,
            )));
        }
        Ok(())
    }

    /// **只插入新行**:`process_id` 已存在或已退役时返回 `false`,既有行一个字段都不动。
    ///
    /// 建线专用,与 [`Self::upsert_process`] 的分工是硬的:`upsert_process` 的
    /// `ON CONFLICT DO UPDATE` 会**连 `worktree_path` 一起覆盖**,那对「改一条已知的线」
    /// 是对的,对「新建一条线」是灾难 —— 桌面端的 `p{n}` 编号一旦跟库里已有行撞上
    /// (重启后内存表是空的,而库里还留着上次的 p1),新线就会把旧线那一行整个改写,
    /// 旧线绑的那棵工作树从此在库里失联:磁盘上有树、库里指向别处,界面上再也找不到它。
    ///
    /// 编号分配已经改成「内存表 ∪ 库」取最大值,正常不会撞;这个方法是第二道闸 ——
    /// 万一还是撞了,宁可让建线失败(调用方会回滚掉刚建的工作树),也不许静默改写既有行。
    pub fn insert_new_process(&self, process: &StoredProcess) -> Result<bool, StoreError> {
        let manual_models = serde_json::to_string(&process.manual_models)?;
        let forms = process_id_forms(&process.process_id);
        let affected = self.connection.execute(
            "INSERT INTO processes
                 (process_id, origin_project, project_dir, worktree_path,
                  model, profile, reasoning, manual_models, phase_pipeline, subagents_enabled, tracker_writes_enabled, updated_at, research_topic)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
             WHERE NOT EXISTS (SELECT 1 FROM retired_processes WHERE process_id IN (?1, ?14, ?15))
             ON CONFLICT(process_id) DO NOTHING",
            params![
                process.process_id,
                process.origin_project,
                process.project_dir,
                process.worktree_path,
                process.model,
                process.profile,
                process.reasoning,
                manual_models,
                process.phase_pipeline,
                process.subagents_enabled,
                process.tracker_writes_enabled,
                process.updated_at,
                process.research_topic,
                forms[1],
                forms[2],
            ],
        )?;
        Ok(affected > 0)
    }

    /// 列出一个主项目的全部非默认线/进程,按 process_id 排序(稳定顺序)。
    /// 退役账本优先于旧竞态残留的活动登记。
    ///
    /// UI2-0926 #13 读时兜底:v25 迁移把存量 `\\?\` 形态改成了 simplify 形态,这里仍按
    /// 「带前缀 / 不带前缀」两种写法一起匹配,并把读出的 id 与路径归一成 simplify 形态——
    /// 调用方传哪种写法都列得出同一批线,内存进程表只会出现一种 id。
    pub fn list_processes(&self, origin_project: &str) -> Result<Vec<StoredProcess>, StoreError> {
        let forms = super::path_migration::path_forms(origin_project);
        let mut stmt = self.connection.prepare(
            "SELECT process_id, origin_project, project_dir, worktree_path,
                    model, profile, reasoning, manual_models, phase_pipeline, subagents_enabled, tracker_writes_enabled, updated_at, research_topic
             FROM processes WHERE origin_project IN (?1, ?2, ?3)
               AND NOT EXISTS (SELECT 1 FROM retired_processes
                               WHERE retired_processes.process_id = processes.process_id)
             ORDER BY process_id",
        )?;
        let rows = stmt.query_map(params![forms[0], forms[1], forms[2]], |row| {
            Ok(StoredProcess {
                process_id: row.get(0)?,
                origin_project: row.get(1)?,
                project_dir: row.get(2)?,
                worktree_path: row.get(3)?,
                model: row.get(4)?,
                profile: row.get(5)?,
                research_topic: row.get(12)?,
                reasoning: row.get(6)?,
                manual_models: serde_json::from_str(row.get::<_, String>(7)?.as_str())
                    .unwrap_or_default(),
                phase_pipeline: row.get::<_, i64>(8)? != 0,
                subagents_enabled: row.get::<_, i64>(9)? != 0,
                tracker_writes_enabled: row.get::<_, i64>(10)? != 0,
                updated_at: row.get(11)?,
            })
        })?;
        let mut out: Vec<StoredProcess> = Vec::new();
        for record in rows {
            let record = normalize_process(record?);
            // 两种写法同时在库(迁移之后又被旧形态写入)时,同一 id 只留较新的一条。
            match out
                .iter_mut()
                .find(|kept| kept.process_id == record.process_id)
            {
                Some(kept) if kept.updated_at >= record.updated_at => {}
                Some(kept) => *kept = record,
                None => out.push(record),
            }
        }
        let retired = self.list_retired_process_ids(origin_project)?;
        out.retain(|record| retired.binary_search(&record.process_id).is_err());
        out.sort_by(|a, b| a.process_id.cmp(&b.process_id));
        Ok(out)
    }

    /// 删除一条线/进程注册(进程关闭时)。
    pub fn delete_process(&self, process_id: &str) -> Result<(), StoreError> {
        let forms = process_id_forms(process_id);
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO retired_processes(process_id, origin_project, retired_at)
                 SELECT ?2, origin_project, ?4 FROM processes
                 WHERE process_id IN (?1, ?2, ?3)
                   AND NOT EXISTS (SELECT 1 FROM retired_processes WHERE process_id IN (?1, ?2, ?3))
                 ORDER BY updated_at DESC, process_id LIMIT 1
                 ON CONFLICT(process_id) DO NOTHING",
            params![forms[0], forms[1], forms[2], super::now_ms()],
        )?;
        tx.execute(
            "DELETE FROM processes WHERE process_id IN (?1, ?2, ?3)",
            params![forms[0], forms[1], forms[2]],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 已注销线路仍占用其身份。历史会话、前端持久设置和审计事件都以 p{n}/session_id
    /// 为键；若删除登记后复用编号，新线路会继承旧线路的全部事实。
    pub fn list_retired_process_ids(
        &self,
        origin_project: &str,
    ) -> Result<Vec<String>, StoreError> {
        let forms = super::path_migration::path_forms(origin_project);
        let mut statement = self.connection.prepare(
            "SELECT process_id FROM retired_processes WHERE origin_project IN (?1, ?2, ?3) ORDER BY process_id",
        )?;
        let rows = statement.query_map(params![forms[0], forms[1], forms[2]], |row| {
            row.get::<_, String>(0)
        })?;
        let mut ids = rows
            .map(|id| id.map(|id| super::path_migration::simplify_process_id(&id).unwrap_or(id)))
            .collect::<Result<Vec<String>, _>>()?;
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// 已注销线路的身份与注销时间(毫秒),最近注销的在前(UX-035「已关闭」分组用)。
    pub fn list_retired_processes(
        &self,
        origin_project: &str,
    ) -> Result<Vec<(String, i64)>, StoreError> {
        let forms = super::path_migration::path_forms(origin_project);
        let mut statement = self.connection.prepare(
            "SELECT process_id, retired_at FROM retired_processes
             WHERE origin_project IN (?1, ?2, ?3) ORDER BY retired_at DESC, process_id",
        )?;
        let rows = statement.query_map(params![forms[0], forms[1], forms[2]], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut out: Vec<(String, i64)> = Vec::new();
        for row in rows {
            let (id, retired_at) = row?;
            let id = super::path_migration::simplify_process_id(&id).unwrap_or(id);
            // 迁移前后两种写法同时在库时,同一 id 只留一条。
            if !out.iter().any(|(kept, _)| *kept == id) {
                out.push((id, retired_at));
            }
        }
        Ok(out)
    }

    /// 查单条(不存在或已退役的线返回 None，旧竞态残留的活动行不能复活身份)。
    pub fn get_process(&self, process_id: &str) -> Result<Option<StoredProcess>, StoreError> {
        let forms = process_id_forms(process_id);
        let record = self
            .connection
            .query_row(
                "SELECT process_id, origin_project, project_dir, worktree_path,
                        model, profile, reasoning, manual_models, phase_pipeline, subagents_enabled, tracker_writes_enabled, updated_at, research_topic
                 FROM processes WHERE process_id IN (?1, ?2, ?3)
                   AND NOT EXISTS (SELECT 1 FROM retired_processes
                                   WHERE process_id IN (?1, ?2, ?3))
                 ORDER BY updated_at DESC, process_id LIMIT 1",
                params![forms[0], forms[1], forms[2]],
                |row| {
                    Ok(StoredProcess {
                        process_id: row.get(0)?,
                        origin_project: row.get(1)?,
                        project_dir: row.get(2)?,
                        worktree_path: row.get(3)?,
                        model: row.get(4)?,
                        profile: row.get(5)?,
                        research_topic: row.get(12)?,
                        reasoning: row.get(6)?,
                        manual_models: serde_json::from_str(row.get::<_, String>(7)?.as_str())
                            .unwrap_or_default(),
                        phase_pipeline: row.get::<_, i64>(8)? != 0,
                        subagents_enabled: row.get::<_, i64>(9)? != 0,
                        tracker_writes_enabled: row.get::<_, i64>(10)? != 0,
                        updated_at: row.get(11)?,
                    })
                },
            )
            .optional()?;
        Ok(record.map(normalize_process))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil;

    fn sample() -> StoredProcess {
        StoredProcess {
            process_id: "p1|C:/project".into(),
            origin_project: "C:/project".into(),
            project_dir: "C:/project".into(),
            worktree_path: None,
            model: Some("deepseek:deepseek-v4-flash".into()),
            profile: Some("dev".into()),
            research_topic: None,
            reasoning: Some("high".into()),
            manual_models: vec!["deepseek:deepseek-chat".into()],
            phase_pipeline: true,
            subagents_enabled: true,
            tracker_writes_enabled: true,
            updated_at: 42,
        }
    }

    #[test]
    fn 删除进程后身份进入退役账本且不会丢失() {
        let store = SessionStore::open_in_memory().unwrap();
        let process = sample();
        store.insert_new_process(&process).unwrap();
        store.delete_process(&process.process_id).unwrap();
        assert!(store.list_processes("C:/project").unwrap().is_empty());
        assert_eq!(
            store.list_retired_process_ids("C:/project").unwrap(),
            vec![process.process_id]
        );
    }

    #[test]
    fn retired_process_rejects_late_snapshot_and_identity_reuse() {
        let path = std::env::temp_dir().join(format!(
            "kz-process-retirement-{}-{}.db",
            std::process::id(),
            crate::store::now_ms()
        ));
        {
            let updating = SessionStore::open(&path).unwrap();
            let retiring = SessionStore::open(&path).unwrap();
            let process = sample();
            assert!(updating.insert_new_process(&process).unwrap());
            // 设置更新已拿到快照，另一连接随后注销线路；旧更新最后才落库。
            let mut stale = updating.get_process(&process.process_id).unwrap().unwrap();
            stale.model = Some("deepseek:deepseek-chat".into());
            stale.updated_at += 1;
            retiring.delete_process(&process.process_id).unwrap();
            assert!(matches!(
                updating.upsert_process(&stale),
                Err(StoreError::InvalidInput(_))
            ));
            assert!(!updating.insert_new_process(&stale).unwrap());
            assert!(retiring.get_process(&process.process_id).unwrap().is_none());
            assert_eq!(
                updating.list_retired_process_ids("C:/project").unwrap(),
                vec![process.process_id]
            );
            let mut fresh = sample();
            fresh.process_id = "p2|C:/project".into();
            assert!(updating.insert_new_process(&fresh).unwrap());
            assert_eq!(
                retiring.get_process(&fresh.process_id).unwrap(),
                Some(fresh)
            );
        }
        {
            let reopened = SessionStore::open(&path).unwrap();
            assert!(reopened.get_process("p1|C:/project").unwrap().is_none());
            assert_eq!(
                reopened.list_retired_process_ids("C:/project").unwrap(),
                vec!["p1|C:/project"]
            );
        }
        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn retired_ledger_hides_live_rows_left_by_old_race_after_reopen() {
        let path = std::env::temp_dir().join(format!(
            "kz-process-retired-overlap-{}-{}.db",
            std::process::id(),
            crate::store::now_ms()
        ));
        let process = sample();
        let mut active = sample();
        active.process_id = "p2|C:/project".into();
        {
            let store = SessionStore::open(&path).unwrap();
            store.insert_new_process(&process).unwrap();
            store.insert_new_process(&active).unwrap();
            // 旧版晚到 upsert 可以造成两表重叠；直接构造该持久状态，不绕新门禁。
            store
                .connection
                .execute(
                    "INSERT INTO retired_processes(process_id, origin_project, retired_at) VALUES (?1, ?2, ?3)",
                    params![process.process_id, process.origin_project, 123],
                )
                .unwrap();
        }
        {
            let store = SessionStore::open(&path).unwrap();
            assert!(store.get_process(&process.process_id).unwrap().is_none());
            assert_eq!(store.list_processes("C:/project").unwrap(), vec![active]);
            assert_eq!(
                store.list_retired_processes("C:/project").unwrap(),
                vec![(process.process_id.clone(), 123)]
            );
            assert!(matches!(
                store.upsert_process(&process),
                Err(StoreError::InvalidInput(_))
            ));
            assert!(!store.insert_new_process(&process).unwrap());
            // 过滤不销毁历史或旧活动行，只让退役账本拥有对外身份判定。
            let physical_rows: i64 = store
                .connection
                .query_row("SELECT COUNT(*) FROM processes", [], |row| row.get(0))
                .unwrap();
            assert_eq!(physical_rows, 2);
        }
        {
            let read_only = SessionStore::open_read_only(&path).unwrap();
            assert!(read_only
                .get_process(&process.process_id)
                .unwrap()
                .is_none());
            assert_eq!(read_only.list_processes("C:/project").unwrap().len(), 1);
        }
        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn retirement_guards_share_existing_verbatim_identity_rules() {
        let canonical_id = r"p1|C:\project";
        let verbatim_id = r"p1|\\?\C:\project";
        for (live_id, retired_id, origin, retired_origin) in [
            (verbatim_id, canonical_id, r"\\?\C:\project", r"C:\project"),
            (canonical_id, verbatim_id, r"C:\project", r"\\?\C:\project"),
        ] {
            let store = SessionStore::open_in_memory().unwrap();
            let mut process = sample();
            process.process_id = live_id.into();
            process.origin_project = origin.into();
            process.project_dir = origin.into();
            store.upsert_process(&process).unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO retired_processes(process_id, origin_project, retired_at) VALUES (?1, ?2, 123)",
                    params![retired_id, retired_origin],
                )
                .unwrap();
            assert!(store.list_processes(r"C:\project").unwrap().is_empty());
            for id in [canonical_id, verbatim_id] {
                assert!(store.get_process(id).unwrap().is_none());
                process.process_id = id.into();
                assert!(matches!(
                    store.upsert_process(&process),
                    Err(StoreError::InvalidInput(_))
                ));
                assert!(!store.insert_new_process(&process).unwrap());
            }
            store.delete_process(canonical_id).unwrap();
            store.delete_process(verbatim_id).unwrap();
            let live_count: i64 = store
                .connection
                .query_row("SELECT COUNT(*) FROM processes", [], |row| row.get(0))
                .unwrap();
            assert_eq!(live_count, 0);
            assert_eq!(
                store.list_retired_processes(r"C:\project").unwrap(),
                vec![(canonical_id.to_string(), 123)],
                "注销不得用新别名覆盖既有退役时间"
            );
        }
    }

    #[test]
    fn deleting_returned_id_retires_all_verbatim_aliases() {
        for has_older_canonical in [false, true] {
            let store = SessionStore::open_in_memory().unwrap();
            let mut legacy = sample();
            legacy.process_id = r"p1|\\?\C:\project".into();
            legacy.origin_project = r"\\?\C:\project".into();
            legacy.project_dir = legacy.origin_project.clone();
            legacy.updated_at = 84;
            store.upsert_process(&legacy).unwrap();
            let listed = store.list_processes(r"C:\project").unwrap();
            let normalized = listed[0].clone();
            assert_eq!(normalized.process_id, r"p1|C:\project");
            assert_eq!(
                store.get_process(&normalized.process_id).unwrap(),
                Some(normalized.clone())
            );
            // 同实体也有较旧的 simplify 行时，读取选最新，注销必须删除两个别名。
            if has_older_canonical {
                let mut older = normalized.clone();
                older.updated_at = 42;
                store.upsert_process(&older).unwrap();
            }
            assert_eq!(
                store.get_process(&normalized.process_id).unwrap(),
                Some(normalized.clone())
            );
            store.delete_process(&normalized.process_id).unwrap();
            assert!(store.list_processes(r"C:\project").unwrap().is_empty());
            assert_eq!(
                store.list_retired_process_ids(r"C:\project").unwrap(),
                vec![normalized.process_id]
            );
            let live_count: i64 = store
                .connection
                .query_row("SELECT COUNT(*) FROM processes", [], |row| row.get(0))
                .unwrap();
            assert_eq!(live_count, 0);
            assert!(matches!(
                store.upsert_process(&legacy),
                Err(StoreError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn 退役线路带注销时间_最近注销的在前_按项目隔离() {
        let store = SessionStore::open_in_memory().unwrap();
        let first = sample();
        let mut second = sample();
        second.process_id = "p2|C:/project".into();
        let mut other = sample();
        other.process_id = "p1|D:/other".into();
        other.origin_project = "D:/other".into();
        for process in [&first, &second, &other] {
            store.insert_new_process(process).unwrap();
        }
        store.delete_process(&first.process_id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        store.delete_process(&second.process_id).unwrap();
        store.delete_process(&other.process_id).unwrap();
        let retired = store.list_retired_processes("C:/project").unwrap();
        let ids: Vec<_> = retired.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["p2|C:/project", "p1|C:/project"]);
        assert!(retired[0].1 >= retired[1].1 && retired[1].1 > 0);
        assert_eq!(store.list_retired_processes("D:/other").unwrap().len(), 1);
        assert!(store.list_retired_processes("E:/none").unwrap().is_empty());
    }

    #[test]
    fn process_upsert_list_roundtrip() {
        let store = testutil::store();
        store.upsert_process(&sample()).unwrap();
        let loaded = store.list_processes("C:/project").unwrap();
        assert_eq!(loaded, vec![sample()]);
        // 手填模型列表随行往返,不被 JSON 编解码吞掉(R-178 批3)。
        assert_eq!(
            loaded[0].manual_models,
            vec!["deepseek:deepseek-chat".to_string()]
        );
        // 另一个主项目互不串扰(D-170 式隔离)。
        assert!(store.list_processes("D:/other").unwrap().is_empty());
    }

    #[test]
    fn research_topic_binding_survives_database_upgrade_and_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "kz-topic-migrate-{}-{}",
            std::process::id(),
            crate::store::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.db");
        let legacy = sample();
        {
            let store = SessionStore::open(&path).unwrap();
            store.upsert_process(&legacy).unwrap();
            store.connection.execute_batch("ALTER TABLE processes DROP COLUMN research_topic; UPDATE schema_meta SET value='21' WHERE key='schema_version';").unwrap();
        }
        let mut bound = sample();
        bound.process_id = "p2|C:/project".into();
        bound.profile = Some("research".into());
        bound.research_topic = Some("topic-a".into());
        {
            let store = SessionStore::open(&path).unwrap();
            assert_eq!(
                store.get_process(&legacy.process_id).unwrap(),
                Some(legacy.clone())
            );
            assert!(store.insert_new_process(&bound).unwrap());
        }
        {
            let store = SessionStore::open(&path).unwrap();
            assert_eq!(store.get_process(&bound.process_id).unwrap(), Some(bound));
            assert_eq!(store.get_process(&legacy.process_id).unwrap(), Some(legacy));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn process_upsert_overwrites_by_id() {
        let store = testutil::store();
        store.upsert_process(&sample()).unwrap();
        let mut updated = sample();
        updated.model = Some("anthropic:claude-sonnet-5".into());
        updated.phase_pipeline = false;
        updated.tracker_writes_enabled = false;
        store.upsert_process(&updated).unwrap();
        assert_eq!(store.list_processes("C:/project").unwrap(), vec![updated]);
    }

    /// 建线专用写法:撞上既有行时**一个字段都不许动**,尤其是 `worktree_path`。
    ///
    /// 这是 `upsert_process` 干不了的事——它的 `ON CONFLICT DO UPDATE` 会把既有行
    /// 整条改写,旧线绑的工作树就此从库里失联。
    #[test]
    fn process_insert_new_never_touches_existing_row() {
        let store = testutil::store();
        let mut existing = sample();
        existing.worktree_path = Some("C:/project/.kanzei-worktree-old".into());
        store.upsert_process(&existing).unwrap();

        let mut intruder = sample();
        intruder.worktree_path = Some("C:/project/.kanzei-worktree-new".into());
        intruder.model = Some("anthropic:claude-sonnet-5".into());
        intruder.updated_at = 999;
        assert!(
            !store.insert_new_process(&intruder).unwrap(),
            "撞上既有 process_id 必须返回 false,不许写进去"
        );
        assert_eq!(
            store.get_process("p1|C:/project").unwrap().unwrap(),
            existing,
            "既有行必须逐字段原封不动(worktree_path 尤其不能被改写)"
        );

        // 不撞的 id 照常插入成功。
        let mut fresh = sample();
        fresh.process_id = "p2|C:/project".into();
        assert!(store.insert_new_process(&fresh).unwrap());
        assert_eq!(store.get_process("p2|C:/project").unwrap(), Some(fresh));
    }

    #[test]
    fn process_delete_removes_row() {
        let store = testutil::store();
        store.upsert_process(&sample()).unwrap();
        store.delete_process("p1|C:/project").unwrap();
        assert!(store.list_processes("C:/project").unwrap().is_empty());
        assert!(store.get_process("p1|C:/project").unwrap().is_none());
    }

    #[test]
    fn process_get_returns_none_for_missing() {
        let store = testutil::store();
        assert!(store.get_process("p2|C:/project").unwrap().is_none());
        store.upsert_process(&sample()).unwrap();
        let loaded = store.get_process("p1|C:/project").unwrap().unwrap();
        assert_eq!(loaded.model.as_deref(), Some("deepseek:deepseek-v4-flash"));
        assert!(loaded.phase_pipeline);
        assert!(loaded.tracker_writes_enabled);
    }

    #[test]
    fn process_phase_pipeline_projection_preserves_bool() {
        let store = testutil::store();
        let mut off = sample();
        off.phase_pipeline = false;
        store.upsert_process(&off).unwrap();
        assert!(
            !store
                .get_process("p1|C:/project")
                .unwrap()
                .unwrap()
                .phase_pipeline
        );
        let mut on = sample();
        on.phase_pipeline = true;
        store.upsert_process(&on).unwrap();
        assert!(
            store
                .get_process("p1|C:/project")
                .unwrap()
                .unwrap()
                .phase_pipeline
        );
    }

    #[test]
    fn process_tracker_write_projection_preserves_bool() {
        let store = testutil::store();
        let mut off = sample();
        off.tracker_writes_enabled = false;
        store.upsert_process(&off).unwrap();
        assert!(
            !store
                .get_process("p1|C:/project")
                .unwrap()
                .unwrap()
                .tracker_writes_enabled
        );
        let mut on = sample();
        on.tracker_writes_enabled = true;
        store.upsert_process(&on).unwrap();
        assert!(
            store
                .get_process("p1|C:/project")
                .unwrap()
                .unwrap()
                .tracker_writes_enabled
        );
    }
}
