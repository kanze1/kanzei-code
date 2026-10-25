//! 项目对话与工作树的协作采样、刷新上下文和只读查询。

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kanzei_harness::{refreshing_source, rule, Component, Effect, HarnessDraft, ResolveCtx, Tool};
use serde::Serialize;

use crate::state::hidden_command;
use crate::{process_session_id, ProcessHandle, SessionRuntime};

mod summary;
use summary::{model_snapshot, render_snapshot, StatusQuery};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CollaborationConversation {
    pub(crate) process_id: String,
    pub(crate) label: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) session_id: String,
    pub(crate) run_id: Option<String>,
    #[serde(flatten)]
    pub(crate) tree: CollaborationWorktree,
    pub(crate) phase: String,
    pub(crate) current_tool: Option<String>,
    pub(crate) running: bool,
    pub(crate) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) event_idle_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) run_age_ms: Option<u64>,
    pub(crate) steps: u32,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
}

/// Git、工作登记和写租约属于工作树，同树参与者共用一次采样结果。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CollaborationWorktree {
    #[serde(skip)]
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) branch_error: Option<String>,
    /// 只保留 tracker「取得线」的登记事实，不推断对话或文件所有权。
    pub(crate) tracker_line: Option<String>,
    pub(crate) tracker_entries: Vec<String>,
    pub(crate) tracker_error: Option<String>,
    pub(crate) uncommitted_files: Vec<String>,
    pub(crate) uncommitted_error: Option<String>,
    pub(crate) unmerged_files: Vec<String>,
    pub(crate) unmerged_error: Option<String>,
    /// 当前工作树的写租约快照，由协调器提供。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) writer_run_id: Option<String>,
    /// 当前工作树等待写租约的运行标识。
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
    /// 工作树写租约的事实来源。
    coordinator: Option<Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>>,
}

#[derive(Default)]
enum ConversationWait {
    #[default]
    None,
    Approval,
    Answer,
}

fn pending_wait(runtime: &SessionRuntime) -> ConversationWait {
    let asks = runtime.asks.lock().unwrap();
    if asks
        .values()
        .any(|ask| matches!(ask.request, kanzei_core::AskRequest::Permission { .. }))
    {
        ConversationWait::Approval
    } else if asks.values().any(|ask| {
        matches!(
            ask.request,
            kanzei_core::AskRequest::Question {
                background: false,
                ..
            }
        )
    }) {
        ConversationWait::Answer
    } else {
        ConversationWait::None
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
struct ConversationProgress<'a> {
    running: bool,
    waiting_for: ConversationWait,
    run_id: &'a str,
    waiting_writers: &'a [String],
    run_age_ms: Option<u64>,
    last_outcome: Option<&'a str>,
}

fn conversation_status(progress: ConversationProgress<'_>) -> &'static str {
    let ConversationProgress {
        running,
        waiting_for,
        run_id,
        waiting_writers,
        run_age_ms,
        last_outcome,
    } = progress;
    if running {
        match waiting_for {
            ConversationWait::Approval => return "awaiting_approval",
            ConversationWait::Answer => return "awaiting_answer",
            ConversationWait::None => {}
        }
        if waiting_writers.iter().any(|waiting| waiting == run_id) {
            return "queued";
        }
        if run_age_ms.is_none() {
            return "starting";
        }
        "running"
    } else {
        match last_outcome {
            Some("completed") => "completed",
            Some("halted") => "stopped",
            Some("incomplete") => "incomplete",
            Some("interrupted") => "interrupted",
            Some("queued") => "queued",
            Some("failed") => "failed",
            None if run_age_ms.is_none() => "idle",
            None => "unavailable",
            Some(_) => "unavailable",
        }
    }
}

fn elapsed_ms(at: Option<std::time::Instant>) -> Option<u64> {
    at.map(|instant| instant.elapsed().as_millis() as u64)
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

    /// 装配运行和协作 IPC 时注入同一个工作树写租约协调器。
    pub(crate) fn with_coordinator(
        mut self,
        coordinator: Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>,
    ) -> Self {
        self.coordinator = Some(coordinator);
        self
    }

    /// 采样当前项目的全部已注册对话，包括空闲、停止和完成的对话。
    /// 句柄在锁内克隆，运行状态与 Git 状态在锁外读取。
    pub(crate) fn conversations(&self) -> Vec<CollaborationConversation> {
        let origin = self.origin_project.display().to_string();
        let handles = self
            .processes
            .lock()
            .unwrap()
            .values()
            .filter(|process| process.origin_project.0.display().to_string() == origin)
            .cloned()
            .collect::<Vec<_>>();
        let runtimes = self.runtimes.lock().unwrap().clone();
        // 一次读取登记关系；字段来源与错误在快照中保留，不推断对话所有权。
        let tracker_claims = kanzei_tools::active_claims_by_line(&self.origin_project);
        let durable_store = open_durable_store(&self.origin_project);
        // Git state belongs to a code tree. Sample it once so participants sharing
        // that tree see the same state and do not repeat Git and filesystem scans.
        let mut worktrees = HashMap::new();
        let mut lines = handles
            .into_iter()
            .map(|process| {
                let session_id = process_session_id(&self.origin_project, Some(&process.id));
                let runtime = runtimes.get(&session_id);
                let running = runtime.is_some_and(|runtime| runtime.is_running());
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
                let tree = worktrees.entry(code_root.clone()).or_insert_with(|| {
                    self.observe_worktree(
                        &code_root,
                        process.worktree_path.is_some(),
                        &tracker_claims,
                    )
                });
                let status = conversation_status(ConversationProgress {
                    running,
                    waiting_for: runtime
                        .map(|runtime| pending_wait(runtime))
                        .unwrap_or_default(),
                    run_id: &run_id,
                    waiting_writers: &tree.waiting_writers,
                    run_age_ms,
                    last_outcome: last_outcome.as_deref(),
                });
                let status = if status_error.is_some() {
                    "unavailable"
                } else {
                    status
                };
                CollaborationConversation {
                    label: conversation_label(
                        &process,
                        durable_store.as_ref().ok().and_then(|store| store.as_ref()),
                        &session_id,
                    ),
                    process_id: process.id,
                    session_id,
                    run_id: (!run_id.is_empty()).then_some(run_id),
                    worktree_path: process
                        .worktree_path
                        .as_ref()
                        .map(|worktree| worktree.0.display().to_string()),
                    tree: tree.clone(),
                    phase,
                    current_tool,
                    running,
                    status: status.into(),
                    status_error,
                    event_idle_ms,
                    run_age_ms,
                    steps,
                    input_tokens,
                    output_tokens,
                }
            })
            .collect::<Vec<_>>();
        // 与对话列表保持相同的稳定数字序。
        lines.sort_by(|a, b| {
            crate::processes::naming::order_key(&a.process_id)
                .cmp(&crate::processes::naming::order_key(&b.process_id))
        });
        lines
    }

    fn observe_worktree(
        &self,
        root: &Path,
        independent: bool,
        registrations: &Result<std::collections::BTreeMap<Option<String>, Vec<String>>, String>,
    ) -> CollaborationWorktree {
        let (branch, branch_error) = match git_output(root, &["branch", "--show-current"]) {
            Ok(branch) => (String::from_utf8_lossy(&branch).trim().to_string(), None),
            Err(error) => (String::new(), Some(error)),
        };
        let tracker_line = independent.then(|| branch.clone());
        let (tracker_entries, tracker_error) = match registrations {
            Err(error) => (Vec::new(), Some(error.clone())),
            Ok(entries) => match branch_error.as_ref().filter(|_| independent) {
                Some(error) => (Vec::new(), Some(error.clone())),
                None => (
                    entries.get(&tracker_line).cloned().unwrap_or_default(),
                    None,
                ),
            },
        };
        let changes = worktree_changes(&self.origin_project, root, independent);
        let (uncommitted_files, uncommitted_error) = match changes.uncommitted {
            Ok(files) => (files, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let (unmerged_files, unmerged_error) = match changes.unmerged {
            Ok(files) => (files, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let (writer_run_id, waiting_writers) = match &self.coordinator {
            Some(coordinator) => {
                let lease = coordinator.snapshot(root);
                (lease.writer_run_id, lease.waiting_writers)
            }
            None => (None, Vec::new()),
        };
        CollaborationWorktree {
            path: root.display().to_string(),
            branch,
            branch_error,
            tracker_line,
            tracker_entries,
            tracker_error,
            uncommitted_files,
            uncommitted_error,
            unmerged_files,
            unmerged_error,
            writer_run_id,
            waiting_writers,
        }
    }

    fn snapshot(&self, path: Option<&str>) -> serde_json::Value {
        use kanzei_tools::worktree::{git_worktrees, worktree_key};
        let conversations = self.conversations();
        let mut snapshot = model_snapshot(
            &self.origin_project.to_string_lossy(),
            &self.current_process_id,
            &conversations,
            path,
        );
        let mut observed = conversations
            .iter()
            .map(|conversation| worktree_key(Path::new(&conversation.tree.path)))
            .collect::<std::collections::HashSet<_>>();
        match git_worktrees(&self.origin_project) {
            Ok(worktrees) => {
                let registrations = kanzei_tools::active_claims_by_line(&self.origin_project);
                let primary = worktree_key(&self.origin_project);
                let trees = snapshot["worktrees"]
                    .as_array_mut()
                    .expect("worktrees are assembled as an array");
                for worktree in worktrees
                    .into_iter()
                    .filter(|tree| !tree.bare && !tree.prunable)
                {
                    let key = worktree_key(&worktree.path);
                    if !observed.insert(key.clone()) {
                        continue;
                    }
                    let tree =
                        self.observe_worktree(&worktree.path, key != primary, &registrations);
                    trees.push(summary::worktree_snapshot(trees.len(), &tree, path));
                }
                snapshot["worktree_discovery_error"] = serde_json::Value::Null;
            }
            Err(error) => snapshot["worktree_discovery_error"] = serde_json::Value::String(error),
        }
        snapshot
    }

    pub(crate) fn context_block(&self) -> Option<String> {
        let snapshot = self.snapshot(None);
        let has_other_conversation =
            snapshot["conversations"]
                .as_array()
                .is_some_and(|conversations| {
                    conversations
                        .iter()
                        .any(|conversation| conversation["process_id"] != self.current_process_id)
                });
        let has_other_tree = snapshot["worktrees"]
            .as_array()
            .is_some_and(|trees| trees.len() > 1);
        (has_other_conversation || has_other_tree).then(|| render_snapshot(&snapshot))
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

/// 复用对话列表的持久标题与首条消息标题；无标题时使用同一命名规则。
fn conversation_label(
    process: &ProcessHandle,
    store: Option<&kanzei_core::SessionStore>,
    session_id: &str,
) -> String {
    use crate::processes::naming::{fallback_label, load_naming, process_ordinal};
    let kind = crate::state::process_kind_of(process);
    let ordinal = process_ordinal(&process.id);
    match store {
        Some(store) => load_naming(store, session_id, kind).display(kind, ordinal),
        None => fallback_label(kind, ordinal),
    }
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    if !kanzei_tools::project_state::is_repository_root(cwd) {
        return Err("代码树根没有独立 Git 仓库，未读取上级仓库".into());
    }
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

struct WorktreeChanges {
    uncommitted: Result<Vec<String>, String>,
    unmerged: Result<Vec<String>, String>,
}

fn worktree_changes(origin_project: &Path, code_root: &Path, is_worktree: bool) -> WorktreeChanges {
    let uncommitted = git_output(
        code_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .map(|status| parse_porcelain_paths(&status));
    let unmerged = if is_worktree {
        (|| -> Result<Vec<String>, String> {
            let main_head = git_output(origin_project, &["rev-parse", "HEAD"])?;
            let main_head = String::from_utf8_lossy(&main_head);
            let merge_base = git_output(code_root, &["merge-base", "HEAD", main_head.trim()])?;
            let merge_base = String::from_utf8_lossy(&merge_base);
            let range = format!("{}..HEAD", merge_base.trim());
            let output = git_output(
                code_root,
                &["diff", "--name-only", "--no-renames", "-z", &range],
            )?;
            Ok(output
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).to_string())
                .collect())
        })()
    } else {
        Ok(Vec::new())
    };
    WorktreeChanges {
        uncommitted,
        unmerged,
    }
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

/// R-184 B 面:跨线并列视图的只读 IPC。进程注册先从 state.db 恢复,避免重启后漏线。
#[tauri::command]
pub(crate) async fn collaboration_snapshot(
    state: tauri::State<'_, crate::AppState>,
    project_dir: String,
) -> Result<Vec<CollaborationConversation>, String> {
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
    // Tauri 命令执行线程拖慢其他对话的发送、停止与权限响应。
    tauri::async_runtime::spawn_blocking(move || probe.conversations())
        .await
        .map_err(|error| format!("读取项目协作状态失败: {error}"))
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
        "查询当前项目相关对话与工作树的协作状态，包括执行状态、工作树关联、Git 变化和写租约。可指定项目相对路径，分别查看未提交变化与分支提交差异。用于了解同树协作与跨树整合范围；文件变化和登记关系不证明具体对话的修改归属。只读。"
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {
            "path": {"type": "string", "description": "项目相对文件或目录路径。指定后分别查询各工作树的未提交变化和分支提交差异是否涉及此范围；省略时返回项目协作概览。"}
        }, "additionalProperties": false})
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &kanzei_harness::ToolCtx,
    ) -> kanzei_harness::ToolOutput {
        let query = match StatusQuery::parse(input) {
            Ok(query) => query,
            Err(error) => return kanzei_harness::ToolOutput::error(error),
        };
        let snapshot = self.probe.snapshot(query.path.as_deref());
        kanzei_harness::ToolOutput::ok(render_snapshot(&snapshot)).with_display(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ensure_default_process, process_session_id, runtime_for, AppState, ProcessHandle};
    use std::sync::atomic::AtomicBool;

    fn fixture_conversation(
        id: &str,
        tree: Option<&str>,
        files: Vec<String>,
    ) -> CollaborationConversation {
        CollaborationConversation {
            process_id: id.into(),
            session_id: format!("session-{id}"),
            run_id: Some(format!("run-{id}")),
            label: id.into(),
            worktree_path: tree.map(str::to_owned),
            phase: "请求".into(),
            current_tool: None,
            running: true,
            status: "running".into(),
            status_error: None,
            event_idle_ms: None,
            run_age_ms: None,
            steps: 0,
            input_tokens: 0,
            output_tokens: 0,
            tree: CollaborationWorktree {
                path: tree.unwrap_or("project").into(),
                branch: "dev".into(),
                branch_error: None,
                tracker_line: tree.map(str::to_owned),
                tracker_entries: vec!["R-184".into()],
                tracker_error: None,
                uncommitted_files: files,
                uncommitted_error: None,
                unmerged_files: Vec::new(),
                unmerged_error: None,
                writer_run_id: Some("writer".into()),
                waiting_writers: vec!["waiter".into()],
            },
        }
    }

    #[test]
    fn collaboration_context_does_not_expand_worktree_file_inventory() {
        let files = (0..5_000)
            .map(|index| {
                format!(
            ".kanzei/tmp/release-plan/snapshot/crates/component/src/very_long_file_name_{index}.rs"
        )
            })
            .collect::<Vec<_>>();
        let lines = vec![
            fixture_conversation("p1", None, files.clone()),
            fixture_conversation("p2", None, files),
        ];
        let snapshot = model_snapshot("project", "p1", &lines, None);
        let rendered = render_snapshot(&snapshot);
        assert!(
            !rendered.contains("very_long_file_name_"),
            "resident context expanded {} characters of worktree paths",
            rendered.chars().count()
        );
        assert_eq!(snapshot["worktrees"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["worktrees"][0]["uncommitted"]["file_count"], 5_000);
        assert_eq!(
            snapshot["conversations"][0]["worktree"],
            snapshot["conversations"][1]["worktree"]
        );
        let mut renamed = lines;
        for line in &mut renamed {
            line.tree.uncommitted_files =
                (0..5_000).map(|index| format!("short{index}.rs")).collect();
        }
        assert_eq!(
            rendered,
            render_snapshot(&model_snapshot("project", "p1", &renamed, None)),
            "inventory path lengths must not affect resident context size"
        );
        eprintln!(
            "compact collaboration fixture: {} characters",
            rendered.chars().count()
        );
    }

    #[test]
    fn model_snapshot_groups_worktrees_and_keeps_failed_queries_unknown() {
        let main = fixture_conversation("p1", None, vec!["src/shared.rs".into()]);
        let mut branch =
            fixture_conversation("p2", Some("branch-tree"), vec!["src/shared.rs".into()]);
        branch.tree.uncommitted_error = Some("Git inventory unavailable".into());
        branch.tree.tracker_error = Some("Tracker unavailable".into());
        let snapshot = model_snapshot("main-tree", "p1", &[main, branch], Some("src"));
        let trees = snapshot["worktrees"].as_array().unwrap();
        assert_eq!(trees.len(), 2);
        assert_eq!(trees[0]["path"], "main-tree");
        assert_eq!(snapshot["current"]["session_id"], "session-p1");
        assert_eq!(snapshot["current"]["run_id"], "run-p1");
        assert_eq!(
            snapshot["current"]["worktree"],
            snapshot["conversations"][0]["worktree"]
        );
        assert!(trees[1]["uncommitted"]["file_count"].is_null());
        assert_eq!(trees[1]["unmerged"]["file_count"], 0);
        assert_eq!(trees[0]["uncommitted"]["path_changed"], true);
        assert_eq!(trees[0]["writer_run_id"], "writer");
        assert_eq!(trees[0]["waiting_writers"], serde_json::json!(["waiter"]));
        assert!(
            trees[1]["uncommitted"]["path_changed"].is_null(),
            "an unreadable tree cannot establish absence of changes"
        );
        assert_eq!(
            trees[1]["uncommitted"]["error"],
            "Git inventory unavailable"
        );
        assert_eq!(trees[1]["registered_work"]["error"], "Tracker unavailable");
        assert!(!snapshot.to_string().contains("src/shared.rs"));
    }

    #[test]
    fn scoped_query_covers_committed_renames_and_shared_uncommitted_changes() {
        let owned = kanzei_base::test_support::TestDir::new("kz-collaboration-scope-");
        let main = owned.join("main");
        let branch = owned.join("branch");
        std::fs::create_dir(&main).unwrap();
        run_git(&main, &["init"]);
        std::fs::write(main.join("seed.rs"), "seed").unwrap();
        run_git(&main, &["add", "seed.rs"]);
        let commit = [
            "-c",
            "user.name=Kanzei Test",
            "-c",
            "user.email=test@kanzei.local",
            "commit",
            "-m",
            "fixture",
        ];
        run_git(&main, &commit);
        run_git(
            &main,
            &[
                "worktree",
                "add",
                "-b",
                "query-branch",
                branch.to_str().unwrap(),
            ],
        );
        run_git(&branch, &["mv", "seed.rs", "renamed.rs"]);
        run_git(&branch, &commit);
        std::fs::write(main.join("seed.rs"), "main change").unwrap();
        std::fs::write(branch.join("untracked.rs"), "new file").unwrap();
        let main_changes = worktree_changes(&main, &main, false);
        let branch_changes = worktree_changes(&main, &branch, true);
        let main_files = main_changes.uncommitted.unwrap();
        let branch_uncommitted = branch_changes.uncommitted.unwrap();
        let branch_unmerged = branch_changes.unmerged.unwrap();
        assert_eq!(main_files, ["seed.rs"]);
        assert_eq!(branch_uncommitted, ["untracked.rs"]);
        assert_eq!(branch_unmerged, ["renamed.rs", "seed.rs"]);
        assert!(main_changes.unmerged.unwrap().is_empty());
        let main_root = main.to_string_lossy();
        let branch_root = branch.to_string_lossy();
        let mut branch_conversation =
            fixture_conversation("p3", Some(&branch_root), branch_uncommitted);
        branch_conversation.tree.unmerged_files = branch_unmerged;
        let conversations = [
            fixture_conversation("p1", None, main_files.clone()),
            fixture_conversation("p2", None, main_files),
            branch_conversation,
        ];
        for (path, uncommitted, unmerged) in [
            ("seed.rs", [true, false], [false, true]),
            ("renamed.rs", [false, false], [false, true]),
            ("untracked.rs", [false, true], [false, false]),
            ("absent.rs", [false, false], [false, false]),
        ] {
            let snapshot = model_snapshot(&main_root, "p1", &conversations, Some(path));
            let trees = snapshot["worktrees"].as_array().unwrap();
            assert_eq!(
                trees.len(),
                2,
                "shared conversations must reference one tree"
            );
            for (index, tree) in trees.iter().enumerate() {
                assert_eq!(
                    tree["uncommitted"]["path_changed"], uncommitted[index],
                    "{path}: {snapshot}"
                );
                assert_eq!(
                    tree["unmerged"]["path_changed"], unmerged[index],
                    "{path}: {snapshot}"
                );
            }
        }
        // Git 登记的保留工作树不依赖对话绑定，不能被空闲或缺失的对话遮蔽。
        let state = AppState::default();
        let main_project = crate::normalized_project_root(&main);
        let current = ensure_default_process(&state, &main_project);
        let probe = CollaborationProbe::new(
            state.processes.clone(),
            state.runtimes.clone(),
            main_project,
            current.id.clone(),
        );
        let snapshot = probe.snapshot(Some("renamed.rs"));
        assert_eq!(snapshot["conversations"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["current"]["process_id"], current.id);
        let retained = snapshot["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tree| tree["branch"] == "query-branch")
            .unwrap();
        assert_eq!(retained["unmerged"]["path_changed"], true);
        assert!(snapshot.get("children").is_none());
        drop(probe);
        drop(state);
        drop(current);
        owned.close().unwrap();
    }

    #[test]
    fn 对话状态来自运行与等待事实_长时间运行不推断卡住() {
        assert_eq!(
            conversation_status(ConversationProgress {
                running: true,
                run_age_ms: Some(1_000),
                ..Default::default()
            }),
            "running"
        );
        assert_eq!(
            conversation_status(ConversationProgress {
                running: true,
                run_age_ms: Some(90_000),
                ..Default::default()
            }),
            "running"
        );
        // 模型或工具长时间运行时，保持引擎的实际运行状态。
        assert_eq!(
            conversation_status(ConversationProgress {
                running: true,
                run_age_ms: Some(90_000),
                ..Default::default()
            }),
            "running"
        );
        assert_eq!(
            conversation_status(ConversationProgress {
                run_age_ms: Some(90_000),
                ..Default::default()
            }),
            "unavailable"
        );
        assert_eq!(
            conversation_status(ConversationProgress {
                run_age_ms: Some(90_000),
                last_outcome: Some("completed"),
                ..Default::default()
            }),
            "completed"
        );
        assert_eq!(conversation_status(ConversationProgress::default()), "idle");
        assert_eq!(
            conversation_status(ConversationProgress {
                run_age_ms: Some(90_000),
                last_outcome: Some("incomplete"),
                ..Default::default()
            }),
            "incomplete"
        );
        let waiting = vec!["run-waiting".to_string()];
        assert_eq!(
            conversation_status(ConversationProgress {
                running: true,
                run_id: "run-waiting",
                waiting_writers: &waiting,
                run_age_ms: Some(90_000),
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
            let status = conversation_status(ConversationProgress {
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
            conversation_status(ConversationProgress {
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
            conversation_status(ConversationProgress {
                running: true,
                waiting_for: pending_wait(&runtime),
                run_age_ms: Some(90_000),
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
    async fn 协作块包含空闲对话与工作树_文件变化后原位刷新() {
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
        kanzei_memory::docstore::DocStore::open(&canonical, &kanzei_memory::docstore::REQUIREMENTS)
            .save(&[kanzei_memory::docstore::Entry {
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
        current_runtime.activate_for_test();

        let peer_root = root.join("peer");
        run_git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "kanzei/thread-r184",
                peer_root.to_str().unwrap(),
            ],
        );
        let peer_root = crate::normalized_project_root(&peer_root);
        let other = ProcessHandle {
            id: format!("p1|{}", canonical.display()),
            origin_project: crate::ProjectRoot(canonical.clone()),
            project_dir: crate::ProjectRoot(canonical.clone()),
            worktree_path: Some(crate::WorktreeRoot(peer_root.clone())),
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
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: canonical.clone(),
            project_root: canonical.clone(),
            config: Arc::new(kanzei_harness::KanzeiConfig::default()),
        };
        let mut harness = kanzei_harness::Harness::default();
        harness.add(CollaborationComponent {
            probe: probe.clone(),
        });
        let snapshot = harness.resolve(&ctx).unwrap();

        // 空闲对话及其工作树仍属于协作范围。
        assert!(snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0
            .contains(&serde_json::to_string(&other.id).unwrap()));
        other_runtime.activate_for_test();
        std::fs::write(peer_root.join("first.rs"), "first\n").unwrap();
        let all = probe.conversations();
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
        assert!(!first.contains("first.rs"), "{first}");
        let first_count = other_line.tree.uncommitted_files.len();
        assert!(
            first.contains(&format!("\"file_count\":{first_count}")),
            "{first}"
        );
        assert!(
            snapshot
                .stable_system_baseline_with_report()
                .unwrap()
                .0
                .is_empty(),
            "协作块不得落进稳定 baseline 后逐轮累积"
        );

        std::fs::write(peer_root.join("second.rs"), "second\n").unwrap();
        let refreshed = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
        assert!(!refreshed.contains("first.rs") && !refreshed.contains("second.rs"));
        assert!(
            refreshed.contains(&format!("\"file_count\":{}", first_count + 1)),
            "{refreshed}"
        );
        assert_eq!(
            refreshed.matches("## 当前项目协作状态").count(),
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
        assert!(!output.content.contains("second.rs"), "{}", output.content);
        let scoped = tool
            .execute(
                serde_json::json!({"path": "second.rs"}),
                &kanzei_harness::ToolCtx::new(canonical.clone(), canonical.clone()),
            )
            .await;
        assert!(!scoped.is_error, "{}", scoped.content);
        let scoped_tree = scoped.display.as_ref().unwrap()["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tree| tree["branch"] == "kanzei/thread-r184")
            .unwrap();
        assert_eq!(scoped_tree["uncommitted"]["path_changed"], true);
        let absent = tool
            .execute(
                serde_json::json!({"path": "absent.rs"}),
                &kanzei_harness::ToolCtx::new(canonical.clone(), canonical.clone()),
            )
            .await;
        assert!(absent.display.as_ref().unwrap()["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tree| tree["uncommitted"]["path_changed"] == false));
        let invalid = tool
            .execute(
                serde_json::json!({"path": "../outside.rs"}),
                &kanzei_harness::ToolCtx::new(canonical.clone(), canonical.clone()),
            )
            .await;
        assert!(invalid.is_error);

        current_runtime.settle_for_test();
        other_runtime.settle_for_test();
        drop(snapshot);
        drop(harness);
        std::fs::remove_dir_all(root).ok();
    }

    /// R-176 验收⑥:协作面板的 writer/waiting 数据来自协调器快照,不是前端推测。
    /// 用真实 MemoryCoordinator:先占住写租约,再采样 CollaborationConversation,断言
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
        let lines = probe.conversations();
        // current(主树)的 writer 与 waiting 就是协调器快照的内容。
        assert!(
            lines.iter().any(|line| {
                line.tree.writer_run_id.as_deref() == Some("line-a")
                    && line.tree.waiting_writers.contains(&"line-b".to_string())
            }),
            "writer/waiting 必须来自协调器快照: {lines:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
