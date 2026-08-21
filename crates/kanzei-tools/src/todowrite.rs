//! todowrite 工具(R-028):维护当前 agent 运行内的结构化计划，不写入项目 backlog。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const MAX_ITEMS: usize = 30;

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct TodoInput {
    /// 当前计划的完整列表；每次调用整体替换。
    todos: Vec<TodoItem>,
}

#[derive(Deserialize, JsonSchema, serde::Serialize)]
#[allow(dead_code)]
struct TodoItem {
    /// 稳定的计划项编号。
    id: String,
    /// 计划项内容。
    content: String,
    /// todo、doing、done 或 dropped。
    status: String,
}

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &'static str {
        "todowrite"
    }

    fn description(&self) -> String {
        "Update the current run plan. Params: todos, a complete list of {id, content, status}; status is todo, doing, done, or dropped.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(TodoInput)).unwrap()
    }

    fn resources(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }

    /// R-323 并发审计:**完全无状态**——校验入参后直接回一个 display,
    /// 不落盘、不写注册表、不碰 session。并行任意多个都安全。
    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
        let input: TodoInput = match crate::parse_input(self, input) {
            Ok(value) => value,
            Err(output) => return output,
        };
        if input.todos.len() > MAX_ITEMS {
            return ToolOutput::error(format!("too many todo items; maximum is {MAX_ITEMS}"));
        }
        if input
            .todos
            .iter()
            .any(|item| item.id.trim().is_empty() || item.content.trim().is_empty())
        {
            return ToolOutput::error("todo id and content must not be empty");
        }
        if input
            .todos
            .iter()
            .any(|item| !matches!(item.status.as_str(), "todo" | "doing" | "done" | "dropped"))
        {
            return ToolOutput::error("todo status must be todo, doing, done, or dropped");
        }
        let done = input
            .todos
            .iter()
            .filter(|item| item.status == "done")
            .count();
        let total = input.todos.len();
        let content = format!("Plan updated: {done}/{total} done");
        ToolOutput::ok(content).with_display(serde_json::json!({
            "kind": "todo",
            "items": input.todos,
            "done": done,
            "total": total,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::TodoWriteTool;
    use kanzei_harness::{Tool, ToolCtx};
    use serde_json::json;

    #[tokio::test]
    async fn returns_bounded_todo_display() {
        let output = TodoWriteTool
            .execute(
                json!({"todos": [{"id": "1", "content": "检查", "status": "done"}]}),
                &ToolCtx::discovering(std::env::current_dir().unwrap()),
            )
            .await;
        assert!(!output.is_error);
        assert_eq!(output.display.unwrap()["kind"], "todo");
    }
}
