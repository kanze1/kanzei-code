//! 项目级 SQLite 会话事件存储。
//!
//! 存储层只负责持久化事实，不负责 runner 的执行策略。事件序列按 session
//! 独立递增；输入先进入 inbox，只有 runner 在安全边界提升后才成为可见消息。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// v3:episodes 表(R-106,轮次情景摘要;回滚 = DROP TABLE episodes 并把版本改回 2)。
// v5:session_inputs 补 running/completed/failed 终态 + finished_at;
//     episodes 补 provider/model/run_id/input_id/duration_ms(D-173 可观测性)。
// v6:回填 v5 之前遗留的 promoted 输入(D-180)。
// v7:v6 回填晚了一步——存量 promoted 已被 v5 期间的停止抹成 cancelled,
//     改从迁移前备份里把状态位捞回来(D-180 续)。
const SCHEMA_VERSION: i64 = 7;
/// v6 回填的保护窗:promoted_at 晚于"迁移时刻减去这个窗口"的输入不回填,
/// 因为它可能正被另一个进程执行(桌面端与 CLI 共用同一个库)。
const LEGACY_PROMOTED_GRACE_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// 库比本二进制新。文案必须给出出路:光说"不兼容"会让人以为库坏了而去删库,
    /// 而删库丢的是全部会话历史——正确动作是把这个二进制升到同一版本。
    #[error(
        "数据库 schema 版本 {found} 高于本程序支持的 {supported}:这个 .kanzei/state.db \
         是更新版本的 kanzei 创建的。请把当前这个程序升到同一版本(桌面端用设置页「检查更新」;\
         CLI 用 cargo install --path crates/kanzei --force),不要删库——降级会丢掉全部会话历史。\
         升级前的库已自动备份为 state.db.v<n>.bak。"
    )]
    UnsupportedSchema { found: i64, supported: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub session_id: String,
    pub project_root: String,
    pub title: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredEvent {
    pub event_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    Steer,
    Queue,
}

impl Delivery {
    /// 仅限 store::* 子模块使用(S5 拆解后提 pub(super))。
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdmittedInput {
    pub input_id: String,
    pub session_id: String,
    pub prompt: String,
    pub delivery: Delivery,
    pub created_at: i64,
}

/// 一轮的情景摘要。参数从 9 个涨到 14 个之后位置参数已经不可读了(错位一个
/// 就把 provider 写进 model 也编译得过),所以收成具名结构。
#[derive(Debug, Clone, Default)]
pub struct EpisodeRecord<'a> {
    pub session_id: &'a str,
    pub prompt_head: &'a str,
    pub outcome: &'a str,
    pub steps: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tools_json: &'a str,
    pub context_json: &'a str,
    pub metrics_json: &'a str,
    /// 这一轮实际使用的 provider/model —— 事后复盘不能靠"当前配置"反推。
    pub provider: &'a str,
    pub model: &'a str,
    pub run_id: &'a str,
    pub input_id: &'a str,
    pub duration_ms: u64,
    /// 上下文溢出压缩时被丢弃轨迹的摘要 JSON 数组字符串(R-106),空数组 `[]` 代表无。
    pub overflow_json: &'a str,
}

mod episodes;
mod notifications;
mod events;
mod inbox;
mod session;
pub use session::{project_session_id, project_state_path};




pub struct SessionStore {
    /// 仅限 store::* 子模块使用(S1 拆壳后本字段 pub(crate))。
    pub(crate) connection: Connection,
    /// 落盘路径;内存库为 None。迁移前的备份要用它。
    /// 仅限 store::* 子模块使用(S1 拆壳后本字段 pub(crate))。
    pub(crate) path: Option<PathBuf>,
}

impl SessionStore {
    fn migrate(&self) -> Result<(), StoreError> {
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


fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间必须晚于 Unix epoch")
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn store() -> SessionStore {
        let store = SessionStore::open_in_memory().unwrap();
        store
            .create_session("ses_test", "C:/project", None)
            .unwrap();
        store
    }

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
    fn 事件序列按会话递增并可回放() {
        let store = store();
        let first = store
            .append_event(
                "ses_test",
                "session.created",
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        let second = store
            .append_event("ses_test", "turn.started", &serde_json::json!({"step": 1}))
            .unwrap();
        assert_eq!((first.sequence, second.sequence), (1, 2));
        assert_eq!(store.list_events("ses_test", 1).unwrap().len(), 1);
    }

    #[test]
    fn 中断收尾恢复空闲并原子取消未完成输入() {
        // D-085:Ctrl+C 收尾必须一次做全——状态复位、事件落库、输入取消。
        let store = store();
        store.set_status("ses_test", "running").unwrap();
        store
            .admit_input("ses_test", "input_promoted", "运行中的输入", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();
        store
            .admit_input("ses_test", "input_queued", "排队中的输入", Delivery::Queue)
            .unwrap();

        let cancelled = store.finalize_interrupt("ses_test").unwrap();
        assert_eq!(cancelled, 2, "promoted 与 pending 输入都要取消");
        assert_eq!(store.get_session("ses_test").unwrap().unwrap().status, "idle");
        let event = store
            .latest_event("ses_test", "session.status_changed")
            .unwrap()
            .unwrap();
        assert_eq!(event.payload["status"], "idle");
        assert_eq!(event.payload["reason"], "stopped_by_user");
        assert!(store.list_pending_inputs("ses_test").unwrap().is_empty());
        // 不存在的会话必须报错而不是静默成功。
        assert!(matches!(
            store.finalize_interrupt("missing"),
            Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
        ));
    }

    #[test]
    fn 通知和移动端_cursor_跨重建可回放() {
        let store = store();
        let event = crate::AgentNotification {
            event_id: "evt_mobile_1".into(),
            thread_id: "thread_a".into(),
            agent_id: "subagent".into(),
            kind: "agent_status_changed".into(),
            status: "succeeded".into(),
            summary: "完成".into(),
            requires_action: false,
            sequence: 1,
            created_at: 123,
        };
        store.append_notification(&event).unwrap();
        assert_eq!(store.replay_notifications("thread_a", 0, 10).unwrap(), vec![event]);
        assert_eq!(store.delivery_cursor("device_a", "thread_a").unwrap(), 0);
        store.set_delivery_cursor("device_a", "thread_a", 1).unwrap();
        assert_eq!(store.delivery_cursor("device_a", "thread_a").unwrap(), 1);
        assert!(store.replay_notifications("thread_a", 1, 10).unwrap().is_empty());
    }

    #[test]
    fn episode_落库并按时间倒序回放() {
        let store = store();
        store
            .append_episode(&EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "修复 D-068 限流分类",
                outcome: "completed",
                steps: 12,
                input_tokens: 50_000,
                output_tokens: 3_000,
                tools_json: r#"{"bash":5,"edit":3}"#,
                context_json: r#"[["agent/system",1200],["dev/memory",800]]"#,
                metrics_json: r#"{"terminal_calls":5,"edit_calls":3,"edit_misses":1}"#,
                provider: "deepseek",
                model: "deepseek-v4-flash",
                run_id: "run_a",
                input_id: "input_a",
                duration_ms: 708_000,
                overflow_json: r#"[{"dropped_messages":3,"tools":{"bash":2},"failures":[],"preview":"旧任务"}]"#,
            })
            .unwrap();
        store
            .append_episode(&EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "第二轮",
                outcome: "halted",
                steps: 3,
                tools_json: "{}",
                context_json: "[]",
                metrics_json: "{}",
                ..EpisodeRecord::default()
            })
            .unwrap();
        let episodes = store.list_episodes("ses_test", 10).unwrap();
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].1, "第二轮");
        assert_eq!(episodes[1].3, 12);
        assert!(episodes[1].4.contains("bash"));
        assert!(store.list_episodes("missing", 10).unwrap().is_empty());

        // R-099:调用画像随轮次落库,并能按时间倒序取回。空对象要与"度量为零"区分开。
        let recent = store.recent_episodes("ses_test", 10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].1, "第二轮");
        assert_eq!(recent[0].8, "{}", "未度量的轮次应保持空对象");
        assert!(recent[1].8.contains("edit_misses"), "画像未随轮次落库: {}", recent[1].8);

        // D-173:轮次归属必须落库。之前只能从"当前配置"反推这一轮跑的哪个模型,
        // 而配置随时会变——复盘时连最基本的事实都无法证伪。
        let identities = store.recent_episode_identities("ses_test", 10).unwrap();
        assert_eq!(identities.len(), 2);
        assert_eq!(identities[1].1, "deepseek");
        assert_eq!(identities[1].2, "deepseek-v4-flash");
        assert_eq!(identities[1].3, "run_a");
        assert_eq!(identities[1].4, "input_a");
        assert_eq!(identities[1].5, 708_000);

        // R-106:上下文压缩时被丢弃的轨迹随 episode 落库,并可查询回放。
        let traces = store.recent_overflow_traces("ses_test", 10).unwrap();
        assert_eq!(traces.len(), 1, "只有第一条 episode 带溢出轨迹");
        assert!(traces[0].1.contains("dropped_messages"));
        assert!(traces[0].1.contains("\"bash\":2"), "工具画像应随轨迹沉淀: {}", traces[0].1);
        assert!(store.recent_overflow_traces("missing", 10).unwrap().is_empty());
    }

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
            super::session::session_identity(bare),
            r"c:\users\kanzei\documents\kanzei code"
        );
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

    #[test]
    fn 已完成的输入不会被后来的停止追认为取消() {
        // D-173:少了 completed 终态时,跑完的输入永远停在 promoted,
        // 于是任何一次停止都会把历史成功输入一并改写成 cancelled。
        let store = store();
        store
            .admit_input("ses_test", "done_earlier", "上一轮已完成", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();
        assert!(store.start_input("done_earlier").unwrap());
        assert_eq!(store.input_status("done_earlier").unwrap().unwrap(), "running");
        assert!(store.finish_input("done_earlier", true).unwrap());
        assert_eq!(store.input_status("done_earlier").unwrap().unwrap(), "completed");

        store
            .admit_input("ses_test", "in_flight", "本轮被打断", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();
        store.start_input("in_flight").unwrap();
        store
            .admit_input("ses_test", "queued", "还没轮到", Delivery::Queue)
            .unwrap();

        store.set_status("ses_test", "running").unwrap();
        assert_eq!(store.finalize_interrupt("ses_test").unwrap(), 2);
        assert_eq!(
            store.input_status("done_earlier").unwrap().unwrap(),
            "completed",
            "已完成的输入必须保持 completed"
        );
        assert_eq!(store.input_status("in_flight").unwrap().unwrap(), "cancelled");
        assert_eq!(store.input_status("queued").unwrap().unwrap(), "cancelled");
        // 终态不可回退:再次 finish 不改写既有结局。
        assert!(!store.finish_input("in_flight", true).unwrap());
    }

    #[test]
    fn latest_event_按类型返回最新事件() {
        let store = store();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"v": 1}))
            .unwrap();
        store
            .append_event("ses_test", "run.completed", &serde_json::json!({}))
            .unwrap();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"v": 2}))
            .unwrap();
        let latest = store
            .latest_event("ses_test", "conversation.updated")
            .unwrap()
            .unwrap();
        assert_eq!(latest.payload["v"], 2);
        assert!(store.latest_event("ses_test", "missing").unwrap().is_none());
    }
    #[test]
    fn clear_conversation_只删除对话快照() {
        let store = store();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"v": 1}))
            .unwrap();
        store
            .append_event("ses_test", "session.status_changed", &serde_json::json!({"status": "idle"}))
            .unwrap();
        assert_eq!(store.clear_conversation("ses_test").unwrap(), 1);
        assert!(store.latest_event("ses_test", "conversation.updated").unwrap().is_none());
        assert!(store.latest_event("ses_test", "session.status_changed").unwrap().is_some());
    }

    #[test]
    fn 不同会话的事件_id_保持唯一() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        let first = store
            .append_event("ses_test", "turn.started", &serde_json::json!({}))
            .unwrap();
        let second = store
            .append_event("ses_other", "turn.started", &serde_json::json!({}))
            .unwrap();
        assert_ne!(first.event_id, second.event_id);
    }

    #[test]
    fn steer_合并且_queue_保持_fifo() {
        let store = store();
        store
            .admit_input("ses_test", "i1", "s1", Delivery::Steer)
            .unwrap();
        store
            .admit_input("ses_test", "i2", "q1", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "i3", "s2", Delivery::Steer)
            .unwrap();
        let steers = store.promote_steers("ses_test").unwrap();
        assert_eq!(
            steers.iter().map(|x| x.prompt.as_str()).collect::<Vec<_>>(),
            ["s1", "s2"]
        );
        assert_eq!(
            store
                .promote_next_queue("ses_test")
                .unwrap()
                .unwrap()
                .prompt,
            "q1"
        );
        assert!(!store.has_pending("ses_test", Delivery::Steer).unwrap());
    }

    #[test]
    fn drain_优先提升_steer_再取_queue() {
        let store = store();
        store
            .admit_input("ses_test", "q1", "队列", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "s1", "插入", Delivery::Steer)
            .unwrap();
        assert_eq!(
            store
                .promote_next_input("ses_test")
                .unwrap()
                .unwrap()
                .delivery,
            Delivery::Steer
        );
        assert_eq!(
            store
                .promote_next_input("ses_test")
                .unwrap()
                .unwrap()
                .delivery,
            Delivery::Queue
        );
    }

    #[test]
    fn drain_依次提升全部_steer_再取_queue() {
        let store = store();
        store
            .admit_input("ses_test", "s1", "插入一", Delivery::Steer)
            .unwrap();
        store
            .admit_input("ses_test", "s2", "插入二", Delivery::Steer)
            .unwrap();
        store
            .admit_input("ses_test", "q1", "队列", Delivery::Queue)
            .unwrap();

        let prompts = (0..3)
            .map(|_| store.promote_next_input("ses_test").unwrap().unwrap().prompt)
            .collect::<Vec<_>>();
        assert_eq!(prompts, ["插入一", "插入二", "队列"]);
        assert!(!store.has_pending("ses_test", Delivery::Steer).unwrap());
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
    }

    #[test]
    fn 重复_admission_是幂等的() {
        let store = store();
        let first = store
            .admit_input("ses_test", "same", "prompt", Delivery::Queue)
            .unwrap();
        let second = store
            .admit_input("ses_test", "same", "other", Delivery::Steer)
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn 只能取消尚未提升的输入() {
        let store = store();
        store
            .admit_input("ses_test", "pending", "待取消", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "promoted", "已提升", Delivery::Queue)
            .unwrap();
        assert!(store.cancel_input("ses_test", "pending").unwrap());
        assert!(!store.cancel_input("ses_test", "pending").unwrap());
        assert!(!store.cancel_input("ses_test", "missing").unwrap());
        assert!(store.has_pending("ses_test", Delivery::Queue).unwrap());

        store.promote_next_queue("ses_test").unwrap();
        assert!(!store.cancel_input("ses_test", "promoted").unwrap());
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
    }

    #[test]
    fn 停止时取消_pending_和已_promoted_输入() {
        let store = store();
        store
            .admit_input("ses_test", "pending", "待执行", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "promoted", "已提升未完成", Delivery::Queue)
            .unwrap();
        let promoted = store.promote_next_input("ses_test").unwrap().unwrap();
        assert_eq!(promoted.input_id, "pending");
        assert_eq!(store.cancel_unfinished_inputs("ses_test").unwrap(), 2);
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
        assert!(!store.cancel_input("ses_test", "promoted").unwrap());
    }

    #[test]
    fn 停止运行时只取消本会话的_pending_输入() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        store
            .admit_input("ses_test", "q1", "当前会话", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_other", "q2", "其他会话", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "s1", "已提升", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();

        assert_eq!(store.cancel_pending_inputs("ses_test").unwrap(), 1);
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
        assert!(store.has_pending("ses_other", Delivery::Queue).unwrap());
        assert!(!store.cancel_input("ses_test", "s1").unwrap());
    }

    #[test]
    fn r050_poc_不同会话事件回放互不串线() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"thread": "a"}))
            .unwrap();
        store
            .append_event("ses_other", "conversation.updated", &serde_json::json!({"thread": "b"}))
            .unwrap();

        let a = store.list_events("ses_test", 0).unwrap();
        let b = store.list_events("ses_other", 0).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].payload["thread"], "a");
        assert_eq!(b[0].payload["thread"], "b");
        assert_eq!(a[0].sequence, 1);
        assert_eq!(b[0].sequence, 1);
    }

    #[test]
    fn r050_poc_停止一个会话不影响另一个会话队列() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        store.admit_input("ses_test", "a-pending", "A", Delivery::Queue).unwrap();
        store.admit_input("ses_other", "b-pending", "B", Delivery::Queue).unwrap();
        store.admit_input("ses_other", "b-steer", "B steer", Delivery::Steer).unwrap();

        assert_eq!(store.cancel_pending_inputs("ses_test").unwrap(), 1);
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
        assert!(store.has_pending("ses_other", Delivery::Queue).unwrap());
        assert_eq!(store.promote_next_input("ses_other").unwrap().unwrap().prompt, "B steer");
        assert_eq!(store.promote_next_input("ses_other").unwrap().unwrap().prompt, "B");
    }

    #[test]
    fn 并发追加事件的_sequence_连续且唯一() {
        let path = std::env::temp_dir().join(format!(
            "kz-store-concurrency-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        let initializer = SessionStore::open(&path).unwrap();
        initializer
            .create_session("ses_concurrent", "C:/project", None)
            .unwrap();
        drop(initializer);

        let stores = (0..4)
            .map(|_| SessionStore::open(&path).unwrap())
            .collect::<Vec<_>>();
        let handles = stores
            .into_iter()
            .enumerate()
            .map(|(worker, store)| {
                std::thread::spawn(move || {
                    (0..20)
                        .map(|index| {
                            store
                                .append_event(
                                    "ses_concurrent",
                                    "test.concurrent",
                                    &serde_json::json!({"worker": worker, "index": index}),
                                )
                                .unwrap()
                                .sequence
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let mut sequences = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=80).collect::<Vec<_>>());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }    #[test]
    fn 并发原子追加通知的_sequence_连续且不丢失() {
        use std::sync::{Arc, Barrier};

        let path = std::env::temp_dir().join(format!(
            "kz-notification-concurrency-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        let stores = (0..4)
            .map(|_| SessionStore::open(&path).unwrap())
            .collect::<Vec<_>>();
        let barrier = Arc::new(Barrier::new(stores.len()));
        let handles = stores
            .into_iter()
            .enumerate()
            .map(|(worker, store)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..20)
                        .map(|index| {
                            store
                                .append_notification_atomic(
                                    "thread_concurrent",
                                    "succeeded",
                                    &format!("worker={worker},index={index}"),
                                    false,
                                )
                                .unwrap()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let notifications = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        let mut sequences = notifications
            .iter()
            .map(|notification| notification.sequence)
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=80).collect::<Vec<_>>());
        assert_eq!(
            SessionStore::open(&path)
                .unwrap()
                .replay_notifications("thread_concurrent", 0, 100)
                .unwrap()
                .len(),
            80
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn 通知的_sequence_冲突不会被静默忽略() {
        let store = store();
        let first = crate::AgentNotification {
            event_id: "notification_first".into(),
            thread_id: "thread_conflict".into(),
            agent_id: "primary".into(),
            kind: "agent_status_changed".into(),
            status: "succeeded".into(),
            summary: "first".into(),
            requires_action: false,
            sequence: 1,
            created_at: now_ms(),
        };
        let mut second = first.clone();
        second.event_id = "notification_second".into();
        store.append_notification(&first).unwrap();
        assert!(store.append_notification(&second).is_err());
        assert_eq!(
            store
                .replay_notifications("thread_conflict", 0, 10)
                .unwrap()
                .len(),
            1
        );
    }
}
