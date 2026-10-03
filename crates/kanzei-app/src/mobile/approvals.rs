//! R-270 批3:移动端 approval 通道(pending 列表 + 回答)。
//!
//! 由 mobile.rs 拆出:这一段只和「runtime 的 ask 表 ↔ 配对设备」有关,不碰 HTTP 解析、
//! 配对与 SSE。回答仍走 runner 既有 ask 流,最终门禁在 harness 侧。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::{project_display_name, project_sessions_if_present, BridgeSession};
use crate::state::SessionRuntime;
use crate::MutexPoisonExt;

/// 完整资源/命令下发的字符上限(超出截断并带 `truncated: true`,免得一条超长 heredoc 撑爆响应)。
const APPROVAL_FULL_LIMIT: usize = 4000;

/// 按字符数截断;返回 (文本, 是否被截断)。
fn truncate_chars(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((byte_index, _)) => (text[..byte_index].to_string(), true),
        None => (text.to_string(), false),
    }
}

/// bash 权限资源 → (完整命令, 工作目录)。资源是 `{"command","workdir"}` JSON,
/// 没写 workdir 时是裸命令文本。
fn bash_command_parts(resource: &str) -> (String, Option<String>) {
    let trimmed = resource.trim();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(command) = value.get("command").and_then(|value| value.as_str()) {
                let workdir = value
                    .get("workdir")
                    .and_then(|value| value.as_str())
                    .filter(|workdir| !workdir.is_empty())
                    .map(str::to_string);
                return (command.to_string(), workdir);
            }
        }
    }
    (resource.to_string(), None)
}

/// R-270 批3:approval pending 列表——遍历所有 runtime 的 asks。
/// `resource` 仍是截断到 80 字符的摘要;question 为让配对设备提交答案,显式投影
/// question/options/default/multiple。
///
/// UX-019:摘要截断会把 bash 的 `{"command","workdir"}` JSON 切成残片,手机上只能盲批。
/// 权限请求另带 `resource_full`(完整资源,封顶 4000 字),bash 再拆出完整的 `command` /
/// `workdir`;PWA 以它们为准。
/// UX-134:每条带 `project` 与 `session_label`(项目名 + 主对话/独立任务名),卡片不再只有 ses_ 哈希。
pub(super) fn approval_pending_list(
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
) -> serde_json::Value {
    struct Snapshot {
        id: u64,
        request: kanzei_core::AskRequest,
        project_root: PathBuf,
        session_id: String,
    }
    // 锁内只拷字段;下面补会话名要读状态库,不该占着 ask 表的锁。
    let mut snapshots = Vec::new();
    {
        let runtimes = runtimes.lock_or_recover();
        for runtime in runtimes.values() {
            let asks = runtime.asks.lock_or_recover();
            for (id, pending) in asks.iter() {
                snapshots.push(Snapshot {
                    id: *id,
                    request: pending.request.clone(),
                    project_root: pending.project_root.clone(),
                    session_id: pending.session_id.clone(),
                });
            }
        }
    }
    snapshots.sort_by_key(|snapshot| snapshot.id);
    let mut sessions_by_project: HashMap<PathBuf, Vec<BridgeSession>> = HashMap::new();
    let mut items = Vec::new();
    for snapshot in snapshots {
        let (kind, action, resource) = match &snapshot.request {
            kanzei_core::AskRequest::Permission { action, resource } => {
                ("permission", action.clone(), resource.clone())
            }
            kanzei_core::AskRequest::Question { question, .. } => {
                ("question", "question".into(), question.clone())
            }
        };
        // 权限资源与问题摘要仍脱敏截断;完整 question 只通过单独字段给已配对设备答题。
        let (summary, cut) = truncate_chars(&resource, 80);
        let summary = if cut {
            format!("{summary}…")
        } else {
            summary
        };
        let sessions = sessions_by_project
            .entry(snapshot.project_root.clone())
            .or_insert_with(|| project_sessions_if_present(&snapshot.project_root));
        let session_label = sessions
            .iter()
            .find(|session| session.session_id == snapshot.session_id)
            .map(|session| session.label.clone());
        let mut item = json!({
            "id": snapshot.id,
            "kind": kind,
            "action": action,
            "resource": summary,
            "session_id": snapshot.session_id,
            "project": project_display_name(&snapshot.project_root),
            "session_label": session_label,
        });
        match &snapshot.request {
            kanzei_core::AskRequest::Permission { action, resource } => {
                let (full, cut) = truncate_chars(resource, APPROVAL_FULL_LIMIT);
                item["resource_full"] = json!(full);
                item["truncated"] = json!(cut);
                if action == "bash" {
                    let (command, workdir) = bash_command_parts(resource);
                    let (command, command_cut) = truncate_chars(&command, APPROVAL_FULL_LIMIT);
                    item["command"] = json!(command);
                    item["workdir"] = json!(workdir);
                    item["truncated"] = json!(cut || command_cut);
                }
            }
            kanzei_core::AskRequest::Question {
                question,
                options,
                default,
                multiple,
                ..
            } => {
                item["question"] = json!(question);
                item["options"] = json!(options);
                item["default"] = json!(default);
                item["multiple"] = json!(multiple);
            }
        }
        items.push(item);
    }
    json!({ "pending": items, "count": items.len() })
}

/// UX-142:「总是允许」——与桌面端 `answer_ask` 的 always 同一条持久化路径(把这条精确
/// 资源写进项目放行规则)。先于摘除 ask 执行:规则没存成就报错、ask 留着,用户还能改点
/// 「批准」,不会悄悄变成拒绝。ask 不存在返回 Ok,交给后面统一报「不存在或已回答」。
fn remember_always_allow(
    runtimes: &HashMap<String, Arc<SessionRuntime>>,
    id: u64,
) -> Result<(), String> {
    let found = runtimes.values().find_map(|runtime| {
        let asks = runtime.asks.lock_or_recover();
        let pending = asks.get(&id)?;
        Some(match &pending.request {
            kanzei_core::AskRequest::Permission { .. } => Ok((
                pending.project_root.clone(),
                pending.action.clone(),
                pending.resource.clone(),
            )),
            kanzei_core::AskRequest::Question { .. } => {
                Err("允许并记住只适用于权限请求".to_string())
            }
        })
    });
    match found {
        None => Ok(()),
        Some(Err(error)) => Err(error),
        Some(Ok((root, action, resource))) => {
            crate::commands::run::persist_always_allow(&root, &action, &resource)
                .map(|_| ())
                .map_err(|error| format!("规则保存失败:{error}"))
        }
    }
}

/// R-270 批3:回答一个 pending ask。`reply` 取值:
/// - permission: "allow"(仅此一次)| "always"(总是允许,UX-142)| "deny";
/// - question: 任意文本原样作为答案(包括字面量 "cancel")。
///
/// 取消走显式字段 `cancel: true`(D-751 跟进):question 回 Cancelled,
/// permission 按拒绝处理;不再与答案文本共用带内 "cancel" 字符串。
///
/// 回答走 runner 既有 ask 流:找到 PendingAsk 后通过 sender 发送 AskResponse,
/// runner 的权限门禁按回复放行/拒绝——本通道不新增能力面,只回答既有询问。
pub(super) fn approval_answer(
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = payload
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "answer 需要 id(数字)".to_string())?;
    let cancel = payload.get("cancel").and_then(|v| v.as_bool()) == Some(true);
    let reply = payload
        .get("reply")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    // 校验在摘除 pending 之前:格式错误的请求不消耗 ask。
    if !cancel && reply.is_empty() {
        return Err("answer 需要 reply(allow|always|deny 或回答文本)或 cancel: true".to_string());
    }
    let runtimes = runtimes.lock_or_recover();
    if !cancel && reply == "always" {
        remember_always_allow(&runtimes, id)?;
    }
    let mut found = None;
    for runtime in runtimes.values() {
        let mut asks = runtime.asks.lock_or_recover();
        if let Some(pending) = asks.get(&id) {
            crate::durable_questions::remember_response(
                &pending.project_root,
                id,
                &reply,
                &format!("mobile:{id}"),
                cancel,
            )?;
        }
        if let Some(pending) = asks.remove(&id) {
            found = Some(pending);
            break;
        }
    }
    let pending = found.ok_or_else(|| format!("ask {id} 不存在或已回答"))?;
    let response = match &pending.request {
        kanzei_core::AskRequest::Question { .. } if cancel => kanzei_core::AskResponse::Cancelled,
        kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Answer(reply),
        kanzei_core::AskRequest::Permission { .. } => {
            // 兼容既有协议:allow 放行一次、always 总是允许,其余(含 deny、cancel: true)一律拒绝。
            let decision = match reply.as_str() {
                "allow" if !cancel => kanzei_core::AskReply::AllowOnce,
                "always" if !cancel => kanzei_core::AskReply::AlwaysAllow,
                _ => kanzei_core::AskReply::Deny,
            };
            kanzei_core::AskResponse::Permission(decision)
        }
    };
    pending
        .sender
        .send(response)
        .map_err(|_| format!("ask {id} 的接收端已关闭(runner 已取消该询问)"))?;
    Ok(json!({ "answered": id, "session_id": pending.session_id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobile::tests::{register_line, temp_project};
    use std::path::Path;

    /// R-270 批3:approval pending 列表——遍历所有 runtime 的 asks 产出脱敏摘要,
    /// resource 截断不暴露完整内容。
    #[test]
    fn approval_pending_列出脱敏摘要() {
        let runtime = Arc::new(SessionRuntime::default());
        let (_sender, _rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock_or_recover().insert(
            7,
            crate::PendingAsk {
                source: String::new(),
                agent_id: None,
                sender: _sender,
                request: kanzei_core::AskRequest::Permission {
                    action: "bash".into(),
                    resource: "long-resource-".repeat(30),
                },
                action: "bash".into(),
                resource: "long-resource-".repeat(30),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtimes = HashMap::new();
        runtimes.insert("ses-1".to_string(), runtime);
        let runtimes = Arc::new(Mutex::new(runtimes));

        let list = approval_pending_list(&runtimes);
        assert_eq!(list["count"], 1, "应列出 1 条 pending");
        assert_eq!(list["pending"][0]["id"], 7);
        assert_eq!(list["pending"][0]["kind"], "permission");
        assert_eq!(list["pending"][0]["action"], "bash");
        // 脱敏:resource 截断到 80 字符。
        let resource = list["pending"][0]["resource"].as_str().unwrap();
        assert!(
            resource.chars().count() <= 81,
            "resource 必须截断: {resource}"
        );
    }

    /// D-751:question 桥接返回完整问题/选项契约,并原样投递真实答案文本。
    #[test]
    fn approval_pending_question_fields_and_answer_roundtrip() {
        let runtime = Arc::new(SessionRuntime::default());
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let question = "问题".repeat(50);
        runtime.asks.lock_or_recover().insert(
            8,
            crate::PendingAsk {
                source: String::new(),
                agent_id: None,
                sender: tx,
                request: kanzei_core::AskRequest::Question {
                    question: question.clone(),
                    options: vec![kanzei_core::AskOption {
                        label: "方案 A".into(),
                        note: Some("适用于本次部署".into()),
                    }],
                    default: Some("方案 A".into()),
                    multiple: true,
                    background: false,
                    callback_id: None,
                },
                action: "question".into(),
                resource: question.clone(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtime_map = HashMap::new();
        runtime_map.insert("ses-1".to_string(), runtime);
        let runtimes = Arc::new(Mutex::new(runtime_map));

        let pending = approval_pending_list(&runtimes);
        let ask = &pending["pending"][0];
        assert_eq!(ask["kind"], "question");
        assert_eq!(ask["question"], question);
        assert_eq!(ask["options"][0]["label"], "方案 A");
        assert_eq!(ask["options"][0]["note"], "适用于本次部署");
        assert_eq!(ask["default"], "方案 A");
        assert_eq!(ask["multiple"], true);
        assert!(ask["resource"].as_str().unwrap().chars().count() <= 81);

        let answer = "方案 A\n补充说明";
        let response = approval_answer(&runtimes, &json!({"id": 8, "reply": answer})).unwrap();
        assert_eq!(response["answered"], 8);
        match rx.try_recv() {
            Ok(kanzei_core::AskResponse::Answer(text)) => assert_eq!(text, answer),
            other => panic!("question 应原样收到用户文本答案,实际: {other:?}"),
        }
    }

    /// R-270 批3:approval answer——permission allow/deny 经 sender 送达 runner,
    /// 门禁决策由 runner 侧执行(本通道不旁路)。
    #[test]
    fn approval_answer_permission放行与拒绝送达() {
        let runtime = Arc::new(SessionRuntime::default());
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock_or_recover().insert(
            3,
            crate::PendingAsk {
                source: String::new(),
                agent_id: None,
                sender: tx,
                request: kanzei_core::AskRequest::Permission {
                    action: "bash".into(),
                    resource: "cargo build".into(),
                },
                action: "bash".into(),
                resource: "cargo build".into(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtimes = HashMap::new();
        runtimes.insert("ses-1".to_string(), runtime.clone());
        let runtimes = Arc::new(Mutex::new(runtimes));

        // allow → AllowOnce 送达。
        let answered = approval_answer(&runtimes, &json!({"id": 3, "reply": "allow"})).unwrap();
        assert_eq!(answered["answered"], 3);
        match rx.try_recv() {
            Ok(kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)) => {}
            other => panic!("allow 应送达 AllowOnce,实得: {other:?}"),
        }
        // 回答后 pending 移除:再答报不存在。
        let err = approval_answer(&runtimes, &json!({"id": 3, "reply": "deny"})).unwrap_err();
        assert!(err.contains("不存在或已回答"), "{err}");
    }

    /// R-270 批3:approval answer——deny 送达 Deny;question 回答送 Answer 文本。
    #[test]
    fn approval_answer_拒绝与问题回答() {
        let runtime = Arc::new(SessionRuntime::default());
        // deny 场景。
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock_or_recover().insert(
            4,
            crate::PendingAsk {
                source: String::new(),
                agent_id: None,
                sender: tx,
                request: kanzei_core::AskRequest::Permission {
                    action: "bash".into(),
                    resource: "rm -rf".into(),
                },
                action: "bash".into(),
                resource: "rm -rf".into(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        // question 场景。
        let (tx2, mut rx2) = tokio::sync::oneshot::channel();
        runtime.asks.lock_or_recover().insert(
            5,
            crate::PendingAsk {
                source: String::new(),
                agent_id: None,
                sender: tx2,
                request: kanzei_core::AskRequest::Question {
                    question: "确认继续?".into(),
                    options: vec!["是".into(), "否".into()],
                    default: None,
                    multiple: false,
                    background: false,
                    callback_id: None,
                },
                action: "question".into(),
                resource: "确认继续?".into(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtimes = HashMap::new();
        runtimes.insert("ses-1".to_string(), runtime.clone());
        let runtimes = Arc::new(Mutex::new(runtimes));

        approval_answer(&runtimes, &json!({"id": 4, "reply": "deny"})).unwrap();
        match rx.try_recv() {
            Ok(kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny)) => {}
            other => panic!("deny 应送达 Deny,实得: {other:?}"),
        }

        approval_answer(&runtimes, &json!({"id": 5, "reply": "是"})).unwrap();
        match rx2.try_recv() {
            Ok(kanzei_core::AskResponse::Answer(text)) => assert_eq!(text, "是"),
            other => panic!("question 回答应送达 Answer,实得: {other:?}"),
        }
    }

    /// D-751 跟进:取消走显式 `cancel: true`;字面量 "cancel" 是合法答案文本;
    /// permission 收到 cancel 按拒绝处理;缺 reply 又未取消的请求不消耗 ask。
    #[test]
    fn approval_answer_显式取消与字面量cancel答案() {
        let runtime = Arc::new(SessionRuntime::default());
        let mut receivers = Vec::new();
        for id in [9u64, 10] {
            let (tx, rx) = tokio::sync::oneshot::channel();
            runtime.asks.lock_or_recover().insert(
                id,
                crate::PendingAsk {
                    source: String::new(),
                    agent_id: None,
                    sender: tx,
                    request: kanzei_core::AskRequest::Question {
                        question: "是否继续?".into(),
                        options: vec!["cancel".into(), "继续".into()],
                        default: None,
                        multiple: false,
                        background: false,
                        callback_id: None,
                    },
                    action: "question".into(),
                    resource: "是否继续?".into(),
                    project_root: PathBuf::from("."),
                    session_id: "ses-1".into(),
                },
            );
            receivers.push(rx);
        }
        let (tx, mut permission_rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock_or_recover().insert(
            11,
            crate::PendingAsk {
                source: String::new(),
                agent_id: None,
                sender: tx,
                request: kanzei_core::AskRequest::Permission {
                    action: "bash".into(),
                    resource: "cargo test".into(),
                },
                action: "bash".into(),
                resource: "cargo test".into(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtime_map = HashMap::new();
        runtime_map.insert("ses-1".to_string(), runtime.clone());
        let runtimes = Arc::new(Mutex::new(runtime_map));

        // 既无 reply 也未取消:报错且 ask 仍在 pending。
        let err = approval_answer(&runtimes, &json!({"id": 9})).unwrap_err();
        assert!(err.contains("cancel: true"), "{err}");
        assert!(runtime.asks.lock_or_recover().contains_key(&9));

        approval_answer(&runtimes, &json!({"id": 9, "cancel": true})).unwrap();
        match receivers[0].try_recv() {
            Ok(kanzei_core::AskResponse::Cancelled) => {}
            other => panic!("cancel: true 应送达 Cancelled,实得: {other:?}"),
        }

        approval_answer(&runtimes, &json!({"id": 10, "reply": "cancel"})).unwrap();
        match receivers[1].try_recv() {
            Ok(kanzei_core::AskResponse::Answer(text)) => assert_eq!(text, "cancel"),
            other => panic!("字面量 cancel 应作为答案送达,实得: {other:?}"),
        }

        approval_answer(
            &runtimes,
            &json!({"id": 11, "reply": "allow", "cancel": true}),
        )
        .unwrap();
        match permission_rx.try_recv() {
            Ok(kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny)) => {}
            other => panic!("permission 收到 cancel 应按拒绝送达,实得: {other:?}"),
        }
    }

    fn pending_permission(
        action: &str,
        resource: &str,
        root: &Path,
        session_id: &str,
    ) -> (
        crate::PendingAsk,
        tokio::sync::oneshot::Receiver<kanzei_core::AskResponse>,
    ) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let pending = crate::PendingAsk {
            source: String::new(),
            agent_id: None,
            sender,
            request: kanzei_core::AskRequest::Permission {
                action: action.into(),
                resource: resource.into(),
            },
            action: action.into(),
            resource: resource.into(),
            project_root: root.to_path_buf(),
            session_id: session_id.into(),
        };
        (pending, receiver)
    }

    /// UX-019:bash 审批下发完整 command/workdir(摘要仍截断);UX-134:带项目名与会话名。
    #[test]
    fn approval_pending_bash下发完整命令与来源名() {
        let root = temp_project("approval-bash");
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        let line = register_line(&store, &root, 3, None);
        drop(store);
        let command = format!(
            "cargo test --workspace -- {}",
            "very-long-filter ".repeat(30)
        );
        let resource = json!({"command": command, "workdir": "C:/work/app"}).to_string();
        let (pending, _receiver) = pending_permission("bash", &resource, &root, &line);
        let runtime = Arc::new(SessionRuntime::default());
        runtime.asks.lock_or_recover().insert(5, pending);
        let mut table = HashMap::new();
        table.insert(line.clone(), runtime);
        let list = approval_pending_list(&Arc::new(Mutex::new(table)));

        let ask = &list["pending"][0];
        assert_eq!(ask["command"], command, "bash 命令必须完整下发");
        assert_eq!(ask["workdir"], "C:/work/app");
        assert_eq!(ask["truncated"], false);
        assert!(
            ask["resource"].as_str().unwrap().chars().count() <= 81,
            "摘要仍截断"
        );
        assert!(ask["resource_full"]
            .as_str()
            .unwrap()
            .contains("very-long-filter"));
        assert_eq!(ask["project"], project_display_name(&root));
        assert_eq!(ask["session_label"], "对话 3");
        std::fs::remove_dir_all(root).ok();
    }

    /// 没有 workdir 的 bash 是裸命令;非 bash 权限只带 resource_full;超长按字符封顶并标 truncated。
    #[test]
    fn bash命令拆分与完整资源封顶() {
        assert_eq!(
            bash_command_parts(r#"{"command":"git status","workdir":"C:/p"}"#),
            ("git status".to_string(), Some("C:/p".to_string()))
        );
        assert_eq!(
            bash_command_parts(r#"{"command":"git status","workdir":""}"#),
            ("git status".to_string(), None)
        );
        assert_eq!(
            bash_command_parts("cargo build"),
            ("cargo build".to_string(), None)
        );
        // 以 { 开头但不是 command JSON:原样当命令。
        assert_eq!(
            bash_command_parts("{not json"),
            ("{not json".to_string(), None)
        );

        let (cut, was_cut) =
            truncate_chars(&"你".repeat(APPROVAL_FULL_LIMIT + 5), APPROVAL_FULL_LIMIT);
        assert!(was_cut);
        assert_eq!(cut.chars().count(), APPROVAL_FULL_LIMIT);
        assert_eq!(truncate_chars("短", 80), ("短".to_string(), false));

        let root = temp_project("approval-edit");
        let long_path = format!("C:/Users/someone/{}/main.rs", "deep/".repeat(40));
        let (pending, _receiver) = pending_permission("edit", &long_path, &root, "ses_x");
        let runtime = Arc::new(SessionRuntime::default());
        runtime.asks.lock_or_recover().insert(6, pending);
        let mut table = HashMap::new();
        table.insert("ses_x".to_string(), runtime);
        let list = approval_pending_list(&Arc::new(Mutex::new(table)));
        let ask = &list["pending"][0];
        assert_eq!(
            ask["resource_full"], long_path,
            "长路径完整下发才看得出改的是什么"
        );
        assert!(ask.get("command").is_none(), "非 bash 不带 command");
        assert_eq!(
            ask["session_label"],
            serde_json::Value::Null,
            "未登记的会话只带项目名"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// UX-142:reply=always 落项目放行规则并送达 AlwaysAllow;规则存不下就报错、ask 保留;
    /// question 不接受 always。
    #[test]
    fn approval_answer_总是允许_落规则_存不下则保留ask() {
        let root = temp_project("always");
        let runtime = Arc::new(SessionRuntime::default());
        let (ok_ask, mut ok_rx) = pending_permission("bash", "git status", &root, "ses-1");
        runtime.asks.lock_or_recover().insert(12, ok_ask);
        let mut table = HashMap::new();
        table.insert("ses-1".to_string(), runtime.clone());
        let runtimes = Arc::new(Mutex::new(table));

        approval_answer(&runtimes, &json!({"id": 12, "reply": "always"})).unwrap();
        match ok_rx.try_recv() {
            Ok(kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AlwaysAllow)) => {}
            other => panic!("always 应送达 AlwaysAllow,实得: {other:?}"),
        }
        let rules = std::fs::read_to_string(root.join(".kanzei/kanzei.toml")).unwrap();
        assert!(rules.contains("git status"), "放行规则应已落盘: {rules}");

        // 规则文件损坏:报错,ask 仍在 pending,可改点「批准」。
        let broken = temp_project("always-broken");
        std::fs::write(broken.join(".kanzei/kanzei.toml"), "[invalid\n").unwrap();
        let (bad_ask, mut bad_rx) = pending_permission("bash", "git log", &broken, "ses-1");
        runtime.asks.lock_or_recover().insert(13, bad_ask);
        let error = approval_answer(&runtimes, &json!({"id": 13, "reply": "always"})).unwrap_err();
        assert!(error.contains("规则保存失败"), "{error}");
        assert!(
            runtime.asks.lock_or_recover().contains_key(&13),
            "ask 不得被消耗"
        );
        assert!(bad_rx.try_recv().is_err(), "没存成规则不得替用户放行或拒绝");
        approval_answer(&runtimes, &json!({"id": 13, "reply": "allow"})).unwrap();
        match bad_rx.try_recv() {
            Ok(kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)) => {}
            other => panic!("改点批准应送达 AllowOnce,实得: {other:?}"),
        }

        // question 不接受 always,ask 保留。
        let (question_tx, _question_rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock_or_recover().insert(
            14,
            crate::PendingAsk {
                source: String::new(),
                agent_id: None,
                sender: question_tx,
                request: kanzei_core::AskRequest::Question {
                    question: "继续?".into(),
                    options: Vec::new(),
                    default: None,
                    multiple: false,
                    background: false,
                    callback_id: None,
                },
                action: "question".into(),
                resource: "继续?".into(),
                project_root: root.clone(),
                session_id: "ses-1".into(),
            },
        );
        let error = approval_answer(&runtimes, &json!({"id": 14, "reply": "always"})).unwrap_err();
        assert!(error.contains("只适用于权限请求"), "{error}");
        assert!(runtime.asks.lock_or_recover().contains_key(&14));
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(broken).ok();
    }
}
