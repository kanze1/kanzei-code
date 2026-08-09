//! schema 域(R-155 S7):migrate 原样搬,不重构。
//! 'schema_version','7' 是硬编码字面量,不许改成常量。
//! 已在事务内不得再调自开 tx 的方法(见 mod.rs unchecked_transaction 注)。

use rusqlite::{params, OptionalExtension};

use super::{now_ms, LEGACY_PROMOTED_GRACE_MS, SCHEMA_VERSION, SessionStore, StoreError};

impl SessionStore {
        pub(crate) fn migrate(&self) -> Result<(), StoreError> {
            self.connection.execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS schema_meta (
                     key TEXT PRIMARY KEY NOT NULL,
                     value TEXT NOT NULL
                 );",
            )?;
            let current: Option<i64> = self
                .connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| value.parse().unwrap_or_default());
            if let Some(version) = current {
                if version > SCHEMA_VERSION {
                    return Err(StoreError::UnsupportedSchema {
                        found: version,
                        supported: SCHEMA_VERSION,
                    });
                }
                if version == SCHEMA_VERSION {
                    return Ok(());
                }
                // 升级前先留一份旧版本的完整副本。迁移是单向的:一旦升上去,旧二进制
                // 就再也打不开这个库(上面那条 UnsupportedSchema),而桌面端与 CLI 是
                // 两个独立安装通道、可能一新一旧,回退也就无路可走。备份是那条退路。
                self.backup_before_upgrade(version)?;
                // v7:从备份里把被抹掉的输入状态位捞回来。ATTACH 不能在事务里执行,
                // 所以放在主迁移事务之前单独做。
                if version < 7 {
                    self.recover_legacy_input_status()?;
                }
            }
            let tx = self.connection.unchecked_transaction()?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                     session_id TEXT PRIMARY KEY NOT NULL,
                     project_root TEXT NOT NULL,
                     title TEXT,
                     status TEXT NOT NULL,
                     created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS session_events (
                     event_id TEXT PRIMARY KEY NOT NULL,
                     session_id TEXT NOT NULL REFERENCES sessions(session_id),
                     sequence INTEGER NOT NULL,
                     event_type TEXT NOT NULL,
                     payload_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL,
                     UNIQUE(session_id, sequence)
                 );
                 CREATE INDEX IF NOT EXISTS session_events_session_sequence
                     ON session_events(session_id, sequence);
                 -- v5 状态机:pending → promoted → running → completed | failed | cancelled。
                 -- 少了 running/completed/failed 时,跑完的输入会永远停在 promoted,于是
                 -- 用户之后任何一次停止都会把历史上已经成功完成的输入一并改成
                 -- cancelled(finalize_interrupt 按 promoted 一刀切),审计语义被彻底破坏。
                 CREATE TABLE IF NOT EXISTS session_inputs (
                     input_id TEXT PRIMARY KEY NOT NULL,
                     session_id TEXT NOT NULL REFERENCES sessions(session_id),
                     prompt TEXT NOT NULL,
                     delivery TEXT NOT NULL CHECK(delivery IN ('steer', 'queue')),
                     status TEXT NOT NULL CHECK(status IN
                         ('pending', 'promoted', 'running', 'completed', 'failed', 'cancelled')),
                     created_at INTEGER NOT NULL,
                     promoted_at INTEGER,
                     finished_at INTEGER
                 );
                 CREATE INDEX IF NOT EXISTS session_inputs_pending
                     ON session_inputs(session_id, delivery, status, created_at);
                 CREATE TABLE IF NOT EXISTS agent_notifications (
                     event_id TEXT PRIMARY KEY NOT NULL,
                     thread_id TEXT NOT NULL,
                     sequence INTEGER NOT NULL,
                     payload_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL,
                     UNIQUE(thread_id, sequence)
                 );
                 CREATE INDEX IF NOT EXISTS agent_notifications_thread_sequence
                     ON agent_notifications(thread_id, sequence);
                 CREATE TABLE IF NOT EXISTS delivery_cursors (
                     device_id TEXT NOT NULL,
                     thread_id TEXT NOT NULL,
                     cursor INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL,
                     PRIMARY KEY(device_id, thread_id)
                 );
                 CREATE TABLE IF NOT EXISTS episodes (
                     episode_id INTEGER PRIMARY KEY AUTOINCREMENT,
                     session_id TEXT NOT NULL,
                     created_at INTEGER NOT NULL,
                     prompt_head TEXT NOT NULL,
                     outcome TEXT NOT NULL,
                     steps INTEGER NOT NULL,
                     input_tokens INTEGER NOT NULL,
                     output_tokens INTEGER NOT NULL,
                     tools_json TEXT NOT NULL,
                     context_json TEXT NOT NULL,
                     -- v4(R-099):调用画像。旧库靠下面的 ALTER 补列;默认空对象代表
                     -- 那一轮还没开始度量,与度量出来全是零必须区分得开。
                     metrics_json TEXT NOT NULL DEFAULT '{}',
                     -- v5:轮次归属。没有这几列时,连本轮实际模型都只能
                     -- 从当前配置反推,而配置随时会变——事后复盘因此无法证伪。
                     provider TEXT NOT NULL DEFAULT '',
                     model TEXT NOT NULL DEFAULT '',
                     run_id TEXT NOT NULL DEFAULT '',
                     input_id TEXT NOT NULL DEFAULT '',
                     duration_ms INTEGER NOT NULL DEFAULT 0,
                     -- v6(R-106):上下文溢出压缩时被丢弃轨迹的摘要 JSON 数组字符串,
                     -- 保证溢出路径不再无声丢弃轨迹。
                     overflow_json TEXT NOT NULL DEFAULT ''
                 );
                 CREATE INDEX IF NOT EXISTS episodes_session_created
                     ON episodes(session_id, created_at);
                 INSERT INTO schema_meta(key, value) VALUES ('schema_version', '7')
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
            )?;
            // 已存在的旧库:上面的 CREATE IF NOT EXISTS 不会改动既有表,逐列补。
            // 列已存在时报错,忽略即可——这是幂等迁移的常规写法。
            for column in [
                "metrics_json TEXT NOT NULL DEFAULT '{}'",
                "provider TEXT NOT NULL DEFAULT ''",
                "model TEXT NOT NULL DEFAULT ''",
                "run_id TEXT NOT NULL DEFAULT ''",
                "input_id TEXT NOT NULL DEFAULT ''",
                "duration_ms INTEGER NOT NULL DEFAULT 0",
                "overflow_json TEXT NOT NULL DEFAULT ''",
            ] {
                let _ = tx.execute(&format!("ALTER TABLE episodes ADD COLUMN {column}"), []);
            }
            // session_inputs 的 status CHECK 写死在建表语句里,ALTER 改不了,只能重建。
            // 只在旧约束still生效时做,重建是幂等的:新库建出来就已经含 running。
            let legacy_check: Option<String> = tx
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'session_inputs'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if legacy_check.is_some_and(|sql| !sql.contains("running")) {
                tx.execute_batch(
                    "ALTER TABLE session_inputs RENAME TO session_inputs_v4;
                     CREATE TABLE session_inputs (
                         input_id TEXT PRIMARY KEY NOT NULL,
                         session_id TEXT NOT NULL REFERENCES sessions(session_id),
                         prompt TEXT NOT NULL,
                         delivery TEXT NOT NULL CHECK(delivery IN ('steer', 'queue')),
                         status TEXT NOT NULL CHECK(status IN
                             ('pending', 'promoted', 'running', 'completed', 'failed', 'cancelled')),
                         created_at INTEGER NOT NULL,
                         promoted_at INTEGER,
                         finished_at INTEGER
                     );
                     INSERT INTO session_inputs
                         (input_id, session_id, prompt, delivery, status, created_at, promoted_at, finished_at)
                     SELECT input_id, session_id, prompt, delivery, status, created_at, promoted_at, NULL
                     FROM session_inputs_v4;
                     DROP TABLE session_inputs_v4;
                     CREATE INDEX IF NOT EXISTS session_inputs_pending
                         ON session_inputs(session_id, delivery, status, created_at);",
                )?;
            }
            // v6 回填(D-180):v5 之前没有 running/completed,跑完的输入永远停在
            // promoted。这些存量不回填的话,用户下一次按停止仍会被 finalize_interrupt
            // 一并改写成 cancelled——新记录不再被污染,存量却还在被反复追认。
            //
            // completed 是**迁移推断值**,不是观测值:v5 之前根本没有记录结局的地方,
            // 只能按"被提升了就说明当时确实执行过"来判定。保护窗内(可能正被另一个
            // 进程执行)的一律不动,宁可漏回填也不误判在飞的输入。
            let backfilled = tx.execute(
                "UPDATE session_inputs SET status = 'completed'
                 WHERE status = 'promoted' AND (promoted_at IS NULL OR promoted_at < ?1)",
                params![now_ms() - LEGACY_PROMOTED_GRACE_MS],
            )?;
            if backfilled > 0 {
                tracing::info!(
                    backfilled,
                    "v6 迁移:遗留 promoted 输入按迁移推断回填为 completed"
                );
            }
            tx.commit()?;
            Ok(())
        }
    }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::*;
    use rusqlite::params;

    /// D-175:迁移是单向的,升级前必须留下可回退的完整副本;
    /// 遇到更新版本的库时,报错要说清该升哪个程序,而不是让人去删库。
    /// D-176:桌面端(canonicalize 带 `\\?\`)与 CLI(裸路径)必须落到同一个会话。
    /// D-180:v5 之前跑完的输入永远停在 promoted,不回填的话下一次停止仍会把它们
    /// 追认为 cancelled。回填是迁移推断值,但保护窗内的在飞输入绝不能被误判。
    #[test]
    fn 迁移把遗留promoted回填为completed但不动可能在飞的输入() {
        let path = std::env::temp_dir().join(format!(
            "kz-legacy-promoted-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        {
            let store = SessionStore::open(&path).unwrap();
            store.create_session("ses_legacy", "C:/p", None).unwrap();
            for id in ["old_a", "old_b", "just_now", "still_pending"] {
                store
                    .admit_input("ses_legacy", id, id, Delivery::Queue)
                    .unwrap();
            }
            let old = now_ms() - LEGACY_PROMOTED_GRACE_MS - 60_000;
            for (id, promoted_at) in [
                ("old_a", Some(old)),
                ("old_b", None),
                ("just_now", Some(now_ms())),
            ] {
                store
                    .connection
                    .execute(
                        "UPDATE session_inputs SET status='promoted', promoted_at=?1 WHERE input_id=?2",
                        params![promoted_at, id],
                    )
                    .unwrap();
            }
            // 退回 v5,让下次 open 触发 v6 迁移。
            store
                .connection
                .execute(
                    "UPDATE schema_meta SET value='5' WHERE key='schema_version'",
                    [],
                )
                .unwrap();
        }

        let store = SessionStore::open(&path).unwrap();
        assert_eq!(store.input_status("old_a").unwrap().unwrap(), "completed");
        assert_eq!(
            store.input_status("old_b").unwrap().unwrap(),
            "completed",
            "promoted_at 缺失的老记录同样是存量,要回填"
        );
        assert_eq!(
            store.input_status("just_now").unwrap().unwrap(),
            "promoted",
            "保护窗内的输入可能正被另一个进程执行,不得回填"
        );
        assert_eq!(store.input_status("still_pending").unwrap().unwrap(), "pending");

        // 回填之后再停止,已回填的历史输入不再被追认为 cancelled。
        store.set_status("ses_legacy", "running").unwrap();
        store.finalize_interrupt("ses_legacy").unwrap();
        assert_eq!(store.input_status("old_a").unwrap().unwrap(), "completed");
        assert_eq!(store.input_status("just_now").unwrap().unwrap(), "cancelled");
        assert_eq!(store.input_status("still_pending").unwrap().unwrap(), "cancelled");

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    /// D-180 续:v6 的回填晚了一步,存量 promoted 已被 v5 期间的停止抹成
    /// cancelled。v7 从迁移前备份里把状态位捞回来,且只捞备份里确实是 promoted 的。
    #[test]
    fn v7从备份恢复被抹掉的输入状态位且不误伤真取消() {
        let dir = std::env::temp_dir().join(format!(
            "kz-recover-legacy-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.db");

        // 备份:v4 时代的快照——ran_a/ran_b 当年被提升执行过,really_cancelled 是真取消。
        {
            let backup = SessionStore::open(&dir.join("backup-seed.db")).unwrap();
            backup.create_session("ses", "C:/p", None).unwrap();
            for id in ["ran_a", "ran_b", "really_cancelled"] {
                backup.admit_input("ses", id, id, Delivery::Queue).unwrap();
            }
            backup
                .connection
                .execute(
                    "UPDATE session_inputs SET status='promoted' WHERE input_id IN ('ran_a','ran_b')",
                    [],
                )
                .unwrap();
            backup
                .connection
                .execute(
                    "UPDATE session_inputs SET status='cancelled' WHERE input_id='really_cancelled'",
                    [],
                )
                .unwrap();
        }
        std::fs::copy(dir.join("backup-seed.db"), dir.join("state.db.v4.bak")).unwrap();

        // 现库:v5 期间的停止把三条全抹成了 cancelled,另有一条备份里没有的新输入。
        {
            let live = SessionStore::open(&path).unwrap();
            live.create_session("ses", "C:/p", None).unwrap();
            for id in ["ran_a", "ran_b", "really_cancelled", "after_backup"] {
                live.admit_input("ses", id, id, Delivery::Queue).unwrap();
            }
            live.connection
                .execute("UPDATE session_inputs SET status='cancelled'", [])
                .unwrap();
            live.connection
                .execute("UPDATE schema_meta SET value='6' WHERE key='schema_version'", [])
                .unwrap();
        }

        let store = SessionStore::open(&path).unwrap();
        assert_eq!(store.input_status("ran_a").unwrap().unwrap(), "completed");
        assert_eq!(store.input_status("ran_b").unwrap().unwrap(), "completed");
        assert_eq!(
            store.input_status("really_cancelled").unwrap().unwrap(),
            "cancelled",
            "备份里就是 cancelled 的,是用户当年真取消,不得复活"
        );
        assert_eq!(
            store.input_status("after_backup").unwrap().unwrap(),
            "cancelled",
            "备份之后才产生的输入无从判断,只能不动"
        );
        assert_eq!(store.legacy_inputs_recovered(), Some(2), "恢复条数要可回查");

        // 幂等:再开一次不重复恢复,也不改写已恢复的记录。
        drop(store);
        let store = SessionStore::open(&path).unwrap();
        assert_eq!(store.input_status("ran_a").unwrap().unwrap(), "completed");
        drop(store);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 没有任何备份时 v7 必须安静地什么都不做,而不是报错挡住迁移。
    #[test]
    fn v7在没有备份时安静通过() {
        let dir = std::env::temp_dir().join(format!(
            "kz-recover-nobackup-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.db");
        {
            let store = SessionStore::open(&path).unwrap();
            store.create_session("ses", "C:/p", None).unwrap();
            store.admit_input("ses", "a", "a", Delivery::Queue).unwrap();
            store
                .connection
                .execute("UPDATE schema_meta SET value='6' WHERE key='schema_version'", [])
                .unwrap();
        }
        let store = SessionStore::open(&path).unwrap();
        assert_eq!(store.input_status("a").unwrap().unwrap(), "pending");
        assert_eq!(store.legacy_inputs_recovered(), Some(0));
        drop(store);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 升级前留下整库备份且更高版本给出可执行指引() {
        let path = std::env::temp_dir().join(format!(
            "kz-migrate-backup-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        // 造一个 v4 库:建到当前版本后把版本号改回去,再塞一条可验证的数据。
        {
            let store = SessionStore::open(&path).unwrap();
            store.create_session("ses_old", "C:/project", None).unwrap();
            store
                .append_event("ses_old", "conversation.updated", &serde_json::json!({"v": 1}))
                .unwrap();
            store
                .connection
                .execute(
                    "UPDATE schema_meta SET value = '4' WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
        }

        let store = SessionStore::open(&path).unwrap();
        let backup = store.backup_path(4).expect("升级前必须留下 v4 备份");
        assert!(backup.is_file(), "{}", backup.display());
        // 备份必须是能打开的一致副本(WAL 下直接拷 .db 会拿到残缺快照)。
        let restored = Connection::open(&backup).unwrap();
        let version: String = restored
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "4", "备份必须保留迁移前的版本号");
        let events: i64 = restored
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 1, "备份必须含有迁移前的数据");
        drop(restored);
        // 迁移本身照常完成。
        assert!(store.get_session("ses_old").unwrap().is_some());
        drop(store);

        // 更新版本的库:旧程序必须拒绝打开,并且文案要给出出路。
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute(
                    "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                    params![SCHEMA_VERSION + 1],
                )
                .unwrap();
        }
        let error = match SessionStore::open(&path) {
            Ok(_) => panic!("更高版本的库必须拒绝打开"),
            Err(error) => error,
        };
        assert!(
            matches!(error, StoreError::UnsupportedSchema { found, supported }
                if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION)
        );
        let text = error.to_string();
        assert!(text.contains("cargo install"), "要告诉 CLI 怎么升: {text}");
        assert!(text.contains("检查更新"), "要告诉桌面端怎么升: {text}");
        assert!(text.contains("不要删库"), "必须堵死删库这条错误动作: {text}");

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = std::fs::remove_file(&backup);
    }
}

