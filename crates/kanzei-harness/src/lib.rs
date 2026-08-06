//! kanzei-harness: 统一扩展层。
//! M0 仅提供 Tool 契约;M1 扩展为六注册表(agents/tools/commands/skills/
//! context-sources/permissions)+ 快照解析 + 拦截器链。

pub mod tool;

pub use tool::{Tool, ToolCtx, ToolOutput};
