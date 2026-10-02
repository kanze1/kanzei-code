use super::*;
use crate::{
    research_loop::ResearchLoopTool, research_plan::ResearchPlanTool,
    research_write::ResearchWriteTool,
};
use kanzei_harness::{Tool, ToolCtx};
use serde_json::json;

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("kz-shared-control-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn topic() -> (TempDir, ToolCtx) {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join(".kanzei/research/shared")).unwrap();
    let ctx = ToolCtx::new(dir.path().to_path_buf(), dir.path().to_path_buf());
    (dir, ctx)
}

#[tokio::test]
async fn auto_pause_and_stage_block_legacy_search_without_resetting_evidence() {
    let (dir, ctx) = topic();
    let state =
        crate::research_workflow::start(dir.path(), "shared", PlanBudget::default(), 2).unwrap();
    let tool = ResearchLoopTool;
    assert!(
        !tool
            .execute(json!({"action":"start","topic":"shared"}), &ctx)
            .await
            .is_error
    );
    let task = tool
        .execute(json!({"action":"begin_search","topic":"shared"}), &ctx)
        .await;
    assert!(!task.is_error, "{}", task.content);
    let before = std::fs::read(dir.path().join(".kanzei/research/shared/loop.json")).unwrap();
    let paused =
        crate::research_workflow::user_action(dir.path(), "shared", state.revision, "pause", None)
            .unwrap();
    let blocked = tool
        .execute(json!({"action":"begin_search","topic":"shared"}), &ctx)
        .await;
    assert_eq!(blocked.code, Some("WORKFLOW_CONTROL"));
    assert!(crate::research_loop::validate_external_task(dir.path(), "shared", "any").is_err());
    assert_eq!(
        before,
        std::fs::read(dir.path().join(".kanzei/research/shared/loop.json")).unwrap()
    );
    let resumed = crate::research_workflow::user_action(
        dir.path(),
        "shared",
        paused.revision,
        "resume",
        None,
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".kanzei/research/shared/survey.md"),
        "facts",
    )
    .unwrap();
    crate::research_workflow::advance(
        dir.path(),
        "shared",
        resumed.revision,
        "survey_complete",
        &json!({"artifact":"survey.md"}),
    )
    .unwrap();
    assert_eq!(
        tool.execute(json!({"action":"reflect","topic":"shared","gaps":[]}), &ctx)
            .await
            .code,
        Some("WORKFLOW_CONTROL")
    );
    assert_eq!(
        ResearchPlanTool
            .execute(
                json!({"action":"clarify","topic":"shared","questions":["x"]}),
                &ctx
            )
            .await
            .code,
        Some("WORKFLOW_CONTROL")
    );
}

#[tokio::test]
async fn auto_budget_is_authoritative_over_stale_plan_and_override() {
    let (dir, ctx) = topic();
    let selected = PlanBudget {
        max_rounds: 7,
        max_tokens: 1234,
        max_concurrency: 2,
    };
    crate::research_workflow::start(dir.path(), "shared", selected.clone(), 2).unwrap();
    std::fs::write(
        dir.path().join(".kanzei/research/shared/budget.json"),
        r#"{"max_rounds":99,"max_tokens":9999,"max_concurrency":9}"#,
    )
    .unwrap();
    assert_eq!(budget(dir.path(), "shared").unwrap(), selected);
    let out = ResearchLoopTool
        .execute(json!({"action":"start","topic":"shared"}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join(".kanzei/research/shared/loop.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["max_rounds"], 7);
    assert_eq!(state["max_tokens"], 1234);
}

#[tokio::test]
async fn section_writer_uses_auto_artifacts_and_cannot_skip_numeric_review() {
    let (dir, ctx) = topic();
    let mut state =
        crate::research_workflow::start(dir.path(), "shared", PlanBudget::default(), 2).unwrap();
    state.stage = crate::research_workflow::Stage::WritePaper;
    std::fs::write(
        dir.path().join(".kanzei/research/shared/workflow.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let tool = ResearchWriteTool;
    let outline = tool.execute(json!({"action":"write_outline","topic":"shared","title":"Shared","sections":[{"id":"intro","title":"Introduction","objective":"facts","source_ids":["S-1"]}]}), &ctx).await;
    assert!(!outline.is_error, "{}", outline.content);
    assert!(!tool.execute(json!({"action":"write_section","topic":"shared","section_id":"intro","content":"Introduction text"}), &ctx).await.is_error);
    assert!(
        !tool
            .execute(json!({"action":"assemble_paper","topic":"shared"}), &ctx)
            .await
            .is_error
    );
    let latex = dir.path().join(".kanzei/research/shared/latex");
    assert!(latex.join("sections/intro.tex").is_file());
    assert!(latex.join("paper.tex").is_file());
    assert!(!dir
        .path()
        .join(".kanzei/research/shared/paper.tex")
        .exists());
    let blocked = tool
        .execute(json!({"action":"compile_paper","topic":"shared"}), &ctx)
        .await;
    assert!(blocked.is_error);
    assert!(
        !latex.join("compile-log.txt").exists(),
        "review gate must run before compiler"
    );

    state.stage = crate::research_workflow::Stage::CompilePaper;
    state.paper = Some(crate::research_workflow::paper::PaperRecord {
        tex: "latex/paper.tex".into(),
        template: "existing:sections".into(),
        claims: vec![crate::research_workflow::paper::Claim {
            text: "Introduction text".into(),
            source_ids: vec!["S-1".into()],
            result_ids: vec![],
            metric: None,
            value: None,
        }],
        reviewed_hash: Some("failed-attempt".into()),
        pdf: None,
        manifest: None,
        compile_attempts: 1,
    });
    std::fs::write(
        dir.path().join(".kanzei/research/shared/workflow.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let repaired = tool
        .execute(
            json!({"action":"repair_paper","topic":"shared","content":"Revised Introduction text"}),
            &ctx,
        )
        .await;
    assert!(!repaired.is_error, "{}", repaired.content);
    let recovered = crate::research_workflow::load(dir.path(), "shared")
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.stage,
        crate::research_workflow::Stage::ReviewPaper
    );
    assert!(recovered.paper.unwrap().reviewed_hash.is_none());
    assert!(
        tool.execute(json!({"action":"compile_paper","topic":"shared"}), &ctx)
            .await
            .is_error
    );
    assert!(!latex.join("compile-log.txt").exists());
}
