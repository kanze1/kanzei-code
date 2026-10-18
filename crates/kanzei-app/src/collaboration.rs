//! R-184 A 面:运行中各线共享的协作真源、轮内刷新上下文与主动查询工具。

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kanzei_harness::{refreshing_source, rule, Component, Effect, HarnessDraft, ResolveCtx, Tool};
use serde::Serialize;

use crate::state::hidden_command;
use crate::{process_session_id, ProcessHandle, SessionRuntime};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CollaborationLine {
    pub(crate) process_id: String,
    pub(crate) label: String,
    pub(crate) branch: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) claim: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) claim_error: Option<String>,
    pub(crate) phase: String,
    pub(crate) current_tool: Option<String>,
    pub(crate) running: bool,
    pub(crate) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) event_idle_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) worktree_idle_ms: Option<u64>,
    pub(crate) steps: u32,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) changed_files_error: Option<String>,
    /// R-176 B5(验收⑥):当前持写权者(协调器快照 writer_run_id,数据来自
    /// coordinator.snapshot() 而非前端推测)。None = 该代码树无 writer。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) writer_run_id: Option<String>,
    /// R-176 B5(验收⑥):该代码树等待写租约的排队者(协调器快照 waiting_writers)。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) waiting_writers: Vec<String>,
}

/// 只持 AppState 内可克隆的共享字段,供 harness source/tool 在 run_task 生命周期内读取。
#[derive(Clone)]
pub(crate) struct CollaborationProbe {
    processes: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    origin_project: PathBuf,
    current_process_id: String,
    /// R-176 B5(验收⑥):项目级协调器——writer/waiting 数据真源,面板只读快照。
    coordinator: Option<Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>>,
}

const LINE_STALE_AFTER_MS: u64 = 30_000;

#[derive(Default)]
enum LineWait {
    #[default]
    None,
    Approval,
    Answer,
}

fn pending_wait(runtime: &SessionRuntime) -> LineWait {
    let asks = runtime.asks.lock().unwrap();
    if asks
        .values()
        .any(|ask| matches!(ask.request, kanzei_core::AskRequest::Permission { .. }))
    {
        LineWait::Approval
    } else if asks.values().any(|ask| {
        matches!(
            ask.request,
            kanzei_core::AskRequest::Question {
                background: false,
                ..
            }
        )
    }) {
        LineWait::Answer
    } else {
        LineWait::None
    }
}

fn open_durable_store(root: &Path) -> Result<Option<kanzei_core::SessionStore>, String> {
    let path = kanzei_core::project_state_path(root);
    match std::fs::metadata(&path) {
        Ok(_) => kanzei_core::SessionStore::open_read_only(&path)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn durable_outcome(
    store: &kanzei_core::SessionStore,
    session: &str,
) -> Result<Option<String>, String> {
    let input = store
        .latest_input_status(session)
        .map_err(|error| error.to_string())?;
    if let Some((input_id, status)) = &input {
        match status.as_str() {
            "pending" => return Ok(Some("queued".into())),
            "promoted" | "running" => return Ok(Some("interrupted".into())),
            "cancelled" => return Ok(Some("halted".into())),
            "completed" => return Ok(Some("completed".into())),
            "failed" => {
                let boundary = store
                    .latest_run_boundary(session)
                    .map_err(|error| error.to_string())?;
                let incomplete = boundary.is_some_and(|event| {
                    event.payload["input_id"].as_str() == Some(input_id.as_str())
                        && event.payload["outcome"].as_str() == Some("incomplete")
                });
                return Ok(Some(
                    if incomplete { "incomplete" } else { "failed" }.into(),
                ));
            }
            other => return Err(format!("unsupported persisted input status: {other}")),
        }
    }
    let Some(event) = store
        .latest_run_boundary(session)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if event.event_type == "session.turn_started" {
        return Ok(Some("interrupted".into()));
    }
    if event.payload["halted_by_user"].as_bool() == Some(true) {
        return Ok(Some("halted".into()));
    }
    match event.payload["outcome"].as_str() {
        Some(outcome @ ("completed" | "halted" | "incomplete" | "failed")) => {
            Ok(Some(outcome.into()))
        }
        Some(outcome) => Err(format!("unsupported persisted run outcome: {outcome}")),
        None => Ok(Some(
            if event.event_type == "run.failed" {
                "failed"
            } else {
                "completed"
            }
            .into(),
        )),
    }
}

#[derive(Default)]
struct LineProgress<'a> {
    running: bool,
    waiting_for: LineWait,
    run_id: &'a str,
    waiting_writers: &'a [String],
    run_age_ms: Option<u64>,
    last_outcome: Option<&'a str>,
    event_idle_ms: Option<u64>,
    worktree_idle_ms: Option<u64>,
}

fn line_status(progress: LineProgress<'_>) -> &'static str {
    let LineProgress {
        running,
        waiting_for,
        run_id,
        waiting_writers,
        run_age_ms,
        last_outcome,
        event_idle_ms,
        worktree_idle_ms,
    } = progress;
    if running {
        match waiting_for {
            LineWait::Approval => return "awaiting_approval",
            LineWait::Answer => return "awaiting_answer",
            LineWait::None => {}
        }
        if waiting_writers.iter().any(|waiting| waiting == run_id) {
            return "queued";
        }
        if run_age_ms.is_none() {
            return "starting";
        }
        let recent_event = event_idle_ms.is_some_and(|age| age <= LINE_STALE_AFTER_MS);
        let recent_worktree = worktree_idle_ms.is_some_and(|age| age <= LINE_STALE_AFTER_MS);
        // 新 run 尚未收到第一条事件时给一个启动宽限；之后必须有事件流或源码现场进展。
        if recent_event
            || recent_worktree
            || (event_idle_ms.is_none() && run_age_ms.is_some_and(|age| age <= LINE_STALE_AFTER_MS))
        {
            "running"
        } else {
            "suspected_stuck"
        }
    } else {
        match last_outcome {
            Some("completed") => "completed",
            Some("halted") => "stopped",
            Some("incomplete") => "incomplete",
            Some("interrupted") => "interrupted",
            Some("queued") => "queued",
            Some("failed") => "failed",
            None if run_age_ms.is_none() => "idle",
            None => "failed",
            Some(_) => "unavailable",
        }
    }
}

fn elapsed_ms(at: Option<std::time::Instant>) -> Option<u64> {
    at.map(|instant| instant.elapsed().as_millis() as u64)
}

fn latest_worktree_idle_ms(code_root: &Path, files: &[String]) -> Option<u64> {
    files
        .iter()
        .filter_map(|file| {
            std::fs::metadata(code_root.join(file))
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| {
                    std::time::SystemTime::now()
                        .duration_since(modified)
                        .ok()
                        .map(|age| age.as_millis() as u64)
                })
        })
        .min()
}

impl CollaborationProbe {
    pub(crate) fn new(
        processes: Arc<Mutex<HashMap<String, ProcessHandle>>>,
        runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
        origin_project: PathBuf,
        current_process_id: String,
    ) -> Self {
        Self {
            processes,
            runtimes,
            origin_project,
            current_process_id,
            coordinator: None,
        }
    }

    /// R-176 B5:注入协调器快照源。装配点在 run_task/协作快照 IPC。
    pub(crate) fn with_coordinator(
        mut self,
        coordinator: Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>,
    ) -> Self {
        self.coordinator = Some(coordinator);
        self
    }

    /// 当前项目里其它正在运行的线。锁内只克隆句柄;git 与 live 采样全部在锁外。
    pub(crate) fn active_others(&self) -> Vec<CollaborationLine> {
        self.collect_lines(false, true)
    }

    /// 前端并列视图读取当前项目全部线(含 idle)。与 agent 协作块共用同一采样路径。
    pub(crate) fn all_lines(&self) -> Vec<CollaborationLine> {
        self.collect_lines(true, false)
    }

    fn collect_lines(&self, include_current: bool, running_only: bool) -> Vec<CollaborationLine> {
        let origin = self.origin_project.display().to_string();
        let handles = self
            .processes
            .lock()
            .unwrap()
            .values()
            .filter(|process| {
                process.origin_project.0.display().to_string() == origin
                    && (include_current || process.id != self.current_process_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let runtimes = self.runtimes.lock().unwrap().clone();
        // R-247:整份快照只读一次 tracker；取得关系来自「取得线」字段，不再逐线
        // 解析 prompt。读失败也要明示，不能回退到猜测路径。
        let tracker_claims = kanzei_tools::active_claims_by_line(&self.origin_project);
        let durable_store = open_durable_store(&self.origin_project);
        let mut lines = handles
            .into_iter()
            .filter_map(|process| {
                let session_id = process_session_id(&self.origin_project, Some(&process.id));
                let runtime = runtimes.get(&session_id);
                let running = runtime.is_some_and(|runtime| {
                    runtime.running.load(std::sync::atomic::Ordering::SeqCst)
                });
                if running_only && !running {
                    return None;
                }
                let (
                    current_tool,
                    steps,
                    input_tokens,
                    output_tokens,
                    event_idle_ms,
                    run_age_ms,
                    mut last_outcome,
                    run_id,
                ) = runtime
                    .map(|runtime| {
                        let live = runtime.live.lock().unwrap();
                        (
                            current_tool(&live.trace),
                            live.steps,
                            live.input_tokens,
                            live.output_tokens,
                            elapsed_ms(live.last_event_at),
                            elapsed_ms(live.started_at),
                            live.last_outcome.clone(),
                            live.run_id.clone(),
                        )
                    })
                    .unwrap_or((None, 0, 0, 0, None, None, None, String::new()));
                let mut status_error = None;
                if !running && last_outcome.is_none() {
                    match &durable_store {
                        Ok(Some(store)) => match durable_outcome(store, &session_id) {
                            Ok(outcome) => last_outcome = outcome,
                            Err(error) => status_error = Some(error),
                        },
                        Ok(None) => {}
                        Err(error) => status_error = Some(error.clone()),
                    }
                }
                let phase = runtime
                    .map(|runtime| runtime.stage.lock().unwrap().clone())
                    .unwrap_or_else(|| "空闲".into());
                let code_root = process
                    .worktree_path
                    .as_ref()
                    .map(|worktree| worktree.0.clone())
                    .unwrap_or_else(|| self.origin_project.clone());
                // R-176 B5(验收⑥):writer/waiting 数据来自协调器快照,不是前端
                // 推测。按每条线的代码树(write_scope)查——同树 writer 与排队者
                // 对这条线可见;跨树并行互不干扰(R-182 口径)。
                let (writer_run_id, waiting_writers) = self
                    .coordinator
                    .as_ref()
                    .map(|coord| {
                        let snap = coord.snapshot(&code_root);
                        (snap.writer_run_id, snap.waiting_writers)
                    })
                    .unwrap_or((None, Vec::new()));
                let (changed_files, changed_files_error) = changed_files(
                    &self.origin_project,
                    &code_root,
                    process.worktree_path.is_some(),
                );
                let worktree_idle_ms = latest_worktree_idle_ms(&code_root, &changed_files);
                let status = line_status(LineProgress {
                    running,
                    waiting_for: runtime
                        .map(|runtime| pending_wait(runtime))
                        .unwrap_or_default(),
                    run_id: &run_id,
                    waiting_writers: &waiting_writers,
                    run_age_ms,
                    last_outcome: last_outcome.as_deref(),
                    event_idle_ms,
                    worktree_idle_ms,
                });
                let status = if status_error.is_some() {
                    "unavailable"
                } else {
                    status
                };
                let branch = process.branch.clone().unwrap_or_else(|| {
                    git_text(&code_root, &["branch", "--show-current"])
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "(主工作树)".into())
                });
                let owner = process.worktree_path.as_ref().map(|_| branch.clone());
                let (claim, claim_error) = match &tracker_claims {
                    Ok(claims) => (
                        claims
                            .get(&owner)
                            .filter(|ids| !ids.is_empty())
                            .map(|ids| ids.join(", "))
                            .unwrap_or_else(|| "未绑定条目".into()),
                        None,
                    ),
                    Err(error) => ("取得线读取失败".into(), Some(error.clone())),
                };
                Some(CollaborationLine {
                    label: process_label(&process),
                    process_id: process.id,
                    branch,
                    worktree_path: process
                        .worktree_path
                        .as_ref()
                        .map(|worktree| worktree.0.display().to_string()),
                    claim,
                    claim_error,
                    phase,
                    current_tool,
                    running,
                    status: status.into(),
                    status_error,
                    event_idle_ms,
                    worktree_idle_ms,
                    steps,
                    input_tokens,
                    output_tokens,
                    changed_files,
                    changed_files_error,
                    writer_run_id,
                    waiting_writers,
                })
            })
            .collect::<Vec<_>>();
        // 数字序:主对话在前,p2 在 p10 之前(按 id 字符串排,p10 会挤到 p2 前面)。
        lines.sort_by(|a, b| {
            crate::processes::naming::order_key(&a.process_id)
                .cmp(&crate::processes::naming::order_key(&b.process_id))
        });
        lines
    }

    pub(crate) fn context_block(&self) -> Option<String> {
        let lines = self.active_others();
        if lines.is_empty() {
            return None;
        }
        Some(render_lines(
            "COLLABORATION — other lines currently writing this project",
            &lines,
        ))
    }
}

fn current_tool(trace: &[serde_json::Value]) -> Option<String> {
    let mut completed = std::collections::HashSet::new();
    for event in trace.iter().rev() {
        match event.get("kind").and_then(serde_json::Value::as_str) {
            Some("tool.completed") => {
                if let Some(id) = event.get("id").and_then(serde_json::Value::as_str) {
                    completed.insert(id);
                }
            }
            Some("tool.started") => {
                let id = event.get("id").and_then(serde_json::Value::as_str);
                if id.is_none_or(|id| !completed.contains(id)) {
                    return event
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
#[test]
fn parallel_tool_completion_keeps_the_other_tool_visible() {
    let mut trace = vec![
        serde_json::json!({"kind":"tool.started","id":"A","name":"read"}),
        serde_json::json!({"kind":"tool.started","id":"B","name":"grep"}),
        serde_json::json!({"kind":"tool.completed","id":"B","name":"grep"}),
    ];
    assert_eq!(current_tool(&trace).as_deref(), Some("read"));
    trace.push(serde_json::json!({"kind":"tool.completed","id":"A","name":"read"}));
    assert_eq!(current_tool(&trace), None);
}

/// 协作视图里的线名:类型 + 序号(「主对话」「独立任务 3」),不再露出 `pN`/「默认」。
/// 这里拿不到会话标题(不开库),完整展示名以 `process_list` 的 `label` 为准。
fn process_label(process: &ProcessHandle) -> String {
    use crate::processes::naming::{fallback_label, process_ordinal};
    fallback_label(
        crate::state::process_kind_of(process),
        process_ordinal(&process.id),
    )
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = hidden_command("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("git 启动失败: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn git_text(cwd: &Path, args: &[&str]) -> Option<String> {
    git_output(cwd, args)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
}

fn changed_files(
    origin_project: &Path,
    code_root: &Path,
    is_worktree: bool,
) -> (Vec<String>, Option<String>) {
    let mut files = BTreeSet::new();
    let status = match git_output(
        code_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    ) {
        Ok(output) => output,
        Err(error) => return (Vec::new(), Some(error)),
    };
    files.extend(parse_porcelain_paths(&status));

    // 已提交但尚未合回主线的文件同样属于“这条线已经改过”。
    if is_worktree {
        let committed = (|| -> Result<Vec<String>, String> {
            let main_head = git_text(origin_project, &["rev-parse", "HEAD"])
                .ok_or_else(|| "无法读取主工作树 HEAD".to_string())?;
            let merge_base = git_text(code_root, &["merge-base", "HEAD", &main_head])
                .ok_or_else(|| "无法计算工作树与主线的 merge-base".to_string())?;
            let range = format!("{merge_base}..HEAD");
            let output = git_output(code_root, &["diff", "--name-only", "-z", &range])?;
            Ok(output
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).to_string())
                .collect())
        })();
        match committed {
            Ok(committed) => files.extend(committed),
            Err(error) => return (files.into_iter().collect(), Some(error)),
        }
    }
    (files.into_iter().collect(), None)
}

fn parse_porcelain_paths(output: &[u8]) -> Vec<String> {
    let records = output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut files = BTreeSet::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.len() >= 4 {
            files.insert(String::from_utf8_lossy(&record[3..]).to_string());
            let renamed = matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C');
            if renamed && index + 1 < records.len() {
                index += 1;
                files.insert(String::from_utf8_lossy(records[index]).to_string());
            }
        }
        index += 1;
    }
    files.into_iter().collect()
}

fn render_lines(title: &str, lines: &[CollaborationLine]) -> String {
    let mut output = vec![format!("## {title}")];
    for line in lines {
        let files = if let Some(error) = &line.changed_files_error {
            format!("unavailable ({error})")
        } else if line.changed_files.is_empty() {
            "none yet".into()
        } else {
            line.changed_files.join(", ")
        };
        output.push(format!(
            "- {} ({}) · claim: {} · branch: {} · status: {} · phase: {} · tool: {} · changed files: {}",
            line.label,
            line.process_id,
            line.claim,
            line.branch,
            line.status,
            line.phase,
            line.current_tool.as_deref().unwrap_or("none"),
            files
        ));
        if let Some(error) = &line.status_error {
            output.push(format!("  status unavailable: {error}"));
        }
    }
    output.push(
        "You are collaborating in the same repository. Before touching a shared file or staging, call `collaboration_status` again because this list changes during the run."
            .into(),
    );
    output.join("\n")
}

/// R-184 B 面:跨线并列视图的只读 IPC。进程注册先从 state.db 恢复,避免重启后漏线。
#[tauri::command]
pub(crate) async fn collaboration_snapshot(
    state: tauri::State<'_, crate::AppState>,
    project_dir: String,
) -> Result<Vec<CollaborationLine>, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    crate::processes::restore_processes_from_store_once(&state, &root)?;
    let probe = CollaborationProbe::new(
        state.processes.clone(),
        state.runtimes.clone(),
        root,
        String::new(),
    )
    .with_coordinator(Arc::clone(&state.coordinator)
        as Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>);
    // git status/diff 在大仓可能到秒级；轮询 IPC 用阻塞线程池采样，不能占住
    // Tauri 命令执行线程拖慢其它线路的发送、停止与权限响应。
    tauri::async_runtime::spawn_blocking(move || probe.all_lines())
        .await
        .map_err(|error| format!("读取独立任务状态失败: {error}"))
}

pub(crate) struct CollaborationComponent {
    pub(crate) probe: CollaborationProbe,
}

impl Component for CollaborationComponent {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        let source_probe = self.probe.clone();
        draft.context.insert(
            "runtime/collaboration",
            refreshing_source("runtime/collaboration", move |_| {
                source_probe.context_block()
            }),
        );
        draft.tools.insert(
            "collaboration_status",
            Arc::new(CollaborationStatusTool {
                probe: self.probe.clone(),
            }),
        );
        draft
            .permissions
            .push(rule("collaboration_status", "*", Effect::Allow));
        Ok(())
    }
}

struct CollaborationStatusTool {
    probe: CollaborationProbe,
}

#[async_trait::async_trait]
impl Tool for CollaborationStatusTool {
    fn name(&self) -> &'static str {
        "collaboration_status"
    }

    fn description(&self) -> String {
        "Refresh the real-time list of other running lines in this project: claimed item, branch, and changed files. Call before editing a shared file and immediately before staging/commit. Read-only."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        ctx: &kanzei_harness::ToolCtx,
    ) -> kanzei_harness::ToolOutput {
        let lines = self.probe.active_others();
        let children = ctx
            .session_id
            .as_deref()
            .and_then(|owner| kanzei_tools::team::find(&ctx.project_root, owner))
            .and_then(|team| team.list().ok())
            .unwrap_or_default();
        if !children.is_empty() {
            let children: Vec<_> = children.iter().map(|job| serde_json::json!({
                "id":job.id,"name":job.name,"role":job.role,"state":job.state,"worktree":job.worktree,
                "latest":job.latest.chars().take(400).collect::<String>(),"files":job.files.iter().take(32).collect::<Vec<_>>()
            })).collect();
            return kanzei_harness::ToolOutput::ok(
                serde_json::json!({"lines":lines,"children":children}).to_string(),
            )
            .with_display(serde_json::json!({"lines":lines,"children":children}));
        }
        if lines.is_empty() {
            kanzei_harness::ToolOutput::ok("No other line is currently running in this project.")
        } else {
            kanzei_harness::ToolOutput::ok(render_lines("Live collaboration status", &lines))
                .with_display(serde_json::json!({"lines": lines}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ensure_default_process, process_session_id, runtime_for, AppState, ProcessHandle};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn 泳道状态机按进程事件和工作树进展判定() {
        assert_eq!(
            line_status(LineProgress {
                running: true,
                run_age_ms: Some(1_000),
                event_idle_ms: Some(100),
                ..Default::default()
            }),
            "running"
        );
        assert_eq!(
            line_status(LineProgress {
                running: true,
                run_age_ms: Some(90_000),
                event_idle_ms: Some(90_000),
                ..Default::default()
            }),
            "suspected_stuck"
        );
        // 真实长任务可能没有新的 RunEvent，但源码仍在变化，不能被判成失败。
        assert_eq!(
            line_status(LineProgress {
                running: true,
                run_age_ms: Some(90_000),
                event_idle_ms: Some(90_000),
                worktree_idle_ms: Some(100),
                ..Default::default()
            }),
            "running"
        );
        assert_eq!(
            line_status(LineProgress {
                run_age_ms: Some(90_000),
                ..Default::default()
            }),
            "failed"
        );
        assert_eq!(
            line_status(LineProgress {
                run_age_ms: Some(90_000),
                last_outcome: Some("completed"),
                ..Default::default()
            }),
            "completed"
        );
        assert_eq!(line_status(LineProgress::default()), "idle");
        assert_eq!(
            line_status(LineProgress {
                run_age_ms: Some(90_000),
                last_outcome: Some("incomplete"),
                ..Default::default()
            }),
            "incomplete"
        );
        let waiting = vec!["run-waiting".to_string()];
        assert_eq!(
            line_status(LineProgress {
                running: true,
                run_id: "run-waiting",
                waiting_writers: &waiting,
                run_age_ms: Some(90_000),
                event_idle_ms: Some(90_000),
                ..Default::default()
            }),
            "queued"
        );
    }

    #[test]
    fn 事件时间戳由真实事件刷新() {
        let mut live = crate::state::LiveRun::default();
        assert!(live.last_event_at.is_none());
        live.note_event();
        assert!(live.last_event_at.is_some());
    }

    #[test]
    fn restart_recovers_the_latest_run_boundary_without_inventing_success() {
        let store = kanzei_core::SessionStore::open_in_memory().unwrap();
        store.create_session("ses", "fixture", None).unwrap();
        assert_eq!(durable_outcome(&store, "ses").unwrap(), None);
        for (event_type, payload, expected) in [
            (
                "run.completed",
                serde_json::json!({"outcome":"completed"}),
                "completed",
            ),
            (
                "run.failed",
                serde_json::json!({"outcome":"incomplete"}),
                "incomplete",
            ),
            (
                "run.completed",
                serde_json::json!({"halted_by_user":true}),
                "halted",
            ),
            ("session.turn_started", serde_json::json!({}), "interrupted"),
            (
                "run.failed",
                serde_json::json!({"error":"failed"}),
                "failed",
            ),
        ] {
            store.append_event("ses", event_type, &payload).unwrap();
            store
                .append_event(
                    "ses",
                    "run.trace",
                    &serde_json::json!({"outcome":"completed"}),
                )
                .unwrap();
            let outcome = durable_outcome(&store, "ses").unwrap();
            assert_eq!(outcome.as_deref(), Some(expected));
            let status = line_status(LineProgress {
                last_outcome: outcome.as_deref(),
                ..Default::default()
            });
            assert_eq!(
                status,
                if expected == "halted" {
                    "stopped"
                } else {
                    expected
                }
            );
        }
        store
            .append_event(
                "ses",
                "run.completed",
                &serde_json::json!({"outcome":"invalid"}),
            )
            .unwrap();
        assert!(durable_outcome(&store, "ses").is_err());
    }

    #[test]
    fn restart_uses_newer_input_facts_for_startup_failure_cancellation_and_queue() {
        let store = kanzei_core::SessionStore::open_in_memory().unwrap();
        store.create_session("ses", "fixture", None).unwrap();
        store
            .append_event(
                "ses",
                "run.completed",
                &serde_json::json!({"outcome":"completed"}),
            )
            .unwrap();
        let input = store
            .admit_input(
                "ses",
                "new-input",
                "new request",
                kanzei_core::Delivery::Queue,
            )
            .unwrap();
        assert_eq!(
            durable_outcome(&store, "ses").unwrap().as_deref(),
            Some("queued")
        );
        store.promote_next_input("ses").unwrap().unwrap();
        assert_eq!(
            durable_outcome(&store, "ses").unwrap().as_deref(),
            Some("interrupted")
        );
        assert!(store.finish_input(&input.input_id, false).unwrap());
        assert_eq!(
            durable_outcome(&store, "ses").unwrap().as_deref(),
            Some("failed")
        );
        let input = store
            .admit_input(
                "ses",
                "budget-input",
                "budget request",
                kanzei_core::Delivery::Queue,
            )
            .unwrap();
        store.promote_next_input("ses").unwrap().unwrap();
        store.finish_input(&input.input_id, false).unwrap();
        store
            .append_event(
                "ses",
                "run.failed",
                &serde_json::json!({"input_id":input.input_id,"outcome":"incomplete"}),
            )
            .unwrap();
        assert_eq!(
            durable_outcome(&store, "ses").unwrap().as_deref(),
            Some("incomplete")
        );
        let cancelled = store
            .admit_input(
                "ses",
                "cancelled-input",
                "cancelled before startup",
                kanzei_core::Delivery::Queue,
            )
            .unwrap();
        assert!(store.cancel_input("ses", &cancelled.input_id).unwrap());
        assert_eq!(
            durable_outcome(&store, "ses").unwrap().as_deref(),
            Some("halted")
        );
    }

    #[test]
    fn preparing_a_new_input_never_reuses_the_previous_run_result_or_identity() {
        let mut live = crate::state::LiveRun::default();
        live.begin("previous-run", "previous-input", "old", "mock", "mock");
        live.finish("completed");
        live.prepare(Some("new-input"), "new");
        assert_eq!(live.input_id, "new-input");
        assert!(live.run_id.is_empty());
        assert!(live.started_at.is_none());
        assert!(live.last_outcome.is_none());
        assert!(!live.flushed);
        assert_eq!(
            line_status(LineProgress {
                running: true,
                ..Default::default()
            }),
            "starting"
        );
    }

    #[test]
    fn pending_runtime_approval_is_waiting_even_when_the_event_stream_is_old() {
        let runtime = SessionRuntime::default();
        let (sender, _receiver) = tokio::sync::oneshot::channel();
        runtime.asks.lock().unwrap().insert(
            1,
            crate::state::PendingAsk {
                source: "primary".into(),
                agent_id: None,
                sender,
                request: kanzei_core::AskRequest::Permission {
                    action: "write".into(),
                    resource: "file".into(),
                },
                action: "write".into(),
                resource: "file".into(),
                project_root: PathBuf::from("fixture"),
                session_id: "ses".into(),
            },
        );
        assert_eq!(
            line_status(LineProgress {
                running: true,
                waiting_for: pending_wait(&runtime),
                run_age_ms: Some(90_000),
                event_idle_ms: Some(90_000),
                ..Default::default()
            }),
            "awaiting_approval"
        );
    }

    #[test]
    fn porcelain_paths_cover_untracked_modified_and_rename_pairs() {
        let parsed = parse_porcelain_paths(b" M src/a.rs\0?? new.txt\0R  dst.rs\0src.rs\0");
        assert_eq!(parsed, ["dst.rs", "new.txt", "src.rs", "src/a.rs"]);
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = hidden_command("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} 失败:{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn 协作块取真实运行态_单线不注入_文件变化后原位刷新() {
        let root = std::env::temp_dir().join(format!(
            "kz-collaboration-{}-{}",
            std::process::id(),
            crate::run::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
        run_git(&root, &["add", "seed.txt"]);
        run_git(
            &root,
            &[
                "-c",
                "user.name=Kanzei Test",
                "-c",
                "user.email=test@kanzei.local",
                "commit",
                "-m",
                "seed",
            ],
        );
        let canonical = crate::normalized_project_root(&root);
        kanzei_tools::docstore::DocStore::open(&canonical, &kanzei_tools::docstore::REQUIREMENTS)
            .save(&[kanzei_tools::docstore::Entry {
                id: "R-184".into(),
                title: "并行协作".into(),
                status: "doing".into(),
                severity: None,
                fields: vec![("取得线".into(), "kanzei/thread-r184".into())],
            }])
            .unwrap();
        let state = AppState::default();
        let current = ensure_default_process(&state, &canonical);
        let current_runtime =
            runtime_for(&state, &process_session_id(&canonical, Some(&current.id)));
        current_runtime.running.store(true, Ordering::SeqCst);

        let other = ProcessHandle {
            id: format!("p1|{}", canonical.display()),
            origin_project: crate::ProjectRoot(canonical.clone()),
            project_dir: crate::ProjectRoot(canonical.clone()),
            // 该测试不需要另建真实 worktree，但必须标成分支线，才能验证
            // 「取得线=分支名」而非默认线(None)的 tracker 归属。
            worktree_path: Some(crate::WorktreeRoot(canonical.clone())),
            branch: Some("kanzei/thread-r184".into()),
            model: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            research_topic: Arc::new(std::sync::Mutex::new(None)),
            reasoning: Arc::new(Mutex::new(None)),
            manual_models: Arc::new(Mutex::new(Vec::new())),

            subagent_mode: Arc::new(std::sync::Mutex::new(kanzei_harness::SubagentMode::Auto)),
            tracker_writes_enabled: Arc::new(AtomicBool::new(false)),
        };
        state
            .processes
            .lock()
            .unwrap()
            .insert(other.id.clone(), other.clone());
        let other_runtime = runtime_for(&state, &process_session_id(&canonical, Some(&other.id)));
        {
            let mut live = other_runtime.live.lock().unwrap();
            live.begin(
                "run-r184",
                "input-r184",
                "实现并行协作上下文",
                "test",
                "test",
            );
            live.steps = 3;
            live.input_tokens = 1200;
            live.output_tokens = 300;
            live.trace.push(serde_json::json!({
                "kind": "tool.started", "id": "call-1", "name": "edit"
            }));
        }
        *other_runtime.stage.lock().unwrap() = "实现".into();

        let probe = CollaborationProbe::new(
            state.processes.clone(),
            state.runtimes.clone(),
            canonical.clone(),
            current.id.clone(),
        );
        let ctx = ResolveCtx {
            profile: kanzei_harness::ProfileKind::Dev,
            cwd: canonical.clone(),
            project_root: canonical.clone(),
            config: Arc::new(kanzei_harness::KanzeiConfig::default()),
        };
        let mut harness = kanzei_harness::Harness::default();
        harness.add(CollaborationComponent {
            probe: probe.clone(),
        });
        let snapshot = harness.resolve(&ctx).unwrap();

        // 只有当前线在跑:动态源不注入任何占位噪音。
        assert!(snapshot.system_baseline().unwrap().is_empty());
        other_runtime.running.store(true, Ordering::SeqCst);
        std::fs::write(root.join("first.rs"), "first\n").unwrap();
        let all = probe.all_lines();
        let other_line = all
            .iter()
            .find(|line| line.process_id == other.id)
            .expect("并列视图漏掉运行线");
        assert!(other_line.running);
        assert_eq!(other_line.status, "running");
        assert_eq!(other_line.phase, "实现");
        assert_eq!(other_line.current_tool.as_deref(), Some("edit"));
        assert_eq!(
            (
                other_line.steps,
                other_line.input_tokens,
                other_line.output_tokens
            ),
            (3, 1200, 300)
        );
        let first = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
        assert!(
            first.contains("R-184"),
            "认领条目必须来自 tracker 取得线:{first}"
        );
        assert!(first.contains("kanzei/thread-r184"), "{first}");
        assert!(first.contains("first.rs"), "{first}");
        assert!(
            snapshot
                .stable_system_baseline_with_report()
                .unwrap()
                .0
                .is_empty(),
            "协作块不得落进稳定 baseline 后逐轮累积"
        );

        std::fs::write(root.join("second.rs"), "second\n").unwrap();
        let refreshed = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
        assert!(refreshed.contains("first.rs") && refreshed.contains("second.rs"));
        assert_eq!(
            refreshed.matches("COLLABORATION").count(),
            1,
            "刷新应替换同一块,不能累计旧副本"
        );

        let tool = snapshot
            .materialize_tools()
            .into_iter()
            .find(|tool| tool.name() == "collaboration_status")
            .expect("主动查询通道未注册");
        assert_eq!(
            snapshot.evaluate("collaboration_status", "*"),
            Effect::Allow,
            "主动查询必须是无需打断用户的只读通道"
        );
        let output = tool
            .execute(
                serde_json::json!({}),
                &kanzei_harness::ToolCtx::new(canonical.clone(), canonical.clone()),
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains("second.rs"), "{}", output.content);

        current_runtime.running.store(false, Ordering::SeqCst);
        other_runtime.running.store(false, Ordering::SeqCst);
        drop(snapshot);
        drop(harness);
        std::fs::remove_dir_all(root).ok();
    }

    /// R-176 验收⑥:协作面板的 writer/waiting 数据来自协调器快照,不是前端推测。
    /// 用真实 MemoryCoordinator:先占住写租约,再采样 CollaborationLine,断言
    /// writer_run_id 与 waiting_writers 从 snapshot() 取到(而非空)。
    #[tokio::test]
    async fn collaboration_line_writer_and_waiting_come_from_coordinator_snapshot() {
        use kanzei_core::orchestration::MemoryCoordinator;
        use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLeaseRequest};
        let root = std::env::temp_dir().join(format!(
            "kz-collab-writer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let canonical = crate::normalized_project_root(&root);
        let coordinator: Arc<dyn ProjectExecutionCoordinator> = Arc::new(MemoryCoordinator::new());

        // 真实占用 write_scope = 主根:writer=line-a,排队=line-b。
        let _lease = coordinator
            .acquire_writer_lease(WriterLeaseRequest {
                write_scope: canonical.clone(),
                run_id: "line-a".into(),
                process_id: "p-a".into(),
                reason: "test".into(),
            })
            .await
            .unwrap();
        let _waiter = tokio::spawn({
            let coord = coordinator.clone();
            let scope = canonical.clone();
            async move {
                let _ = coord
                    .acquire_writer_lease(WriterLeaseRequest {
                        write_scope: scope,
                        run_id: "line-b".into(),
                        process_id: "p-b".into(),
                        reason: "test".into(),
                    })
                    .await;
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // 用带协调器的 Probe 采样:line 的 writer/waiting 必须来自快照。
        let state = AppState::default();
        let current = ensure_default_process(&state, &canonical);
        let probe = CollaborationProbe::new(
            state.processes.clone(),
            state.runtimes.clone(),
            canonical.clone(),
            current.id.clone(),
        )
        .with_coordinator(coordinator);
        let lines = probe.all_lines();
        // current(主树)的 writer 与 waiting 就是协调器快照的内容。
        assert!(
            lines.iter().any(|line| {
                line.writer_run_id.as_deref() == Some("line-a")
                    && line.waiting_writers.contains(&"line-b".to_string())
            }),
            "writer/waiting 必须来自协调器快照: {lines:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
