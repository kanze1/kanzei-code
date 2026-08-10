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
        // 与 bash 同一道收尾:上一个 run 遗留的后台任务在被观察/操作之前先收掉,
        // 保证「后台任务生命周期 ⊆ owner run」这条不变量在每个入口都成立。
        crate::background::finish_foreign_owners(&ctx.project_root, ctx.run_id.as_deref()).await;
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
                    // D-174:owner 与越界计数是这行的重点——没有它们,"谁起的、
                    // 它动过托管文档没有"就只能靠猜。
                    let breaches = p.breaches().len();
                    let fence = if breaches == 0 {
                        String::new()
                    } else {
                        format!(" managed-breaches={breaches}")
                    };
                    out.push_str(&format!(
                        "{} [{}] pid={} owner={} cwd={}{} :: {}\n",
                        p.id,
                        state,
                        p.pid().map_or_else(|| "-".to_string(), |v| v.to_string()),
                        p.owner.run_id,
                        p.workdir,
                        fence,
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
                let rendered = format!("{head}{body}{}", breach_report(&p));
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

/// 越界写入的归因报告。空 = 该后台任务没碰过托管路径。
///
/// 报告必须点名 owner:后台任务的越界是异步发生的,模型看到它时早已不在当初那次
/// 工具调用的上下文里,不写清"谁在什么时候写了什么、改后的内容留在哪"就没法追。
fn breach_report(process: &crate::background::BackgroundProcess) -> String {
    let breaches = process.breaches();
    if breaches.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "\n[managed-files] this background task wrote policy-managed paths {} time(s); \
         every write was quarantined and rolled back, and the process tree was killed. \
         owner run={} process={}, started at {}ms, command: {}\n",
        breaches.len(),
        process.owner.run_id,
        process.owner.process_id,
        process.started_at_ms,
        process.command,
    );
    for breach in &breaches {
        out.push_str(&format!(
            "  at {}ms touched: {} — restored {} file(s), your versions kept at {}\n",
            breach.at_ms,
            breach.touched.join(", "),
            breach.restored,
            breach.quarantine,
        ));
    }
    out.push_str(
        "The shell is not a write channel for .kanzei/project or .kanzei/memory, background or \
         not. Redo the change through the dedicated tool (`req`/`defect`/`goal`/`decision`, \
         `architecture`, `test_record`, `memory_*`).",
    );
    out
}
