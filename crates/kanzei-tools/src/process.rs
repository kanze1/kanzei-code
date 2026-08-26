//! process 工具(R-097):查看与终止 bash background 托管的后台进程。
//!
//! 权限:list/output 是只读的;stop 会终止进程树,按 `process stop <命令>` 走门禁,
//! 让"能起后台进程"和"能杀后台进程"分开授权。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct ProcessInput {
    /// list | output | stop | discover | adopt | kill | wait
    action: String,
    /// output/stop/adopt/kill/wait 必填:bash background 返回的 process_id
    #[serde(default)]
    id: Option<String>,
    /// R-330(wait):等到输出里出现匹配此正则的内容。省略 = 等进程退出。
    #[serde(default)]
    until: Option<String>,
    /// R-330(wait):最长等多久(秒)。默认 60,上限 600。
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// wait 的轮询间隔。够快到「ready 了立刻回」,又不至于把 CPU 烧在自旋上。
const WAIT_POLL_MS: u64 = 200;
/// wait 的墙钟上限。无界等待会把整条 run 挂死在一个永远不出现的字符串上。
const WAIT_MAX_SECS: u64 = 600;
const WAIT_DEFAULT_SECS: u64 = 60;
/// 回给模型的输出尾部字符数。等待的结论在尾部,不是开头。
const WAIT_TAIL_CHARS: usize = 4000;

pub struct ProcessTool;

#[async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &'static str {
        "process"
    }

    fn description(&self) -> String {
        "Inspect background processes started by `bash` with background=true. \
         Actions: list (all processes of this project with state and exit code), \
         output(id) (captured stdout+stderr so far, tail-truncated), stop(id) (terminate the process tree), \
         discover (R-180: list persistent services registered in previous runs — the ones that \
         survived their owner run; each is either running and can be adopted/killed, or dead and \
         gets marked failed and pruned, leaving no ghost entries), \
         adopt(id) (take over a running persistent service into this run's registry), \
         kill(id) (terminate a registered persistent service's process tree and remove its entry),          wait(id, until?, timeout_secs?) (block until the output matches the `until` regex, or —          with no `until` — until the process exits; returns matched/exited/timeout plus the output          tail). Use `wait` instead of calling `output` in a loop: each poll costs a full model          round trip, `wait` costs one. Typical: start a dev server with bash background=true,          then wait for its ready line before driving the UI."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(ProcessInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        // 只读动作不需要单独授权面;stop/kill 带上目标命令,便于规则精确到"杀什么"。
        let action = input["action"].as_str().unwrap_or("");
        if !matches!(action, "stop" | "kill") {
            return vec![format!("process {action}")];
        }
        let target = input["id"]
            .as_str()
            .and_then(crate::background::get)
            .map(|p| p.command.clone())
            .unwrap_or_else(|| "unknown".into());
        vec![format!("process {action} {target}")]
    }

    /// R-330 + R-323 B2:按动作分流并发契约。
    ///
    /// `wait` 会占住一个 wave 槽最长 600 秒,再走 `Exclusive` 默认就等于让一次
    /// 「等 dev server 起来」把整批工具调用全部堵死。观察类动作(list/output/
    /// discover/wait)只读注册表与输出缓冲,标 `Shared`;改注册表与进程树的
    /// (stop/kill/adopt)保持独占。
    fn concurrency(&self, input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        match input.get("action").and_then(|v| v.as_str()) {
            Some("list" | "output" | "discover" | "wait") => ToolConcurrency::shared_worktree(ctx),
            _ => ToolConcurrency::Exclusive,
        }
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
                // R-180 B2:persistent 服务读内存尾部(有界);完整日志通过 log_path
                // 从磁盘回看。非 persistent 也维持内存尾部。
                let body = if p.persistent {
                    p.full_log()
                } else {
                    p.output()
                };
                let body = if body.trim().is_empty() {
                    "(no output yet)".to_string()
                } else {
                    body
                };
                let log_hint = p
                    .log_path
                    .as_ref()
                    .map(|path| format!("\n[persistent log on disk: {}]", path.display()))
                    .unwrap_or_default();
                let head = if p.truncated() {
                    let hint = if p.persistent {
                        "[memory output bounded — showing tail; complete persistent log is on disk]"
                    } else {
                        "[earlier output dropped — showing tail]"
                    };
                    format!("state: {state}\n{hint}\n")
                } else {
                    format!("state: {state}\n")
                };
                let rendered = format!("{head}{body}{log_hint}{}", breach_report(&p));
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
                    if managed_process_id(id) {
                        return ToolOutput::ok(format!(
                            "{id} is no longer active (it already exited or was cleaned up); nothing to stop. Use action=list to see live ids."
                        ));
                    }
                    return ToolOutput::needs_correction(
                        "PROCESS_STOP_BAD_ID",
                        format!(
                            "invalid process id `{id}`; expected the bg<number> id returned by bash background=true. Use action=list to see live ids"
                        ),
                    );
                }
                if crate::background::stop(id).await {
                    ToolOutput::ok(format!("stopped {id}"))
                } else {
                    ToolOutput::ok(format!("{id} was already finished"))
                }
            }
            // R-180 B3 验收②:跨 run 注册表——列出上次未终结的长驻服务并给确定处置。
            "wait" => {
                let Some(id) = input.id.as_deref() else {
                    return ToolOutput::needs_correction(
                        "PROCESS_WAIT_NO_ID",
                        "wait 需要 id(bash background 返回的 process_id)",
                    );
                };
                wait_for(id, input.until.as_deref(), input.timeout_secs).await
            }
            "discover" => {
                let items = crate::background::discover_persistent(&ctx.project_root);
                if items.is_empty() {
                    return ToolOutput::ok(
                        "(no persistent services registered from previous runs)",
                    );
                }
                let mut out = String::new();
                for (entry, alive) in items {
                    let state = if alive {
                        "running"
                    } else {
                        // pid 已死 = 强杀后进程没能活下来,标失败并清出注册表,
                        // 不留幽灵条目(验收②)。
                        crate::background::mark_registry_failed(&ctx.project_root, &entry.id);
                        "failed (pruned)"
                    };
                    out.push_str(&format!(
                        "{} [{}] pid={} owner={} started={} log={} :: {}\n",
                        entry.id,
                        state,
                        entry.pid,
                        entry.owner.run_id,
                        entry.started_at_ms,
                        entry.log,
                        entry.command,
                    ));
                }
                out.push_str(
                    "use {\"action\":\"adopt\",\"id\":\"<id>\"} to take over a running service, \
                     {\"action\":\"kill\",\"id\":\"<id>\"} to terminate it and remove its entry",
                );
                ToolOutput::ok(out)
            }
            "adopt" => {
                let Some(id) = input.id.as_deref() else {
                    return ToolOutput::error("adopt requires `id`");
                };
                match crate::background::adopt_persistent(&ctx.project_root, id).await {
                    Some(process) => {
                        let log_hint = process
                            .log_path
                            .as_ref()
                            .map(|path| format!("\n[persistent log on disk: {}]", path.display()))
                            .unwrap_or_default();
                        ToolOutput::ok(format!(
                            "adopted {id} (pid={}) — now managed by this run; use output to read its log, stop to terminate.{log_hint}",
                            process.pid().map_or("-".into(), |v| v.to_string())
                        ))
                    }
                    None => ToolOutput::error(format!(
                        "cannot adopt `{id}`: not in the cross-run registry or its process is no longer alive"
                    )),
                }
            }
            "kill" => {
                let Some(id) = input.id.as_deref() else {
                    return ToolOutput::error("kill requires `id`");
                };
                if crate::background::kill_registered(&ctx.project_root, id).await {
                    ToolOutput::ok(format!(
                        "killed registered persistent service {id} and removed its entry"
                    ))
                } else {
                    ToolOutput::error(format!("unknown registered persistent service `{id}`"))
                }
            }
            other => ToolOutput::error(format!(
                "unknown action `{other}`; use list | output | stop | wait | discover | adopt | kill"
            )),
        }
    }
}

fn managed_process_id(id: &str) -> bool {
    id.strip_prefix("bg").is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
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
         not. Redo the change through the dedicated tool (`req`/`defect`/`idea`/`decision`, \
         `architecture`, `test_record`, `memory_*`).",
    );
    out
}

/// R-330:等后台进程满足条件,**在工具内部轮询**。
///
/// 这是它存在的全部理由:模型此前只能反复调 `output` 自己比对,每次一个完整的
/// 模型往返。等一个 dev server 起来要花掉五六轮,而这五六轮除了「还没好」什么
/// 信息都没产生。轮询挪进工具里之后,同一件事是一次调用。
///
/// 三种终态各自说清楚,不含糊:
/// - `matched`:命中行原文一并给出——模型要据此判断是不是它想等的那一行;
/// - `exited`:带退出码。**进程退出优先于超时**:已经退出就不该报「超时」;
/// - `timeout`:如实说没等到,并给出尾部,让模型自己看是卡在哪。
///
/// 匹配范围是**全部已捕获输出**而不是「调用之后的新增」:等待的语义是
/// 「这个条件成立了吗」,而不是「它再发生一次」。ready 行在调用前就打出来了,
/// 立刻返回才是对的——否则会永远等一个不会重复的一次性事件。
async fn wait_for(id: &str, until: Option<&str>, timeout_secs: Option<u64>) -> ToolOutput {
    let Some(process) = crate::background::get(id) else {
        return ToolOutput::failed(
            "PROCESS_NOT_FOUND",
            format!("no background process `{id}`; use action=list to see live ids"),
        );
    };
    let pattern = match until.map(regex_lite_compile) {
        Some(Ok(pattern)) => Some(pattern),
        Some(Err(error)) => {
            return ToolOutput::needs_correction("PROCESS_WAIT_BAD_REGEX", error);
        }
        None => None,
    };
    let budget = timeout_secs
        .unwrap_or(WAIT_DEFAULT_SECS)
        .clamp(1, WAIT_MAX_SECS);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(budget);

    loop {
        let output = process.output();
        if let Some(pattern) = &pattern {
            let hit = output.lines().rev().find(|line| pattern.is_match(line));
            if let Some(line) = hit {
                return ToolOutput::ok(format!(
                    "[wait matched] {id} matched `{}`\nline: {}\n--- tail ---\n{}",
                    until.unwrap_or_default(),
                    line.trim_end(),
                    tail(&output)
                ));
            }
        }
        // 退出判定放在匹配之后:进程可能在打出目标行之后立刻退出(一次性命令),
        // 那种情况该报 matched 而不是 exited。
        if let Some(code) = process.exit_code() {
            let state = match code {
                Some(code) => format!("exited({code})"),
                None => "terminated".to_string(),
            };
            let verdict = if pattern.is_some() {
                "[wait ended without match]"
            } else {
                "[wait exited]"
            };
            return ToolOutput::ok(format!(
                "{verdict} {id} {state}\n--- tail ---\n{}",
                tail(&output)
            ));
        }
        if std::time::Instant::now() >= deadline {
            return ToolOutput::ok(format!(
                "[wait timeout] {id} still running after {budget}s{}\n--- tail ---\n{}",
                until
                    .map(|u| format!(", never matched `{u}`"))
                    .unwrap_or_default(),
                tail(&output)
            ));
        }
        // 进度让人看得见「在等什么」——长静默是最容易被误当卡死的形态。
        kanzei_harness::progress::emit(&format!("waiting on {id}…"));
        tokio::time::sleep(std::time::Duration::from_millis(WAIT_POLL_MS)).await;
    }
}

fn tail(output: &str) -> String {
    let chars: Vec<char> = output.chars().collect();
    if chars.len() <= WAIT_TAIL_CHARS {
        return output.trim_end().to_string();
    }
    chars[chars.len() - WAIT_TAIL_CHARS..]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// `until` 的匹配器。用 `regex` 而不是 grep 那套 `RegexMatcher`:后者的
/// `Matcher` trait 面向字节流搜索(需要 `grep-matcher` 才能调 `is_match`),
/// 而这里只是对一行字符串做一次判定,`regex::Regex` 直接、依赖也更少。
fn regex_lite_compile(pattern: &str) -> Result<regex::Regex, String> {
    regex::Regex::new(pattern)
        .map_err(|error| format!("invalid `until` regex `{pattern}`: {error}"))
}

#[cfg(test)]
mod wait_tests {
    use super::{managed_process_id, regex_lite_compile, tail, WAIT_MAX_SECS, WAIT_TAIL_CHARS};
    use kanzei_harness::{Tool, ToolConcurrency, ToolCtx};
    use serde_json::json;

    fn ctx() -> ToolCtx {
        ToolCtx::new(
            std::path::PathBuf::from("/repo/wt"),
            std::path::PathBuf::from("/repo/wt"),
        )
    }

    /// wait 最长占住一个 wave 槽 600 秒。走 Exclusive 默认等于让一次
    /// 「等 dev server 起来」把整批工具调用堵死,所以它必须是 Shared。
    #[test]
    fn 观察类动作可并行_变更类动作独占() {
        let tool = super::ProcessTool;
        let ctx = ctx();
        for action in ["list", "output", "discover", "wait"] {
            let c = tool.concurrency(&json!({"action": action}), &ctx);
            assert!(
                matches!(c, ToolConcurrency::Shared(_)),
                "{action} 应可并行,实得 {c:?}"
            );
        }
        for action in ["stop", "kill", "adopt"] {
            let c = tool.concurrency(&json!({"action": action}), &ctx);
            assert_eq!(c, ToolConcurrency::Exclusive, "{action} 必须独占");
        }
        // 未知动作保守独占,不因为没列举到就被误判成可并行。
        assert_eq!(
            tool.concurrency(&json!({"action": "future"}), &ctx),
            ToolConcurrency::Exclusive
        );
    }

    #[test]
    fn until_正则非法时给可行动错误() {
        let err = regex_lite_compile("[unclosed").unwrap_err();
        assert!(err.contains("invalid `until` regex"), "{err}");
        assert!(regex_lite_compile(r"listening on \d+").is_ok());
    }

    /// 等待的结论在尾部不在开头:超长输出保留尾部。
    #[test]
    fn 尾部截断保留末尾() {
        let short = "ready\n";
        assert_eq!(tail(short), "ready");
        let long: String = std::iter::repeat_n('x', WAIT_TAIL_CHARS + 500)
            .chain("END".chars())
            .collect();
        let cut = tail(&long);
        assert!(cut.ends_with("END"), "必须保留末尾");
        assert!(cut.chars().count() <= WAIT_TAIL_CHARS, "长度受限");
    }

    #[test]
    fn 超时预算封顶() {
        assert_eq!(WAIT_MAX_SECS, 600);
        // clamp 语义:0 抬到 1,超限压回上限。无界等待会把整条 run 挂死在一个
        // 永远不出现的字符串上。
        assert_eq!(0u64.clamp(1, WAIT_MAX_SECS), 1);
        assert_eq!(99_999u64.clamp(1, WAIT_MAX_SECS), WAIT_MAX_SECS);
    }

    #[tokio::test]
    async fn 进程不存在时点名可用动作() {
        let out = super::wait_for("nope", None, Some(1)).await;
        assert_eq!(out.code, Some("PROCESS_NOT_FOUND"));
        assert!(
            out.content.contains("action=list"),
            "要指路: {}",
            out.content
        );
    }

    #[tokio::test]
    async fn stop已回收bg句柄幂等_非法id仍拒绝() {
        assert!(managed_process_id("bg1"));
        assert!(managed_process_id("bg999999999"));
        assert!(!managed_process_id("bg"));
        assert!(!managed_process_id("process-1"));

        let tool = super::ProcessTool;
        let stopped = tool
            .execute(json!({"action": "stop", "id": "bg999999999"}), &ctx())
            .await;
        assert!(!stopped.is_error, "{}", stopped.content);
        assert!(stopped.content.contains("no longer active"));
        assert!(stopped.content.contains("nothing to stop"));

        let invalid = tool
            .execute(json!({"action": "stop", "id": "process-1"}), &ctx())
            .await;
        assert!(invalid.is_error);
        assert_eq!(invalid.code, Some("PROCESS_STOP_BAD_ID"));
        assert!(invalid.content.contains("bg<number>"));
    }
}
