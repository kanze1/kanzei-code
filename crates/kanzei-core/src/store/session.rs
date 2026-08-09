//! session 域(R-155 S6):会话生命周期、打开/路径身份与迁移前备份。
//! StoreError/Session/SessionStore 类型定义留 mod.rs(跨域共享契约)。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::{now_ms, Session, SessionStore, StoreError};

impl SessionStore {
        pub fn open(path: &Path) -> Result<Self, StoreError> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            }
            let mut connection = Connection::open(path)?;
            // 同一个 state.db 会被大量短命连接并发读写(每个 Tauri 命令、每次运行、移动端线程各开一条)。
            // 默认 rollback journal + busy_timeout=0 会让并发写直接 SQLITE_BUSY,
            // 表现为运行成功却报失败、事件丢失、入队被拒(D-064)。
            connection.busy_timeout(std::time::Duration::from_secs(5))?;
            connection.pragma_update(None, "journal_mode", "WAL")?;
            connection.pragma_update(None, "synchronous", "NORMAL")?;
            connection.set_transaction_behavior(TransactionBehavior::Immediate);
            let store = Self {
                connection,
                path: Some(path.to_path_buf()),
            };
            store.migrate()?;
            Ok(store)
        }
    
        pub fn open_in_memory() -> Result<Self, StoreError> {
            let mut connection = Connection::open_in_memory()?;
            connection.set_transaction_behavior(TransactionBehavior::Immediate);
            let store = Self {
                connection,
                path: None,
            };
            store.migrate()?;
            Ok(store)
        }
    
        pub fn create_session(
            &self,
            session_id: &str,
            project_root: &str,
            title: Option<&str>,
        ) -> Result<Session, StoreError> {
            let now = now_ms();
            self.connection.execute(
                "INSERT INTO sessions(session_id, project_root, title, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'idle', ?4, ?4)
                 ON CONFLICT(session_id) DO NOTHING",
                params![session_id, project_root, title, now],
            )?;
            self.get_session(session_id)?
                .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows.into())
        }
    
        pub fn get_session(&self, session_id: &str) -> Result<Option<Session>, StoreError> {
            self.connection
                .query_row(
                    "SELECT session_id, project_root, title, status, created_at, updated_at
                     FROM sessions WHERE session_id = ?1",
                    params![session_id],
                    |row| {
                        Ok(Session {
                            session_id: row.get(0)?,
                            project_root: row.get(1)?,
                            title: row.get(2)?,
                            status: row.get(3)?,
                            created_at: row.get(4)?,
                            updated_at: row.get(5)?,
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
        }
    
        /// 更新会话生命周期状态，并同步更新时间。
        ///
        /// 状态值由 runner 约定为 `idle`、`running`、`failed`；存储层不限制
        /// 未来新增的状态，以便迁移时保持向后兼容。
        pub fn set_status(&self, session_id: &str, status: &str) -> Result<(), StoreError> {
            let changed = self.connection.execute(
                "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE session_id = ?3",
                params![status, now_ms(), session_id],
            )?;
            if changed == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows.into());
            }
            Ok(())
        }
    
        /// 迁移前的整库备份:state.db.v<旧版本>.bak。
        ///
        /// 用 VACUUM INTO 而不是复制文件——库跑在 WAL 模式下,单独拷 .db 会漏掉
        /// -wal 里尚未 checkpoint 的事务,拷出来的是个残缺快照。VACUUM INTO 由
        /// SQLite 自己生成一致副本。内存库无需备份。
        pub(crate) fn backup_before_upgrade(&self, from_version: i64) -> Result<(), StoreError> {
            let Some(path) = &self.path else {
                return Ok(());
            };
            let backup = path.with_extension(format!("db.v{from_version}.bak"));
            // 上一次升级尝试留下的同名备份直接覆盖:它描述的是同一个旧版本。
            // VACUUM INTO 要求目标不存在,所以必须先删。
            let _ = std::fs::remove_file(&backup);
            self.connection
                .execute("VACUUM INTO ?1", params![backup.to_string_lossy()])?;
            Ok(())
        }
    
        /// 已存在的迁移前备份路径(供调用方提示用户如何回退)。
        pub fn backup_path(&self, from_version: i64) -> Option<PathBuf> {
            let backup = self
                .path
                .as_ref()?
                .with_extension(format!("db.v{from_version}.bak"));
            backup.is_file().then_some(backup)
        }
    
        /// 从迁移前备份里把被抹掉的输入状态位捞回来(D-180 续)。
        ///
        /// v6 的回填来晚了一步:它要修的那批 `promoted`,在 v5 期间的两次停止里已经
        /// 被 `finalize_interrupt` 抹成 `cancelled`,于是回填在真实机器上扑了个空。
        /// 唯一还留着原始状态的地方,是升级时 `VACUUM INTO` 出来的那份备份。
        ///
        /// 判定刻意用备份当权威,而不是猜:备份里是 `promoted` 的,说明它在 v5 之前
        /// **被提升执行过**却没有终态可记(那时根本没有 completed);备份里已经是
        /// `cancelled` 的,是用户当年真的取消过,不动。备份里根本没有的(升级之后
        /// 才产生的输入),更不动——这台机器上 21:40 那条就属于此类,它是真被停掉的。
        pub(crate) fn recover_legacy_input_status(&self) -> Result<(), StoreError> {
            let Some(path) = &self.path else {
                return Ok(());
            };
            let Some(parent) = path.parent() else {
                return Ok(());
            };
            let stem = path
                .file_name()
                .map(|name| format!("{}.v", name.to_string_lossy()))
                .unwrap_or_default();
            let mut backups: Vec<PathBuf> = std::fs::read_dir(parent)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|candidate| {
                    let name = candidate.file_name().map(|n| n.to_string_lossy().into_owned());
                    name.is_some_and(|name| name.starts_with(&stem) && name.ends_with(".bak"))
                })
                .collect();
            backups.sort();
            let mut recovered = 0usize;
            for backup in backups {
                // 备份可能来自更旧的 schema、也可能损坏;单个失败不该阻断迁移。
                let attached = self
                    .connection
                    .execute_batch(&format!(
                        "ATTACH DATABASE '{}' AS legacy",
                        backup.to_string_lossy().replace('\'', "''")
                    ))
                    .is_ok();
                if !attached {
                    continue;
                }
                let updated = self.connection.execute(
                    "UPDATE session_inputs SET status = 'completed'
                     WHERE status = 'cancelled'
                       AND input_id IN (
                           SELECT input_id FROM legacy.session_inputs WHERE status = 'promoted'
                       )",
                    [],
                );
                recovered += updated.unwrap_or(0);
                let _ = self.connection.execute_batch("DETACH DATABASE legacy");
            }
            if recovered > 0 {
                tracing::info!(recovered, "v7 迁移:从备份恢复了被抹掉的输入状态位");
            }
            // 落一个可回查的痕迹:回填是推断值,事后要能知道动过多少条。
            self.connection.execute(
                "INSERT INTO schema_meta(key, value) VALUES ('legacy_inputs_recovered', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![recovered.to_string()],
            )?;
            Ok(())
        }
    
        /// v7 从备份恢复的输入条数(事后核对用)。
        pub fn legacy_inputs_recovered(&self) -> Option<usize> {
            self.connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key = 'legacy_inputs_recovered'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
                .and_then(|value| value.parse().ok())
        }
    
}

pub fn project_state_path(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei").join("state.db")
}

pub fn project_session_id(project_root: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_identity(project_root).hash(&mut hasher);
    format!("ses_project_{:016x}", hasher.finish())
}

/// 会话身份用的路径形态(D-176)。
///
/// 同一个目录必须只有一个会话 id。桌面端走 `normalized_project_root`,里面的
/// `std::fs::canonicalize` 在 Windows 上会返回 `\\?\C:\...` 扩展长度前缀;CLI 走
/// 裸 `discover_project_root`,拿到的是 `C:\...`。两者只做 lowercase 后哈希,于是
/// 同一个项目裂成两条会话线,历史与队列互相看不见——实测本仓库就同时存在
/// `ses_project_c0b8d63…`(裸)与 `ses_project_ce2fce9…`(canonical)两条。
///
/// 这里剥掉 `\\?\` / `\\?\UNC\` 前缀并去掉末尾分隔符。**分隔符刻意不做统一**:
/// 裸路径形态的哈希必须与历史保持一致,否则所有既有会话会一次性全部改名、
/// 历史集体失联。代价是 `C:/x` 这种正斜杠写法仍算另一个 id,但实际调用方
/// (discover_project_root / canonicalize / PathBuf::display)在 Windows 上一律
/// 产出反斜杠,这条路径不会被走到。
pub(crate) fn session_identity(project_root: &Path) -> String {
    let raw = project_root.to_string_lossy();
    let stripped = raw
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| raw.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| raw.to_string());
    stripped.trim_end_matches(['\\', '/']).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::*;
    use crate::store::testutil::store;
    use std::path::Path;

    #[test]
    fn 会话状态更新并刷新时间() {
        let store = store();
        let before = store.get_session("ses_test").unwrap().unwrap();
        store.set_status("ses_test", "running").unwrap();
        let after = store.get_session("ses_test").unwrap().unwrap();
        assert_eq!(after.status, "running");
        assert!(after.updated_at >= before.updated_at);
        assert!(matches!(
            store.set_status("missing", "running"),
            Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
        ));
    }

    #[test]
    fn 同一目录的各种路径写法收敛到同一个会话id() {
        let bare = Path::new(r"C:\Users\kanzei\Documents\kanzei code");
        let canonical = Path::new(r"\\?\C:\Users\kanzei\Documents\kanzei code");
        assert_eq!(project_session_id(bare), project_session_id(canonical));
        // 大小写与末尾分隔符同样不该分裂会话。
        assert_eq!(
            project_session_id(bare),
            project_session_id(Path::new(r"C:\Users\Kanzei\Documents\KANZEI CODE\"))
        );
        // UNC 的扩展长度前缀映射回普通 UNC 写法。
        assert_eq!(
            project_session_id(Path::new(r"\\?\UNC\server\share\proj")),
            project_session_id(Path::new(r"\\server\share\proj"))
        );
        // 不同目录仍然是不同会话。
        assert_ne!(
            project_session_id(bare),
            project_session_id(Path::new(r"C:\Users\kanzei\Documents\other"))
        );

        // 向后兼容的硬约束:裸路径的身份串必须仍是"原样小写",否则既有会话
        // 会被一次性改名、全部历史失联。这里不断言哈希字面量——DefaultHasher
        // 跨 Rust 版本不保证稳定,断言身份串才是真正的不变量。
        assert_eq!(
            super::session_identity(bare),
            r"c:\users\kanzei\documents\kanzei code"
        );
    }
}
