//! Explicit real-model probe. Normal test runs never require a model service.
use futures::StreamExt;
use kanzei_llm::{
    LlmClient, LlmError, LlmEvent, LlmRequest, Message, ProxyConfig, RequestPriority, Route,
};
use serde_json::json;
use std::{path::PathBuf, time::Instant};
use tokio::sync::oneshot;

fn request(model: &str, prompt: &str, budget: u32, trial: usize) -> LlmRequest {
    LlmRequest {
        model: model.into(),
        system: vec![format!(
            "Real-model scheduling trial {trial}. Follow the current request."
        )],
        messages: vec![Message::user_text(prompt)],
        tools: vec![],
        hosted_tools: vec![],
        max_tokens: budget,
        temperature: Some(0.0),
        reasoning: Default::default(),
        service_tier: None,
    }
}

async fn consume(
    client: LlmClient,
    route: Route,
    request: LlmRequest,
    mut entered: Option<oneshot::Sender<()>>,
) -> (f64, usize, u64) {
    let start = Instant::now();
    let mut yielded = 0;
    loop {
        let mut stream = match client.stream(&route, &request).await {
            Err(LlmError::Yielded) => {
                yielded += 1;
                continue;
            }
            result => result.expect("real inference must establish a stream"),
        };
        let mut first = None;
        let mut output = 0;
        let mut completed = false;
        let mut interrupted = false;
        while let Some(event) = stream.next().await {
            match event {
                Err(LlmError::Yielded) => {
                    yielded += 1;
                    interrupted = true;
                    break;
                }
                Ok(LlmEvent::TextDelta { .. } | LlmEvent::ReasoningDelta { .. }) => {
                    first.get_or_insert_with(|| start.elapsed().as_secs_f64() * 1000.0);
                    if let Some(entered) = entered.take() {
                        entered
                            .send(())
                            .expect("foreground observer must remain alive");
                    }
                }
                Ok(LlmEvent::StepFinish { usage, .. }) => {
                    output = usage.output;
                    completed = true;
                }
                Err(error) => panic!("real inference failed: {error}"),
                _ => {}
            }
        }
        drop(stream);
        if interrupted {
            continue;
        }
        assert!(completed, "real service must report completion");
        return (
            first.expect("model must produce reasoning or content"),
            yielded,
            output,
        );
    }
}

#[tokio::test]
#[ignore = "requires an idle local OpenAI-compatible model service, KANZEI_LOCAL_PROBE_URL/MODEL and a lifecycle evidence directory"]
async fn real_local_interactive_priority_latency() {
    let base = std::env::var("KANZEI_LOCAL_PROBE_URL").expect("set KANZEI_LOCAL_PROBE_URL");
    let model = std::env::var("KANZEI_LOCAL_PROBE_MODEL").expect("set KANZEI_LOCAL_PROBE_MODEL");
    let evidence = PathBuf::from(
        std::env::var("KANZEI_RUN_EVIDENCE").expect("run through artifact-lifecycle exec"),
    );
    let route = Route::openai_at(&base, None);
    let mut rows = Vec::new();
    for (trial, priority) in [
        RequestPriority::Interactive,
        RequestPriority::Maintenance,
        RequestPriority::Interactive,
        RequestPriority::Maintenance,
    ]
    .into_iter()
    .enumerate()
    {
        let (sender, receiver) = oneshot::channel();
        let background = LlmClient::new(&ProxyConfig::Disabled)
            .unwrap()
            .with_priority(priority);
        let interactive = LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let background_request=request(&model,"List integers 1 to 1000 and their squares in numbered rows. Start at 1 and continue until the output limit.",256,trial);
        let foreground_request = request(&model, "Reply briefly with 收到。", 64, trial);
        let background_route = route.clone();
        let row=tokio::time::timeout(std::time::Duration::from_secs(90),async {
            let background=tokio::spawn(consume(background,background_route,background_request,Some(sender)));
            receiver.await.expect("background must begin generating before foreground submission");
            let foreground=consume(interactive,route.clone(),foreground_request,None).await;
            let background=background.await.unwrap();
            if priority==RequestPriority::Maintenance {assert!(background.1>0,"real stream must yield");} else {assert_eq!(background.1,0);}
            json!({"trial":trial,"maintenance":priority==RequestPriority::Maintenance,"foreground_first_token_ms":foreground.0,"background_first_token_ms":background.0,"background_yields":background.1,"background_completed_output_tokens":background.2,"foreground_output_tokens":foreground.2})
        }).await.expect("bounded real-model priority probe");
        println!("{row}");
        rows.push(row);
        std::fs::write(evidence.join("priority-measurements.json"),serde_json::to_vec_pretty(&json!({"model":model,"thinking":"model default, unchanged","boundary":"foreground submitted after background begins generating; same local service; no configuration changes","results":rows})).unwrap()).unwrap();
    }
}
