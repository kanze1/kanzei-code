//! kanzei-harness: 统一扩展层。
//! 一切喂给模型的东西都是组件 → 六注册表 → 每轮不可变快照;
//! 规则全走代码硬门禁(权限 Ruleset + 拦截器),不靠提示词恳求。

pub mod auto_run;
pub mod config;
pub mod context;
pub mod defs;
pub mod harness;
pub mod home;
pub mod markdown;
pub mod permission;
pub mod progress;
pub mod registry;
pub mod repair;
pub mod tool;

pub use config::{KanzeiConfig, ResolvedModel};
pub use context::{source, ContextSource};
pub use defs::{AgentDef, AgentMode, CommandDef, ProfileKind, ProfileScope, SkillDef};
pub use harness::{
    rule, Component, ConfigComponent, Harness, HarnessDraft, HarnessSnapshot, ResolveCtx,
};
pub use home::kanzei_home;
pub use markdown::MarkdownComponent;
pub use permission::{Effect, ManagedResource, Rule, Ruleset};
pub use registry::Registry;
pub use repair::tolerant_parse;
pub use tool::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
