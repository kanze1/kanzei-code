use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_harness::KanzeiConfig;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;

use crate::common;

/// 桩服务器等一次模型请求的上限。
///
/// 没有它的时候,这几个测试的 `server.await` 是**无条件**等第 N 次请求的:一旦生产侧
/// 的轮次数变了(R-183 B1 把「无 TTY = 非交互 = 默认拒绝」立成契约,被拒的那一轮直接
/// 收口,第二次请求再也不会来),`accept()` 就永远挂在那里。后果不是某个测试变红,
/// 是 `cargo test --workspace` 整个卡死、一行输出都没有——门禁从「会报错」退化成
/// 「会静默挂起」,比测试失败危险得多。超时之后至少能指着名字说是谁没等到。
const SERVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 等一次模型请求并回一段 SSE。超时即 panic,绝不无限等待。
async fn serve_response_within(
    listener: &TcpListener,
    response: serde_json::Value,
    label: &str,
) -> Vec<u8> {
    match tokio::time::timeout(SERVE_TIMEOUT, serve_response(listener, response)).await {
        Ok(request) => request,
        Err(_) => panic!(
            "等待「{label}」模型请求超过 {}s 仍未到达。多半是生产侧的轮次数变了\
             (例如权限被拒导致本轮提前收口),桩服务器还在等下一次请求——\
             改测试对齐新契约,不要靠加时间蒙混过去。",
            SERVE_TIMEOUT.as_secs()
        ),
    }
}

async fn serve_response(listener: &TcpListener, response: serde_json::Value) -> Vec<u8> {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        request.extend_from_slice(&chunk[..count]);
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let content_length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        request.extend_from_slice(&chunk[..count]);
    }

    let body = format!("data: {response}\n\ndata: [DONE]\n\n");
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
    request
}

/// R-183 的非交互执行通道走通一整轮:无 TTY 下按 `allow_listed` 档 + `--allow` 放行,
/// bash 真的执行、文件真的落地、本轮正常收口。
///
/// 这条测试原来叫 `cli_always_allow_persists_structured_bash_rule_and_executes_it`,
/// 走的是「喂 `a\n` → 交互式 always-allow → 持久化规则 → 执行」。R-183 B1(caa9d62)
/// 把契约改成 **stdin 不是 TTY 就一律非交互**,而测试用的是 `Stdio::piped()`——
/// 那条路从此走不到:`a` 永远不会被读,权限按缺省 deny 拒掉,bash 不执行,本轮提前收口,
/// 于是桩服务器永远等不到第二次请求,`cargo test --workspace` 整个挂死。
/// (契约变了却没同批改测试,这是 R-183 B1 的遗留。)
///
/// 交互式 always-allow 需要真 PTY/ConPTY 才能端到端跑,不是这套夹具能覆盖的;
/// 它的**持久化与规则形态**已由 main.rs 的两个单测钉住
/// (`persist_always_allow_returns_always_only_after_successful_write` /
/// `persist_always_allow_does_not_grant_when_config_write_fails`)。
/// 所以这里改测同一条链路上今天真实存在、且此前只有单测覆盖的那一段:
/// 非交互放行 → bash 执行 → 收口。断言强度不降(仍然验「真的执行了」与「本轮成功」)。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_allow_listed_executes_bash_without_tty() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kanzei-cli-bash-e2-{}-{suffix}",
        std::process::id()
    ));
    let home_guard = common::TestHome::new("bash-e2");
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(
        project.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n\n[permissions]\nnon_interactive = \"allow_listed\"\n"
        ),
    )
    .unwrap();

    let first_response = json!({
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_bash_e2",
                    "type": "function",
                    "function": {
                        "name": "bash",
                        "arguments": "{\"command\":\"echo kz-e2 > marker.txt\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let second_response = json!({
        "choices": [{
            "index": 0,
            "delta": {"content": "bash executed"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let server = tokio::spawn(async move {
        serve_response_within(&listener, first_response, "第一轮 · 派发 bash").await;
        serve_response_within(&listener, second_response, "第二轮 · 工具结果回喂").await;
    });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kz"));
    // `--allow bash:*` = 操作员对本次运行显式放行 bash。这是 R-183 给无人值守留的正门:
    // 不改配置里的常驻规则、不落盘,只对这一次进程有效。
    cmd.args(["run", "--allow", "bash:*", "执行 bash E2"])
        .current_dir(&project);
    // 全局根隔离(见 tests/common/mod.rs,R-200):三连缺一即退回读开发者真实配置(D-292)。
    home_guard.apply(&mut cmd);
    let mut child = cmd
        .env("KANZEI_MODEL", "mock:test-model")
        .env("KANZEI_AGENT", "dev-pair")
        .env("KANZEI_PROFILE", "dev")
        .env("KANZEI_PROXY", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // stdin 立刻关掉:非 TTY 且无输入,正是脚手架/CI 派发 kz 的形态。
    drop(child.stdin.take().unwrap());
    let output = child.wait_with_output().await.unwrap();
    server.await.unwrap();

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(project.join("marker.txt"))
            .unwrap()
            .trim(),
        "kz-e2",
        "非交互放行档下 bash 必须真的执行(这是 R-183 无人值守通道的全部意义)"
    );

    // 反证:本次放行是**一次性**的,不得偷偷落成常驻规则。--allow 的语义是
    // 「这一次运行」,写进 kanzei.toml 就等于把临时授权变成永久授权。
    let config_text = std::fs::read_to_string(project.join(".kanzei/kanzei.toml")).unwrap();
    let saved: KanzeiConfig = toml::from_str(&config_text).unwrap();
    assert!(
        !saved
            .permissions
            .rules
            .iter()
            .any(|rule| rule.action == "bash"),
        "--allow 是本次运行的一次性放行,不该被持久化成 bash 规则:{config_text}"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_declined_permission_persists_paired_tool_results() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("kanzei-cli-d054-{}-{suffix}", std::process::id()));
    let home_guard = common::TestHome::new("bash-d054");
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(
        project.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n\n[[permissions.rules]]\naction = \"write\"\nresource = \"allowed.md\"\neffect = \"allow\"\n"
        ),
    )
    .unwrap();

    let response = json!({
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [
                    {
                        "index": 0,
                        "id": "call_write_d054",
                        "type": "function",
                        "function": {
                            "name": "write",
                            "arguments": "{\"path\":\"allowed.md\",\"content\":\"executed\"}"
                        }
                    },
                    {
                        "index": 1,
                        "id": "call_bash_d054",
                        "type": "function",
                        "function": {
                            "name": "bash",
                            "arguments": "{\"command\":\"echo must-not-run > refused-marker.txt\"}"
                        }
                    }
                ]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let server =
        tokio::spawn(async move { serve_response_within(&listener, response, "单轮").await });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kz"));
    cmd.args(["run", "拒绝第二个工具"]).current_dir(&project);
    // 全局根隔离(见 tests/common/mod.rs,R-200):三连缺一即退回读开发者真实配置(D-292)。
    home_guard.apply(&mut cmd);
    let mut child = cmd
        .env("KANZEI_MODEL", "mock:test-model")
        .env("KANZEI_AGENT", "dev-pair")
        .env("KANZEI_PROFILE", "dev")
        .env("KANZEI_PROXY", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"n\n").await.unwrap();
    drop(stdin);
    let output = child.wait_with_output().await.unwrap();
    server.await.unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(project.join("allowed.md")).unwrap(),
        "executed"
    );
    assert!(!project.join("refused-marker.txt").exists());

    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project)).unwrap();
    let session_id = kanzei_core::project_session_id(&project);
    let event = store
        .list_events(&session_id, 0)
        .unwrap()
        .into_iter()
        .rev()
        .find(|event| event.event_type == "conversation.updated")
        .expect("declined run should persist conversation");
    let messages: Vec<kanzei_llm::Message> =
        serde_json::from_value(event.payload["messages"].clone()).unwrap();
    let results: Vec<&kanzei_llm::Part> = messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter(|part| matches!(part, kanzei_llm::Part::ToolResult { .. }))
        .collect();
    assert_eq!(results.len(), 2);
    assert!(
        matches!(results[0], kanzei_llm::Part::ToolResult { call_id, is_error: false, .. } if call_id == "call_write_d054")
    );
    assert!(
        matches!(results[1], kanzei_llm::Part::ToolResult { call_id, is_error: true, content } if call_id == "call_bash_d054" && content.contains("declined"))
    );
    let typed_facts = store.list_session_facts(&session_id).unwrap();
    let typed_projection = kanzei_core::project_session_facts(&typed_facts);
    let typed_comparison = kanzei_core::compare_shadow(&typed_projection, &messages);
    assert!(
        typed_comparison.equal,
        "拒绝/部分完成路径的 shadow 应与快照相等"
    );
    assert!(typed_facts.iter().any(|(_, envelope)| matches!(
        &envelope.fact,
        kanzei_core::SessionFact::ToolResultCommitted {
            call_id,
            is_error: true,
            ..
        } if call_id == "call_bash_d054"
    )));
    assert!(typed_facts
        .iter()
        .any(|(_, envelope)| matches!(&envelope.fact, kanzei_core::SessionFact::TurnStopped)));
    let shadow = store
        .latest_event(&session_id, "session.shadow_compared")
        .unwrap()
        .expect("CLI 轮末应写 shadow report");
    assert_eq!(shadow.payload["equal"], true);
    assert_eq!(shadow.payload["typed_write_errors"], json!([]));

    let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address2 = listener2.local_addr().unwrap();
    std::fs::write(
        project.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address2}/v1\"\n\n[[permissions.rules]]\naction = \"write\"\nresource = \"allowed.md\"\neffect = \"allow\"\n"
        ),
    )
    .unwrap();
    let response2 = json!({
        "choices": [{
            "index": 0,
            "delta": {"content": "recovered after denial"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let server2 = tokio::spawn(async move {
        serve_response_within(&listener2, response2, "第二次运行").await
    });
    let mut cmd2 = Command::new(env!("CARGO_BIN_EXE_kz"));
    cmd2.args(["run", "拒绝后继续对话"]).current_dir(&project);
    // 全局根隔离(见 tests/common/mod.rs,R-200):三连缺一即退回读开发者真实配置(D-292)。
    home_guard.apply(&mut cmd2);
    let output2 = cmd2
        .env("KANZEI_MODEL", "mock:test-model")
        .env("KANZEI_AGENT", "dev-pair")
        .env("KANZEI_PROFILE", "dev")
        .env("KANZEI_PROXY", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .unwrap();
    server2.await.unwrap();
    assert!(
        output2.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output2.stdout),
        String::from_utf8_lossy(&output2.stderr)
    );
    assert!(String::from_utf8_lossy(&output2.stdout).contains("recovered after denial"));

    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_filters_preexisting_orphan_tool_call_before_next_request() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kanzei-cli-legacy-d054-{}-{suffix}",
        std::process::id()
    ));
    let home_guard = common::TestHome::new("bash-legacy");
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let state_path = kanzei_core::project_state_path(&project);
    let store = kanzei_core::SessionStore::open(&state_path).unwrap();
    let session_id = kanzei_core::project_session_id(&project);
    store
        .create_session(&session_id, &project.display().to_string(), None)
        .unwrap();
    let damaged = vec![
        kanzei_llm::Message::user_text("旧任务"),
        kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall {
            id: "legacy_orphan".into(),
            name: "bash".into(),
            input: json!({"command": "echo should-not-be-replayed"}),
        }]),
    ];
    store
        .append_event(
            &session_id,
            "conversation.updated",
            &json!({"messages": damaged}),
        )
        .unwrap();
    drop(store);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(
        project.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n"
        ),
    )
    .unwrap();
    let response = json!({
        "choices": [{
            "index": 0,
            "delta": {"content": "recovered legacy snapshot"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let server =
        tokio::spawn(async move { serve_response_within(&listener, response, "单轮").await });

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kz"));
    cmd.args(["run", "继续旧会话"]).current_dir(&project);
    // 全局根隔离(见 tests/common/mod.rs,R-200):三连缺一即退回读开发者真实配置(D-292)。
    home_guard.apply(&mut cmd);
    let output = cmd
        .env("KANZEI_MODEL", "mock:test-model")
        .env("KANZEI_AGENT", "dev-pair")
        .env("KANZEI_PROFILE", "dev")
        .env("KANZEI_PROXY", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .unwrap();
    let request = server.await.unwrap();

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("recovered legacy snapshot"));
    assert!(!String::from_utf8_lossy(&request).contains("legacy_orphan"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_prompt_file_feeds_big_prompt_without_argv() {
    // R-238 ②:`kz run --prompt-file` 从 UTF-8 文件读 prompt 跑通一轮——大文本
    // 交付不进命令行参数(避开 Windows 32767 上限),这是验收②的正门。
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kanzei-cli-promptfile-{}-{suffix}",
        std::process::id()
    ));
    let home_guard = common::TestHome::new("prompt-file");
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(
        project.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n"
        ),
    )
    .unwrap();

    let response = json!({
        "choices": [{
            "index": 0,
            "delta": {"content": "prompt received"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    let server = tokio::spawn(async move {
        serve_response_within(&listener, response, "单轮文本响应").await;
    });

    // 大文本(>8k 字符)写进文件,不进 argv。
    let prompt_path = project.join("big-prompt.txt");
    let big_prompt = "任务:读完这份材料后给出总结。\n".to_string() + &"内容".repeat(4200);
    std::fs::write(&prompt_path, &big_prompt).unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kz"));
    cmd.arg("run")
        .arg("--prompt-file")
        .arg(&prompt_path)
        .current_dir(&project);
    home_guard.apply(&mut cmd);
    let mut child = cmd
        .env("KANZEI_MODEL", "mock:test-model")
        .env("KANZEI_AGENT", "dev-pair")
        .env("KANZEI_PROFILE", "dev")
        .env("KANZEI_PROXY", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take().unwrap());
    let output = child.wait_with_output().await.unwrap();
    server.await.unwrap();

    assert!(
        output.status.success(),
        "--prompt-file 应跑通一轮:stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(root).unwrap();
}
