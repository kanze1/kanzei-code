use std::env;
use std::path::PathBuf;

use kanzei_harness::Tool;
use kanzei_tools::research_index::ResearchIndexTool;
use kanzei_tools::research_loop::ResearchLoopTool;
use kanzei_tools::research_plan::{
    approve_plan, save_plan, PlanBudget, PlanNode, PlanNodeStatus, PlanStatus, ResearchPlan,
};
use kanzei_tools::research_write::ResearchWriteTool;
use serde_json::json;

fn smoke_plan(topic: &str) -> ResearchPlan {
    ResearchPlan {
        version: 1,
        topic: topic.into(),
        title: "Research writing acceptance smoke".into(),
        status: PlanStatus::AwaitingApproval,
        open_questions: Vec::new(),
        nodes: vec![PlanNode {
            id: "write".into(),
            title: "Prepare a citable writing artifact".into(),
            objective: "Exercise the approved loop and writing consumer".into(),
            status: PlanNodeStatus::Pending,
            depends_on: Vec::new(),
            children: Vec::new(),
        }],
        budget: PlanBudget {
            max_rounds: 1,
            max_tokens: 1_000,
            max_concurrency: 1,
        },
        revision: 1,
    }
}

async fn execute_or_exit<T: Tool>(
    tool: &T,
    input: serde_json::Value,
    ctx: &kanzei_harness::ToolCtx,
) {
    let output = tool.execute(input, ctx).await;
    println!("{}", output.content);
    if output.is_error {
        std::process::exit(1);
    }
}

#[tokio::main]
async fn main() {
    let project = env::var_os("KZ_SMOKE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("current directory"));
    let topic = env::var("KZ_SMOKE_TOPIC").expect("KZ_SMOKE_TOPIC");
    let action = env::args().nth(1).unwrap_or_else(|| "index_build".into());
    let cwd = env::current_dir().expect("cwd");
    let ctx = kanzei_harness::ToolCtx::new(project.clone(), cwd);

    match action.as_str() {
        "prepare-write" => {
            save_plan(&project, &smoke_plan(&topic)).expect("save smoke plan");
            approve_plan(&project, &topic).expect("approve smoke plan");
            execute_or_exit(
                &ResearchLoopTool,
                json!({ "action": "start", "topic": topic }),
                &ctx,
            )
            .await;
            execute_or_exit(
                &ResearchLoopTool,
                json!({ "action": "begin_search", "topic": topic }),
                &ctx,
            )
            .await;
            execute_or_exit(
                &ResearchLoopTool,
                json!({
                    "action": "add_evidence",
                    "topic": topic,
                    "task_id": "r0-t0",
                    "summary": "The approved smoke plan produced a bounded, source-linked writing input.",
                    "relevance": 1.0,
                    "source_ids": ["S-SMOKE"]
                }),
                &ctx,
            )
            .await;
            execute_or_exit(
                &ResearchLoopTool,
                json!({ "action": "reflect", "topic": topic, "gaps": [] }),
                &ctx,
            )
            .await;
        }
        "write-heavy" => {
            let tool = ResearchWriteTool;
            execute_or_exit(
                &tool,
                json!({
                    "action": "write_outline",
                    "topic": topic,
                    "title": "Research writing acceptance smoke",
                    "sections": [{
                        "id": "evidence",
                        "title": "Evidence",
                        "objective": "State the bounded smoke result",
                        "source_ids": ["S-SMOKE"]
                    }]
                }),
                &ctx,
            )
            .await;
            execute_or_exit(
                &tool,
                json!({
                    "action": "write_section",
                    "topic": topic,
                    "section_id": "evidence",
                    "content": "The approved loop reached synthesis and preserved the source marker."
                }),
                &ctx,
            )
            .await;
            execute_or_exit(
                &tool,
                json!({ "action": "assemble_paper", "topic": topic }),
                &ctx,
            )
            .await;
            execute_or_exit(
                &tool,
                json!({ "action": "compile_paper", "topic": topic }),
                &ctx,
            )
            .await;
        }
        _ => {
            execute_or_exit(
                &ResearchIndexTool,
                json!({ "action": action, "topic": topic }),
                &ctx,
            )
            .await;
        }
    }
}
