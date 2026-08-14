//! kanzei-memory:记忆控制平面(R-103/R-104/R-161~R-167 主战场)。
//!
//! 2026-08-16 R-203 从 kanzei-tools 拆出。随迁模块:
//! - `memory/`:分级记忆系统(文件真源 + SQLite 派生索引);
//! - `docstore`:结构化文档引擎——memory 校验 refs / 解析 memory.md 依赖它,
//!   tools 内无其他生产使用者,为断 memory↔tools 循环依赖一并迁入;
//! - `embed`:向量通道(openai 兼容 /embeddings),memory 检索的 dense 通道;
//! - `replay_eval`:记忆检索策略的六臂回放评估(依赖 core replay 与 llm);
//! - `scheduling`:workable_titles 取活调度链副本(见 scheduling.rs 头部说明)。
//!
//! kanzei-tools 经 `pub use kanzei_memory::{memory, docstore, embed, replay_eval}`
//! 再导出,`kanzei_tools::memory::*` 等全部调用点零改动。

pub mod docstore;
pub mod embed;
pub mod memory;
pub mod replay_eval;
/// R-203:workable_titles 调度链副本(原 tools/tracker.rs 逐字复制)。memory 的
/// prompt_hints 在自主轮用取活条目标题做检索键,为断 memory↔tools 循环依赖而
/// 复制,不是新的调度实现;R-204 把取活调度抽成独立可审计模块时在此统一。
pub mod scheduling;

/// 原子写原语:memory/docstore 共用 kanzei-base 的 atomic_file(原 tools 再导出)。
pub use kanzei_base::atomic_file;

/// 工具输入解析的公共入口:serde 失败时返回纠错反馈而不是崩溃。
/// 原 tools lib.rs 同名函数复制(R-203 破环:memory 的 manager/tools 用它解析
/// 工具输入,不能反向依赖 kanzei-tools)。
pub(crate) fn parse_input<T: serde::de::DeserializeOwned>(
    tool: &dyn kanzei_harness::Tool,
    input: serde_json::Value,
) -> Result<T, kanzei_harness::ToolOutput> {
    let raw = input.to_string();
    serde_json::from_value(input)
        .map_err(|e| kanzei_harness::tool::repair_hint(tool, &raw, &e.to_string()))
}
