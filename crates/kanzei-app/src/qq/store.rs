//! Transport facts and delivery receipts. Agent history remains in SessionStore.
use super::input::ImageSource;
use kanzei_harness::{QqConversation, QqPeerKind};
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
    pub images: Vec<ImageSource>,
    pub at: i64,
}

pub(crate) struct Job {
    pub id: String,
    pub identity: QqConversation,
    pub watermark: i64,
    pub prompt: String,
    pub trigger: Trigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    Direct,
    Ambient,
    Proactive,
}
impl Trigger {
    fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Ambient => "ambient",
            Self::Proactive => "proactive",
        }
    }
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "direct" => Ok(Self::Direct),
            "ambient" => Ok(Self::Ambient),
            "proactive" => Ok(Self::Proactive),
            _ => anyhow::bail!("Unknown QQ job trigger: {value}"),
        }
    }
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS qq_peers(account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, title TEXT NOT NULL, process TEXT NOT NULL, session TEXT NOT NULL, watermark INTEGER NOT NULL DEFAULT 0, last_started INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(account,kind,peer));
            CREATE TABLE IF NOT EXISTS qq_messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, message_id TEXT NOT NULL, sender TEXT NOT NULL, nickname TEXT NOT NULL, body TEXT NOT NULL, at INTEGER NOT NULL, images TEXT NOT NULL DEFAULT '[]', UNIQUE(account,kind,peer,message_id));
            CREATE TABLE IF NOT EXISTS qq_jobs(id TEXT PRIMARY KEY, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, watermark INTEGER NOT NULL, prompt TEXT NOT NULL, trigger TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending');
            CREATE TABLE IF NOT EXISTS qq_deliveries(id TEXT PRIMARY KEY, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, body TEXT NOT NULL, state TEXT NOT NULL, remote_id TEXT, error TEXT, at INTEGER NOT NULL);")?;
        let columns = {
            let mut query = db.prepare("PRAGMA table_info(qq_jobs)")?;
            let columns = query
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            columns
        };
        if columns.iter().any(|name| name == "automatic") {
            let tx = db.transaction()?;
            tx.execute_batch("ALTER TABLE qq_jobs RENAME TO qq_jobs_legacy;
                CREATE TABLE qq_jobs(id TEXT PRIMARY KEY, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, watermark INTEGER NOT NULL, prompt TEXT NOT NULL, trigger TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending');
                INSERT INTO qq_jobs SELECT id,account,kind,peer,watermark,prompt,CASE WHEN kind='friend' THEN 'direct' WHEN automatic=1 THEN 'proactive' ELSE 'ambient' END,CASE WHEN admitted=1 THEN 'accepted' ELSE 'pending' END FROM qq_jobs_legacy;
                DROP TABLE qq_jobs_legacy;")?;
            tx.commit()?;
        }
        let has_images = db
            .prepare("PRAGMA table_info(qq_messages)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "images");
        if !has_images {
            db.execute_batch(
                "ALTER TABLE qq_messages ADD COLUMN images TEXT NOT NULL DEFAULT '[]'",
            )?;
        }
        Ok(Self { db })
    }

    pub fn recover_deliveries(&self) -> anyhow::Result<()> {
        self.db.execute("UPDATE qq_deliveries SET state='unknown', error='进程在发送期间退出，送达结果未知' WHERE state='sending'", [])?;
        self.db.execute(
            "UPDATE qq_jobs SET state='superseded' WHERE state='pending' AND trigger='proactive'",
            [],
        )?;
        Ok(())
    }
    pub fn latest_sequence(&self, identity: &QqConversation) -> anyhow::Result<i64> {
        Ok(self.db.query_row("SELECT COALESCE(MAX(sequence),0) FROM qq_messages WHERE account=?1 AND kind=?2 AND peer=?3",params![identity.account,identity.kind.label(),identity.peer],|row|row.get(0))?)
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
            let root = crate::normalized_project_root(&identity.root()?);
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
        wake_prompt: Option<&str>,
    ) -> anyhow::Result<bool> {
        let tx = self.db.transaction()?;
        let fresh = tx.execute("INSERT OR IGNORE INTO qq_messages(account,kind,peer,message_id,sender,nickname,body,at,images) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![identity.account,identity.kind.label(),identity.peer,message.message_id,message.sender,message.nickname,message.text,message.at,serde_json::to_string(&message.images)?])? == 1;
        if let Some(prompt) = wake_prompt.filter(|_| fresh) {
            let sequence = tx.last_insert_rowid();
            if identity.kind == QqPeerKind::Group {
                tx.execute("UPDATE qq_jobs SET state='superseded' WHERE account=?1 AND kind='group' AND peer=?2 AND state='pending' AND trigger!='direct' AND watermark<=?3",params![identity.account,identity.peer,sequence])?;
            }
            let id = format!(
                "qq:{}:{}:{}:{sequence}",
                identity.account,
                identity.kind.label(),
                identity.peer
            );
            tx.execute("INSERT INTO qq_jobs(id,account,kind,peer,watermark,prompt,trigger) VALUES(?1,?2,?3,?4,?5,?6,'direct')", params![id,identity.account,identity.kind.label(),identity.peer,sequence,prompt])?;
        }
        tx.commit()?;
        Ok(fresh)
    }

    pub fn messages(&self, identity: &QqConversation, after: i64) -> anyhow::Result<Vec<Message>> {
        // Pending input drains oldest-first, independently of display pagination.
        let mut stmt=self.db.prepare("SELECT sequence,message_id,sender,nickname,body,at,images FROM qq_messages WHERE account=?1 AND kind=?2 AND peer=?3 AND sequence>?4 ORDER BY sequence LIMIT 100")?;
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
                        images: serde_json::from_str(&r.get::<_, String>(6)?).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                6,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?,
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

    pub fn message_watermark(&self, identity: &QqConversation) -> anyhow::Result<i64> {
        Ok(self.db.query_row("SELECT COALESCE(MAX(sequence),0) FROM qq_messages WHERE account=?1 AND kind=?2 AND peer=?3", params![identity.account,identity.kind.label(),identity.peer], |row|row.get(0))?)
    }

    pub fn supersede_job(&self, job: &Job) -> anyhow::Result<()> {
        self.db.execute(
            "UPDATE qq_jobs SET state='superseded' WHERE id=?1 AND state='pending'",
            [&job.id],
        )?;
        Ok(())
    }

    pub fn message_sender(
        &self,
        identity: &QqConversation,
        message_id: &str,
    ) -> anyhow::Result<String> {
        let sender: Option<String> = self.db.query_row("SELECT sender FROM qq_messages WHERE account=?1 AND kind=?2 AND peer=?3 AND message_id=?4",
            params![identity.account, identity.kind.label(), identity.peer, message_id], |row| row.get(0)).optional()?;
        sender.ok_or_else(|| anyhow::anyhow!("当前 QQ 会话中没有这条消息"))
    }

    /// Frozen job boundary; later messages cannot change this input's image.
    /// Direct replies include the caller's latest image from the same recent
    /// conversation window used in the prompt, including image-then-mention.
    pub fn input_image(
        &self,
        job: &Job,
        after: i64,
    ) -> anyhow::Result<Option<(String, String, usize)>> {
        let row: Option<(String, String, String)> = self.db.query_row(
            "SELECT message_id,sender,images FROM (SELECT message_id,sender,images,sequence FROM qq_messages WHERE account=?1 AND kind=?2 AND peer=?3 AND sequence<=?4 AND sequence>?6 ORDER BY sequence DESC LIMIT 100) WHERE images!='[]' AND (?5!='direct' OR sender=(SELECT sender FROM qq_messages WHERE sequence=?4)) ORDER BY sequence DESC LIMIT 1",
            params![job.identity.account,job.identity.kind.label(),job.identity.peer,job.watermark,job.trigger.label(),after],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        row.map(|(message_id, sender, images)| {
            let images: Vec<ImageSource> = serde_json::from_str(&images)?;
            Ok((message_id, sender, images.len()))
        })
        .transpose()
    }

    pub fn prepare_job(&self, peer: &Peer, watermark: i64, prompt: &str) -> anyhow::Result<()> {
        let pending:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM qq_jobs WHERE account=?1 AND kind=?2 AND peer=?3 AND state='pending')",params![peer.identity.account,peer.identity.kind.label(),peer.identity.peer],|r|r.get(0))?;
        if pending {
            return Ok(());
        }
        let id = format!(
            "qq:{}:{}:{}:{watermark}",
            peer.identity.account,
            peer.identity.kind.label(),
            peer.identity.peer
        );
        self.db.execute("INSERT OR IGNORE INTO qq_jobs(id,account,kind,peer,watermark,prompt,trigger) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![id,peer.identity.account,peer.identity.kind.label(),peer.identity.peer,watermark,prompt,Trigger::Ambient.label()])?;
        Ok(())
    }

    pub fn pending_jobs(&self) -> anyhow::Result<Vec<Job>> {
        let mut stmt=self.db.prepare("SELECT id,account,kind,peer,watermark,prompt,trigger FROM qq_jobs WHERE state='pending' ORDER BY trigger='direct' DESC,rowid")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, account, kind, peer, watermark, prompt, trigger) = row?;
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
                trigger: Trigger::parse(&trigger)?,
            });
        }
        Ok(out)
    }

    pub fn admitted(&mut self, job: &Job, at: i64) -> anyhow::Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("UPDATE qq_jobs SET state='accepted' WHERE id=?1", [&job.id])?;
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
    fn images_survive_restart_without_losing_message_or_conversation_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qq.db");
        let identity = QqConversation::new("1", QqPeerKind::Group, "2").unwrap();
        let sources = vec![
            ImageSource {
                url: Some("https://example.com/a.png".into()),
            },
            ImageSource { url: None },
        ];
        let mut db = Store::open(&path).unwrap();
        let message = Message {
            sequence: 0,
            message_id: "7".into(),
            sender: "3".into(),
            nickname: "name".into(),
            text: "photo".into(),
            images: sources.clone(),
            at: 1,
        };
        assert!(db.receive(&identity, &message, None).unwrap());
        drop(db);
        let mut db = Store::open(&path).unwrap();
        assert_eq!(db.recent_messages(&identity).unwrap()[0].images, sources);
        assert_eq!(db.message_sender(&identity, "7").unwrap(), "3");
        assert!(db.message_sender(&identity, "unknown").is_err());
        for other in [
            QqConversation::new("9", QqPeerKind::Group, "2").unwrap(),
            QqConversation::new("1", QqPeerKind::Group, "9").unwrap(),
            QqConversation::new("1", QqPeerKind::Friend, "2").unwrap(),
        ] {
            assert!(db.message_sender(&other, "7").is_err());
        }
        let changed = Message {
            images: Vec::new(),
            ..message
        };
        assert!(!db.receive(&identity, &changed, None).unwrap());
        assert_eq!(db.recent_messages(&identity).unwrap()[0].images, sources);
    }

    #[test]
    fn legacy_messages_migrate_without_inventing_visual_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qq.db");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE qq_messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, message_id TEXT NOT NULL, sender TEXT NOT NULL, nickname TEXT NOT NULL, body TEXT NOT NULL, at INTEGER NOT NULL, UNIQUE(account,kind,peer,message_id));
            INSERT INTO qq_messages(account,kind,peer,message_id,sender,nickname,body,at) VALUES('1','group','2','7','3','name','[图片 https://example.com/a.png]',1);").unwrap();
        drop(legacy);
        for _ in 0..2 {
            let db = Store::open(&path).unwrap();
            let identity = QqConversation::new("1", QqPeerKind::Group, "2").unwrap();
            assert!(db.recent_messages(&identity).unwrap()[0].images.is_empty());
            assert_eq!(db.message_sender(&identity, "7").unwrap(), "3");
        }
    }

    #[test]
    fn mention_image_selection_is_frozen_and_scoped_to_the_calling_sender() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Store::open(&dir.path().join("qq.db")).unwrap();
        let identity = QqConversation::new("1", QqPeerKind::Group, "2").unwrap();
        let mut message = Message {
            sequence: 0,
            message_id: "10".into(),
            sender: "3".into(),
            nickname: "caller".into(),
            text: "image".into(),
            images: vec![ImageSource {
                url: Some("https://example.com/first.png".into()),
            }],
            at: 1,
        };
        db.receive(&identity, &message, None).unwrap();
        message.message_id = "11".into();
        message.sender = "4".into();
        db.receive(&identity, &message, None).unwrap();
        message.message_id = "12".into();
        message.sender = "3".into();
        message.images.clear();
        message.text = "@1".into();
        db.receive(&identity, &message, Some("frozen mention"))
            .unwrap();
        let job = db.pending_jobs().unwrap().remove(0);
        message.message_id = "13".into();
        message.images = vec![ImageSource {
            url: Some("https://example.com/later.png".into()),
        }];
        db.receive(&identity, &message, None).unwrap();
        assert_eq!(
            db.input_image(&job, 0).unwrap(),
            Some(("10".into(), "3".into(), 1))
        );
        assert!(db.input_image(&job, job.watermark).unwrap().is_none());
        assert_eq!(db.message_watermark(&identity).unwrap(), job.watermark + 1);
        db.supersede_job(&job).unwrap();
        assert!(db.pending_jobs().unwrap().is_empty());
        assert_eq!(
            db.recent_messages(&identity).unwrap().len(),
            4,
            "reset boundaries preserve transport history"
        );
        let other = Job {
            identity: QqConversation::new("1", QqPeerKind::Group, "9").unwrap(),
            ..job
        };
        assert!(db.input_image(&other, 0).unwrap().is_none());
    }
    #[test]
    fn group_mentions_replace_pending_ambient_input_and_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qq.db");
        let identity = QqConversation::new("1", QqPeerKind::Group, "2").unwrap();
        let peer = Peer {
            identity: identity.clone(),
            title: "group".into(),
            process_id: "p".into(),
            session_id: "s".into(),
            root: dir.path().into(),
            watermark: 0,
            last_started: 0,
        };
        let mut db = Store::open(&path).unwrap();
        db.register(&peer).unwrap();
        let mut message = Message {
            sequence: 0,
            message_id: "7".into(),
            sender: "3".into(),
            nickname: "name".into(),
            text: "ordinary".into(),
            images: Vec::new(),
            at: 1,
        };
        db.receive(&identity, &message, None).unwrap();
        let watermark = db.messages(&identity, 0).unwrap()[0].sequence;
        db.prepare_job(&peer, watermark, "frozen ambient").unwrap();
        db.prepare_job(&peer, watermark + 1, "must not duplicate pending batch")
            .unwrap();
        assert_eq!(db.pending_jobs().unwrap().len(), 1);
        message.message_id = "8".into();
        message.text = "@1 reply please".into();
        assert!(db
            .receive(
                &identity,
                &message,
                Some("frozen direct request and context")
            )
            .unwrap());
        db.prepare_job(&peer, watermark + 1, "must not duplicate mention")
            .unwrap();
        drop(db);
        let mut db = Store::open(&path).unwrap();
        assert!(!db
            .receive(&identity, &message, Some("changed retry prompt"))
            .unwrap());
        let jobs = db.pending_jobs().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].trigger, Trigger::Direct);
        assert_eq!(jobs[0].prompt, "frozen direct request and context");
        assert_eq!(jobs[0].identity, identity);
        assert_eq!(
            db.db
                .query_row(
                    "SELECT state FROM qq_jobs WHERE trigger='ambient'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "superseded"
        );
        db.admitted(&jobs[0], 10).unwrap();
        assert!(db.pending_jobs().unwrap().is_empty());
        assert!(db
            .messages(&identity, db.peers().unwrap()[0].watermark)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn legacy_job_migration_preserves_identity_payload_and_admission() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("qq.db");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE qq_jobs(id TEXT PRIMARY KEY, account TEXT NOT NULL, kind TEXT NOT NULL, peer TEXT NOT NULL, watermark INTEGER NOT NULL, prompt TEXT NOT NULL, automatic INTEGER NOT NULL DEFAULT 0, admitted INTEGER NOT NULL DEFAULT 0);
            INSERT INTO qq_jobs VALUES('friend','1','friend','2',7,'friend prompt',0,0),('ambient','1','group','3',8,'ambient prompt',0,0),('proactive','1','group','4',9,'proactive prompt',1,0),('accepted','1','friend','5',10,'already accepted',0,1);").unwrap();
        drop(legacy);
        for _ in 0..2 {
            let db = Store::open(&path).unwrap();
            let jobs = db.pending_jobs().unwrap();
            assert_eq!(jobs.len(), 3);
            assert_eq!(jobs[0].id, "friend");
            assert_eq!(jobs[0].trigger, Trigger::Direct);
            assert_eq!(jobs[0].prompt, "friend prompt");
            assert_eq!(jobs[0].watermark, 7);
            assert_eq!(jobs[1].trigger, Trigger::Ambient);
            assert_eq!(jobs[2].trigger, Trigger::Proactive);
            assert_eq!(
                db.db
                    .query_row("SELECT state FROM qq_jobs WHERE id='accepted'", [], |r| {
                        r.get::<_, String>(0)
                    })
                    .unwrap(),
                "accepted"
            );
        }
    }
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
            images: Vec::new(),
            at: 1,
        };
        let mut db = Store::open(&path).unwrap();
        assert!(db
            .receive(&identity, &message, Some(&message.text))
            .unwrap());
        drop(db);
        let mut db = Store::open(&path).unwrap();
        assert!(!db
            .receive(&identity, &message, Some(&message.text))
            .unwrap());
        let jobs = db.pending_jobs().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].prompt, "do local work");
        assert_eq!(jobs[0].identity, identity);
        assert_eq!(jobs[0].trigger, Trigger::Direct);
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
            images: Vec::new(),
            at: 1,
        };
        assert!(db.receive(&identity, &message, None).unwrap());
        assert!(!db.receive(&identity, &message, None).unwrap());
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
