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
    pub(crate) phase: String,
    pub(crate) current_tool: Option<String>,
    pub(crate) running: bool,
    pub(crate) steps: u32,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) changed_files_error: Option<String>,
}

/// 只持 AppState 内可克隆的共享字段,供 harness source/tool 在 run_task 生命周期内读取。
#[derive(Clone)]
pub(crate) struct CollaborationProbe {
    processes: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    origin_project: PathBuf,
    current_process_id: String,
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
        }
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
                process.origin_project == origin
                    && (include_current || process.id != self.current_process_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let runtimes = self.runtimes.lock().unwrap().clone();
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
                let (claim, current_tool, steps, input_tokens, output_tokens) = runtime
                    .map(|runtime| {
                        let live = runtime.live.lock().unwrap();
                        (
                            claim_from_prompt(&live.prompt_head),
                            current_tool(&live.trace),
                            live.steps,
                            live.input_tokens,
                            live.output_tokens,
                        )
                    })
                    .unwrap_or_else(|| ("未声明条目".into(), None, 0, 0, 0));
                let phase = runtime
                    .map(|runtime| runtime.stage.lock().unwrap().clone())
                    .unwrap_or_else(|| "空闲".into());
                let code_root = process
                    .worktree_path
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.origin_project.clone());
                let (changed_files, changed_files_error) = changed_files(
                    &self.origin_project,
                    &code_root,
                    process.worktree_path.is_some(),
                );
                let branch = process.branch.clone().unwrap_or_else(|| {
                    git_text(&code_root, &["branch", "--show-current"])
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "(主工作树)".into())
                });
                Some(CollaborationLine {
                    label: process_label(&process),
                    process_id: process.id,
                    branch,
                    worktree_path: process.worktree_path,
                    claim,
                    phase,
                    current_tool,
                    running,
                    steps,
                    input_tokens,
                    output_tokens,
                    changed_files,
                    changed_files_error,
                })
            })
            .collect::<Vec<_>>();
        lines.sort_by(|a, b| a.process_id.cmp(&b.process_id));
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
    let latest = trace.iter().rev().find(|event| {
        matches!(
            event.get("kind").and_then(serde_json::Value::as_str),
            Some("tool.started" | "tool.completed")
        )
    })?;
    (latest.get("kind")?.as_str()? == "tool.started")
        .then(|| latest.get("name")?.as_str().map(str::to_string))
        .flatten()
}

fn process_label(process: &ProcessHandle) -> String {
    if process.id.starts_with("d|") {
        "默认".into()
    } else {
        process.id.split('|').next().unwrap_or("进程").into()
    }
}

fn claim_from_prompt(prompt: &str) -> String {
    let item = prompt
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .find(|word| {
            let Some((prefix, digits)) = word.split_once('-') else {
                return false;
            };
            matches!(prefix, "R" | "D" | "G" | "S" | "F" | "T")
                && !digits.is_empty()
                && digits.chars().all(|ch| ch.is_ascii_digit())
        });
    item.map(str::to_string).unwrap_or_else(|| {
        let head = prompt.trim().chars().take(100).collect::<String>();
        if head.is_empty() {
            "未声明条目".into()
        } else {
            head
        }
    })
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
            "- {} ({}) · claim: {} · branch: {} · phase: {} · tool: {} · changed files: {}",
            line.label,
            line.process_id,
            line.claim,
            line.branch,
            line.phase,
            line.current_tool.as_deref().unwrap_or("none"),
            files
        ));
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
    crate::ensure_default_process(&state, &root);
    crate::processes::restore_processes_from_store(&state, &root)?;
    let probe = CollaborationProbe::new(
        state.processes.clone(),
        state.runtimes.clone(),
        root,
        String::new(),
    );
    // git status/diff 在大仓可能到秒级；轮询 IPC 用阻塞线程池采样，不能占住
    // Tauri 命令执行线程拖慢其它线路的发送、停止与权限响应。
    tauri::async_runtime::spawn_blocking(move || probe.all_lines())
        .await
        .map_err(|error| format!("并行线路采样任务失败: {error}"))
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
        _ctx: &kanzei_harness::ToolCtx,
    ) -> kanzei_harness::ToolOutput {
        let lines = self.probe.active_others();
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
    fn porcelain_paths_cover_untracked_modified_and_rename_pairs() {
        let parsed = parse_porcelain_paths(b" M src/a.rs\0?? new.txt\0R  dst.rs\0src.rs\0");
        assert_eq!(parsed, ["dst.rs", "new.txt", "src.rs", "src/a.rs"]);
    }

    #[test]
    fn claimed_item_prefers_tracker_id_and_falls_back_to_prompt_head() {
        assert_eq!(claim_from_prompt("继续修 R-184 的 A 面"), "R-184");
        assert_eq!(
            claim_from_prompt("实现并行协作上下文"),
            "实现并行协作上下文"
        );
        assert_eq!(claim_from_prompt("  "), "未声明条目");
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
        let state = AppState::default();
        let current = ensure_default_process(&state, &canonical);
        let current_runtime =
            runtime_for(&state, &process_session_id(&canonical, Some(&current.id)));
        current_runtime.running.store(true, Ordering::SeqCst);

        let other = ProcessHandle {
            id: format!("p1|{}", canonical.display()),
            origin_project: canonical.display().to_string(),
            project_dir: canonical.display().to_string(),
            worktree_path: None,
            branch: Some("kanzei/thread-r184".into()),
            model: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            reasoning: Arc::new(Mutex::new(None)),
            phase_pipeline_enabled: Arc::new(AtomicBool::new(false)),
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
            live.begin("run-r184", "input-r184", "继续推进 R-184", "test", "test");
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
        assert!(snapshot.system_baseline().is_empty());
        other_runtime.running.store(true, Ordering::SeqCst);
        std::fs::write(root.join("first.rs"), "first\n").unwrap();
        let all = probe.all_lines();
        let other_line = all
            .iter()
            .find(|line| line.process_id == other.id)
            .expect("并列视图漏掉运行线");
        assert!(other_line.running);
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
        let first = snapshot.refreshable_system_baseline_with_report().0;
        assert!(
            first.contains("R-184"),
            "认领条目必须来自 live prompt:{first}"
        );
        assert!(first.contains("kanzei/thread-r184"), "{first}");
        assert!(first.contains("first.rs"), "{first}");
        assert!(
            snapshot.stable_system_baseline_with_report().0.is_empty(),
            "协作块不得落进稳定 baseline 后逐轮累积"
        );

        std::fs::write(root.join("second.rs"), "second\n").unwrap();
        let refreshed = snapshot.refreshable_system_baseline_with_report().0;
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
}
