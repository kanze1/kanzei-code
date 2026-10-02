use super::quick_req_with_coordinator;
use kanzei_core::orchestration::MemoryCoordinator;
use kanzei_tools::docstore::{DocStore, REQUIREMENTS};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn project(address: std::net::SocketAddr) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kz_capture_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(root.join(".kanzei/kanzei.toml"),format!(
        "proxy = \"off\"\n[models]\nprimary = \"mock:local\"\nfast = \"mock:local\"\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n")).unwrap();
    std::fs::write(
        root.join(".kanzei/project/requirements.md"),
        "# Requirements\n",
    )
    .unwrap();
    root
}

async fn respond(listener: &TcpListener, mode: u8) {
    let (mut socket, _) =
        tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
            .await
            .expect("capture mock did not receive the expected request")
            .unwrap();
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let end = loop {
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        request.extend_from_slice(&chunk[..n]);
        if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let length = String::from_utf8_lossy(&request[..end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or_default();
    while request.len() < end + length {
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        request.extend_from_slice(&chunk[..n]);
    }
    let (status, kind, body) = if mode == 1 || mode == 2 || mode == 4 {
        let complexity = if mode == 1 { "小" } else { "大" };
        let mut fields = json!({"验收":"保存个人评分","原始描述":"保存个人评分"});
        if mode == 4 {
            fields["来源"] = json!("用户原话「保存个人评分」");
            fields["发现记录"] = json!(json!({
                "Intent":"保存个人评分","Explicit":"保存个人评分","Assumptions":"无额外假设",
                "Ambiguities":"无未决语义","领域对象":"个人评分","最小成功闭环":"写入后读取评分",
                "延后决策":"外部资料接入另批处理"
            })
            .to_string());
        }
        let tag = if mode == 4 { "核心" } else { "前端" };
        let response = json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"capture",
            "type":"function","function":{"name":"req","arguments":json!({"action":"add","title":"收藏评分",
            "priority":"P2","tag":tag,"complexity":complexity,"fields":fields}).to_string()}}]},"finish_reason":"tool_calls"}]});
        (
            "200 OK",
            "text/event-stream",
            format!("data: {response}\n\ndata: [DONE]\n\n"),
        )
    } else if mode == 3 {
        let response =
            json!({"choices":[{"index":0,"delta":{"content":"未能登记"},"finish_reason":"stop"}]});
        (
            "200 OK",
            "text/event-stream",
            format!("data: {response}\n\ndata: [DONE]\n\n"),
        )
    } else {
        (
            "401 Unauthorized",
            "application/json",
            "{\"error\":{\"message\":\"test authentication rejected\"}}".into(),
        )
    };
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
    socket.write_all(head.as_bytes()).await.unwrap();
    socket.write_all(body.as_bytes()).await.unwrap();
}

#[tokio::test]
async fn capture_reports_model_authentication_error_without_claiming_storage_failed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        respond(&listener, 0).await;
        respond(&listener, 0).await;
    });
    let result = quick_req_with_coordinator(
        Arc::new(MemoryCoordinator::new()),
        root.display().to_string(),
        "保存个人评分".into(),
        None,
    )
    .await;
    server.await.unwrap();
    let error = result.unwrap_err();
    assert!(error.contains("401"), "{error}");
    assert!(error.contains("模型执行失败"), "{error}");
    assert!(!error.contains("落库"), "{error}");
    assert!(DocStore::open(&root, &REQUIREMENTS)
        .load()
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn persisted_capture_survives_later_model_failure_and_avoids_duplicate_fallback() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        respond(&listener, 1).await;
        respond(&listener, 0).await;
    });
    let result = quick_req_with_coordinator(
        Arc::new(MemoryCoordinator::new()),
        root.display().to_string(),
        "保存个人评分".into(),
        None,
    )
    .await;
    assert!(result.unwrap().starts_with("R-001 "));
    server.await.unwrap();
    let entries = DocStore::open(&root, &REQUIREMENTS).load().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].fields.iter().any(|(key, _)| key == "归属"));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn rejected_capture_reports_actual_discovery_gate_instead_of_only_empty_result() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        respond(&listener, 2).await;
        respond(&listener, 2).await;
        respond(&listener, 3).await;
    });
    let result = quick_req_with_coordinator(
        Arc::new(MemoryCoordinator::new()),
        root.display().to_string(),
        "保存个人评分".into(),
        None,
    )
    .await;
    server.await.unwrap();
    let error = result.unwrap_err();
    assert!(error.contains("登记工具拒绝"), "{error}");
    assert_eq!(error.matches("条目校验失败:").count(), 1, "{error}");
    assert!(
        error.contains("来源") || error.contains("发现记录"),
        "{error}"
    );
    assert!(DocStore::open(&root, &REQUIREMENTS)
        .load()
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn core_capture_is_saved_and_visible_before_research_or_model_fallback() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        respond(&listener, 4).await;
        respond(&listener, 0).await;
    });
    let receipt = quick_req_with_coordinator(
        Arc::new(MemoryCoordinator::new()),
        root.display().to_string(),
        "保存个人评分".into(),
        None,
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert!(receipt.starts_with("R-001 "), "{receipt}");
    let entries = DocStore::open(&root, &REQUIREMENTS).load().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, "todo");
    assert!(entries[0]
        .fields
        .iter()
        .any(|(key, value)| key == "原始描述" && value == "保存个人评分"));
    assert!(!entries[0]
        .fields
        .iter()
        .any(|(key, _)| key == "先行调研豁免"));
    let snapshot = crate::docs::docs_snapshot(root.display().to_string()).unwrap();
    let requirement = &snapshot["requirements"][0];
    assert_eq!(requirement["id"], "R-001");
    assert_eq!(requirement["prior_art"]["status"], "pending");
    let relative = requirement["prior_art"]["path"].as_str().unwrap();
    assert!(std::fs::read_to_string(root.join(relative))
        .unwrap()
        .contains("status: pending"));
    std::fs::remove_dir_all(root).ok();
}
