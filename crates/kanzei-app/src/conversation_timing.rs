//! Fixed round receipts. Join engine identities, never infer turns from message order.
use std::collections::HashMap;

use kanzei_core::{SessionStore, StoredEvent};
use serde_json::{json, Value};

// Admission commits with session_inputs. This is earlier than promotion, baseline
// preparation and the runner Instant, and remains valid when an input was queued.
fn admissions(store: &SessionStore, session: &str) -> Result<HashMap<String, i64>, String> {
    Ok(store
        .list_events_by_type(session, 0, "prompt.admitted")
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|event| event.session_id == session)
        .filter_map(|event| {
            let input = event.payload["input_id"].as_str()?.to_owned();
            (!input.is_empty()).then_some((input, event.created_at))
        })
        .collect())
}

fn elapsed_since(admitted_at_ms: i64, ended_at_ms: i64) -> Option<u64> {
    if admitted_at_ms <= 0 {
        return None;
    }
    u64::try_from(ended_at_ms.checked_sub(admitted_at_ms)?).ok()
}

fn admission_elapsed_at(
    store: &SessionStore,
    session: &str,
    input: &str,
    ended_at_ms: i64,
) -> Result<Option<u64>, String> {
    Ok(admissions(store, session)?
        .get(input)
        .and_then(|start| elapsed_since(*start, ended_at_ms)))
}

/// Freeze the admission-to-terminal wall duration immediately before committing
/// a terminal. Missing identity, database failures or a backwards clock never
/// fall back to the shorter runner clock or block the actual outcome commit.
pub(crate) fn with_admission_elapsed(
    store: &SessionStore,
    session: &str,
    input: &str,
    mut payload: Value,
    ended_at_ms: i64,
) -> Value {
    match admission_elapsed_at(store, session, input, ended_at_ms) {
        Ok(Some(ms)) => {
            payload["elapsed_ms"] = json!(ms);
            payload["elapsed_source"] = json!("admission_wall");
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(%error, session, input, "could not resolve input admission time")
        }
    }
    payload
}

fn receipt(event: &StoredEvent, starts: &HashMap<String, Value>) -> Option<Value> {
    let run_id = event.payload["run_id"]
        .as_str()
        .filter(|id| !id.is_empty())?;
    let input_id = event.payload["input_id"]
        .as_str()
        .filter(|id| !id.is_empty())?;
    let (elapsed_ms, source) = if let Some(ms) = event.payload["elapsed_ms"].as_u64() {
        (
            ms,
            event.payload["elapsed_source"]
                .as_str()
                .unwrap_or("backend_elapsed"),
        )
    } else {
        let facts = starts.get(run_id)?;
        if facts["input_id"].as_str() != Some(input_id) {
            return None;
        }
        let ms = elapsed_since(facts["admitted_at_ms"].as_i64()?, event.created_at)?;
        (ms, "admission_wall")
    };
    let outcome = if event.event_type == "run.failed" {
        "failed"
    } else if event.payload["halted_by_user"] == true {
        "halted"
    } else {
        event.payload["outcome"].as_str().unwrap_or("completed")
    };
    Some(json!({
        "kind": "turn.elapsed", "runId": run_id, "inputId": input_id,
        "sessionId": event.session_id, "elapsedMs": elapsed_ms,
        "outcome": outcome, "source": source, "sequence": event.sequence,
    }))
}

pub(super) fn load(
    store: &SessionStore,
    session: &str,
    start: i64,
    end: i64,
) -> Result<Vec<Value>, String> {
    let admitted = admissions(store, session)?;
    let starts = store
        .list_events_by_type(session, start, "run.facts")
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|event| event.sequence <= end)
        .filter_map(|event| {
            let mut facts = event.payload;
            let run = facts["run_id"].as_str()?.to_owned();
            let input = facts["input_id"].as_str()?;
            let admitted_at = admitted.get(input)?;
            facts["admitted_at_ms"] = json!(admitted_at);
            Some((run, facts))
        })
        .collect();
    let mut terminal = Vec::new();
    for kind in ["run.completed", "run.failed"] {
        terminal.extend(
            store
                .list_events_by_type(session, start, kind)
                .map_err(|e| e.to_string())?,
        );
    }
    terminal.sort_by_key(|event| event.sequence);
    Ok(terminal
        .iter()
        .filter(|event| event.sequence <= end)
        .filter_map(|event| receipt(event, &starts))
        .collect())
}

pub(crate) fn current(
    store: &SessionStore,
    session: &str,
    run: &str,
) -> Result<Option<Value>, String> {
    for kind in ["run.completed", "run.failed"] {
        if let Some(event) = store
            .latest_event(session, kind)
            .map_err(|e| e.to_string())?
        {
            if event.payload["run_id"].as_str() == Some(run) {
                return Ok(receipt(&event, &HashMap::new()));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terminal(kind: &str, payload: Value) -> StoredEvent {
        StoredEvent {
            event_id: "event".into(),
            session_id: "session".into(),
            sequence: 4,
            event_type: kind.into(),
            payload,
            created_at: 1704067204500,
        }
    }
    #[test]
    fn elapsed_receipts_require_identity_and_reliable_time() {
        let event = terminal("run.completed", json!({"run_id":"r", "input_id":"i"}));
        assert!(receipt(&event, &HashMap::new()).is_none());
        let mut starts = HashMap::from([(
            "r".into(),
            json!({"input_id":"wrong", "admitted_at_ms":1704067200000_i64}),
        )]);
        assert!(receipt(&event, &starts).is_none());
        starts.get_mut("r").unwrap()["input_id"] = json!("i");
        assert_eq!(receipt(&event, &starts).unwrap()["elapsedMs"], 4500);
        let event = terminal(
            "run.failed",
            json!({"run_id":"r", "input_id":"i", "elapsed_ms":1250}),
        );
        let result = receipt(&event, &starts).unwrap();
        assert_eq!(result["elapsedMs"], 1250);
        assert_eq!(result["outcome"], "failed");
        assert_eq!(result["source"], "backend_elapsed");
        let event = terminal(
            "run.completed",
            json!({"run_id":"r", "input_id":"i", "elapsed_ms":0, "halted_by_user":true}),
        );
        assert_eq!(receipt(&event, &starts).unwrap()["outcome"], "halted");
        assert!(receipt(
            &terminal("run.completed", json!({"elapsed_ms":100})),
            &starts
        )
        .is_none());
    }
    #[test]
    fn elapsed_receipt_includes_admitted_preparation_time() {
        let store = SessionStore::open_in_memory().unwrap();
        for session in ["session", "other"] {
            store.create_session(session, "fixture", None).unwrap();
        }
        store
            .admit_input("session", "i", "queued input", kanzei_core::Delivery::Queue)
            .unwrap();
        let admission = store
            .latest_event("session", "prompt.admitted")
            .unwrap()
            .unwrap();
        // A deterministic slow baseline finishes before the runner Instant exists.
        let runner_started_at = admission.created_at + 5000;
        let terminal_at = runner_started_at + 750;
        let payload = with_admission_elapsed(
            &store,
            "session",
            "i",
            json!({"run_id":"r", "input_id":"i"}),
            terminal_at,
        );
        assert_eq!(payload["elapsed_ms"], 5750);
        assert_eq!(payload["elapsed_source"], "admission_wall");
        let mut event = terminal("run.completed", payload);
        event.created_at = terminal_at;
        let result = receipt(&event, &HashMap::new()).unwrap();
        assert_eq!(
            result["elapsedMs"], 5750,
            "must include preparation, not just the 750ms runner"
        );
        assert_eq!(result["source"], "admission_wall");
        // Exact session/input ownership and backwards clocks must not manufacture time.
        for (session, input, end) in [
            ("other", "i", terminal_at),
            ("session", "missing", terminal_at),
            ("session", "i", admission.created_at - 1),
        ] {
            let payload = with_admission_elapsed(&store, session, input, json!({}), end);
            assert!(payload.get("elapsed_ms").is_none());
        }
    }
    #[test]
    fn elapsed_receipts_respect_session_and_segment_bounds() {
        let store = SessionStore::open_in_memory().unwrap();
        for session in ["a", "b"] {
            store.create_session(session, "fixture", None).unwrap();
        }
        store
            .append_event(
                "a",
                "run.completed",
                &json!({"run_id":"old", "input_id":"i", "elapsed_ms":10}),
            )
            .unwrap();
        let floor = store
            .append_event("a", "conversation.reset", &json!({}))
            .unwrap()
            .sequence;
        let end = store
            .append_event(
                "a",
                "run.failed",
                &json!({"run_id":"new", "input_id":"j", "elapsed_ms":20}),
            )
            .unwrap()
            .sequence;
        store
            .append_event(
                "a",
                "run.completed",
                &json!({"run_id":"later", "input_id":"k", "elapsed_ms":30}),
            )
            .unwrap();
        store
            .append_event(
                "b",
                "run.completed",
                &json!({"run_id":"other", "input_id":"l", "elapsed_ms":40}),
            )
            .unwrap();
        let result = load(&store, "a", floor, end).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["runId"], "new");
        assert_eq!(result[0]["sessionId"], "a");
    }
}
