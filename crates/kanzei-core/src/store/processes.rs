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
    pub reasoning: Option<String>,
    /// 项目级手填模型候选(provider:model 列表)。R-178 批3 起由默认进程行承载,
    /// 前端下拉的「手填」候选从后端读,不再以 localStorage 为真源。
    pub manual_models: Vec<String>,
    pub phase_pipeline: bool,
    pub tracker_writes_enabled: bool,
    pub updated_at: i64,
}

impl SessionStore {
    /// 插入或覆盖一条线/进程注册。`phase_pipeline` 以 bool 投影成 INTEGER。
    pub fn upsert_process(&self, process: &StoredProcess) -> Result<(), StoreError> {
        let manual_models = serde_json::to_string(&process.manual_models)?;
        self.connection.execute(
            "INSERT INTO processes
                 (process_id, origin_project, project_dir, worktree_path,
                  model, profile, reasoning, manual_models, phase_pipeline, tracker_writes_enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(process_id) DO UPDATE SET
                 origin_project = excluded.origin_project,
                 project_dir = excluded.project_dir,
                 worktree_path = excluded.worktree_path,
                 model = excluded.model,
                 profile = excluded.profile,
                 reasoning = excluded.reasoning,
                 manual_models = excluded.manual_models,
                 phase_pipeline = excluded.phase_pipeline,
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
                process.tracker_writes_enabled,
                process.updated_at,
            ],
        )?;
        Ok(())
    }

    /// **只插入新行**:`process_id` 已存在时返回 `false`,既有行一个字段都不动。
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
        let affected = self.connection.execute(
            "INSERT INTO processes
                 (process_id, origin_project, project_dir, worktree_path,
                  model, profile, reasoning, manual_models, phase_pipeline, tracker_writes_enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                process.tracker_writes_enabled,
                process.updated_at,
            ],
        )?;
        Ok(affected > 0)
    }

    /// 列出一个主项目的全部非默认线/进程,按 process_id 排序(稳定顺序)。
    pub fn list_processes(&self, origin_project: &str) -> Result<Vec<StoredProcess>, StoreError> {
        let mut stmt = self.connection.prepare(
            "SELECT process_id, origin_project, project_dir, worktree_path,
                    model, profile, reasoning, manual_models, phase_pipeline, tracker_writes_enabled, updated_at
             FROM processes WHERE origin_project = ?1 ORDER BY process_id",
        )?;
        let rows = stmt.query_map(params![origin_project], |row| {
            Ok(StoredProcess {
                process_id: row.get(0)?,
                origin_project: row.get(1)?,
                project_dir: row.get(2)?,
                worktree_path: row.get(3)?,
                model: row.get(4)?,
                profile: row.get(5)?,
                reasoning: row.get(6)?,
                manual_models: serde_json::from_str(row.get::<_, String>(7)?.as_str())
                    .unwrap_or_default(),
                phase_pipeline: row.get::<_, i64>(8)? != 0,
                tracker_writes_enabled: row.get::<_, i64>(9)? != 0,
                updated_at: row.get(10)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 删除一条线/进程注册(进程关闭时)。
    pub fn delete_process(&self, process_id: &str) -> Result<(), StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO retired_processes(process_id, origin_project, retired_at)
                 SELECT process_id, origin_project, ?2 FROM processes WHERE process_id = ?1
                 ON CONFLICT(process_id) DO NOTHING",
            params![process_id, super::now_ms()],
        )?;
        tx.execute(
            "DELETE FROM processes WHERE process_id = ?1",
            params![process_id],
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
        let mut statement = self.connection.prepare(
            "SELECT process_id FROM retired_processes WHERE origin_project = ?1 ORDER BY process_id",
        )?;
        let rows = statement.query_map(params![origin_project], |row| row.get(0))?;
        rows.collect::<Result<Vec<String>, _>>().map_err(Into::into)
    }

    /// 查单条(不存在的线返回 None)。
    pub fn get_process(&self, process_id: &str) -> Result<Option<StoredProcess>, StoreError> {
        self.connection
            .query_row(
                "SELECT process_id, origin_project, project_dir, worktree_path,
                        model, profile, reasoning, manual_models, phase_pipeline, tracker_writes_enabled, updated_at
                 FROM processes WHERE process_id = ?1",
                params![process_id],
                |row| {
                    Ok(StoredProcess {
                        process_id: row.get(0)?,
                        origin_project: row.get(1)?,
                        project_dir: row.get(2)?,
                        worktree_path: row.get(3)?,
                        model: row.get(4)?,
                        profile: row.get(5)?,
                        reasoning: row.get(6)?,
                        manual_models: serde_json::from_str(row.get::<_, String>(7)?.as_str())
                            .unwrap_or_default(),
                        phase_pipeline: row.get::<_, i64>(8)? != 0,
                        tracker_writes_enabled: row.get::<_, i64>(9)? != 0,
                        updated_at: row.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
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
            reasoning: Some("high".into()),
            manual_models: vec!["deepseek:deepseek-chat".into()],
            phase_pipeline: true,
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
