//! question 的交互/自主决策共用入口；权限门禁不受决策影响。
use super::*;
use crate::store::decisions::{AgentDecision, DecisionRecord, DecisionResolution, DecisionStatus};
use kanzei_harness::ToolOutput;

pub(super) const AUTONOMOUS_DECISIONS: &str = "当前为自动模式。普通选择由你根据用户要求、已确认偏好和可验证事实自行决定。每当你准备向用户问一个选择题，先使用 question，填写 decision: {answer, rationale, impact, preference_refs}，记录简短理由和影响，成功后继续工作，留待用户集中复核。不要等待回答，不要把自己的决定写成用户授权，也不要输出隐藏思维过程。没有足够事实时先调查；只有无法取得的外部事实才使用 missing_fact，给相关条目标记阻塞和解除条件，然后推进独立工作。用户纠正默认只用于本次；长期偏好由用户在复核时明确选择范围。";

pub(super) async fn execute_question(
    ask_policy: AskPolicy,
    ctx: &ToolCtx,
    call_id: &str,
    input: &serde_json::Value,
    ask: &mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> ToolOutput {
    let question = input["question"].as_str().unwrap_or("").trim();
    let options: Vec<AskOption> = input["options"]
        .as_array()
        .map(|items| items.iter().filter_map(AskOption::from_json).collect())
        .unwrap_or_default();
    if question.is_empty() {
        return ToolOutput::error("question must not be empty");
    }
    if !ask_policy.allows_user_prompt() {
        return match record_question(ctx, call_id, question, &options, input) {
            Ok(record) => decision_output(record),
            Err(error) => ToolOutput::needs_correction(
                "DECISION_NOT_RECORDED",
                format!("决策尚未记录，不能声称已确认。修正 question 输入后重试：{error}"),
            ),
        };
    }
    let background = input["background"].as_bool().unwrap_or(false);
    if background && ctx.async_mailbox.is_none() {
        return ToolOutput::error("当前入口未连接异步回答通道；请使用普通 question");
    }
    let id = format!(
        "question:{}:{call_id}",
        ctx.run_id.as_deref().unwrap_or("run")
    );
    let response = ask(AskRequest::Question {
        question: question.into(),
        options,
        default: input["default"].as_str().map(Into::into),
        multiple: input["multiple"].as_bool().unwrap_or(false),
        background,
        callback_id: background.then(|| id.clone()),
    });
    if background {
        let mailbox = ctx.async_mailbox.clone().unwrap();
        let question = question.to_owned();
        let root = ctx.project_root.clone();
        let reply_id = id.clone();
        tokio::spawn(async move {
            let answer = tokio::select! { biased;
                _ = mailbox.cancelled() => { let _ = kanzei_harness::pending_question::settle(&root,&reply_id,"cancelled"); return; },
                answer = response => answer,
            };
            let text = match answer {
                AskResponse::Answer(answer) => {
                    format!("用户回答异步问题 {reply_id}\n原问题：{question}\n回答：{answer}")
                }
                AskResponse::Cancelled => format!(
                    "用户取消了异步问题 {reply_id}：{question}。没有提供回答，不得视为同意。"
                ),
                AskResponse::Permission(_) => return,
            };
            if let Err(error) = mailbox.publish(kanzei_harness::AsyncNotice {
                id: reply_id.clone(),
                text,
            }) {
                tracing::warn!(%error, "async question delivery failed");
            } else {
                let _ = kanzei_harness::pending_question::settle(&root, &reply_id, "delivered");
            }
        });
        return ToolOutput::ok(format!("问题已发送（{id}）。尚无答案；可以继续独立工作，依赖答案的部分保持等待。回答会送回此任务。"));
    }
    match response.await {
        AskResponse::Answer(answer) => ToolOutput::ok(format!("User answer: {answer}")),
        AskResponse::Cancelled => ToolOutput::error("question cancelled by user"),
        AskResponse::Permission(_) => ToolOutput::error("invalid question response"),
    }
}

fn record_question(
    ctx: &ToolCtx,
    call_id: &str,
    question: &str,
    options: &[AskOption],
    input: &serde_json::Value,
) -> anyhow::Result<DecisionRecord> {
    let run_id = ctx
        .run_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("runner did not provide run_id"))?;
    let session_id = ctx
        .session_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("runner did not provide session_id"))?;
    let resolution = input
        .get("decision")
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value::<DecisionResolution>(v.clone()))
        .transpose()?;
    let missing_fact = input
        .get("missing_fact")
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("missing_fact must be a string"))
        })
        .transpose()?;
    let project = ctx.project_root.display().to_string();
    let journal = crate::project_session_id(&ctx.project_root);
    let store = crate::SessionStore::open(&crate::project_state_path(&ctx.project_root))?;
    store.create_session(&journal, &project, None)?;
    store.create_session(session_id, &project, None)?;
    Ok(store.record_agent_decision(
        &journal,
        AgentDecision {
            project: &project,
            session_id,
            process_id: ctx.process_id.as_deref(),
            run_id,
            call_id,
            question,
            options: options
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
            work_unit_id: input["work_unit_id"].as_str().map(Into::into),
            resolution,
            missing_fact,
        },
    )?)
}

fn decision_output(record: DecisionRecord) -> ToolOutput {
    let mut display = serde_json::to_value(&record).expect("serializable decision");
    display["kind"] = serde_json::json!("autonomous_decision");
    let content = match record.status {
        DecisionStatus::Deciding => format!(
            "决策点 {} 已记录，当前是自动模式。请先调查必要事实，然后再次调用 question，保持相同 question，填写 decision: {{answer, rationale, impact, preference_refs}}。成功记录决定后继续，不要等待用户，也不要机械选择 default。确实缺少无法取得的外部事实时填写 missing_fact。", record.id),
        DecisionStatus::Decided => format!(
            "Agent decision recorded: {}\n{}\n这是模型决定，待用户批量复核。继续落实；不代表用户回答或权限授权。",
            record.id, serde_json::to_string(&record.resolution).unwrap()),
        DecisionStatus::NeedsInput => format!(
            "外部事实缺失已记录：{}\n只阻塞依赖这个事实的工作，记录解除条件，然后用 work next 推进独立任务。不要虚构事实。", record.missing_fact.as_deref().unwrap_or("")),
    };
    if record.status == DecisionStatus::Deciding {
        ToolOutput::needs_correction("DECISION_REQUIRED", content).with_display(display)
    } else {
        ToolOutput::ok(content).with_display(display)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn async_answer_returns_before_reply_and_routes_to_original_actor() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mailbox = kanzei_harness::AsyncMailbox::new(move |notice| {
            tx.send(notice).map_err(|e| e.to_string())
        });
        let ctx = ToolCtx {
            run_id: Some("original-run".into()),
            async_mailbox: Some(mailbox.clone()),
            ..Default::default()
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        let mut answer = Some(answer);
        let mut ask = |request| -> AskFuture {
            assert!(matches!(
                request,
                AskRequest::Question {
                    background: true,
                    ..
                }
            ));
            let answer = answer.take().unwrap();
            Box::pin(async move { answer.await.unwrap() })
        };
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            execute_question(
                AskPolicy::Interactive,
                &ctx,
                "original-question",
                &serde_json::json!({"question":"选择发布目录","background":true}),
                &mut ask,
            ),
        )
        .await
        .unwrap();
        assert!(!out.is_error);
        assert!(rx.try_recv().is_err(), "no synthetic/default answer");
        reply
            .send(AskResponse::Answer("docs/output".into()))
            .unwrap();
        let notice = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(notice.id, "question:original-run:original-question");
        assert!(notice.text.contains("docs/output") && notice.text.contains("选择发布目录"));
        mailbox.close();
        assert!(mailbox
            .publish(kanzei_harness::AsyncNotice {
                id: "late".into(),
                text: "late".into()
            })
            .is_err());
        assert!(rx.try_recv().is_err());
    }
    #[tokio::test]
    async fn stop_cancels_pending_async_question_receiver() {
        let mailbox = kanzei_harness::AsyncMailbox::new(|_| {
            panic!("stopped task must not receive an answer")
        });
        let ctx = ToolCtx {
            async_mailbox: Some(mailbox.clone()),
            ..Default::default()
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        let mut answer = Some(answer);
        let mut ask = |_| -> AskFuture {
            let answer = answer.take().unwrap();
            Box::pin(async move { answer.await.unwrap_or(AskResponse::Cancelled) })
        };
        execute_question(
            AskPolicy::Interactive,
            &ctx,
            "q",
            &serde_json::json!({"question":"confirm","background":true}),
            &mut ask,
        )
        .await;
        mailbox.close();
        tokio::task::yield_now().await;
        assert!(reply.send(AskResponse::Answer("late".into())).is_err());
    }
    #[tokio::test]
    async fn autonomous_question_records_then_decides_without_asking_or_using_default() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-decisions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for policy in [AskPolicy::NonInteractive, AskPolicy::AutoAllow] {
            let ctx = ToolCtx::new(root.clone(), root.clone())
                .with_session_id("origin".into())
                .with_identity(
                    "tree".into(),
                    "project".into(),
                    format!("run-{policy:?}"),
                    "p2".into(),
                );
            let mut ask = |_| -> AskFuture { panic!("auto mode must not ask") };
            let mut input = serde_json::json!({"question":"选择 provider", "options":["本地","远程"], "default":"远程"});
            let pending = execute_question(policy, &ctx, "call-1", &input, &mut ask).await;
            assert_eq!(pending.code, Some("DECISION_REQUIRED"));
            assert_eq!(pending.display.as_ref().unwrap()["status"], "deciding");
            input["decision"] =
                serde_json::json!({"answer":"本地", "rationale":"已有配置", "impact":"不新增服务"});
            let output = execute_question(policy, &ctx, "call-2", &input, &mut ask).await;
            assert!(!output.is_error, "{}", output.content);
            assert!(!output.content.contains("User answer:"));
            assert_eq!(
                output.display.as_ref().unwrap()["resolution"]["answer"],
                "本地"
            );
            let repeated = execute_question(policy, &ctx, "call-3", &input, &mut ask).await;
            assert_eq!(repeated.display, output.display);
        }
        let store = crate::SessionStore::open(&crate::project_state_path(&root)).unwrap();
        assert_eq!(
            store
                .list_decisions(&crate::project_session_id(&root))
                .unwrap()
                .len(),
            2
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn interactive_question_still_requires_real_answer() {
        let mut ask = |request| -> AskFuture {
            assert!(matches!(request, AskRequest::Question { .. }));
            Box::pin(async { AskResponse::Answer("本地".into()) })
        };
        let output = execute_question(
            AskPolicy::Interactive,
            &ToolCtx::default(),
            "call-1",
            &serde_json::json!({"question":"选择 provider", "decision":{"answer":"远程"}}),
            &mut ask,
        )
        .await;
        assert!(!output.is_error);
        assert_eq!(output.content, "User answer: 本地");
    }
}
