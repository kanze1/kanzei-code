//! 集成门禁子系统(R-254 批1,纯搬迁自 processes.rs)。
//!
//! 独立理由:门禁是「收活/合并后验证代码库能否通过 fmt/clippy/test/ui-smoke」的
//! 变更理由——`gate_steps` 按目录探测步骤表、`run_gate_step` 隐藏控制台执行、
//! `run_worktree_gate` 聚合结果、`worktree_gate`/`worktree_post_merge_gate` 是
//! IPC 入口。它是 Integration Gate 子系统,不是 Process 子系统:改一条门禁规则
//! 不必读懂进程注册或合并策略(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):步骤表只在对应文件存在时才纳入(非 Rust 仓库不装样子跑 cargo);
//! `CREATE_NO_WINDOW` 隐藏控制台窗口;摘要截断避免一次全量 test 输出撑爆前端面板。

use std::path::Path;

use crate::normalized_project_root;
use kanzei_tools::worktree as wt;

/// 收活五格之③(门禁)的返回:每个门禁步骤的名称与成败摘要。
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct GateStep {
    pub(crate) name: String,
    pub(crate) ok: bool,
    pub(crate) summary: String,
}

/// 一个门禁步骤的规格:程序 + 参数(在线的树里执行)。
pub(crate) struct GateStepSpec {
    pub(crate) name: &'static str,
    pub(crate) program: &'static str,
    pub(crate) args: Vec<String>,
}

/// 收活门禁的步骤表(设计文档 §5:fmt / clippy / test / 前端冒烟)。
///
/// 只在对应文件存在时才纳入——非 Rust 仓库不装样子跑 cargo,没有前端冒烟脚本的
/// 线不装样子跑 node;「门禁要么真的能验,要么不列」。
pub(crate) fn gate_steps(worktree: &Path) -> Vec<GateStepSpec> {
    let mut steps = Vec::new();
    if worktree.join("Cargo.toml").is_file() {
        steps.push(GateStepSpec {
            name: "fmt",
            program: "cargo",
            args: vec!["fmt".into(), "--all".into(), "--".into(), "--check".into()],
        });
        steps.push(GateStepSpec {
            name: "clippy",
            program: "cargo",
            args: vec![
                "clippy".into(),
                "--workspace".into(),
                "--all-targets".into(),
                "--quiet".into(),
            ],
        });
        steps.push(GateStepSpec {
            name: "test",
            program: "cargo",
            args: vec!["test".into(), "--workspace".into()],
        });
    }
    if worktree.join("scripts/ui-runtime-smoke.mjs").is_file() {
        steps.push(GateStepSpec {
            name: "ui-smoke",
            program: "node",
            args: vec!["scripts/ui-runtime-smoke.mjs".into()],
        });
    }
    steps
}

/// 执行一个门禁步骤:隐藏控制台窗口异步跑,收集成败与输出摘要。
async fn run_gate_step(cwd: &Path, spec: &GateStepSpec) -> GateStep {
    let mut command = tokio::process::Command::new(spec.program);
    command.args(&spec.args).current_dir(cwd);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().await;
    let (ok, body) = match output {
        Ok(out) => {
            let ok = out.status.success();
            let text = if ok {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stderr.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    stderr.trim().to_string()
                }
            };
            (ok, text)
        }
        Err(error) => (false, format!("无法执行 {}: {error}", spec.program)),
    };
    GateStep {
        name: spec.name.into(),
        ok,
        // 摘要截断,避免一次全量 test 的输出把前端面板撑爆。
        summary: {
            let head: String = body.chars().take(1200).collect();
            if body.chars().count() > 1200 {
                format!("{head}\n…(输出过长已截断)")
            } else {
                head
            }
        },
    }
}

/// 收活门禁(设计文档 §5 步骤③):在线的树里依次跑 fmt/clippy/test/前端冒烟,
/// 任何一步失败都不阻断后续(收活要求看到全貌),整体成败由调用方按步骤聚合。
pub(crate) async fn run_worktree_gate(worktree: &Path) -> Vec<GateStep> {
    let mut results = Vec::new();
    for spec in gate_steps(worktree) {
        results.push(run_gate_step(worktree, &spec).await);
    }
    results
}

#[tauri::command]
pub async fn worktree_gate(
    project_dir: String,
    worktree_path: String,
) -> Result<Vec<GateStep>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = wt::validate_worktree_path(&root, &worktree_path)?;
    Ok(run_worktree_gate(&worktree).await)
}

/// R-222 防线②:合并后全量——两条线各自绿≠合起来绿(设计文档 §5 ④)。
/// 合并成功后在主根跑与收活门禁相同的步骤(fmt/clippy/test/ui-smoke),
/// 结果可见;通过后前端才解锁回写 tracker。复用 `run_worktree_gate`
/// (gate_steps 按目录探测),不另造一套门禁定义。
#[tauri::command]
pub async fn worktree_post_merge_gate(project_dir: String) -> Result<Vec<GateStep>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    Ok(run_worktree_gate(&root).await)
}
