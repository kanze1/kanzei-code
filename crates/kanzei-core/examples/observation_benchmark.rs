//! Paired, frozen source questions through the configured real Luna route.
//! Raw and projected requests differ only by lossless observation references.
use futures::StreamExt;
use kanzei_harness::{ExternalSourceKind, KanzeiConfig, ToolOutput};
use kanzei_llm::{
    LlmClient, LlmEvent, LlmRequest, Message, Part, ProxyConfig, ReasoningEffort, ToolSpec, Usage,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;

fn function(root: &Path, path: &str, name: &str) -> anyhow::Result<String> {
    let text = std::fs::read_to_string(root.join(path))?.replace("\r\n", "\n");
    let start = text
        .find(&format!("fn {name}("))
        .ok_or_else(|| anyhow::anyhow!("Missing frozen function {name}"))?;
    let end = text[start..]
        .find("\n}\n")
        .ok_or_else(|| anyhow::anyhow!("Missing function boundary"))?
        + start
        + 3;
    Ok(text[start..end].into())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let output = root.join(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "output/remediation-20261018/observation-benchmark".into()),
    );
    anyhow::ensure!(!output.exists(), "Choose a fresh output directory");
    std::fs::create_dir_all(&output)?;
    let config = KanzeiConfig::load_at_root(&root)?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(value) => ProxyConfig::Explicit(value.into()),
    };
    let resolved = config.resolve_model("primary")?;
    anyhow::ensure!(
        resolved.model.contains("luna"),
        "Benchmark requires the configured Luna primary"
    );
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    let client = LlmClient::new(&proxy)?;
    let cases = [
        ("stale-recipient", "crates/kanzei-app/src/commands/run.rs", "recipient_mode", "For general=false, stored=Some(\"isolated\"), requested=Some(\"dev\"), agent=Some(\"dev-pair\"), return JSON with profile and agent strings and evidence_path naming the observed repository source file.", json!({"profile":"isolated","agent":"isolated","evidence_path":"crates/kanzei-app/src/commands/run.rs"})),
        ("general-normalization", "crates/kanzei-app/src/general_chat.rs", "normalize_profile", "Evaluate normalize_profile for Some(\"isolated\"), Some(\"research\"), and None. The supplied supports_profile defines exactly dev or isolated. Return JSON with isolated, research, default strings and evidence_path naming the observed repository source file.", json!({"isolated":"isolated","research":"dev","default":"dev","evidence_path":"crates/kanzei-app/src/general_chat.rs"})),
    ];
    let mut records = Vec::new();
    for (case_index, (id, path, name, question, expected)) in cases.into_iter().enumerate() {
        let mut source = function(&root, path, name)?;
        if id == "general-normalization" {
            source = format!("{}\n{source}", function(&root, path, "supports_profile")?);
        }
        std::fs::write(output.join(format!("{id}.rs")), &source)?;
        let source_hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        let mut messages = vec![Message::user_text("Review the supplied repository code. Answer the final concrete question only using the observed source.")];
        for observation in 0..4 {
            let call_id = format!("obs-{observation}");
            messages.push(Message::assistant(vec![Part::ToolCall {
                id: call_id.clone(),
                name: "read".into(),
                input: json!({"path":path}),
            }]));
            messages.push(Message::tool_results(vec![Part::ToolResult {
                call_id,
                is_error: false,
                content: ToolOutput::ok(&source)
                    .with_external_source(ExternalSourceKind::File, path)
                    .model_content(),
            }]));
        }
        messages.push(Message::user_text(format!(
            "{question} Return a JSON object only, no Markdown."
        )));
        let projection = kanzei_core::project_observation_context(&messages);
        std::fs::write(
            output.join(format!("{id}-frozen.json")),
            serde_json::to_vec_pretty(&messages)?,
        )?;
        let order = if case_index % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for projected in order {
            let sent = if projected {
                projection.messages.to_vec()
            } else {
                messages.clone()
            };
            let wire_bytes = serde_json::to_vec(&sent)?.len();
            let request = LlmRequest {
                model: resolved.model.clone(),
                system: vec![kanzei_harness::EXTERNAL_SOURCE_POLICY.into()],
                messages: sent,
                tools: vec![ToolSpec {
                    name: "read".into(),
                    description: "Read the already supplied frozen repository evidence".into(),
                    input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
                }],
                hosted_tools: vec![],
                max_tokens: 1024,
                temperature: None,
                reasoning: ReasoningEffort::Low,
                service_tier: None,
            };
            let started = std::time::Instant::now();
            let mut stream = client
                .stream_with_retry_notice_with_limits(&route, &request, 0, 0, |_, _| {})
                .await?;
            let mut text = String::new();
            let mut usage = Usage::default();
            let mut finished = false;
            while let Some(event) = stream.next().await {
                match event? {
                    LlmEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
                    LlmEvent::StepFinish { usage: actual, .. } => {
                        usage = actual;
                        finished = true;
                    }
                    _ => {}
                }
            }
            anyhow::ensure!(finished, "Provider did not finish {id}");
            let answer = serde_json::from_str::<serde_json::Value>(text.trim());
            let correct = answer.as_ref().is_ok_and(|answer| *answer == expected);
            records.push(json!({"case":id,"projected":projected,"source_sha256":source_hash,"model":resolved.model,"source_bytes":source.len(),"observations":4,"repeated_results_replaced":if projected {projection.repeated_results} else {0},"omitted_observation_bytes":if projected {projection.omitted_bytes} else {0},"message_bytes":wire_bytes,"peak_context_tokens":usage.context_input_tokens(),"usage":usage,"compactions":0,"elapsed_ms":started.elapsed().as_millis(),"answer":text,"expected":expected,"correct":correct}));
            std::fs::write(
                output.join("results.json"),
                serde_json::to_vec_pretty(
                    &json!({"scope":"Frozen one-step source questions; not a general agent intelligence or long-run exploration benchmark. No retries or exclusions.","records":records}),
                )?,
            )?;
            println!("{id} projected={projected}: correct={correct}, context_input={}, bytes={wire_bytes}",usage.context_input_tokens());
        }
    }
    anyhow::ensure!(
        records.iter().all(|record| record["correct"] == true),
        "At least one paired answer failed; preserve all results"
    );
    Ok(())
}
