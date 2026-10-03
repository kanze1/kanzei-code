//! 真实 CLI/WorkTool/typed 恢复链；只把远端模型替换成确定性 SSE。
use super::context_overflow_recovery::{persisted_messages, run_cli_with_setup, success_response};
use kanzei_llm::{Message, Part};
use serde_json::json;

fn append_claim(messages: &mut Vec<Message>, call_id: &str, id: &str) {
    messages.push(Message::assistant(vec![Part::ToolCall {
        id: call_id.into(),
        name: "work".into(),
        input: json!({ "action": "claim", "id": id }),
    }]));
    messages.push(Message::tool_results(vec![Part::ToolResult {
        call_id: call_id.into(),
        content: json!({ "claimed": id, "lifecycle_status": "doing" }).to_string(),
        is_error: false,
    }]));
}

fn reader_prior() -> Vec<Message> {
    let mut prior = vec![Message::user_text(
        "完成手机 Markdown 阅读器；保留离线方案和后续发版要求。",
    )];
    append_claim(&mut prior, "claim-old", "R-001");
    for i in 0..6 {
        let id = format!("read-{i}");
        prior.push(Message::assistant(vec![Part::ToolCall {
            id: id.clone(),
            name: "read".into(),
            input: json!({ "path": format!("reader-{i}.dart") }),
        }]));
        prior.push(Message::tool_results(vec![Part::ToolResult {
            call_id: id,
            content: format!("READER_SOURCE_{i}\n{}", "x".repeat(9_000)),
            is_error: false,
        }]));
    }
    prior
}

fn verify_result(
    project: &std::path::Path,
    output: &std::process::Output,
    request: &serde_json::Value,
) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let request = request.to_string();
    assert!(
        request.contains("work_context_archived"),
        "next provider request was not reduced"
    );
    assert!(!request.contains("READER_SOURCE_0"));
    assert!(request.contains("READER_SOURCE_5"));
    assert!(request.contains("后续发版要求"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("旧读取/搜索结果"));
    let model_history = serde_json::to_string(&persisted_messages(project)).unwrap();
    assert!(model_history.contains("work_context_archived"));
    assert!(model_history.contains("新的阅读器条目完成"));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(project)).unwrap();
    let facts = store
        .list_latest_segment_facts(&kanzei_core::project_session_id(project))
        .unwrap();
    let transcript =
        serde_json::to_string(&kanzei_core::project_session_facts(&facts).transcript_messages)
            .unwrap();
    assert!(
        transcript.contains("READER_SOURCE_0"),
        "original evidence was lost"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_context_recovers_claim_at_previous_turn_end_before_first_request() {
    let mut prior = reader_prior();
    append_claim(&mut prior, "claim-new", "D-002");
    let (project, output, requests) = run_cli_with_setup(
        "item-context-restore",
        prior,
        "继续处理下一条",
        vec![success_response("新的阅读器条目完成")],
        |project| {
            // 本用例验证条目边界；显式给 mock 足够窗口，避免提示词增减触发预算压缩。
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(project.join(".kanzei/kanzei.toml"))
                    .unwrap(),
                "context_limit = 131072"
            )
            .unwrap();
        },
    )
    .await;
    assert_eq!(requests.len(), 1, "no extra model request for summaries");
    verify_result(&project, &output, &requests[0]);
    std::fs::remove_dir_all(project.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_context_successful_real_claim_reduces_next_request_without_summary_call() {
    let claim = json!({ "choices": [{ "index": 0, "delta": {
        "tool_calls": [{ "index": 0, "id": "claim-new", "type": "function", "function": {
            "name": "work", "arguments": json!({ "action": "claim", "id": "D-002" }).to_string()
        }}]
    }, "finish_reason": "tool_calls" }] });
    let (project, output, requests) = run_cli_with_setup(
        "item-context-claim", reader_prior(), "旧条目完成，接手阅读器的下一条缺陷",
        vec![claim, success_response("新的阅读器条目完成")],
        |project| {
            // 单独验证条目边界；避免 mock 的保守 32k 默认先触发预算压缩。
            use std::io::Write;
            writeln!(std::fs::OpenOptions::new().append(true).open(project.join(".kanzei/kanzei.toml")).unwrap(),
                "context_limit = 131072\n\n[[permissions.rules]]\naction = \"work\"\nresource = \"write:claim\"\neffect = \"allow\"").unwrap();
            use kanzei_tools::docstore::{DocStore, Entry, DEFECTS};
            DocStore::open(project, &DEFECTS).save(&[Entry {
                id: "D-002".into(), title: "手机阅读器滚动位置".into(), status: "open".into(),
                severity: Some("medium".into()), fields: vec![],
            }]).unwrap();
        },
    ).await;
    assert_eq!(
        requests.len(),
        2,
        "claim and continuation only, no summary request"
    );
    assert!(requests[0].to_string().contains("READER_SOURCE_0"));
    verify_result(&project, &output, &requests[1]);
    let before = requests[0].to_string().len();
    let after = requests[1].to_string().len();
    assert!(
        before > after + 8_000,
        "request did not materially shrink: {before} -> {after}"
    );
    eprintln!("item switch provider request bytes: {before} -> {after}; exactly 2 requests");
    std::fs::remove_dir_all(project.parent().unwrap()).unwrap();
}
