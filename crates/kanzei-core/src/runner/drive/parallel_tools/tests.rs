use super::*;
use crate::runner::drive::permissions::{resolve_permission_gate, PermissionGateRequest};
use async_trait::async_trait;
use kanzei_harness::{Component, Harness, HarnessDraft, ResolveCtx, ToolOutput};
use serde_json::json;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

struct PermissionProbe(Arc<AtomicUsize>);

#[async_trait]
impl Tool for PermissionProbe {
    fn name(&self) -> &'static str {
        "permission_probe"
    }
    fn action(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> String {
        "permission event fixture".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        input["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().into())
            .collect()
    }
    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::ok("read complete")
    }
}

struct PermissionRules;
impl Component for PermissionRules {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        for (resource, effect) in [("allowed", Effect::Allow), ("blocked", Effect::Deny)] {
            draft.permissions.push(kanzei_harness::permission::Rule {
                action: "read".into(),
                resource: resource.into(),
                effect,
            });
        }
        Ok(())
    }
}

fn config() -> RunnerConfig {
    RunnerConfig {
        digest_model: None,
        intensity: Default::default(),
        model: "fixture".into(),
        max_tokens: 1,
        reasoning: Default::default(),
        service_tier: None,
        hosted_tools: vec![],
        context_limit: None,
        limits: Default::default(),
        recall: None,
        execution_policy: Default::default(),
        ask_policy: AskPolicy::Interactive,
        halt: None,
    }
}

fn permission_events(events: Vec<RunEvent>) -> Vec<serde_json::Value> {
    events.into_iter().filter_map(|event| match event {
        RunEvent::PermissionResolved { tool_call_id, action, resource, decision, source, rule } =>
            Some(json!({"id":tool_call_id,"action":action,"resource":resource,"decision":decision,"source":source,"rule":rule})),
        _ => None,
    }).collect()
}

async fn assert_permission_parity(resources: &[&str]) {
    let root = std::env::temp_dir().join(format!(
        "kz-permission-parity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let mut harness = Harness::default();
    harness.add(PermissionRules);
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let enumerations = Arc::new(AtomicUsize::new(0));
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(PermissionProbe(enumerations.clone()))];
    let calls: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|id| {
            (
                id.into(),
                "permission_probe".into(),
                json!({"resources":resources}),
                "{}".into(),
            )
        })
        .collect();
    let approved = HashSet::from([("read".into(), "approved".into())]);
    let rules = vec![("read".into(), "scoped/*".into())];
    let plan = prepare_parallel_tool_calls(ParallelPreflight {
        ctx: &ctx,
        snapshot: &snapshot,
        tools: &tools,
        calls: &calls,
        has_subagent: false,
        session_approved: &approved,
        session_rules: &rules,
        force_serial: false,
        execution_policy: kanzei_harness::ExecutionPolicy::Default,
    })
    .expect("batch is preapproved");
    let mut parallel = Vec::new();
    let (results, _) = execute_parallel_tool_calls(ParallelToolRequest {
        config: &config(),
        ctx: &ctx,
        snapshot: &snapshot,
        tools: &tools,
        calls: &calls,
        plan,
        subagent: None,
        task_results: &mut Default::default(),
        images_supported: false,
        halt: None,
        on_event: &mut |event| parallel.push(event),
    })
    .await;
    assert_eq!(results.len(), calls.len());
    assert!(results.iter().all(|result| matches!(
        result,
        Part::ToolResult { is_error, .. } if *is_error == resources.contains(&"blocked")
    )));
    let parallel_enumerations = enumerations.load(Ordering::SeqCst);
    let mut serial = Vec::new();
    let mut approved = approved;
    let mut rules = rules;
    for (id, _, input, _) in &calls {
        resolve_permission_gate(&mut PermissionGateRequest {
            config: &config(),
            snapshot: &snapshot,
            tool: tools[0].as_ref(),
            input,
            id,
            ctx: &ctx,
            on_event: &mut |event| serial.push(event),
            ask: &mut |_| panic!("preapproved batch must not prompt"),
            session_approved: &mut approved,
            session_rules: &mut rules,
        })
        .await;
    }
    assert_eq!(
        permission_events(parallel),
        permission_events(serial),
        "parallel decisions must retain every rule/session source exactly once"
    );
    assert_eq!(
        parallel_enumerations,
        calls.len(),
        "execution must consume the checked permission plan rather than re-evaluate its resources"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn parallel_permission_events_match_serial_rules_and_session_approvals() {
    assert_permission_parity(&["allowed", "approved", "scoped/file"]).await;
}

#[tokio::test]
async fn parallel_permission_events_match_serial_denial_after_allow() {
    assert_permission_parity(&["allowed", "blocked"]).await;
}

#[tokio::test]
async fn parallel_permission_unresolved_ask_uses_serial_user_decision() {
    let root = std::env::temp_dir();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let mut harness = Harness::default();
    harness.add(PermissionRules);
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(PermissionProbe(Arc::new(AtomicUsize::new(0))))];
    let calls: Vec<_> = ["allowed", "unresolved"]
        .into_iter()
        .map(|resource| {
            (
                resource.into(),
                "permission_probe".into(),
                json!({"resources":[resource]}),
                "{}".into(),
            )
        })
        .collect();
    let mut approved = HashSet::new();
    let mut rules = Vec::new();
    assert!(prepare_parallel_tool_calls(ParallelPreflight {
        ctx: &ctx,
        snapshot: &snapshot,
        tools: &tools,
        calls: &calls,
        has_subagent: false,
        session_approved: &approved,
        session_rules: &rules,
        force_serial: false,
        execution_policy: kanzei_harness::ExecutionPolicy::Default,
    })
    .is_none());
    let mut asked = 0;
    let mut events = Vec::new();
    for (id, _, input, _) in &calls {
        let gate = resolve_permission_gate(&mut PermissionGateRequest {
            config: &config(),
            snapshot: &snapshot,
            tool: tools[0].as_ref(),
            input,
            id,
            ctx: &ctx,
            on_event: &mut |event| events.push(event),
            ask: &mut |_| {
                asked += 1;
                Box::pin(async { AskResponse::Permission(AskReply::AllowOnce) })
            },
            session_approved: &mut approved,
            session_rules: &mut rules,
        })
        .await;
        assert!(matches!(gate, Gate::Pass));
    }
    assert_eq!(asked, 1);
    let events = permission_events(events);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["source"], "ruleset");
    assert_eq!(events[1]["source"], "user");
    assert_eq!(events[1]["decision"], "allow_once");
}
