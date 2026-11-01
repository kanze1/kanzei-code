use super::{model::*, store::Store};
use kanzei_harness::{Component, HarnessDraft, ResolveCtx, Tool, ToolCtx, ToolOutput};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct InitiativeComponent;
impl Component for InitiativeComponent {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if !kanzei_harness::is_social_conversation(&ctx.project_root) {
            return Ok(());
        }
        draft.tools.insert("social_plan", Arc::new(PlanTool));
        for name in ["general", "dev-pair", "dev-auto", "isolated"] {
            if let Some(mut agent) = draft.agents.get(name).cloned() {
                agent.system.push_str("\n用 social_plan 保存当前会话里尚想继续的话题、好奇的问题、值得分享的内容或需要跟进的事，并保留聊天依据或来源。记录的是意图，不是准备定时发出的成品消息，也不等同于已确认的长期记忆。对方已回答、话题已过时或改变主意时关闭或修订原计划。可以自己发现新话题，不只登记提醒。只有原会话能读取和修改这些话题；不要把别的群或私聊的内容搬进来。");
                draft.agents.insert(name, agent);
            }
        }
        Ok(())
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    List,
    Remember {
        plan: Plan,
    },
    Revise {
        id: String,
        revision: u64,
        plan: Plan,
    },
    Close {
        id: String,
        revision: u64,
    },
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanCall {
    operation: Input,
}
struct PlanTool;
#[async_trait::async_trait]
impl Tool for PlanTool {
    fn name(&self) -> &'static str {
        "social_plan"
    }
    fn description(&self) -> String {
        "记录、读取、修订或关闭原会话的主动话题意图。可记跟进、好奇、分享和新发现；保存主题与依据，不保存待发送正文。not_before/expires 可为空，非空为 Unix 毫秒。修改必须带当前 revision。不能选择其他会话或群号。".into()
    }
    fn input_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(PlanCall)).unwrap()
    }
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        match execute(input, ctx) {
            Ok(value) => ToolOutput::ok(value.to_string()),
            Err(error) => ToolOutput::error(format!("{error:#}")),
        }
    }
}
fn execute(input: Value, ctx: &ToolCtx) -> anyhow::Result<Value> {
    let input: PlanCall = serde_json::from_value(input)?;
    let identity = super::identity(ctx)?;
    let run = ctx
        .run_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("主动话题缺少来源运行"))?;
    let now = chrono::Utc::now().timestamp_millis();
    let store = super::open_store()?;
    store.register(identity.clone(), None, Settings::default(), now)?;
    execute_scoped(&store, &identity.session_id, run, input.operation, now)
}
fn execute_scoped(
    store: &Store,
    session: &str,
    run: &str,
    input: Input,
    now: i64,
) -> anyhow::Result<Value> {
    Ok(match input {
        Input::List => serde_json::to_value(store.topics(session)?)?,
        Input::Remember { plan } => serde_json::to_value(store.remember(session, plan, run, now)?)?,
        Input::Revise { id, revision, plan } => serde_json::to_value(store.revise_topic(
            session,
            &id,
            revision,
            Some(plan),
            run,
            now,
        )?)?,
        Input::Close { id, revision } => {
            serde_json::to_value(store.revise_topic(session, &id, revision, None, run, now)?)?
        }
    })
}
