//! Transport facts and delivery receipts. Agent history remains in SessionStore.
use kanzei_harness::{QqConversation, QqPeerKind};
#[cfg(test)]
use rusqlite::OptionalExtension;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) struct Store {
    db: Connection,
}
#[derive(Clone, Serialize)]
pub(crate) struct Peer {
    pub identity: QqConversation,
    pub title: String,
    pub process_id: String,
    pub session_id: String,
    pub root: PathBuf,
    pub watermark: i64,
    pub last_started: i64,
}

#[derive(Serialize)]
pub(crate) struct Message {
    pub sequence: i64,
    pub message_id: String,
    pub sender: String,
    pub nickname: String,
    pub text: String,
    pub at: i64,
}

pub(crate) struct Job {
    pub id: String,
    pub identity: QqConversation,
    pub watermark: i64,
    pub prompt: String,
    pub automatic: bool,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS qq_peers(account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, title TEXT NOT NULL, process TEXT NOT NULL, session TEXT NOT NULL, watermark INTEGER NOT NULL DEFAULT 0, last_started INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(account,kind,peer));
            CREATE TABLE IF NOT EXISTS qq_messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, message_id TEXT NOT NULL, sender TEXT NOT NULL, nickname TEXT NOT NULL, body TEXT NOT NULL, at INTEGER NOT NULL, UNIQUE(account,kind,peer,message_id));
            CREATE TABLE IF NOT EXISTS qq_jobs(id TEXT PRIMARY KEY, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, watermark INTEGER NOT NULL, prompt TEXT NOT NULL, automatic INTEGER NOT NULL DEFAULT 0, admitted INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS qq_deliveries(id TEXT PRIMARY KEY, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, body TEXT NOT NULL, state TEXT NOT NULL, remote_id TEXT, error TEXT, at INTEGER NOT NULL);")?;
        Ok(Self { db })
    }

    pub fn recover_deliveries(&self) -> anyhow::Result<()> {
        self.db.execute("UPDATE qq_deliveries SET state='unknown', error='进程在发送期间退出，送达结果未知' WHERE state='sending'", [])?;
        Ok(())
    }

    pub fn register(&self, peer: &Peer) -> anyhow::Result<()> {
        self.db.execute("INSERT INTO qq_peers(account,kind,peer,title,process,session) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(account,kind,peer) DO UPDATE SET title=excluded.title,process=excluded.process,session=excluded.session",
            params![peer.identity.account,peer.identity.kind.label(),peer.identity.peer,peer.title,peer.process_id,peer.session_id])?;
        Ok(())
    }

    pub fn peers(&self) -> anyhow::Result<Vec<Peer>> {
        let mut stmt = self.db.prepare("SELECT account,kind,peer,title,process,session,watermark,last_started FROM qq_peers ORDER BY last_started DESC,account,kind,peer")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (account, kind, id, title, process_id, session_id, watermark, last_started) = row?;
            let kind = match kind.as_str() {
                "friend" => QqPeerKind::Friend,
                "group" => QqPeerKind::Group,
                _ => anyhow::bail!("Unknown QQ peer kind"),
            };
            let identity = QqConversation::new(&account, kind, &id)?;
            let root = identity.root()?;
            out.push(Peer {
                identity,
                title,
                process_id,
                session_id,
                root,
                watermark,
                last_started,
            });
        }
        Ok(out)
    }

    pub fn receive(
        &mut self,
        identity: &QqConversation,
        message: &Message,
        wake: bool,
    ) -> anyhow::Result<bool> {
        let tx = self.db.transaction()?;
        let fresh = tx.execute("INSERT OR IGNORE INTO qq_messages(account,kind,peer,message_id,sender,nickname,body,at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![identity.account,identity.kind.label(),identity.peer,message.message_id,message.sender,message.nickname,message.text,message.at])? == 1;
        if fresh && wake {
            let sequence = tx.last_insert_rowid();
            let id = format!(
                "qq:{}:{}:{}:{sequence}",
                identity.account,
                identity.kind.label(),
                identity.peer
            );
            tx.execute("INSERT INTO qq_jobs(id,account,kind,peer,watermark,prompt) VALUES(?1,?2,?3,?4,?5,?6)", params![id,identity.account,identity.kind.label(),identity.peer,sequence,message.text])?;
        }
        tx.commit()?;
        Ok(fresh)
    }

    pub fn messages(&self, identity: &QqConversation, after: i64) -> anyhow::Result<Vec<Message>> {
        // Pending input drains oldest-first, independently of display pagination.
        let mut stmt=self.db.prepare("SELECT sequence,message_id,sender,nickname,body,at FROM qq_messages WHERE account=?1 AND kind=?2 AND peer=?3 AND sequence>?4 ORDER BY sequence LIMIT 100")?;
        let rows = stmt
            .query_map(
                params![
                    identity.account,
                    identity.kind.label(),
                    identity.peer,
                    after
                ],
                |r| {
                    Ok(Message {
                        sequence: r.get(0)?,
                        message_id: r.get(1)?,
                        sender: r.get(2)?,
                        nickname: r.get(3)?,
                        text: r.get(4)?,
                        at: r.get(5)?,
                    })
                },
            )?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn recent_messages(&self, identity: &QqConversation) -> anyhow::Result<Vec<Message>> {
        let first: i64 = self.db.query_row("SELECT COALESCE(MIN(sequence),1) FROM (SELECT sequence FROM qq_messages WHERE account=?1 AND kind=?2 AND peer=?3 ORDER BY sequence DESC LIMIT 100)",params![identity.account,identity.kind.label(),identity.peer],|r|r.get(0))?;
        self.messages(identity, first - 1)
    }

    pub fn prepare_job(
        &self,
        peer: &Peer,
        watermark: i64,
        prompt: &str,
        automatic: bool,
    ) -> anyhow::Result<()> {
        let suffix = if automatic {
            format!("wake:{}", peer.last_started)
        } else {
            watermark.to_string()
        };
        let id = format!(
            "qq:{}:{}:{}:{suffix}",
            peer.identity.account,
            peer.identity.kind.label(),
            peer.identity.peer
        );
        self.db.execute("INSERT OR IGNORE INTO qq_jobs(id,account,kind,peer,watermark,prompt,automatic) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![id,peer.identity.account,peer.identity.kind.label(),peer.identity.peer,watermark,prompt,automatic])?;
        Ok(())
    }

    pub fn pending_jobs(&self) -> anyhow::Result<Vec<Job>> {
        let mut stmt=self.db.prepare("SELECT id,account,kind,peer,watermark,prompt,automatic FROM qq_jobs WHERE admitted=0 ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, account, kind, peer, watermark, prompt, automatic) = row?;
            let kind = match kind.as_str() {
                "friend" => QqPeerKind::Friend,
                "group" => QqPeerKind::Group,
                _ => anyhow::bail!("Unknown QQ job kind"),
            };
            out.push(Job {
                id,
                identity: QqConversation::new(&account, kind, &peer)?,
                watermark,
                prompt,
                automatic,
            });
        }
        Ok(out)
    }

    pub fn admitted(&mut self, job: &Job, at: i64) -> anyhow::Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("UPDATE qq_jobs SET admitted=1 WHERE id=?1", [&job.id])?;
        tx.execute("UPDATE qq_peers SET watermark=MAX(watermark,?4),last_started=?5 WHERE account=?1 AND kind=?2 AND peer=?3",params![job.identity.account,job.identity.kind.label(),job.identity.peer,job.watermark,at])?;
        tx.commit()?;
        Ok(())
    }

    pub fn begin_send(
        &self,
        identity: &QqConversation,
        id: &str,
        body: &Value,
        at: i64,
    ) -> anyhow::Result<bool> {
        Ok(self.db.execute("INSERT OR IGNORE INTO qq_deliveries(id,account,kind,peer,body,state,at) VALUES(?1,?2,?3,?4,?5,'sending',?6)",params![id,identity.account,identity.kind.label(),identity.peer,body.to_string(),at])?==1)
    }

    pub fn finish_send(
        &self,
        id: &str,
        result: &Result<Value, super::protocol::Failure>,
    ) -> anyhow::Result<()> {
        let (state, remote, error) = match result {
            Ok(value) => ("sent", Some(value.to_string()), None),
            Err(error) => (error.state(), None, Some(error.to_string())),
        };
        self.db.execute(
            "UPDATE qq_deliveries SET state=?2,remote_id=?3,error=?4 WHERE id=?1",
            params![id, state, remote, error],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn delivery(&self, id: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT state FROM qq_deliveries WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn deliveries(&self, identity: &QqConversation) -> anyhow::Result<Vec<Value>> {
        let mut stmt=self.db.prepare("SELECT id,body,state,remote_id,error,at FROM qq_deliveries WHERE account=?1 AND kind=?2 AND peer=?3 ORDER BY at DESC LIMIT 100")?;
        let rows = stmt.query_map(params![identity.account,identity.kind.label(),identity.peer],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"body":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"remote_id":r.get::<_,Option<String>>(3)?,"error":r.get::<_,Option<String>>(4)?,"at":r.get::<_,i64>(5)?})))?.collect::<Result<_,_>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_friend_messages_and_frozen_jobs_commit_together() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qq.db");
        let identity = QqConversation::new("1", QqPeerKind::Friend, "2").unwrap();
        let message = Message {
            sequence: 0,
            message_id: "7".into(),
            sender: "2".into(),
            nickname: "friend".into(),
            text: "do local work".into(),
            at: 1,
        };
        let mut db = Store::open(&path).unwrap();
        assert!(db.receive(&identity, &message, true).unwrap());
        drop(db);
        let mut db = Store::open(&path).unwrap();
        assert!(!db.receive(&identity, &message, true).unwrap());
        let jobs = db.pending_jobs().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].prompt, "do local work");
        assert_eq!(jobs[0].identity, identity);
        assert!(!jobs[0].automatic);
        let id = &jobs[0].id;
        assert!(db.begin_send(&identity, id, &Value::Null, 2).unwrap());
        db.finish_send(
            id,
            &Err(super::super::protocol::Failure::Rejected("rejected".into())),
        )
        .unwrap();
        assert_eq!(db.delivery(id).unwrap().as_deref(), Some("failed"));
        db.admitted(&jobs[0], 3).unwrap();
        assert!(db.pending_jobs().unwrap().is_empty());
    }
    #[test]
    fn receipt_recovery_and_inbound_dedup_do_not_replay_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qq.db");
        let identity = QqConversation::new("1", QqPeerKind::Group, "2").unwrap();
        let mut db = Store::open(&path).unwrap();
        let message = Message {
            sequence: 0,
            message_id: "7".into(),
            sender: "8".into(),
            nickname: "name".into(),
            text: "hello".into(),
            at: 1,
        };
        assert!(db.receive(&identity, &message, false).unwrap());
        assert!(!db.receive(&identity, &message, false).unwrap());
        let friend = QqConversation::new("1", QqPeerKind::Friend, "2").unwrap();
        assert!(db.messages(&friend, 0).unwrap().is_empty());
        assert!(db
            .begin_send(&identity, "run:1", &serde_json::json!({"text":"hi"}), 1)
            .unwrap());
        drop(db);
        let db = Store::open(&path).unwrap();
        db.recover_deliveries().unwrap();
        assert_eq!(db.delivery("run:1").unwrap().as_deref(), Some("unknown"));
        assert!(!db.begin_send(&identity, "run:1", &Value::Null, 2).unwrap());
    }
}
