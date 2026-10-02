//! R-365 B1:手动订阅通道探针。默认忽略,绝不能进入常规 verify。
//! 只打印协议形状/状态码与脱敏样例;不输出认证头、响应正文、完整 URL。

use std::path::Path;
use std::time::Duration;

use kanzei_harness::config::KanzeiConfig;
use kanzei_llm::auth::codex::codex_headers;
use kanzei_llm::protocol::{self, ProtocolKind};
use kanzei_llm::proxy::{build_http_client, ProxyConfig};
use kanzei_llm::request::{LlmRequest, Message, ReasoningEffort};
use kanzei_llm::sse::SseParser;
use serde_json::{json, Value};

struct ProbeResponse {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

fn codex_primary() -> (String, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves");
    let config = KanzeiConfig::load_at_root(&root).expect("load current project configuration");
    let resolved = config
        .resolve_model("primary")
        .expect("resolve configured primary model");
    assert_eq!(
        resolved.provider.auth.as_deref(),
        Some("codex"),
        "live Codex probe requires primary to resolve through Codex subscription auth"
    );
    (resolved.provider.base_url, resolved.model)
}

fn endpoint(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

async fn codex_headers_live() -> Vec<(String, String)> {
    codex_headers(&ProxyConfig::Env)
        .await
        .expect("read/refresh Codex CLI credentials; secret values are never printed")
}

async fn post_json(url: &str, headers: &[(String, String)], body: &Value) -> ProbeResponse {
    let client = build_http_client(&ProxyConfig::Env).expect("build configured HTTP client");
    let mut request = client
        .post(url)
        .timeout(Duration::from_secs(120))
        .header("content-type", "application/json")
        .json(body);
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.expect("send live subscription probe");
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response
        .bytes()
        .await
        .expect("read bounded probe response")
        .to_vec();
    ProbeResponse {
        status,
        content_type,
        body,
    }
}

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys = value
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn safe_error_shape(response: &ProbeResponse) -> Value {
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    json!({
        "top_level_keys": sorted_keys(&body),
        "error_type": body["error"]["type"],
        "error_code": body["error"]["code"],
        "error_param": body["error"]["param"],
    })
}

fn sanitized_host(url: &Value) -> Option<String> {
    let raw = url.as_str()?;
    url::Url::parse(raw)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
}

fn add_hosted_item(item: &Value, items: &mut Vec<Value>, shapes: &mut Vec<Value>) {
    if item["type"].as_str() != Some("web_search_call") {
        return;
    }
    // 保留 added 与 done 两份 raw item;added 通常只有 id/status/type,不能拿它回放。
    items.push(item.clone());
    shapes.push(json!({
        "type": item["type"],
        "id": "<opaque-redacted>",
        "status": item["status"],
        "item_keys": sorted_keys(item),
        "action_type": item["action"]["type"],
        "action_keys": sorted_keys(&item["action"]),
        "query": if item["action"]["query"].is_string() { "<probe-query-redacted>" } else { "<absent>" },
        "url_host": sanitized_host(&item["action"]["url"]),
    }));
}

fn hosted_items_and_shapes(response: &ProbeResponse) -> (Vec<Value>, Vec<Value>) {
    let mut parser = SseParser::default();
    let events = parser.feed(&response.body);
    let mut items = Vec::new();
    let mut shapes = Vec::new();
    for event in events {
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            continue;
        };
        if data["type"].as_str() == Some("response.output_item.added")
            || data["type"].as_str() == Some("response.output_item.done")
        {
            let item = &data["item"];
            let before = items.len();
            add_hosted_item(item, &mut items, &mut shapes);
            if items.len() > before {
                if let Some(last) = shapes.last_mut() {
                    last["sse_event"] = json!(event.event);
                }
            }
        }
        if let Some(output) = data["response"]["output"].as_array() {
            for item in output {
                add_hosted_item(item, &mut items, &mut shapes);
            }
        }
        if let Some(output_item) = data["item"].as_object() {
            if output_item.get("type").and_then(Value::as_str) == Some("message") {
                if let Some(content) = output_item.get("content").and_then(Value::as_array) {
                    for block in content {
                        if let Some(annotations) = block["annotations"].as_array() {
                            for annotation in annotations {
                                shapes.push(json!({
                                    "citation_type": annotation["type"],
                                    "citation_keys": sorted_keys(annotation),
                                    "url_host": sanitized_host(&annotation["url"]),
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
    (items, shapes)
}

fn hosted_probe_request(model: &str) -> Value {
    let request = LlmRequest {
        model: model.to_string(),
        system: Vec::new(),
        messages: vec![Message::user_text(
            "Use web_search to find the latest stable Rust release and briefly identify its source.",
        )],
        tools: Vec::new(),
        hosted_tools: Vec::new(),
        max_tokens: 2500,
        temperature: None,
        reasoning: ReasoningEffort::Off,
        service_tier: None,
    };
    let mut body = protocol::build_body(ProtocolKind::OpenAiResponses, &request);
    body["tools"] = json!([{"type":"web_search","external_web_access":true}]);
    body
}

fn p2_request_id() -> String {
    let ticks = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("kz-r365-{}-{ticks}", std::process::id())
}

fn alpha_search_request(model: &str, with_settings: bool, with_input: bool) -> Value {
    let mut body = json!({
        "id": p2_request_id(),
        "model": model,
        "commands": {"search_query": [
            {"q":"latest stable Rust release", "recency":7},
            {"q":"Rust API documentation", "domains":["docs.rs"]}
        ]},
        "max_output_tokens": 2500
    });
    if with_settings {
        body["settings"] = json!({
            "allowed_callers":["direct"],
            "external_web_access":true
        });
    }
    if with_input {
        body["input"] = json!([{
            "type":"message", "role":"user",
            "content":[{"type":"input_text","text":"Search for the current stable Rust release."}]
        }]);
    }
    body
}

fn ref_id_shape(value: &Value) -> Option<String> {
    value.as_str().map(|value| {
        value
            .chars()
            .map(|character| {
                if character.is_ascii_digit() {
                    '#'
                } else if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                    character
                } else {
                    '?'
                }
            })
            .collect()
    })
}

fn alpha_response_shape(response: &ProbeResponse) -> Value {
    let Ok(body) = serde_json::from_slice::<Value>(&response.body) else {
        return json!({"parse":"not_json", "body_bytes":response.body.len()});
    };
    let first = body["results"]
        .as_array()
        .and_then(|results| results.first());
    let result_url_host = first.and_then(|result| sanitized_host(&result["url"]));
    json!({
        "top_level_keys": sorted_keys(&body),
        "output_chars": body["output"].as_str().map(str::len),
        "results_count": body["results"].as_array().map(Vec::len),
        "first_result_keys": first.map(sorted_keys),
        "first_result_type": first.and_then(|value| value["type"].as_str()),
        "first_ref_shape": first.and_then(|value| ref_id_shape(&value["ref_id"])),
        "first_url_host": result_url_host,
    })
}

#[tokio::test]
#[ignore = "live: requires Codex subscription login and may consume search quota"]
async fn p1_codex_hosted_search_and_p4_same_channel_replay_probe() {
    let (base_url, model) = codex_primary();
    println!(
        "P1 model={model} endpoint={}/responses",
        base_url.trim_end_matches('/')
    );
    let headers = codex_headers_live().await;
    let url = endpoint(&base_url, "responses");
    let body = hosted_probe_request(&model);
    let response = post_json(&url, &headers, &body).await;
    println!(
        "P1 status={} content_type={}",
        response.status, response.content_type
    );
    if !response.status.eq(&200) {
        println!("P1 safe_error={}", safe_error_shape(&response));
        return;
    }
    let (items, shapes) = hosted_items_and_shapes(&response);
    println!("P1 result_shapes={shapes:?}");
    let Some(hosted_item) = items
        .iter()
        .rev()
        .find(|item| item["action"].is_object())
        .or_else(|| items.first())
    else {
        println!("P1 result=NO_WEB_SEARCH_CALL; hosted search support is not established");
        return;
    };
    println!(
        "P1 result=WEB_SEARCH_CALL; sanitized_sample={}",
        shapes
            .iter()
            .rev()
            .find(|shape| shape["type"].as_str() == Some("web_search_call"))
            .unwrap_or(&Value::Null)
    );

    let mut replay = body.clone();
    let mut input = replay["input"].as_array().cloned().unwrap_or_default();
    input.push(hosted_item.clone());
    input.push(json!({
        "type":"message", "role":"user",
        "content":[{"type":"input_text","text":"Summarize the prior search result in one sentence."}]
    }));
    replay["input"] = Value::Array(input);
    let replay_response = post_json(&url, &headers, &replay).await;
    println!(
        "P4 same_channel_replay status={} content_type={}",
        replay_response.status, replay_response.content_type
    );
    if replay_response.status != 200 {
        println!(
            "P4 same_channel_replay safe_error={}",
            safe_error_shape(&replay_response)
        );
    }

    let mut last_step = replay;
    if let Some(object) = last_step.as_object_mut() {
        object.remove("tools");
    }
    let last_step_response = post_json(&url, &headers, &last_step).await;
    println!(
        "P4 undeclared_history_replay status={} content_type={}",
        last_step_response.status, last_step_response.content_type
    );
    if last_step_response.status != 200 {
        println!(
            "P4 undeclared_history_replay safe_error={}",
            safe_error_shape(&last_step_response)
        );
    }
}

fn claude_oauth_headers_for_probe() -> Vec<(String, String)> {
    let token = std::env::var("CLAUDE_CODE_OAUTH_TOKEN").expect(
        "set existing CLAUDE_CODE_OAUTH_TOKEN; the probe never reads or refreshes credential files",
    );
    let token = token.trim();
    assert!(
        !token.is_empty(),
        "CLAUDE_CODE_OAUTH_TOKEN must not be empty"
    );
    vec![
        ("authorization".into(), format!("Bearer {token}")),
        ("anthropic-version".into(), "2023-06-01".into()),
        ("anthropic-beta".into(), "oauth-2025-04-20".into()),
        ("x-app".into(), "cli".into()),
    ]
}

fn claude_probe_request(model: &str, tool_version: &str) -> Value {
    let request = LlmRequest {
        model: model.to_string(),
        system: Vec::new(),
        messages: vec![Message::user_text(
            "Use web search to find the latest stable Rust release and cite its source.",
        )],
        tools: Vec::new(),
        hosted_tools: Vec::new(),
        max_tokens: 2500,
        temperature: None,
        reasoning: ReasoningEffort::Off,
        service_tier: None,
    };
    let mut body = protocol::build_body(ProtocolKind::AnthropicMessages, &request);
    body["tools"] = json!([{
        "type": tool_version,
        "name": "web_search",
        "max_uses": 3
    }]);
    body
}

fn anthropic_event_shapes(response: &ProbeResponse) -> Vec<Value> {
    let mut parser = SseParser::default();
    parser
        .feed(&response.body)
        .into_iter()
        .filter_map(|event| {
            let data: Value = serde_json::from_str(&event.data).ok()?;
            let event_type = data["type"].as_str().unwrap_or(&event.event);
            let shape = match event_type {
                "content_block_start" => {
                    let block = &data["content_block"];
                    json!({
                        "event": event_type,
                        "index": data["index"],
                        "block_type": block["type"],
                        "block_keys": sorted_keys(block),
                        "name": block["name"],
                        "id": if block["id"].is_string() { "<opaque-redacted>" } else { "<absent>" },
                    })
                }
                "content_block_delta" => {
                    let delta = &data["delta"];
                    let delta_type = delta["type"].as_str().unwrap_or("");
                    let citation = &delta["citation"];
                    json!({
                        "event": event_type,
                        "index": data["index"],
                        "delta_type": delta_type,
                        "delta_keys": sorted_keys(delta),
                        "partial_json_chars": delta["partial_json"].as_str().map(str::len),
                        "citation_keys": sorted_keys(citation),
                        "citation_url_host": sanitized_host(&citation["url"]),
                    })
                }
                "message_start" => {
                    let usage = &data["message"]["usage"];
                    json!({
                        "event": event_type,
                        "usage_keys": sorted_keys(usage),
                        "server_tool_use": usage["server_tool_use"],
                    })
                }
                "message_delta" => {
                    let usage = &data["usage"];
                    json!({
                        "event": event_type,
                        "stop_reason": data["delta"]["stop_reason"],
                        "usage_keys": sorted_keys(usage),
                        "server_tool_use": usage["server_tool_use"],
                    })
                }
                "message_stop" | "content_block_stop" => json!({"event":event_type}),
                _ => return None,
            };
            Some(shape)
        })
        .collect()
}

#[tokio::test]
#[ignore = "live: requires existing Claude Code OAuth token and may consume search quota"]
async fn p3_claude_hosted_search_probe() {
    let model =
        std::env::var("KANZEI_R365_CLAUDE_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".into());
    let headers = claude_oauth_headers_for_probe();
    let url = "https://api.anthropic.com/v1/messages";
    for tool_version in ["web_search_20260209", "web_search_20250305"] {
        let body = claude_probe_request(&model, tool_version);
        let response = post_json(url, &headers, &body).await;
        println!(
            "P3 model={model} tool_version={tool_version} status={} content_type={}",
            response.status, response.content_type
        );
        if response.status == 200 {
            println!("P3 event_shapes={:?}", anthropic_event_shapes(&response));
            return;
        }
        println!("P3 safe_error={}", safe_error_shape(&response));
        if response.status != 400 || tool_version == "web_search_20250305" {
            return;
        }
    }
}

#[tokio::test]
#[ignore = "live: requires Codex subscription login and may consume search quota"]
async fn p2_codex_alpha_search_shapes_probe() {
    let (base_url, model) = codex_primary();
    println!(
        "P2 model={model} endpoint={}/alpha/search",
        base_url.trim_end_matches('/')
    );
    let headers = codex_headers_live().await;
    let url = endpoint(&base_url, "alpha/search");
    for (label, with_settings, with_input) in [
        ("settings_and_commands", true, false),
        ("without_settings", false, false),
        ("with_input", true, true),
    ] {
        let body = alpha_search_request(&model, with_settings, with_input);
        let response = post_json(&url, &headers, &body).await;
        println!(
            "P2 variant={label} status={} content_type={} shape={}",
            response.status,
            response.content_type,
            alpha_response_shape(&response)
        );
        if response.status != 200 {
            println!(
                "P2 variant={label} safe_error={}",
                safe_error_shape(&response)
            );
        }
    }
}
