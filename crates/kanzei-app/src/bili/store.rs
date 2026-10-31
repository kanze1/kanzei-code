use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

pub(crate) struct Store(Connection);
impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS drafts(id TEXT PRIMARY KEY,account INTEGER NOT NULL,bvid TEXT NOT NULL,aid INTEGER NOT NULL,root INTEGER,parent INTEGER,body TEXT NOT NULL,state TEXT NOT NULL,receipt TEXT,error TEXT,at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS browsing(id INTEGER PRIMARY KEY,account INTEGER NOT NULL,bvid TEXT NOT NULL,title TEXT NOT NULL,source TEXT NOT NULL,at INTEGER NOT NULL);").map_err(|e|e.to_string())?;
        db.execute("UPDATE drafts SET state='unknown',error='发送期间程序退出，结果未知；未自动重发' WHERE state='sending'",[]).map_err(|e|e.to_string())?;
        Ok(Self(db))
    }
    pub fn draft(
        &self,
        account: u64,
        bvid: &str,
        aid: u64,
        root: Option<u64>,
        parent: Option<u64>,
        body: &str,
    ) -> Result<Value, String> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        let id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        self.0.execute("INSERT INTO drafts(id,account,bvid,aid,root,parent,body,state,at) VALUES(?1,?2,?3,?4,?5,?6,?7,'draft',?8)",params![id,account,bvid,aid,root,parent,body,chrono::Utc::now().timestamp_millis()]).map_err(|e|e.to_string())?;
        self.get(&id)?.ok_or("草稿没有保存".into())
    }
    pub fn get(&self, id: &str) -> Result<Option<Value>, String> {
        self.0.query_row("SELECT id,account,bvid,aid,root,parent,body,state,receipt,error,at FROM drafts WHERE id=?1",[id],read_draft).optional().map_err(|e|e.to_string())
    }
    pub fn edit(&self, id: &str, body: &str, revision: &str) -> Result<(), String> {
        let old = self.get(id)?.ok_or("评论草稿不存在")?;
        if old["revision"] != revision || old["state"] != "draft" {
            return Err("草稿已改变或已发送，请刷新后查看".into());
        }
        let changed = self
            .0
            .execute(
                "UPDATE drafts SET body=?2 WHERE id=?1 AND body=?3 AND state='draft'",
                params![id, body, old["body"].as_str().ok_or("草稿正文无效")?],
            )
            .map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("草稿已被其他操作修改".into());
        }
        Ok(())
    }
    pub fn begin(
        &self,
        id: &str,
        account: u64,
        revision: &str,
        daily: u32,
        interval: u64,
    ) -> Result<Value, String> {
        let tx = self.0.unchecked_transaction().map_err(|e| e.to_string())?;
        let draft=tx.query_row("SELECT id,account,bvid,aid,root,parent,body,state,receipt,error,at FROM drafts WHERE id=?1",[id],read_draft).map_err(|e|e.to_string())?;
        if draft["account"] != account || draft["state"] != "draft" || draft["revision"] != revision
        {
            return Err("草稿账号、版本或发送状态已改变，未发送".into());
        }
        let now = chrono::Utc::now();
        let day = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .ok_or("日期无效")?
            .and_utc()
            .timestamp_millis();
        let (used,last):(u32,Option<i64>)=tx.query_row("SELECT COUNT(CASE WHEN at>=?2 THEN 1 END),MAX(at) FROM drafts WHERE account=?1 AND state!='draft' AND state!='discarded'",params![account,day],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        if used >= daily {
            return Err("今日评论发送次数已达到设置的上限".into());
        }
        if last.is_some_and(|at| now.timestamp_millis() - at < (interval as i64) * 1000) {
            return Err("评论发送间隔尚未结束".into());
        }
        tx.execute(
            "UPDATE drafts SET state='sending',at=?2 WHERE id=?1",
            params![id, now.timestamp_millis()],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(draft)
    }
    pub fn finish(
        &self,
        id: &str,
        state: &str,
        receipt: Option<&Value>,
        error: Option<&str>,
    ) -> Result<(), String> {
        self.0
            .execute(
                "UPDATE drafts SET state=?2,receipt=?3,error=?4 WHERE id=?1 AND state='sending'",
                params![id, state, receipt.map(Value::to_string), error],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn discard(&self, id: &str) -> Result<(), String> {
        if self
            .0
            .execute(
                "UPDATE drafts SET state='discarded' WHERE id=?1 AND state='draft'",
                [id],
            )
            .map_err(|e| e.to_string())?
            != 1
        {
            return Err("只能丢弃尚未发送的草稿".into());
        }
        Ok(())
    }
    pub fn browsed(
        &self,
        account: u64,
        bvid: &str,
        title: &str,
        source: &str,
    ) -> Result<(), String> {
        self.0
            .execute(
                "INSERT INTO browsing(account,bvid,title,source,at) VALUES(?1,?2,?3,?4,?5)",
                params![
                    account,
                    bvid,
                    title,
                    source,
                    chrono::Utc::now().timestamp_millis()
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn records(&self) -> Result<Value, String> {
        let mut query=self.0.prepare("SELECT id,account,bvid,aid,root,parent,body,state,receipt,error,at FROM drafts ORDER BY at DESC LIMIT 100").map_err(|e|e.to_string())?;
        let drafts = query
            .query_map([], read_draft)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut query = self
            .0
            .prepare("SELECT account,bvid,title,source,at FROM browsing ORDER BY at DESC LIMIT 100")
            .map_err(|e| e.to_string())?;
        let browsing=query.query_map([],|r|Ok(json!({"account":r.get::<_,u64>(0)?,"bvid":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,"source":r.get::<_,String>(3)?,"at":r.get::<_,i64>(4)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        Ok(json!({"drafts":drafts,"browsing":browsing}))
    }
}
fn read_draft(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let body: String = r.get(6)?;
    Ok(
        json!({"id":r.get::<_,String>(0)?,"account":r.get::<_,u64>(1)?,"bvid":r.get::<_,String>(2)?,"aid":r.get::<_,u64>(3)?,"root":r.get::<_,Option<u64>>(4)?,"parent":r.get::<_,Option<u64>>(5)?,"body":body,"revision":kanzei_base::content_hash(body.as_bytes()),"state":r.get::<_,String>(7)?,"receipt":r.get::<_,Option<String>>(8)?,"error":r.get::<_,Option<String>>(9)?,"at":r.get::<_,i64>(10)?}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sending_is_account_bound_revision_checked_and_never_replayed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bili.db");
        let db = Store::open(&path).unwrap();
        let draft = db.draft(1, "BV1234567890", 2, None, None, "hello").unwrap();
        let id = draft["id"].as_str().unwrap();
        let revision = draft["revision"].as_str().unwrap();
        assert!(db.begin(id, 9, revision, 10, 0).is_err());
        assert!(db.begin(id, 1, "wrong", 10, 0).is_err());
        db.begin(id, 1, revision, 10, 0).unwrap();
        assert!(db.begin(id, 1, revision, 10, 0).is_err());
        drop(db);
        let db = Store::open(&path).unwrap();
        assert_eq!(db.get(id).unwrap().unwrap()["state"], "unknown");
        assert!(db.begin(id, 1, revision, 10, 0).is_err());
    }
}
