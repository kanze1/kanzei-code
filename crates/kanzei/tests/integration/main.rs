//! 集成测试的**单一**二进制入口。
//!
//! 每个顶层 `tests/*.rs` 都是独立 crate，各自把整个 workspace(kanzei → harness/
//! tools/core/llm/memory/base)完整链接一遍。拆到 14 个文件时就是 14 次全量链接，
//! 这是 `cargo test --workspace` 的主要成本，而链接本身不产生任何测试价值。
//!
//! 这里把它们收成一个 target：`tests/integration/main.rs` 一次链接，各原文件降级为
//! 模块。子目录下的 `.rs` 不会被 cargo 当作独立测试目标，所以只需在此登记。
//!
//! 合并安全的前提是**隔离在子进程级**：全仓 0 处进程内 `env::set_var`，环境隔离一律
//! 走 `Command::env(...)`(见 `common::TestHome`)。若将来有测试需要改本进程环境，
//! 它必须重新拆成独立 target，否则同进程并行会互相污染。

mod common;

mod always_allow_bash;
mod background_subagent_dispatch;
mod bash_action_literal;
mod context_overflow_recovery;
mod cooperative_halt;
mod ctrl_c_finalize;
mod d364_concurrent_doc_add;
mod d368_concurrent_memory_write;
mod global_home_guard;
mod max_tasks_parallel_dispatch;
mod memory_hints_not_persisted;
mod parallel_scouting_under_serial_writer;
mod task_cancel_parallel;
mod worktree_main_root;
