//! Non-modal question transport. Reading this projection never creates a runtime.
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use serde_json::{json, Value};
use tauri::{Emitter, State, Window};

use crate::{AppState, MutexPoisonExt, PendingAsk};

// A receipt confirms delivery to the waiting channel, not completion of the work.
static RECEIPTS: LazyLock<Mutex<BTreeMap<String, (Value, Value)>>> = LazyLock::new(Mutex::default);

fn question(id: u64, pending: &PendingAsk) -> Option<Value> {
    if !matches!(pending.request, kanzei_core::AskRequest::Question { .. }) {
        return None;
    }
    let mut value = crate::pending_ask_payload(id, pending);
    value["projectDir"] = json!(pending.project_root.display().to_string());
    value["revision"] = json!(kanzei_core::store::stable_json_hash(&value));
    Some(value)
}

#[tauri::command]
pub(crate) fn softwire_questions(state: State<'_, AppState>) -> Result<Vec<Value>, String> {
    let runtimes = state
        .runtimes
        .lock_or_recover()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let mut values: Vec<Value> = runtimes
        .iter()
        .flat_map(|runtime| {
            runtime
                .asks
                .lock_or_recover()
                .iter()
                .filter_map(|(id, ask)| question(*id, ask))
                .collect::<Vec<_>>()
        })
        .collect();
    for root in crate::durable_questions::roots() {
        for value in crate::durable_questions::pending(&root, None)? {
            if !values.iter().any(|v| v["id"] == value["id"]) {
                values.push(value);
            }
        }
    }
    Ok(values)
}

fn check_target(
    value: &Value,
    project: &Path,
    session: &str,
    revision: &str,
) -> Result<(), String> {
    let actual =
        crate::normalized_project_root(Path::new(value["projectDir"].as_str().unwrap_or("")));
    if actual != project || value["sessionId"] != session {
        return Err("question_target_mismatch: 问题不属于这个项目或对话".into());
    }
    if value["revision"] != revision {
        return Err("question_changed: 问题已变化，请重新查看后回复".into());
    }
    Ok(())
}

struct Reply<'a> {
    project: &'a Path,
    session: &'a str,
    id: u64,
    revision: &'a str,
    request_id: &'a str,
    text: &'a str,
}
impl Reply<'_> {
    fn identity(&self) -> Value {
        json!([
            self.project.display().to_string(),
            self.session,
            self.id,
            self.revision,
            self.text
        ])
    }
}
type ReceiptCache = BTreeMap<String, (Value, Value)>;
fn existing_receipt(receipts: &ReceiptCache, reply: &Reply<'_>) -> Result<Option<Value>, String> {
    if let Some((identity, receipt)) = receipts.get(reply.request_id) {
        if identity != &reply.identity() {
            return Err("question_request_conflict: 同一回复标识不能改写内容".into());
        }
        return Ok(Some(receipt.clone()));
    }
    Ok(None)
}
fn deliver_question(
    asks: &mut HashMap<u64, PendingAsk>,
    receipts: &mut ReceiptCache,
    reply: &Reply<'_>,
) -> Result<Value, String> {
    if let Some(receipt) = existing_receipt(receipts, reply)? {
        return Ok(receipt);
    }
    let value = asks
        .get(&reply.id)
        .and_then(|ask| question(reply.id, ask))
        .ok_or("question_expired: 问题已处理或已结束")?;
    check_target(&value, reply.project, reply.session, reply.revision)?;
    crate::durable_questions::remember_answer(
        reply.project,
        reply.id,
        reply.text,
        reply.request_id,
    )?;
    let pending = asks
        .remove(&reply.id)
        .ok_or("question_expired: 问题已结束")?;
    pending
        .sender
        .send(kanzei_core::AskResponse::Answer(reply.text.into()))
        .map_err(|_| "question_expired: 发起者已停止，回复未送达")?;
    let receipt = json!({"id":reply.id,"requestId":reply.request_id,"projectDir":reply.project.display().to_string(),
        "sessionId":reply.session,"status":"delivered"});
    if receipts.len() >= 512 {
        if let Some(key) = receipts.keys().next().cloned() {
            receipts.remove(&key);
        }
    }
    receipts.insert(reply.request_id.into(), (reply.identity(), receipt.clone()));
    Ok(receipt)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn softwire_answer_question(
    window: Window,
    state: State<'_, AppState>,
    project_dir: String,
    session_id: String,
    id: u64,
    expected_revision: String,
    request_id: String,
    reply: String,
) -> Result<Value, String> {
    if reply.trim().is_empty()
        || reply.chars().count() > 8000
        || request_id.is_empty()
        || request_id.len() > 128
    {
        return Err("question_invalid_reply: 回复不能为空，且须在 8000 字以内".into());
    }
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let reply = Reply {
        project: &root,
        session: &session_id,
        id,
        revision: &expected_revision,
        request_id: &request_id,
        text: &reply,
    };
    // A successful foreground reply has no durable record; check its receipt first.
    if let Some(receipt) = existing_receipt(&RECEIPTS.lock_or_recover(), &reply)? {
        return Ok(receipt);
    }
    let live = state
        .runtimes
        .lock_or_recover()
        .get(&session_id)
        .is_some_and(|r| r.asks.lock_or_recover().contains_key(&id));
    if !live {
        let record = kanzei_harness::pending_question::get(&root, id)?;
        check_target(
            &crate::durable_questions::projection(record["payload"].clone()),
            &root,
            &session_id,
            &expected_revision,
        )?;
        let record = kanzei_harness::pending_question::answer(&root, id, reply.text, &request_id)?;
        let receipt = crate::durable_questions::deliver(&window, &state, &root, record).await?;
        let _ = window.emit("kz:ask-resolved", json!({"id":id,"sessionId":session_id}));
        let _ = window.emit("kz:question-replied", &receipt);
        return Ok(receipt);
    }
    let mut receipts = RECEIPTS.lock_or_recover();
    if let Some(receipt) = existing_receipt(&receipts, &reply)? {
        return Ok(receipt);
    }
    let runtime = state
        .runtimes
        .lock_or_recover()
        .get(&session_id)
        .cloned()
        .ok_or("question_expired: 原对话已结束")?;
    let mut asks = runtime.asks.lock_or_recover();
    let receipt = deliver_question(&mut asks, &mut receipts, &reply)?;
    drop(asks);
    drop(receipts);
    let _ = window.emit("kz:question-replied", &receipt);
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_id_in_another_session_or_revision_cannot_consume_question() {
        let root = crate::normalized_project_root(&std::env::temp_dir());
        let value =
            json!({"projectDir":root.display().to_string(),"sessionId":"a","revision":"v1"});
        assert!(check_target(&value, &root, "a", "v1").is_ok());
        assert!(check_target(&value, &root, "b", "v1")
            .unwrap_err()
            .contains("target_mismatch"));
        assert!(check_target(&value, &root, "a", "v0")
            .unwrap_err()
            .contains("question_changed"));
    }

    #[test]
    fn question_projection_preserves_text_and_is_stable() {
        let (sender, _receiver) = tokio::sync::oneshot::channel();
        let pending = PendingAsk {
            source: String::new(),
            agent_id: None,
            sender,
            request: kanzei_core::AskRequest::Question {
                question: "先检查哪个？".into(),
                options: vec![kanzei_core::AskOption::plain("浏览器")],
                default: None,
                multiple: false,
                background: false,
                callback_id: None,
            },
            action: String::new(),
            resource: String::new(),
            project_root: std::env::temp_dir(),
            session_id: "a".into(),
        };
        let first = question(7, &pending).unwrap();
        assert_eq!(first, question(7, &pending).unwrap());
        assert_eq!(first["question"], "先检查哪个？");
        assert_ne!(
            first["revision"],
            question(8, &pending).unwrap()["revision"]
        );
    }

    fn pending(
        root: &Path,
    ) -> (
        PendingAsk,
        tokio::sync::oneshot::Receiver<kanzei_core::AskResponse>,
    ) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (
            PendingAsk {
                source: String::new(),
                agent_id: None,
                sender,
                request: kanzei_core::AskRequest::Question {
                    question: "先检查哪个？".into(),
                    options: vec![],
                    default: None,
                    multiple: false,
                    background: false,
                    callback_id: None,
                },
                action: String::new(),
                resource: String::new(),
                project_root: root.into(),
                session_id: "a".into(),
            },
            receiver,
        )
    }

    #[test]
    fn delivery_preserves_text_and_retry_does_not_consume_another_question() {
        let root = crate::normalized_project_root(&std::env::temp_dir());
        let (ask, mut receiver) = pending(&root);
        let revision = question(7, &ask).unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut asks = HashMap::from([(7, ask)]);
        let mut receipts = ReceiptCache::new();
        let text = "  中文回复\n不要改写  ";
        let mut reply = Reply {
            project: &root,
            session: "a",
            id: 7,
            revision: &revision,
            request_id: "r1",
            text,
        };
        reply.session = "wrong";
        assert!(deliver_question(&mut asks, &mut receipts, &reply)
            .unwrap_err()
            .contains("target_mismatch"));
        assert!(asks.contains_key(&7));
        assert!(receiver.try_recv().is_err());
        reply.session = "a";
        reply.revision = "stale";
        assert!(deliver_question(&mut asks, &mut receipts, &reply)
            .unwrap_err()
            .contains("question_changed"));
        assert!(asks.contains_key(&7));
        reply.revision = &revision;
        let receipt = deliver_question(&mut asks, &mut receipts, &reply).unwrap();
        assert!(
            matches!(receiver.try_recv().unwrap(), kanzei_core::AskResponse::Answer(answer) if answer == text)
        );
        assert_eq!(receipt["status"], "delivered");
        let (next, mut next_receiver) = pending(&root);
        asks.insert(7, next);
        assert_eq!(
            receipt,
            deliver_question(&mut asks, &mut receipts, &reply).unwrap()
        );
        assert!(next_receiver.try_recv().is_err());
        assert!(asks.contains_key(&7));
        reply.text = "different";
        assert!(deliver_question(&mut asks, &mut receipts, &reply)
            .unwrap_err()
            .contains("request_conflict"));
    }

    #[test]
    fn closed_sender_does_not_produce_a_delivery_receipt() {
        let root = crate::normalized_project_root(&std::env::temp_dir());
        let (ask, receiver) = pending(&root);
        let revision = question(7, &ask).unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut asks = HashMap::from([(7, ask)]);
        let mut receipts = ReceiptCache::new();
        drop(receiver);
        let reply = Reply {
            project: &root,
            session: "a",
            id: 7,
            revision: &revision,
            request_id: "r1",
            text: "reply",
        };
        assert!(deliver_question(&mut asks, &mut receipts, &reply)
            .unwrap_err()
            .contains("未送达"));
        assert!(receipts.is_empty());
    }
}
