//! process 工具(R-097):查看与终止 bash background 托管的后台进程。
//!
//! 权限:list/output 是只读的;stop 会终止进程树,按 `process stop <命令>` 走门禁,
//! 让"能起后台进程"和"能杀后台进程"分开授权。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct ProcessInput {
    /// list | output | stop
    action: String,
    /// output/stop 必填:bash background 返回的 process_id
    #[serde(default)]
    id: Option<String>,
}

pub struct ProcessTool;

#[async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &'static str {
        "process"
    }

    fn description(&self) -> String {
        "Inspect background processes started by `bash` with background=true. \
         Actions: list (all processes of this project with state and exit code), \
         output(id) (captured stdout+stderr so far, tail-truncated), stop(id) (terminate the process tree)."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(ProcessInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        // 只读动作不需要单独授权面;stop 带上目标命令,便于规则精确到"杀什么"。
        let action = input["action"].as_str().unwrap_or("");
        if action != "stop" {
            return vec![format!("process {action}")];
        }
        let target = input["id"]
            .as_str()
            .and_then(crate::background::get)
            .map(|p| p.command.clone())
            .unwrap_or_else(|| "unknown".into());
        vec![format!("process stop {target}")]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: ProcessInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        match input.action.as_str() {
            "list" => {
                let items = crate::background::list(&ctx.project_root);
                if items.is_empty() {
                    return ToolOutput::ok("(no background processes)");
                }
                let mut out = String::new();
                for p in items {
                    let state = match p.exit_code() {
                        None => "running".to_string(),
                        Some(Some(code)) => format!("exited({code})"),
                        Some(None) => "terminated".to_string(),
                    };
                    out.push_str(&format!(
                        "{} [{}] pid={} cwd={} :: {}\n",
                        p.id,
                        state,
                        p.pid().map_or_else(|| "-".to_string(), |v| v.to_string()),
                        p.workdir,
                        p.command,
                    ));
                }
                ToolOutput::ok(out)
            }
            "output" => {
                let Some(id) = input.id.as_deref() else {
                    return ToolOutput::error("output requires `id`");
                };
                let Some(p) = crate::background::get(id) else {
                    return ToolOutput::error(format!("unknown process id `{id}`"));
                };
                let state = match p.exit_code() {
                    None => "running".to_string(),
                    Some(Some(code)) => format!("exited({code})"),
                    Some(None) => "terminated".to_string(),
                };
                let body = p.output();
                let body = if body.trim().is_empty() {
                    "(no output yet)".to_string()
                } else {
                    body
                };
                let head = if p.truncated() {
                    format!("state: {state}\n[earlier output dropped — showing tail]\n")
                } else {
                    format!("state: {state}\n")
                };
                let rendered = format!("{head}{body}");
                ToolOutput::ok(rendered.clone()).with_display(serde_json::json!({
                    "kind": "terminal",
                    "command": p.command,
                    "background": true,
                    "processId": p.id,
                    "output": rendered.chars().take(4000).collect::<String>(),
                }))
            }
            "stop" => {
                let Some(id) = input.id.as_deref() else {
                    return ToolOutput::error("stop requires `id`");
                };
                if crate::background::get(id).is_none() {
                    return ToolOutput::error(format!("unknown process id `{id}`"));
                }
                if crate::background::stop(id).await {
                    ToolOutput::ok(format!("stopped {id}"))
                } else {
                    ToolOutput::ok(format!("{id} was already finished"))
                }
            }
            other => ToolOutput::error(format!(
                "unknown action `{other}`; use list | output | stop"
            )),
        }
    }
}
