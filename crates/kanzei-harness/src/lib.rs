//! kanzei-harness: 统一扩展层。
//! 一切喂给模型的东西都是组件 → 六注册表 → 每轮不可变快照;
//! 规则全走代码硬门禁(权限 Ruleset + 拦截器),不靠提示词恳求。

pub mod auto_run;
pub mod config;
pub mod context;
pub mod conventions;
pub mod defs;
pub mod harness;
pub mod home;
pub mod managed_fence;
pub mod markdown;
pub mod orchestration;
pub mod permission;
pub mod progress;
/// R-205:项目根发现与 HOME 守卫(config.rs 拆出,D-270 修复落点)。
pub mod project_root;
pub mod registry;
pub mod repair;
pub mod tool;

pub use config::{KanzeiConfig, ResolvedModel};
pub use context::{refreshing_source, source, ContextSource};
pub use conventions::DEFAULT_CONVENTIONS;
pub use defs::{AgentDef, AgentMode, CommandDef, ProfileKind, ProfileScope, SkillDef};
pub use harness::{
    rule, Component, ConfigComponent, Harness, HarnessDraft, HarnessSnapshot, ResolveCtx,
};
pub use home::kanzei_home;
pub use markdown::MarkdownComponent;
pub use orchestration::{
    BarrierKind, BarrierOutcome, CoordinatorSnapshot, ExecutionPolicy, OrchestrationEvent, Phase,
    PhaseError, PhaseObserver, ProjectExecutionCoordinator, ReadPermit, ReadSlotRequest,
    ScoutOutcome, WriterLease, WriterLeaseRequest,
};
pub use permission::{Effect, ManagedResource, Rule, Ruleset};
pub use registry::Registry;
pub use repair::tolerant_parse;
pub use tool::{Tool, ToolConcurrency, ToolCtx, ToolOutcome, ToolOutput};
