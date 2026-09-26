use super::*;
use kanzei_harness::auto_run::{BacklogStatus, RoundFailure};

fn root() -> PathBuf {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kz-auto-research-app-{time}"));
    std::fs::create_dir_all(root.join(".kanzei/research/demo")).unwrap();
    workflow::start(&root, "demo", Default::default(), 4).unwrap();
    root
}

fn context<'a>(tools: &'a [String], signature: &'a str) -> AutoRunCtx<'a> {
    AutoRunCtx {
        backlog: BacklogStatus::AllBlocked,
        halted: false,
        steps: 3,
        tools,
        intensity: kanzei_harness::HarnessIntensity::Autonomous,
        auto_allowed: false,
        model_declared_done: false,
        awaiting_user: false,
        goal_active: false,
        closed_this_round: 50,
        verify_every_n: 1,
        round_failure: None,
        progress_signature: signature,
    }
}

#[test]
fn research_ignores_dev_backlog_and_uses_checkpoint_to_stop() {
    let root = root();
    let mut ctrl = crate::auto_run::AutoRunController {
        enabled: true,
        ..Default::default()
    };
    let tools = vec!["research_workflow".into()];
    let next = decide(&mut ctrl, context(&tools, "first"), &root, Some("demo"));
    assert_eq!(next["type"], "Continue");
    assert!(next["prompt"].as_str().unwrap().contains("文献调研"));
    let state = workflow::load(&root, "demo").unwrap().unwrap();
    workflow::user_action(&root, "demo", state.revision, "pause", None).unwrap();
    assert_eq!(
        decide(&mut ctrl, context(&tools, "second"), &root, Some("demo"))["reason"],
        "ResearchWaiting"
    );
    let state = workflow::load(&root, "demo").unwrap().unwrap();
    workflow::user_action(&root, "demo", state.revision, "resume", None).unwrap();
    assert_eq!(
        decide(&mut ctrl, context(&tools, "third"), &root, Some("demo"))["type"],
        "Continue"
    );
}

#[test]
fn cancellation_failure_and_no_progress_still_stop_research() {
    let root = root();
    let mut ctrl = crate::auto_run::AutoRunController {
        enabled: true,
        ..Default::default()
    };
    let tools = vec!["research_workflow".into()];
    let mut ctx = context(&tools, "fixed");
    ctx.halted = true;
    assert_eq!(
        decide(&mut ctrl, ctx, &root, Some("demo"))["type"],
        "NoContinue"
    );
    let mut ctx = context(&tools, "fixed");
    ctx.round_failure = Some(RoundFailure::Transient);
    assert_eq!(
        decide(&mut ctrl, ctx, &root, Some("demo"))["type"],
        "RetryAfterFailure"
    );
    ctrl.state.reset();
    let mut stopped = false;
    for _ in 0..10 {
        if decide(&mut ctrl, context(&tools, "fixed"), &root, Some("demo"))["reason"]
            == "ZeroOutput"
        {
            stopped = true;
            break;
        }
    }
    assert!(stopped);
    assert_eq!(
        decide(&mut ctrl, context(&tools, "fixed"), &root, None)["reason"],
        "ResearchWaiting"
    );
}
