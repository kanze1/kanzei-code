use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_harness::KanzeiConfig;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;

async fn serve_response(listener: &TcpListener, response: serde_json::Value) {
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_always_allow_persists_structured_bash_rule_and_executes_it() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kanzei-cli-bash-e2-{}-{suffix}", std::process::id()));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();
    std::fs::create_dir_all(&home).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(
        project.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n"
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
        serve_response(&listener, first_response).await;
        serve_response(&listener, second_response).await;
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_kz"))
        .args(["run", "执行 bash E2"])
        .current_dir(&project)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
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
    stdin.write_all(b"a\n").await.unwrap();
    drop(stdin);
    let output = child.wait_with_output().await.unwrap();
    server.await.unwrap();

    assert!(output.status.success(), "stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(std::fs::read_to_string(project.join("marker.txt")).unwrap().trim(), "kz-e2");

    let config_text = std::fs::read_to_string(project.join(".kanzei/kanzei.toml")).unwrap();
    let saved: KanzeiConfig = toml::from_str(&config_text).unwrap();
    let rule = saved
        .permissions
        .rules
        .iter()
        .find(|rule| rule.action == "bash")
        .expect("CLI should persist the bash permission");
    let resource: serde_json::Value = serde_json::from_str(&rule.resource).unwrap();
    assert_eq!(resource["command"], "echo kz-e2 > marker.txt");
    assert_eq!(
        resource["workdir"],
        kanzei_harness::permission::normalize_resource(&project.display().to_string())
    );

    std::fs::remove_dir_all(root).unwrap();
}
