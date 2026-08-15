//! 并行开发子系统(R-254 批1,由 processes.rs 拆分)。
//!
//! 独立理由:并行开发子系统总入口原来是 1562 行单文件,按变更理由切成四个正交域:
//! [`registry`](registry)(进程注册与持久化:p{n} 编号/查重/state.db 恢复)、
//! [`lifecycle`](lifecycle)(进程生命周期与 IPC 命令:建线/改线/关线/列表)、
//! [`workspace`](workspace)(工作树生命周期/合并/收割/写租约)、
//! [`gate`](gate)(集成门禁:fmt/clippy/test/ui-smoke)。改合并策略不必读懂
//! 进程编号,改门禁步骤不必读懂关线顺序(照 files_view.rs 模式)。
//!
//! 模块头硬不变式(原 processes.rs L3-18,由 D-367 类型化站岗):
//! `ProcessHandle.project_dir` 与 `origin_project` **恒为主根**
//! (`normalized_project_root` 的规范化形态);一条线的执行工作区**只**由
//! `worktree_path` 承担,`ProjectRoot` / `WorktreeRoot` 是两个不同 newtype,
//! 互相传参编译器直接拒绝(反例注释见 state.rs)。

pub(crate) mod gate;
pub(crate) mod lifecycle;
pub(crate) mod registry;
pub(crate) mod workspace;

// 非测试消费方(projects.rs / collaboration.rs / commands/run.rs)走 re-export;
// Tauri command 入口在 main.rs 直接引用子模块路径(宏辅助符号不随 re-export 走)。
pub(crate) use lifecycle::{list_pending_inputs, process_list, unregister_parallel_process};
pub(crate) use registry::restore_processes_from_store_once;

// worktree_tests 经 `super::` 引用本 mod 命名空间,测试专用符号在此挂出(cfg(test))。
#[cfg(test)]
pub(crate) use gate::{gate_steps, run_worktree_gate};
#[cfg(test)]
pub(crate) use lifecycle::{close_process, create_process, create_process_with_work_item};
#[cfg(test)]
pub(crate) use registry::{persist_process, restore_processes_from_store};
#[cfg(test)]
pub(crate) use workspace::{
    acquire_project_write_lease_within, create_worktree_arbitrated,
    discard_worktree_and_unregister, discard_worktree_checked,
    harvest_tracker_candidates_from_messages, merge_worktree_and_release, parse_harvest_claim,
    reclaim_worktree_on_close, with_idle_bound_process, worktree_diff, WRITE_LEASE_TIMEOUT,
};

// R-177 验收⑦:processes.rs 在 F4 之前零测试(既无 mod tests 也无 #[test])。
// 真测试在同级 worktree_tests.rs,经 #[path] 挂载本模块,`super::` 指向本 mod。
#[cfg(test)]
#[path = "../worktree_tests.rs"]
mod tests;
