//! User-facing file receipts survive context compaction. Old successful deliver calls
//! are projected from paired typed facts; assistant prose is never delivery evidence.
use super::{SessionStore, StoreError, StoredEvent};

impl SessionStore {
    /// Successful structured handoff/Git calls are batch evidence, never file receipts.
    pub fn batch_tool_events(&self) -> Result<Vec<StoredEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT r.event_id, r.session_id, r.sequence, 'batch.tool_evidence',
                    json_object('name', json_extract(c.payload_json, '$.fact.name'),
                                'input', json_extract(c.payload_json, '$.fact.input'),
                                'content', json_extract(r.payload_json, '$.fact.content'),
                                'run_id', json_extract(c.payload_json, '$.turn_id')),
                    r.created_at
             FROM session_events c JOIN session_events r
               ON r.session_id = c.session_id AND r.event_type = 'session.tool_result_committed'
              AND r.sequence > c.sequence
              AND json_extract(r.payload_json, '$.turn_id') = json_extract(c.payload_json, '$.turn_id')
              AND json_extract(r.payload_json, '$.fact.call_id') = json_extract(c.payload_json, '$.fact.call_id')
             WHERE c.event_type = 'session.tool_called'
               AND json_extract(r.payload_json, '$.fact.is_error') = 0
               AND ((json_extract(c.payload_json, '$.fact.name') = 'work'
                     AND json_extract(c.payload_json, '$.fact.input.action') = 'handoff')
                 OR (json_extract(c.payload_json, '$.fact.name') = 'git'
                     AND json_extract(c.payload_json, '$.fact.input.action') IN ('commit', 'finalize')))
               AND NOT EXISTS (
                 SELECT 1 FROM session_events next_call
                 WHERE next_call.session_id = c.session_id AND next_call.event_type = c.event_type
                   AND next_call.sequence > c.sequence AND next_call.sequence < r.sequence
                   AND json_extract(next_call.payload_json, '$.turn_id') = json_extract(c.payload_json, '$.turn_id')
                   AND json_extract(next_call.payload_json, '$.fact.call_id') = json_extract(c.payload_json, '$.fact.call_id'))
             ORDER BY r.created_at DESC, r.sequence DESC LIMIT 200",
        )?;
        let rows = statement
            .query_map([], super::events::event_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delivery_events(&self) -> Result<Vec<StoredEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
             FROM session_events WHERE event_type IN ('file.delivered', 'file.delivery_managed')
             UNION ALL
             SELECT r.event_id, r.session_id, r.sequence, 'file.delivered.legacy',
                    json_object('input', json_extract(c.payload_json, '$.fact.input'),
                                'content', json_extract(r.payload_json, '$.fact.content'),
                                'run_id', json_extract(c.payload_json, '$.turn_id')),
                    r.created_at
             FROM session_events c JOIN session_events r
               ON r.session_id = c.session_id AND r.event_type = 'session.tool_result_committed'
              AND r.sequence > c.sequence
              AND json_extract(r.payload_json, '$.turn_id') = json_extract(c.payload_json, '$.turn_id')
              AND json_extract(r.payload_json, '$.fact.call_id') = json_extract(c.payload_json, '$.fact.call_id')
             WHERE c.event_type = 'session.tool_called'
               AND json_extract(c.payload_json, '$.fact.name') = 'deliver'
               AND json_extract(r.payload_json, '$.fact.is_error') = 0
               AND json_extract(r.payload_json, '$.fact.content') LIKE '[delivered] %'
               AND NOT EXISTS (
                 SELECT 1 FROM session_events next_call
                 WHERE next_call.session_id = c.session_id AND next_call.event_type = c.event_type
                   AND next_call.sequence > c.sequence AND next_call.sequence < r.sequence
                   AND json_extract(next_call.payload_json, '$.turn_id') = json_extract(c.payload_json, '$.turn_id')
                   AND json_extract(next_call.payload_json, '$.fact.call_id') = json_extract(c.payload_json, '$.fact.call_id'))
             ORDER BY created_at DESC, sequence DESC",
        )?;
        let rows = statement
            .query_map([], super::events::event_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into);
        rows
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    #[test]
    fn batch_evidence_requires_paired_structured_success_and_is_not_a_file_receipt() {
        let store = super::super::testutil::store();
        for (run, name, action, error) in [
            ("handoff", "work", "handoff", false),
            ("commit", "git", "commit", false),
            ("failed", "git", "commit", true),
            ("prose", "bash", "commit", false),
            ("claim", "work", "claim", false),
        ] {
            store.append_event("ses_test", "session.tool_called", &json!({"turn_id":run,"fact":{"call_id":"same","name":name,"input":{"action":action}}})).unwrap();
            store.append_event("ses_test", "session.tool_result_committed", &json!({"turn_id":run,"fact":{"call_id":"same","is_error":error,"content":"done"}})).unwrap();
        }
        // A result from another run must not complete the pending finalize call.
        store.append_event("ses_test", "session.tool_called", &json!({"turn_id":"pending","fact":{"call_id":"other","name":"git","input":{"action":"finalize"}}})).unwrap();
        store.append_event("ses_test", "session.tool_result_committed", &json!({"turn_id":"wrong","fact":{"call_id":"other","is_error":false,"content":"done"}})).unwrap();
        store
            .append_event("ses_test", "surface_replaced", &json!({"messages":[]}))
            .unwrap();
        let rows = store.batch_tool_events().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.payload["run_id"] == "handoff"));
        assert!(rows.iter().any(|row| row.payload["run_id"] == "commit"));
        assert!(store.delivery_events().unwrap().is_empty());
    }

    #[test]
    fn receipts_require_matching_success_and_survive_surface_replacement() {
        let store = super::super::testutil::store();
        for (run, name, error, content) in [
            ("one", "deliver", false, "[delivered] report.txt (4 bytes)"),
            ("two", "deliver", true, "[delivered] failed.txt (4 bytes)"),
            ("three", "bash", false, "[delivered] fake.txt (4 bytes)"),
        ] {
            store.append_event("ses_test", "session.tool_called", &json!({"turn_id":run,"fact":{"call_id":"same","name":name,"input":{"path":"report.txt"}}})).unwrap();
            store.append_event("ses_test", "session.tool_result_committed", &json!({"turn_id":run,"fact":{"call_id":"same","is_error":error,"content":content}})).unwrap();
        }
        store
            .append_event("ses_test", "surface_replaced", &json!({"messages":[]}))
            .unwrap();
        store
            .append_event("ses_test", "file.delivered", &json!({"path":"report.txt"}))
            .unwrap();
        let rows = store.delivery_events().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.payload["run_id"] == "one"));
    }
}
