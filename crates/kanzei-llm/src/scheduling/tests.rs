use super::*;
use std::task::Poll;

#[tokio::test]
async fn queued_maintenance_does_not_consume_its_active_inference_deadline() {
    let (_listener, url) = endpoint();
    let interactive = admit(&url, RequestPriority::Interactive).await;
    let client = crate::LlmClient::new(&crate::ProxyConfig::Disabled)
        .unwrap()
        .with_priority(RequestPriority::Maintenance);
    let route = crate::Route::openai_at(&url, None);
    let request = crate::LlmRequest {
        model: "fixture".into(),
        system: vec![],
        messages: vec![crate::Message::user_text("hello")],
        tools: vec![],
        hosted_tools: vec![],
        max_tokens: 16,
        temperature: None,
        reasoning: Default::default(),
        service_tier: None,
    };
    let mut completion =
        Box::pin(client.complete_with_timeout(&route, &request, std::time::Duration::ZERO));
    assert!(
        matches!(futures::poll!(&mut completion), Poll::Pending),
        "a queued request cannot time out before admission"
    );
    drop(interactive);
    assert!(matches!(
        completion.await,
        Err(crate::LlmError::InferenceTimeout(_))
    ));
}

fn endpoint() -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, url)
}

#[tokio::test]
async fn interactive_preempts_active_maintenance_and_reserves_tool_gaps() {
    let (_listener, url) = endpoint();
    let active = admit(&url, RequestPriority::Maintenance).await;
    let reservation = reserve_interactive(&url);
    active.yielded().await;
    let mut interactive = Box::pin(admit(&url, RequestPriority::Interactive));
    assert!(matches!(futures::poll!(&mut interactive), Poll::Pending));
    let mut queued = Box::pin(admit(&url, RequestPriority::Maintenance));
    assert!(matches!(futures::poll!(&mut queued), Poll::Pending));
    drop(active);
    let request = interactive.await;
    assert!(matches!(futures::poll!(&mut queued), Poll::Pending));
    drop(request);
    assert!(
        matches!(futures::poll!(&mut queued), Poll::Pending),
        "tool gaps stay reserved"
    );
    drop(reservation);
    drop(queued.await);
}

#[tokio::test]
async fn cancelling_waiters_releases_reservations_and_maintenance_is_serial() {
    let (_listener, url) = endpoint();
    let active = admit(&url, RequestPriority::Maintenance).await;
    let mut queued = Box::pin(admit(&url, RequestPriority::Maintenance));
    assert!(matches!(futures::poll!(&mut queued), Poll::Pending));
    let mut interactive = Box::pin(admit(&url, RequestPriority::Interactive));
    assert!(matches!(futures::poll!(&mut interactive), Poll::Pending));
    drop(interactive);
    drop(active);
    drop(queued.await);
    assert_eq!(service(&url).unwrap().state.lock().unwrap().interactive, 0);
}

#[tokio::test]
async fn interactive_calls_stay_concurrent_and_endpoints_stay_independent() {
    let (_listener, url) = endpoint();
    let (_other_listener, other_url) = endpoint();
    let one = admit(&url, RequestPriority::Interactive).await;
    let mut second = Box::pin(admit(&url, RequestPriority::Interactive));
    assert!(matches!(futures::poll!(&mut second), Poll::Ready(_)));
    let mut independent = Box::pin(admit(&other_url, RequestPriority::Maintenance));
    assert!(matches!(futures::poll!(&mut independent), Poll::Ready(_)));
    drop(one);
}

#[tokio::test]
async fn native_and_chat_share_admission_but_cloud_is_not_serialized() {
    let (_listener, url) = endpoint();
    let native = url.replace("/v1", "/api");
    let active = admit(&native, RequestPriority::Maintenance).await;
    let _reservation = reserve_interactive(&url);
    assert!(active.yielded.is_cancelled());
    assert!(service("https://models.example.test/v1").is_none());
    let first = admit(
        "https://models.example.test/v1",
        RequestPriority::Maintenance,
    )
    .await;
    let mut second = Box::pin(admit(
        "https://models.example.test/v1",
        RequestPriority::Maintenance,
    ));
    assert!(matches!(futures::poll!(&mut second), Poll::Ready(_)));
    assert!(!first.yielded.is_cancelled());
}

#[tokio::test]
async fn pure_inference_resumes_after_interactive_work_without_exposing_partial_output() {
    let (_listener, url) = endpoint();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let entered = Arc::new(Notify::new());
    let inference = tokio::spawn({
        let url = url.clone();
        let attempts = attempts.clone();
        let entered = entered.clone();
        async move {
            maintenance(&url, || {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let entered = entered.clone();
                async move {
                    if attempt == 0 {
                        entered.notify_one();
                        std::future::pending::<()>().await;
                    }
                    Ok("complete")
                }
            })
            .await
        }
    });
    entered.notified().await;
    let interactive = admit(&url, RequestPriority::Interactive).await;
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(interactive);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(1), inference)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        "complete"
    );
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
}
