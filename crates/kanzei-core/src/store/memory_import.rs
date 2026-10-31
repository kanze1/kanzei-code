//! Copy immutable episode evidence into a shared memory owner. Conversation
//! messages, inputs and process ownership remain in their original database.
use super::{SessionStore, StoreError};
use rusqlite::{params, params_from_iter, types::Value, OptionalExtension, TransactionBehavior};
use std::path::Path;
pub struct MemorySourceEvidence {
    pub episode_id: i64,
    pub event_start: Option<i64>,
    pub event_end: Option<i64>,
    pub source_hash: String,
}
impl SessionStore {
    pub fn import_memory_episode(
        &self,
        source_path: &Path,
        episode_id: i64,
    ) -> Result<i64, StoreError> {
        let source_path = source_path.canonicalize()?;
        let source = Self::open(&source_path)?;
        let mut values:Vec<Value>=source.connection.query_row("SELECT session_id,created_at,prompt_head,outcome,steps,input_tokens,output_tokens,tools_json,context_json,metrics_json,provider,model,run_id,input_id,duration_ms,overflow_json FROM episodes WHERE episode_id=?1",[episode_id],|row|(0..16).map(|i|row.get(i)).collect())?;
        let session = match &values[0] {
            Value::Text(s) => s.clone(),
            _ => return Err(StoreError::InvalidInput("episode has no session".into())),
        };
        let root = source_path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| StoreError::InvalidInput("source database has no owner".into()))?;
        self.create_session(&session, &root.display().to_string(), None)?;
        let source_key = source_path.to_string_lossy().to_string();
        let tx =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        if let Some(id)=tx.query_row("SELECT episode_id FROM episodes WHERE session_id=?1 AND json_valid(context_json) AND json_extract(context_json,'$.memory_origin.database')=?2 AND json_extract(context_json,'$.memory_origin.episode_id')=?3",params![session,source_key,episode_id],|r|r.get(0)).optional()?{return Ok(id);}
        let old = match &values[8] {
            Value::Text(s) => match serde_json::from_str::<serde_json::Value>(s) {
                Ok(value) => value,
                Err(_) => serde_json::json!({"source_context_raw":s}),
            },
            _ => {
                return Err(StoreError::InvalidInput(
                    "episode context is invalid".into(),
                ))
            }
        };
        let mut context = if old.is_object() {
            old
        } else {
            serde_json::json!({"source_context":old})
        };
        context["memory_origin"] =
            serde_json::json!({"database":source_key,"episode_id":episode_id});
        values[8] = Value::Text(serde_json::to_string(&context)?);
        tx.execute("INSERT INTO episodes(session_id,created_at,prompt_head,outcome,steps,input_tokens,output_tokens,tools_json,context_json,metrics_json,provider,model,run_id,input_id,duration_ms,overflow_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",params_from_iter(values))?;
        let imported = tx.last_insert_rowid();
        let mut query=source.connection.prepare("SELECT contract_version,fingerprint,failed_call_id,recovered_call_id,tool,target FROM memory_recoveries WHERE episode_id=?1")?;
        let rows = query.query_map([episode_id], |r| {
            (0..6)
                .map(|i| r.get::<_, Value>(i))
                .collect::<Result<Vec<_>, _>>()
        })?;
        for row in rows {
            let mut values = vec![Value::Integer(imported)];
            values.extend(row?);
            tx.execute("INSERT INTO memory_recoveries(episode_id,contract_version,fingerprint,failed_call_id,recovered_call_id,tool,target) VALUES(?1,?2,?3,?4,?5,?6,?7)",params_from_iter(values))?;
        }
        tx.commit()?;
        Ok(imported)
    }
    pub fn memory_source_evidence(
        &self,
        memory_id: &str,
    ) -> Result<Vec<MemorySourceEvidence>, StoreError> {
        let mut query=self.connection.prepare("SELECT episode_id,event_start,event_end,source_hash FROM memory_sources WHERE memory_id=?1")?;
        let rows = query
            .query_map([memory_id], |r| {
                Ok(MemorySourceEvidence {
                    episode_id: r.get(0)?,
                    event_start: r.get(1)?,
                    event_end: r.get(2)?,
                    source_hash: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_import_keeps_one_real_episode_without_copying_conversation_messages() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let source_path = a.path().join("state.db");
        let source = SessionStore::open(&source_path).unwrap();
        source.create_session("friend", "qq", None).unwrap();
        let id = source
            .append_episode(&crate::EpisodeRecord {
                session_id: "friend",
                prompt_head: "remember",
                outcome: "completed",
                steps: 1,
                input_tokens: 1,
                output_tokens: 1,
                tools_json: "{}",
                context_json: "{}",
                metrics_json: "{}",
                provider: "fixture",
                model: "test",
                run_id: "actual-run",
                input_id: "input",
                duration_ms: 10,
                overflow_json: "[]",
            })
            .unwrap();
        let target = SessionStore::open(&b.path().join("state.db")).unwrap();
        let copied = target.import_memory_episode(&source_path, id).unwrap();
        assert_eq!(
            target.import_memory_episode(&source_path, id).unwrap(),
            copied
        );
        assert!(target.episode_exists(copied).unwrap());
        assert_eq!(target.list_episodes("friend", 10).unwrap().len(), 1);
        assert!(target.import_memory_episode(&source_path, id + 1).is_err());
    }
}
